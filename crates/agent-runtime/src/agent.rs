//! Bucle agéntico: LLM → llamadas a herramientas MCP → LLM … → respuesta.

use std::sync::Arc;

use core_types::{
    AssistantError, ChatMessage, LlmGateway, McpClient, Result, Role, StreamChunk, ToolDefinition,
};
use futures_util::StreamExt;
use llm_client::ToolCallAccumulator;
use tokio::sync::broadcast;

use crate::events::{AgentEvent, AgentState, EventSender};

/// Prompt de sistema por defecto (la salida se pronuncia en voz alta).
pub const DEFAULT_SYSTEM_PROMPT: &str = "Eres un asistente de voz útil y conciso. \
Responde en el idioma del usuario con frases cortas y naturales, sin markdown ni listas, \
porque tu respuesta se leerá en voz alta. Usa las herramientas disponibles cuando \
necesites acceder al correo, calendario, archivos o navegador. \
Pide confirmación antes de acciones destructivas o que envíen mensajes.";

/// Parámetros del agente.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Mensaje de sistema inicial (vacío = ninguno).
    pub system_prompt: String,
    /// Máximo de rondas LLM↔herramientas por turno.
    pub max_tool_iterations: usize,
    /// Máximo de mensajes de historial (sin contar el de sistema).
    pub max_history_messages: usize,
    /// Máximo de caracteres de salida de herramienta que se devuelven al LLM.
    pub max_tool_output_chars: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_owned(),
            max_tool_iterations: 8,
            max_history_messages: 60,
            max_tool_output_chars: 8_000,
        }
    }
}

/// Agente conversacional con herramientas.
pub struct Agent {
    llm: Arc<dyn LlmGateway>,
    mcp: Arc<dyn McpClient>,
    config: AgentConfig,
    history: Vec<ChatMessage>,
    events: EventSender,
}

impl Agent {
    /// Crea un agente con historial inicial (solo el mensaje de sistema).
    pub fn new(
        llm: Arc<dyn LlmGateway>,
        mcp: Arc<dyn McpClient>,
        config: AgentConfig,
        events: EventSender,
    ) -> Self {
        let mut agent = Self {
            llm,
            mcp,
            config,
            history: Vec::new(),
            events,
        };
        agent.reset();
        agent
    }

    /// Historial actual.
    pub fn history(&self) -> &[ChatMessage] {
        &self.history
    }

    /// Nueva suscripción a los eventos del agente.
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.events.subscribe()
    }

    /// Borra la conversación, conservando el mensaje de sistema.
    pub fn reset(&mut self) {
        self.history.clear();
        if !self.config.system_prompt.is_empty() {
            self.history
                .push(ChatMessage::system(self.config.system_prompt.clone()));
        }
    }

    /// Publica un evento (se ignora si nadie escucha).
    pub fn emit(&self, event: AgentEvent) {
        let _ = self.events.send(event);
    }

    /// Publica un cambio de estado.
    pub fn emit_state(&self, state: AgentState) {
        self.emit(AgentEvent::State { state });
    }

    /// Procesa un turno del usuario y devuelve la respuesta final.
    ///
    /// `on_text` recibe cada fragmento de texto en cuanto llega (útil para
    /// sintetizar voz por frases). Si el turno falla, el historial vuelve al
    /// estado previo para no dejar llamadas a herramientas sin respuesta.
    pub async fn run_turn(&mut self, user_text: &str, on_text: impl FnMut(&str)) -> Result<String> {
        self.trim_history();
        let checkpoint = self.history.len();
        self.history.push(ChatMessage::user(user_text));

        let outcome = self.turn_loop(on_text).await;
        if let Err(e) = &outcome {
            self.history.truncate(checkpoint);
            self.emit(AgentEvent::Error {
                message: e.to_string(),
            });
        }
        self.emit_state(AgentState::Idle);
        outcome
    }

    async fn turn_loop(&mut self, mut on_text: impl FnMut(&str)) -> Result<String> {
        self.emit_state(AgentState::Thinking);
        let tools: Vec<ToolDefinition> = match self.mcp.list_tools().await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("no se pudieron listar herramientas MCP: {e}");
                Vec::new()
            }
        };

        for _ in 0..self.config.max_tool_iterations {
            let mut stream = self.llm.stream_chat(&self.history, &tools).await?;
            let mut text = String::new();
            let mut acc = ToolCallAccumulator::new();

            while let Some(chunk) = stream.next().await {
                match chunk? {
                    StreamChunk::Text(t) => {
                        on_text(&t);
                        self.emit(AgentEvent::AssistantDelta { text: t.clone() });
                        text.push_str(&t);
                    }
                    delta @ StreamChunk::ToolCallDelta { .. } => acc.push(&delta),
                    StreamChunk::Done => break,
                }
            }
            drop(stream);

            let calls = acc.finish()?;
            if calls.is_empty() {
                self.history.push(ChatMessage::assistant(text.clone()));
                self.emit(AgentEvent::Response { text: text.clone() });
                return Ok(text);
            }

            self.history.push(ChatMessage {
                role: Role::Assistant,
                content: (!text.is_empty()).then(|| text.clone()),
                tool_calls: Some(calls.clone()),
                tool_call_id: None,
            });

            for call in calls {
                self.emit_state(AgentState::UsingTool);
                self.emit(AgentEvent::ToolStarted {
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                });
                // Un fallo de herramienta se devuelve al LLM para que se recupere.
                let (ok, output) = match self.mcp.call_tool(&call.name, call.arguments).await {
                    Ok(out) => (true, out),
                    Err(e) => (false, format!("Error: {e}")),
                };
                let output = truncate_chars(&output, self.config.max_tool_output_chars);
                self.emit(AgentEvent::ToolFinished {
                    name: call.name,
                    ok,
                    output: output.clone(),
                });
                self.history.push(ChatMessage::tool_result(call.id, output));
            }
            self.emit_state(AgentState::Thinking);
        }

        Err(AssistantError::Llm(format!(
            "se superó el límite de {} rondas de herramientas",
            self.config.max_tool_iterations
        )))
    }

    /// Descarta los turnos más antiguos sin partir pares llamada/resultado.
    fn trim_history(&mut self) {
        let start = usize::from(self.history.first().is_some_and(|m| m.role == Role::System));
        while self.history.len() - start > self.config.max_history_messages {
            let next_user = self.history[start + 1..]
                .iter()
                .position(|m| m.role == Role::User)
                .map(|i| i + start + 1);
            match next_user {
                Some(idx) => {
                    self.history.drain(start..idx);
                }
                None => break,
            }
        }
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("… [salida truncada]");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::event_channel;
    use async_trait::async_trait;
    use core_types::BoxStream;
    use futures_util::stream;
    use serde_json::{json, Value};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct ScriptedLlm(Mutex<VecDeque<Vec<StreamChunk>>>);

    #[async_trait]
    impl LlmGateway for ScriptedLlm {
        async fn stream_chat(
            &self,
            _m: &[ChatMessage],
            _t: &[ToolDefinition],
        ) -> Result<BoxStream<'static, Result<StreamChunk>>> {
            let next = self
                .0
                .lock()
                .map_err(|_| AssistantError::Llm("mutex".into()))?
                .pop_front()
                .ok_or_else(|| AssistantError::Llm("sin más respuestas".into()))?;
            Ok(Box::pin(stream::iter(next.into_iter().map(Ok))))
        }
    }

    struct FakeMcp {
        fail: bool,
    }

    #[async_trait]
    impl McpClient for FakeMcp {
        async fn list_tools(&self) -> Result<Vec<ToolDefinition>> {
            Ok(vec![ToolDefinition {
                name: "fs__read".into(),
                description: "lee".into(),
                parameters: json!({"type": "object"}),
            }])
        }
        async fn call_tool(&self, name: &str, arguments: Value) -> Result<String> {
            if self.fail {
                return Err(AssistantError::Mcp("denegado".into()));
            }
            Ok(format!("{name}:{arguments}"))
        }
    }

    fn tool_call_chunks() -> Vec<StreamChunk> {
        vec![
            StreamChunk::ToolCallDelta {
                index: 0,
                id: Some("c1".into()),
                name: Some("fs__read".into()),
                arguments_delta: "{\"p\":".into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                id: None,
                name: None,
                arguments_delta: "1}".into(),
            },
            StreamChunk::Done,
        ]
    }

    fn agent(script: Vec<Vec<StreamChunk>>, fail: bool) -> Agent {
        let (tx, _rx) = event_channel();
        Agent::new(
            Arc::new(ScriptedLlm(Mutex::new(script.into()))),
            Arc::new(FakeMcp { fail }),
            AgentConfig::default(),
            tx,
        )
    }

    #[tokio::test]
    async fn plain_answer_streams_text() -> Result<()> {
        let mut a = agent(
            vec![vec![
                StreamChunk::Text("Hola".into()),
                StreamChunk::Text(" mundo".into()),
                StreamChunk::Done,
            ]],
            false,
        );
        let mut seen = Vec::new();
        let out = a.run_turn("hi", |t| seen.push(t.to_owned())).await?;
        assert_eq!(out, "Hola mundo");
        assert_eq!(seen, vec!["Hola", " mundo"]);
        assert_eq!(a.history().len(), 3); // system, user, assistant
        Ok(())
    }

    #[tokio::test]
    async fn tool_loop_executes_and_continues() -> Result<()> {
        let mut a = agent(
            vec![
                tool_call_chunks(),
                vec![StreamChunk::Text("listo".into()), StreamChunk::Done],
            ],
            false,
        );
        let out = a.run_turn("lee", |_| {}).await?;
        assert_eq!(out, "listo");
        let tool_msg = a
            .history()
            .iter()
            .find(|m| m.role == Role::Tool)
            .ok_or_else(|| AssistantError::Llm("falta mensaje de herramienta".into()))?;
        assert_eq!(tool_msg.tool_call_id.as_deref(), Some("c1"));
        assert_eq!(tool_msg.content.as_deref(), Some("fs__read:{\"p\":1}"));
        Ok(())
    }

    #[tokio::test]
    async fn tool_failure_is_reported_to_llm_not_fatal() -> Result<()> {
        let mut a = agent(
            vec![
                tool_call_chunks(),
                vec![StreamChunk::Text("no pude".into()), StreamChunk::Done],
            ],
            true,
        );
        let out = a.run_turn("lee", |_| {}).await?;
        assert_eq!(out, "no pude");
        assert!(a.history().iter().any(|m| m.role == Role::Tool
            && m.content.as_deref().is_some_and(|c| c.contains("denegado"))));
        Ok(())
    }

    #[tokio::test]
    async fn failed_turn_rolls_back_history() {
        let mut a = agent(vec![], false); // el LLM no tiene respuestas → error
        let before = a.history().len();
        assert!(a.run_turn("hola", |_| {}).await.is_err());
        assert_eq!(a.history().len(), before);
    }

    #[tokio::test]
    async fn endless_tool_calls_hit_the_limit() {
        let script = (0..20).map(|_| tool_call_chunks()).collect();
        let mut a = agent(script, false);
        let r = a.run_turn("x", |_| {}).await;
        assert!(matches!(r, Err(AssistantError::Llm(m)) if m.contains("límite")));
    }

    #[test]
    fn trims_on_turn_boundaries() {
        let mut a = agent(vec![], false);
        a.config.max_history_messages = 4;
        for i in 0..5 {
            a.history.push(ChatMessage::user(format!("u{i}")));
            a.history.push(ChatMessage::assistant(format!("a{i}")));
        }
        a.trim_history();
        assert_eq!(a.history[0].role, Role::System);
        assert_eq!(a.history[1].role, Role::User);
        assert!(a.history.len() - 1 <= 5);
    }
}
