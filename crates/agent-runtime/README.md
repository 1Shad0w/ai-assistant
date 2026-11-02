# agent-runtime

**Rama:** `feat/05-agent-runtime`

Bucle agéntico: Voz → Transcripción → LLM → herramientas MCP (hasta respuesta final) → síntesis de voz.

## Piezas
- `Agent`: turno LLM↔herramientas con límite de rondas, historial recortado por turnos y *rollback* si el turno falla. Los fallos de herramientas se devuelven al LLM para que se recupere.
- `AgentEvent` / `AgentState`: eventos serializables por `tokio::sync::broadcast`. Es la única vía de salida hacia la UI.
- `run_voice_loop` + `VoiceControl`: bucle de voz con sintetizado por frases en paralelo al streaming del LLM, escucha activable/desactivable (atajo global) y apagado limpio.
- Binario `assistant`: modo texto por defecto, `--voice` para voz.

## Uso
```bash
cargo run -p agent-runtime                      # modo texto
cargo run -p agent-runtime --features voice -- --voice   # voz (requiere CMake y modelos)
```
Features: `voice-io` (mic + altavoz + Silero) y `voice` (añade Whisper).
Variables: `OMNIROUTE_BASE_URL`, `OMNIROUTE_API_KEY`, `OMNIROUTE_MODEL`, `PIPER_BIN`, `PIPER_VOICE`.

## Dependencias
`core-types`, `llm-client`, `mcp-client`, `voice-engine`, `tokio`, `anyhow`, `tracing`.

## Plataforma objetivo
Windows, Linux y macOS.
