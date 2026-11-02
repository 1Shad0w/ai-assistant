//! Orquestador de servidores MCP vía stdio (JSON-RPC 2.0).
//!
//! [`McpOrchestrator`] implementa [`core_types::McpClient`] agregando las
//! herramientas de todos los servidores. Cada herramienta se expone con el
//! prefijo `servidor__herramienta` para evitar colisiones y enrutar llamadas.

#![forbid(unsafe_code)]

pub mod config;
pub mod jsonrpc;
pub mod server;

use std::time::Duration;

use async_trait::async_trait;
use core_types::{AssistantError, McpClient, Result, ToolDefinition};
use serde_json::Value;

pub use config::{McpConfig, ServerConfig};
pub use server::McpServer;

/// Separador entre nombre de servidor y de herramienta.
pub const NAMESPACE_SEP: &str = "__";

/// Timeout por defecto de cada petición MCP.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Agrega varios servidores MCP tras un único [`McpClient`].
pub struct McpOrchestrator {
    servers: Vec<McpServer>,
    failures: Vec<(String, AssistantError)>,
}

impl McpOrchestrator {
    /// Arranca todos los servidores de la configuración.
    ///
    /// Un servidor que falle no impide arrancar el resto; los fallos se
    /// consultan con [`McpOrchestrator::failures`].
    pub async fn start(config: &McpConfig, timeout: Duration) -> Self {
        let mut servers = Vec::new();
        let mut failures = Vec::new();
        for (name, cfg) in &config.servers {
            match McpServer::spawn(name, cfg, timeout).await {
                Ok(s) => {
                    tracing::info!("servidor MCP `{name}` listo");
                    servers.push(s);
                }
                Err(e) => {
                    tracing::warn!("servidor MCP `{name}` no disponible: {e}");
                    failures.push((name.clone(), e));
                }
            }
        }
        Self { servers, failures }
    }

    /// Construye el orquestador a partir de servidores ya inicializados.
    pub fn from_servers(servers: Vec<McpServer>) -> Self {
        Self {
            servers,
            failures: Vec::new(),
        }
    }

    /// Servidores que no pudieron arrancar, con su causa.
    pub fn failures(&self) -> &[(String, AssistantError)] {
        &self.failures
    }

    /// Nombres de los servidores activos.
    pub fn server_names(&self) -> Vec<&str> {
        self.servers.iter().map(McpServer::name).collect()
    }

    fn route<'a>(&self, qualified: &'a str) -> Option<(&McpServer, &'a str)> {
        self.servers.iter().find_map(|s| {
            qualified
                .strip_prefix(s.name())
                .and_then(|r| r.strip_prefix(NAMESPACE_SEP))
                .map(|tool| (s, tool))
        })
    }
}

#[async_trait]
impl McpClient for McpOrchestrator {
    async fn list_tools(&self) -> Result<Vec<ToolDefinition>> {
        let mut all = Vec::new();
        for s in &self.servers {
            match s.list_tools().await {
                Ok(tools) => all.extend(tools.into_iter().map(|mut t| {
                    t.name = format!("{}{NAMESPACE_SEP}{}", s.name(), t.name);
                    t
                })),
                Err(e) => tracing::warn!(
                    "no se pudieron listar las herramientas de `{}`: {e}",
                    s.name()
                ),
            }
        }
        Ok(all)
    }

    async fn call_tool(&self, name: &str, arguments: Value) -> Result<String> {
        let (server, tool) = self
            .route(name)
            .ok_or_else(|| AssistantError::Mcp(format!("herramienta desconocida: `{name}`")))?;
        server.call_tool(tool, arguments).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};

    /// Servidor MCP simulado: responde a initialize, tools/list y tools/call.
    fn mock_server(tool: &'static str) -> McpServer {
        let (client_io, server_io) = duplex(8192);
        let (cr, cw) = tokio::io::split(client_io);
        let (sr, mut sw) = tokio::io::split(server_io);
        tokio::spawn(async move {
            let mut lines = BufReader::new(sr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let (Some(id), Some(method)) = (msg.get("id").cloned(), msg["method"].as_str())
                else {
                    continue; // notificación
                };
                let result = match method {
                    "initialize" => json!({ "protocolVersion": "2024-11-05", "capabilities": {} }),
                    "tools/list" => json!({ "tools": [{
                        "name": tool, "description": "d",
                        "inputSchema": { "type": "object" } }] }),
                    "tools/call" => {
                        if msg["params"]["arguments"]["fail"] == json!(true) {
                            json!({ "isError": true, "content": [{ "type": "text", "text": "mal" }] })
                        } else {
                            json!({ "content": [{ "type": "text", "text": format!("eco:{}", msg["params"]["arguments"]["x"]) }] })
                        }
                    }
                    _ => {
                        let e = json!({ "jsonrpc": "2.0", "id": id,
                            "error": { "code": -32601, "message": "nope" } });
                        let _ = sw.write_all(format!("{e}\n").as_bytes()).await;
                        continue;
                    }
                };
                let r = json!({ "jsonrpc": "2.0", "id": id, "result": result });
                let _ = sw.write_all(format!("{r}\n").as_bytes()).await;
            }
        });
        McpServer::from_transport("mock", cr, cw, Duration::from_secs(2))
    }

    #[tokio::test]
    async fn discovers_and_routes_tools() -> Result<()> {
        let a = mock_server("echo");
        a.initialize().await?;
        let orch = McpOrchestrator::from_servers(vec![a]);

        let tools = orch.list_tools().await?;
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "mock__echo");

        let out = orch.call_tool("mock__echo", json!({ "x": 7 })).await?;
        assert_eq!(out, "eco:7");
        Ok(())
    }

    #[tokio::test]
    async fn tool_errors_and_unknown_tools_are_typed() -> Result<()> {
        let a = mock_server("echo");
        a.initialize().await?;
        let orch = McpOrchestrator::from_servers(vec![a]);

        let r = orch.call_tool("mock__echo", json!({ "fail": true })).await;
        assert!(matches!(r, Err(AssistantError::Mcp(m)) if m.contains("mal")));
        let r = orch.call_tool("otro__x", json!({})).await;
        assert!(matches!(r, Err(AssistantError::Mcp(_))));
        Ok(())
    }

    #[tokio::test]
    async fn closed_server_fails_instead_of_hanging() {
        let (client_io, server_io) = duplex(64);
        drop(server_io);
        let (cr, cw) = tokio::io::split(client_io);
        let s = McpServer::from_transport("dead", cr, cw, Duration::from_millis(500));
        assert!(s.initialize().await.is_err());
    }

    #[tokio::test]
    async fn missing_binary_is_reported_not_fatal() {
        let cfg = McpConfig {
            servers: [(
                "ghost".to_owned(),
                ServerConfig {
                    command: "__binario_inexistente__".into(),
                    args: vec![],
                    env: Default::default(),
                },
            )]
            .into(),
        };
        let orch = McpOrchestrator::start(&cfg, Duration::from_secs(1)).await;
        assert!(orch.server_names().is_empty());
        assert_eq!(orch.failures().len(), 1);
    }
}
