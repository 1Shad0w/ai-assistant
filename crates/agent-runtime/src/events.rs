//! Eventos que el runtime publica hacia cualquier interfaz (CLI, Tauri...).
//!
//! La UI nunca llama al agente directamente para leer estado: se suscribe a
//! un canal `broadcast` y recibe estos eventos serializables.

use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

/// Estado general del asistente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    /// En reposo (por ejemplo, escucha desactivada).
    Idle,
    /// Esperando a que el usuario hable.
    Listening,
    /// Transcribiendo el audio del usuario.
    Transcribing,
    /// Esperando al LLM.
    Thinking,
    /// Ejecutando una herramienta MCP.
    UsingTool,
    /// Reproduciendo la respuesta.
    Speaking,
}

/// Evento publicado por el runtime.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Cambio de estado.
    State {
        /// Nuevo estado.
        state: AgentState,
    },
    /// Texto reconocido del usuario.
    Transcript {
        /// Texto transcrito.
        text: String,
    },
    /// Fragmento de la respuesta del asistente (streaming).
    AssistantDelta {
        /// Fragmento de texto.
        text: String,
    },
    /// Comienza la ejecución de una herramienta.
    ToolStarted {
        /// Nombre cualificado (`servidor__herramienta`).
        name: String,
        /// Argumentos recibidos del LLM.
        arguments: Value,
    },
    /// Termina la ejecución de una herramienta.
    ToolFinished {
        /// Nombre cualificado.
        name: String,
        /// Si terminó sin error.
        ok: bool,
        /// Salida (o mensaje de error), truncada.
        output: String,
    },
    /// Respuesta final completa.
    Response {
        /// Texto final.
        text: String,
    },
    /// Error recuperable del turno actual.
    Error {
        /// Descripción del error.
        message: String,
    },
}

/// Emisor de eventos.
pub type EventSender = broadcast::Sender<AgentEvent>;

/// Crea un canal de eventos con búfer para varios suscriptores.
pub fn event_channel() -> (EventSender, broadcast::Receiver<AgentEvent>) {
    broadcast::channel(256)
}
