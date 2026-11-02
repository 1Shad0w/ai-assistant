# core-types

Único punto de verdad tipado del sistema.

## Responsabilidad
- **Mensajería**: `Role`, `ToolCall`, `ChatMessage`, `ToolDefinition`, `StreamChunk`.
- **Contratos asíncronos**: `LlmGateway`, `McpClient`, `VoiceTranscriber`, `VoiceSynthesizer`.
- **Errores**: `AssistantError` (LLM, MCP, audio, JSON, E/S) y alias `Result`.

## Dependencias
Solo `serde`, `serde_json`, `thiserror`, `async-trait` y `futures-core`.
Sin red, audio ni UI, para que cualquier crate (incluida la capa Tauri) pueda depender de él.

## Plataforma objetivo
Windows, Linux y macOS (Rust puro).

## Reglas
Sin `unwrap` (denegado por clippy) y sin `unsafe`.
