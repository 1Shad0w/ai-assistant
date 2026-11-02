//! Runtime del asistente: orquesta voz local, LLM en la nube y herramientas MCP.
//!
//! Este crate no depende de ninguna interfaz gráfica: la UI se conecta con
//! [`events::AgentEvent`] (salida) y [`voice_loop::VoiceControl`] (entrada).

#![forbid(unsafe_code)]

pub mod agent;
pub mod events;
pub mod voice_loop;

#[cfg(feature = "voice-io")]
pub mod voice_mode;

pub use agent::{Agent, AgentConfig, DEFAULT_SYSTEM_PROMPT};
pub use events::{event_channel, AgentEvent, AgentState, EventSender};
pub use voice_loop::{run_voice_loop, VoiceComponents, VoiceControl};
