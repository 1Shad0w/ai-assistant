# llm-client

**Rama:** `feat/02-cloud-llm-omniroute`

## Objetivos técnicos
- Implementar `LlmGateway` contra OmniRoute (`http://localhost:20128/v1`, API compatible con OpenAI).
- Parser de streaming SSE (`data: ...` / `[DONE]`) hacia `StreamChunk`.
- Ensamblado de deltas de llamadas a herramientas, reintentos y timeouts.

## Dependencias previstas
`reqwest` (rustls, stream), `tokio`, `futures`, `eventsource-stream`, `serde_json`, `core-types`.

## Plataforma objetivo
Windows, Linux y macOS.
