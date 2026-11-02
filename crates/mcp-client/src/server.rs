//! Un servidor MCP individual: handshake, descubrimiento y ejecución.

use std::time::Duration;

use core_types::{AssistantError, Result, ToolDefinition};
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::{Child, Command};

use crate::config::{expand_vars, ServerConfig};
use crate::jsonrpc::McpConnection;

/// Versión del protocolo MCP que declaramos en `initialize`.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Servidor MCP conectado e inicializado.
pub struct McpServer {
    name: String,
    conn: McpConnection,
    // Mantiene vivo el subproceso; `kill_on_drop` lo termina al soltar.
    _child: Option<Child>,
}

impl McpServer {
    /// Lanza el subproceso descrito en `cfg` y completa el handshake.
    pub async fn spawn(name: &str, cfg: &ServerConfig, timeout: Duration) -> Result<Self> {
        let command = expand_vars(&cfg.command)?;
        let args = cfg
            .args
            .iter()
            .map(|a| expand_vars(a))
            .collect::<Result<Vec<_>>>()?;

        // En Windows, `npx` es un script `.cmd`: se lanza mediante `cmd /C`.
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&command);
            c
        } else {
            Command::new(&command)
        };
        cmd.args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        for (k, v) in &cfg.env {
            cmd.env(k, expand_vars(v)?);
        }

        let mut child = cmd.spawn().map_err(|e| {
            AssistantError::Mcp(format!("no se pudo lanzar `{command}` para `{name}`: {e}"))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AssistantError::Mcp("sin stdin en el subproceso".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AssistantError::Mcp("sin stdout en el subproceso".into()))?;

        let server = Self::connect(name, stdout, stdin, timeout, Some(child));
        server.initialize().await?;
        Ok(server)
    }

    /// Crea un servidor sobre un transporte arbitrario (p. ej. para tests).
    /// No realiza el handshake; llama a [`McpServer::initialize`].
    pub fn from_transport<R, W>(name: &str, reader: R, writer: W, timeout: Duration) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        Self::connect(name, reader, writer, timeout, None)
    }

    fn connect<R, W>(
        name: &str,
        reader: R,
        writer: W,
        timeout: Duration,
        child: Option<Child>,
    ) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        Self {
            name: name.to_owned(),
            conn: McpConnection::new(reader, writer, timeout),
            _child: child,
        }
    }

    /// Nombre lógico del servidor.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Handshake `initialize` + notificación `initialized`.
    pub async fn initialize(&self) -> Result<()> {
        self.conn
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "ai-assistant", "version": env!("CARGO_PKG_VERSION") },
                }),
            )
            .await?;
        self.conn
            .notify("notifications/initialized", json!({}))
            .await
    }

    /// Lista todas las herramientas (siguiendo la paginación).
    pub async fn list_tools(&self) -> Result<Vec<ToolDefinition>> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let res = self.conn.request("tools/list", params).await?;
            if let Some(items) = res.get("tools").and_then(Value::as_array) {
                for t in items {
                    let Some(name) = t.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    tools.push(ToolDefinition {
                        name: name.to_owned(),
                        description: t
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        parameters: t
                            .get("inputSchema")
                            .cloned()
                            .unwrap_or_else(|| json!({ "type": "object" })),
                    });
                }
            }
            cursor = res
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if cursor.is_none() {
                return Ok(tools);
            }
        }
    }

    /// Ejecuta una herramienta y devuelve su salida como texto.
    pub async fn call_tool(&self, tool: &str, arguments: Value) -> Result<String> {
        let res = self
            .conn
            .request(
                "tools/call",
                json!({ "name": tool, "arguments": arguments }),
            )
            .await?;

        let mut parts: Vec<String> = Vec::new();
        if let Some(content) = res.get("content").and_then(Value::as_array) {
            for item in content {
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(t) = item.get("text").and_then(Value::as_str) {
                            parts.push(t.to_owned());
                        }
                    }
                    Some(other) => parts.push(format!("[contenido {other} omitido]")),
                    None => {}
                }
            }
        }
        if parts.is_empty() {
            if let Some(sc) = res.get("structuredContent") {
                parts.push(serde_json::to_string(sc)?);
            }
        }
        let text = parts.join("\n");
        if res.get("isError").and_then(Value::as_bool).unwrap_or(false) {
            return Err(AssistantError::Mcp(format!(
                "la herramienta `{tool}` falló: {text}"
            )));
        }
        Ok(text)
    }
}
