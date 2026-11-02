//! Cliente asíncrono del gateway OmniRoute (API compatible con OpenAI).
//!
//! Implementa [`core_types::LlmGateway`] con streaming SSE y reensamblado de
//! llamadas a herramientas ([`ToolCallAccumulator`]).

#![forbid(unsafe_code)]

mod protocol;
mod sse;

use std::collections::VecDeque;
use std::fmt::Display;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use core_types::{
    AssistantError, BoxStream, ChatMessage, LlmGateway, Result, StreamChunk, ToolDefinition,
};
use futures_util::{stream, Stream, StreamExt};

pub use protocol::{build_request_body, parse_event, ToolCallAccumulator};
pub use sse::SseParser;

/// URL base por defecto de OmniRoute.
pub const DEFAULT_BASE_URL: &str = "http://localhost:20128/v1";

/// Configuración del cliente.
#[derive(Debug, Clone)]
pub struct OmniRouteConfig {
    /// URL base (sin barra final), p. ej. `http://localhost:20128/v1`.
    pub base_url: String,
    /// Modelo o *combo* de enrutamiento de OmniRoute.
    pub model: String,
    /// Clave de API opcional (`Authorization: Bearer`).
    pub api_key: Option<String>,
    /// Timeout de conexión.
    pub connect_timeout: Duration,
}

impl OmniRouteConfig {
    /// Configuración por defecto para el `model` indicado.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            model: model.into(),
            api_key: None,
            connect_timeout: Duration::from_secs(10),
        }
    }

    /// Lee `OMNIROUTE_BASE_URL`, `OMNIROUTE_API_KEY` y `OMNIROUTE_MODEL`.
    pub fn from_env() -> Self {
        let mut cfg = Self::new(std::env::var("OMNIROUTE_MODEL").unwrap_or_else(|_| "auto".into()));
        if let Ok(url) = std::env::var("OMNIROUTE_BASE_URL") {
            cfg.base_url = url;
        }
        cfg.api_key = std::env::var("OMNIROUTE_API_KEY")
            .ok()
            .filter(|k| !k.is_empty());
        cfg
    }
}

/// Cliente HTTP de OmniRoute.
#[derive(Debug, Clone)]
pub struct OmniRouteClient {
    http: reqwest::Client,
    config: OmniRouteConfig,
}

impl OmniRouteClient {
    /// Construye el cliente. Falla si no se puede inicializar el stack HTTP.
    pub fn new(config: OmniRouteConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .build()
            .map_err(|e| AssistantError::Llm(e.to_string()))?;
        Ok(Self { http, config })
    }
}

#[async_trait]
impl LlmGateway for OmniRouteClient {
    async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<BoxStream<'static, Result<StreamChunk>>> {
        let body = build_request_body(&self.config.model, messages, tools)?;
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let mut req = self.http.post(url).json(&body);
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| AssistantError::Llm(format!("no se pudo contactar con OmniRoute: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let detail = resp.text().await.unwrap_or_default();
            return Err(AssistantError::Llm(format!("HTTP {status}: {detail}")));
        }
        Ok(chunks_from_bytes(Box::pin(resp.bytes_stream())))
    }
}

struct State<S> {
    bytes: S,
    parser: SseParser,
    queue: VecDeque<Result<StreamChunk>>,
    finished: bool,
}

/// Convierte un flujo de bytes SSE en un flujo de [`StreamChunk`].
///
/// Termina tras `Done` o el primer error; si el servidor cierra sin
/// `[DONE]`, emite `Done` de forma sintética.
pub fn chunks_from_bytes<S, E>(bytes: S) -> BoxStream<'static, Result<StreamChunk>>
where
    S: Stream<Item = std::result::Result<Bytes, E>> + Send + Unpin + 'static,
    E: Display,
{
    let state = State {
        bytes,
        parser: SseParser::new(),
        queue: VecDeque::new(),
        finished: false,
    };
    Box::pin(stream::unfold(state, |mut st| async move {
        loop {
            if let Some(item) = st.queue.pop_front() {
                if matches!(item, Ok(StreamChunk::Done) | Err(_)) {
                    st.finished = true;
                    st.queue.clear();
                }
                return Some((item, st));
            }
            if st.finished {
                return None;
            }
            match st.bytes.next().await {
                Some(Ok(b)) => {
                    for data in st.parser.push(&b) {
                        match parse_event(&data) {
                            Ok(chunks) => st.queue.extend(chunks.into_iter().map(Ok)),
                            Err(e) => st.queue.push_back(Err(e)),
                        }
                    }
                }
                Some(Err(e)) => st
                    .queue
                    .push_back(Err(AssistantError::Llm(format!("flujo interrumpido: {e}")))),
                None => st.queue.push_back(Ok(StreamChunk::Done)),
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(
        parts: Vec<&'static str>,
    ) -> impl Stream<Item = std::result::Result<Bytes, String>> + Send + Unpin {
        stream::iter(
            parts
                .into_iter()
                .map(|p| Ok(Bytes::from_static(p.as_bytes()))),
        )
    }

    #[tokio::test]
    async fn streams_text_then_done() {
        let s = fake(vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"Ho\"}}]}\n\ndata: {\"choi",
            "ces\":[{\"delta\":{\"content\":\"la\"}}]}\n\ndata: [DONE]\n\n",
        ]);
        let items: Vec<_> = chunks_from_bytes(s).collect().await;
        let ok: Vec<_> = items.into_iter().filter_map(|r| r.ok()).collect();
        assert_eq!(
            ok,
            vec![
                StreamChunk::Text("Ho".into()),
                StreamChunk::Text("la".into()),
                StreamChunk::Done
            ]
        );
    }

    #[tokio::test]
    async fn emits_done_when_connection_closes() {
        let items: Vec<_> = chunks_from_bytes(fake(vec![])).collect().await;
        assert!(matches!(items.as_slice(), [Ok(StreamChunk::Done)]));
    }

    #[tokio::test]
    async fn stops_after_error() {
        let s = fake(vec![
            "data: {\"error\":{\"message\":\"boom\"}}\n\ndata: [DONE]\n\n",
        ]);
        let items: Vec<_> = chunks_from_bytes(s).collect().await;
        assert_eq!(items.len(), 1);
        assert!(items[0].is_err());
    }
}
