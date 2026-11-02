//! Lectura de `.agents/mcp_config.json` con expansión de `${VARIABLES}`.

use std::collections::BTreeMap;
use std::path::Path;

use core_types::{AssistantError, Result};
use serde::Deserialize;

/// Configuración de un servidor MCP lanzado por stdio.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ServerConfig {
    /// Ejecutable (`npx`, `omniroute`...).
    pub command: String,
    /// Argumentos.
    #[serde(default)]
    pub args: Vec<String>,
    /// Variables de entorno adicionales.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Fichero de configuración completo.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct McpConfig {
    /// Servidores por nombre.
    #[serde(rename = "mcpServers")]
    pub servers: BTreeMap<String, ServerConfig>,
}

impl McpConfig {
    /// Interpreta el JSON de configuración.
    pub fn from_json_str(s: &str) -> Result<Self> {
        Ok(serde_json::from_str(s)?)
    }

    /// Carga la configuración desde disco.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_json_str(&std::fs::read_to_string(path)?)
    }
}

/// Expande `${NAME}`. En Windows, `HOME` cae a `USERPROFILE`.
///
/// Falla si la variable no existe, para no lanzar un servidor con rutas vacías.
pub fn expand_vars(input: &str) -> Result<String> {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            return Err(AssistantError::Mcp(format!(
                "`${{` sin cerrar en `{input}`"
            )));
        };
        let name = &after[..end];
        let value = std::env::var(name)
            .ok()
            .or_else(|| {
                (name == "HOME")
                    .then(|| std::env::var("USERPROFILE").ok())
                    .flatten()
            })
            .ok_or_else(|| {
                AssistantError::Mcp(format!("variable de entorno `{name}` no definida"))
            })?;
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repo_config() -> Result<()> {
        let cfg = McpConfig::from_json_str(include_str!("../../../.agents/mcp_config.json"))?;
        assert_eq!(cfg.servers.len(), 4);
        assert_eq!(cfg.servers["omniroute"].args, vec!["--mcp"]);
        Ok(())
    }

    #[test]
    fn expands_known_and_rejects_unknown() -> Result<()> {
        assert_eq!(expand_vars("sin variables")?, "sin variables");
        assert!(expand_vars("${__NO_EXISTE_XYZ__}/a").is_err());
        assert!(expand_vars("${PATH").is_err());
        Ok(())
    }
}
