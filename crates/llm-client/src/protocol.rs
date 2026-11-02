//! Conversión entre los tipos de `core-types` y el formato OpenAI-compatible.

use std::collections::BTreeMap;

use core_types::{AssistantError, ChatMessage, Result, StreamChunk, ToolCall, ToolDefinition};
use serde_json::{json, Value};

/// Construye el cuerpo JSON de `POST /chat/completions` con `stream: true`.
pub fn build_request_body(
    model: &str,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
) -> Result<Value> {
    let msgs = messages
        .iter()
        .map(message_to_json)
        .collect::<Result<Vec<_>>>()?;
    let mut body = json!({ "model": model, "messages": msgs, "stream": true });
    if !tools.is_empty() {
        let tools_json: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect();
        body["tools"] = Value::Array(tools_json);
    }
    Ok(body)
}

fn message_to_json(m: &ChatMessage) -> Result<Value> {
    let mut v = json!({ "role": m.role });
    // OpenAI exige `content` presente (null) cuando solo hay tool_calls.
    v["content"] = match &m.content {
        Some(c) => Value::String(c.clone()),
        None => Value::Null,
    };
    if let Some(calls) = &m.tool_calls {
        let calls_json = calls
            .iter()
            .map(|c| {
                // En el protocolo, los argumentos viajan como cadena JSON.
                Ok(json!({
                    "id": c.id,
                    "type": "function",
                    "function": {
                        "name": c.name,
                        "arguments": serde_json::to_string(&c.arguments)?,
                    }
                }))
            })
            .collect::<Result<Vec<_>>>()?;
        v["tool_calls"] = Value::Array(calls_json);
    }
    if let Some(id) = &m.tool_call_id {
        v["tool_call_id"] = Value::String(id.clone());
    }
    Ok(v)
}

/// Traduce la carga `data` de un evento SSE a cero o más [`StreamChunk`].
pub fn parse_event(data: &str) -> Result<Vec<StreamChunk>> {
    let data = data.trim();
    if data == "[DONE]" {
        return Ok(vec![StreamChunk::Done]);
    }
    let v: Value = serde_json::from_str(data)?;

    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("error desconocido del gateway");
        return Err(AssistantError::Llm(msg.to_owned()));
    }

    let mut out = Vec::new();
    let Some(delta) = v.pointer("/choices/0/delta") else {
        return Ok(out);
    };

    if let Some(text) = delta.get("content").and_then(Value::as_str) {
        if !text.is_empty() {
            out.push(StreamChunk::Text(text.to_owned()));
        }
    }
    if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
        for (pos, call) in calls.iter().enumerate() {
            let index = call
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|i| usize::try_from(i).ok())
                .unwrap_or(pos);
            let str_field = |p: &str| {
                call.pointer(p)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            };
            out.push(StreamChunk::ToolCallDelta {
                index,
                id: str_field("/id"),
                name: str_field("/function/name"),
                arguments_delta: call
                    .pointer("/function/arguments")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            });
        }
    }
    Ok(out)
}

#[derive(Debug, Default)]
struct PartialCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

/// Ensambla los deltas de llamadas a herramientas en [`ToolCall`] completas.
#[derive(Debug, Default)]
pub struct ToolCallAccumulator {
    calls: BTreeMap<usize, PartialCall>,
}

impl ToolCallAccumulator {
    /// Crea un acumulador vacío.
    pub fn new() -> Self {
        Self::default()
    }

    /// Incorpora un chunk; ignora los que no son deltas de herramienta.
    pub fn push(&mut self, chunk: &StreamChunk) {
        if let StreamChunk::ToolCallDelta {
            index,
            id,
            name,
            arguments_delta,
        } = chunk
        {
            let entry = self.calls.entry(*index).or_default();
            if id.is_some() {
                entry.id.clone_from(id);
            }
            if name.is_some() {
                entry.name.clone_from(name);
            }
            entry.arguments.push_str(arguments_delta);
        }
    }

    /// Finaliza el ensamblado, validando nombre, id y JSON de argumentos.
    pub fn finish(self) -> Result<Vec<ToolCall>> {
        self.calls
            .into_iter()
            .map(|(index, p)| {
                let name = p.name.ok_or_else(|| {
                    AssistantError::Llm(format!("llamada {index} sin nombre de función"))
                })?;
                let id = p.id.unwrap_or_else(|| format!("call_{index}"));
                let arguments = if p.arguments.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&p.arguments)?
                };
                Ok(ToolCall {
                    id,
                    name,
                    arguments,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_done() -> Result<()> {
        let c = parse_event(r#"{"choices":[{"delta":{"content":"Hola"}}]}"#)?;
        assert_eq!(c, vec![StreamChunk::Text("Hola".into())]);
        assert_eq!(parse_event("[DONE]")?, vec![StreamChunk::Done]);
        Ok(())
    }

    #[test]
    fn accumulates_tool_calls() -> Result<()> {
        let mut acc = ToolCallAccumulator::new();
        for ev in [
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"read","arguments":""}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"/tmp\"}"}}]}}]}"#,
        ] {
            for chunk in parse_event(ev)? {
                acc.push(&chunk);
            }
        }
        let calls = acc.finish()?;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "c1");
        assert_eq!(calls[0].name, "read");
        assert_eq!(calls[0].arguments, json!({"path": "/tmp"}));
        Ok(())
    }

    #[test]
    fn gateway_error_is_typed() {
        let r = parse_event(r#"{"error":{"message":"rate limit"}}"#);
        assert!(matches!(r, Err(AssistantError::Llm(m)) if m == "rate limit"));
    }

    #[test]
    fn request_body_serializes_tool_flow() -> Result<()> {
        let call = ToolCall {
            id: "c1".into(),
            name: "read".into(),
            arguments: json!({"a": 1}),
        };
        let msgs = vec![
            ChatMessage::user("hi"),
            ChatMessage::assistant_tool_calls(vec![call]),
            ChatMessage::tool_result("c1", "ok"),
        ];
        let tools = vec![ToolDefinition {
            name: "read".into(),
            description: "d".into(),
            parameters: json!({"type": "object"}),
        }];
        let body = build_request_body("auto", &msgs, &tools)?;
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["messages"][1]["content"], Value::Null);
        assert_eq!(
            body["messages"][1]["tool_calls"][0]["function"]["arguments"],
            json!("{\"a\":1}")
        );
        assert_eq!(body["messages"][2]["tool_call_id"], json!("c1"));
        assert_eq!(body["tools"][0]["function"]["name"], json!("read"));
        Ok(())
    }
}
