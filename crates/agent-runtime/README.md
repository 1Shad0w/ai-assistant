# agent-runtime

**Rama:** `feat/05-agent-runtime`

## Objetivos técnicos
Bucle agéntico: Voz → Transcripción → LLM → ejecución de herramientas MCP (repetir hasta respuesta final) → síntesis de voz.
Expone eventos de estado y canales asíncronos (`tokio::sync`) para que la UI (Tauri, fase 6) se conecte sin acoplamiento.

## Dependencias previstas
`tokio`, `tracing`, `anyhow`, `core-types`, `llm-client`, `mcp-client`, `voice-engine`.

## Plataforma objetivo
Windows, Linux y macOS.
