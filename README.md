# ai-assistant

Asistente autónomo de alto rendimiento, de código abierto y multiplataforma (Windows, Linux, macOS), escrito en Rust.

- **Voz 100% local**: captura (`cpal`), VAD (Silero + ONNX Runtime), STT (whisper.cpp) y TTS (Piper + `rodio`). Baja latencia, sin coste por minuto.
- **Razonamiento en la nube**: todo el LLM pasa por el gateway local [OmniRoute](http://localhost:20128/v1) (OpenAI, Anthropic, Google...).
- **Herramientas vía MCP**: servidores Model Context Protocol por stdio (Google Workspace, Puppeteer, sistema de archivos, OmniRoute).
- **UI desacoplada**: Tauri v2 en la fase final; el backend solo expone canales IPC asíncronos.

> Estado: **Fase 1** (tipos base y workspace). Los demás crates son esqueletos.

## Arquitectura

```text
        ┌──────────────── LOCAL (sin internet) ────────────────┐
        │  Micrófono → cpal → Silero VAD → whisper.cpp (STT)   │
        │  Altavoz  ← rodio ← Piper (TTS, por chunks)          │
        └───────────────┬───────────────────────▲──────────────┘
                texto   │                       │ texto de respuesta
                        ▼                       │
              ┌──────────────────────────────────────┐
              │          agent-runtime (Rust)        │
              │   bucle: LLM → tool_calls → LLM ...  │
              └───────┬──────────────────────┬───────┘
          SSE / HTTP  │                      │ JSON-RPC (stdio)
                      ▼                      ▼
        ┌───────────────────────┐   ┌─────────────────────────────┐
        │ OmniRoute :20128/v1   │   │ Servidores MCP              │
        │  → OpenAI / Anthropic │   │  Google Workspace · Puppeteer│
        │  → Google (CLOUD)     │   │  Filesystem · OmniRoute     │
        └───────────────────────┘   └─────────────────────────────┘
```

## Estructura del monorepo

| Crate | Responsabilidad |
|---|---|
| `core-types` | Tipos, traits y errores compartidos |
| `llm-client` | Cliente OmniRoute (HTTP + SSE) |
| `mcp-client` | Orquestador MCP (JSON-RPC por stdio) |
| `voice-engine` | Pipeline de voz local |
| `agent-runtime` | Bucle agéntico |

## Requisitos previos

| Herramienta | Versión | Uso |
|---|---|---|
| Rust (stable) | ≥ 1.75 | Compilar el workspace |
| CMake | ≥ 3.20 | Compilar whisper.cpp (fase 4) |
| Node.js | ≥ 18 | Servidores MCP vía `npx` |
| OmniRoute | última | Gateway LLM en `localhost:20128` |

## Instalación rápida

```bash
# 1. Clonar
git clone https://github.com/OWNER/ai-assistant.git && cd ai-assistant

# 2. Compilar y validar
cargo check --workspace

# 3. Arrancar OmniRoute y revisar la config MCP
omniroute            # en otra terminal
$EDITOR .agents/mcp_config.json
```

La configuración MCP (`.agents/mcp_config.json`) restringe el servidor de archivos a carpetas de trabajo concretas; ajústalas a tu sistema. Si tu cliente no expande `${HOME}`, usa rutas absolutas.

## Hoja de ruta

- [x] **01** `feat/01-core-types-and-workspace`: tipos, workspace, documentación
- [ ] **02** `feat/02-cloud-llm-omniroute`: conector HTTP, parser SSE, tool calls
- [ ] **03** `feat/03-mcp-orchestrator`: subprocesos JSON-RPC y ejecución de herramientas
- [ ] **04** `feat/04-local-voice-engine`: cpal, Silero VAD, Whisper, Piper
- [ ] **05** `feat/05-agent-runtime`: bucle Voz → LLM → MCP → Voz
- [ ] **06** `feat/06-tauri-desktop-ui`: UI Tauri v2, atajo global, visor de estado

## Contribuir

Lee [CONTRIBUTING.md](CONTRIBUTING.md). Licencia [MIT](LICENSE).
