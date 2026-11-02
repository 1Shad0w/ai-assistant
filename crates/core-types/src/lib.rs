//! # core-types
//!
//! Único punto de verdad tipado del sistema. Define los mensajes, los
//! eventos de streaming, los contratos asíncronos (traits) y la jerarquía de
//! errores compartidos por todos los crates del workspace.
//!
//! Este crate **no** depende de red, audio ni UI: solo de `serde`,
//! `thiserror`, `async-trait` y `futures-core`.

#![forbid(unsafe_code)]

use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Errores
// ---------------------------------------------------------------------------

/// Error centralizado del asistente.
#[derive(Debug, thiserror::Error)]
pub enum AssistantError {
    /// Fallo de red o del proveedor LLM (HTTP, SSE, límites, timeouts...).
    #[error("error de red/LLM: {0}")]
    Llm(String),

    /// Fallo del protocolo MCP (JSON-RPC, arranque del servidor, herramienta).
    #[error("error del protocolo MCP: {0}")]
    Mcp(String),

    /// Fallo del motor de audio (captura, VAD, STT, TTS, reproducción).
    #[error("error del motor de audio: {0}")]
    Audio(String),

    /// Problema de serialización o deserialización JSON.
    #[error("error de serialización JSON: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Error de entrada/salida del sistema operativo.
    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),
}

/// Alias de resultado estándar del sistema.
pub type Result<T> = std::result::Result<T, AssistantError>;

/// Flujo asíncrono, sendable y fijado en memoria dinámica.
pub type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;

// ---------------------------------------------------------------------------
// Mensajería
// ---------------------------------------------------------------------------

/// Rol del emisor de un mensaje de chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Instrucciones de sistema.
    System,
    /// Entrada del usuario.
    User,
    /// Respuesta del modelo.
    Assistant,
    /// Resultado de una herramienta.
    Tool,
}

/// Petición del modelo para ejecutar una herramienta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Identificador único de la llamada.
    pub id: String,
    /// Nombre de la función/herramienta a invocar.
    pub name: String,
    /// Argumentos como objeto JSON.
    pub arguments: Value,
}

/// Mensaje del historial de conversación.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Rol del emisor.
    pub role: Role,
    /// Contenido textual (opcional, p. ej. si solo hay llamadas a herramientas).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Llamadas a herramientas solicitadas por el asistente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// Identificador de la llamada a la que responde un mensaje de rol `Tool`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    fn with_text(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Crea un mensaje de sistema.
    pub fn system(content: impl Into<String>) -> Self {
        Self::with_text(Role::System, content)
    }

    /// Crea un mensaje de usuario.
    pub fn user(content: impl Into<String>) -> Self {
        Self::with_text(Role::User, content)
    }

    /// Crea un mensaje de asistente con texto.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::with_text(Role::Assistant, content)
    }

    /// Crea un mensaje de asistente que solicita herramientas.
    pub fn assistant_tool_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    /// Crea el resultado de una herramienta para la llamada `tool_call_id`.
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

/// Descripción de una herramienta expuesta al LLM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Nombre único de la herramienta.
    pub name: String,
    /// Descripción legible para el LLM.
    pub description: String,
    /// Esquema JSON Schema de los parámetros.
    pub parameters: Value,
}

/// Evento de un flujo de respuesta en streaming.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamChunk {
    /// Fragmento de texto (el consumidor lo acumula).
    Text(String),
    /// Delta incremental de una llamada a herramienta.
    ToolCallDelta {
        /// Índice de la llamada dentro de la respuesta.
        index: usize,
        /// Identificador de la llamada, normalmente solo en el primer delta.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Nombre de la función, normalmente solo en el primer delta.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Fragmento de texto JSON de los argumentos.
        arguments_delta: String,
    },
    /// Fin de la transmisión.
    Done,
}

// ---------------------------------------------------------------------------
// Contratos asíncronos
// ---------------------------------------------------------------------------

/// Pasarela hacia el LLM en la nube (OmniRoute).
#[async_trait]
pub trait LlmGateway: Send + Sync {
    /// Envía el historial y las herramientas disponibles y devuelve un flujo
    /// de chunks en tiempo real.
    async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<BoxStream<'static, Result<StreamChunk>>>;
}

/// Cliente de servidores MCP.
#[async_trait]
pub trait McpClient: Send + Sync {
    /// Descubre dinámicamente las herramientas registradas.
    async fn list_tools(&self) -> Result<Vec<ToolDefinition>>;

    /// Ejecuta una herramienta por nombre con argumentos JSON y devuelve su
    /// salida textual.
    async fn call_tool(&self, name: &str, arguments: Value) -> Result<String>;
}

/// Transcriptor de voz a texto (STT).
#[async_trait]
pub trait VoiceTranscriber: Send + Sync {
    /// Transcribe muestras `f32` mono a la frecuencia `sample_rate` (Hz).
    async fn transcribe(&self, samples: Vec<f32>, sample_rate: u32) -> Result<String>;
}

/// Sintetizador de texto a voz (TTS).
#[async_trait]
pub trait VoiceSynthesizer: Send + Sync {
    /// Convierte una frase en un flujo de muestras de audio `f32`.
    async fn synthesize(&self, text: &str) -> Result<BoxStream<'static, Result<Vec<f32>>>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_result_roundtrip() -> Result<()> {
        let msg = ChatMessage::tool_result("call_1", "ok");
        let json = serde_json::to_string(&msg)?;
        let back: ChatMessage = serde_json::from_str(&json)?;
        assert_eq!(msg, back);
        assert_eq!(back.role, Role::Tool);
        Ok(())
    }

    #[test]
    fn io_error_converts() {
        let err: AssistantError = std::io::Error::other("boom").into();
        assert!(matches!(err, AssistantError::Io(_)));
    }
}
