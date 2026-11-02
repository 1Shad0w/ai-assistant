# ai-assistant

Asistente autónomo de alto rendimiento, de código abierto y multiplataforma (Windows, Linux, macOS), escrito en Rust.

- **Voz 100% local**: captura (`cpal`), VAD (Silero + ONNX Runtime), STT (whisper.cpp) y TTS (Piper + `rodio`). Baja latencia, sin coste por minuto.
- **Razonamiento en la nube**: todo el LLM pasa por el gateway local [OmniRoute](http://localhost:20128/v1) (OpenAI, Anthropic, Google...).
- **Herramientas vía MCP**: servidores Model Context Protocol por stdio (Google Workspace, Puppeteer, sistema de archivos, OmniRoute).
- **UI desacoplada**: Interfaz gráfica de escritorio con Tauri v2 y Svelte 5; el backend de Rust no depende de la UI y se comunica exclusivamente por IPC asíncrono y bus de eventos.

> Estado: **Fase 6** (interfaz de escritorio Tauri v2 completada). Arquitectura y pipeline completos.

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

| Directorio | Responsabilidad |
|---|---|
| `crates/core-types` | Tipos, traits y errores compartidos (punto único de verdad) |
| `crates/llm-client` | Cliente OmniRoute (HTTP + SSE streaming + acumulación de tools) |
| `crates/mcp-client` | Orquestador MCP (JSON-RPC por stdio y enrutamiento con namespace) |
| `crates/voice-engine` | Pipeline de voz 100% local (cpal, Silero VAD, Whisper, Piper, rodio) |
| `crates/agent-runtime` | Bucle agéntico autónomo, bus de eventos y binario CLI `assistant` |
| `apps/desktop` | Aplicación de escritorio multiplataforma (Tauri v2 + Svelte 5 + Vite) |

## Requisitos previos

| Herramienta | Versión | Uso |
|---|---|---|
| Rust (stable) | ≥ 1.88 | Compilar el workspace y Tauri |
| CMake | ≥ 3.20 | Compilar whisper.cpp (fase 4 / modo voz) |
| Node.js | ≥ 18 | Servidores MCP vía `npx` y frontend Vite/Svelte |
| OmniRoute | última | Gateway LLM en `localhost:20128` |

## Instalación rápida y ejecución

```bash
# 1. Clonar el repositorio
git clone https://github.com/1Shad0w/ai-assistant.git && cd ai-assistant

# 2. Compilar crates principales y descargar modelos de voz locales (~120 MB)
cargo check --workspace
scripts/download-models.sh

# 3. Arrancar OmniRoute en otra terminal
omniroute

# 4. Modo CLI (Consola)
cargo run -p agent-runtime                      # Modo texto interactivo
cargo run -p agent-runtime --features voice -- --voice   # Modo voz

# 5. Modo Escritorio (Tauri v2 UI)
cd apps/desktop
npm install
npm run tauri dev                              # Modo texto / dev
npm run tauri dev -- --features voice          # Con captura y síntesis de voz
```

La configuración MCP (`.agents/mcp_config.json`) restringe el servidor de archivos a carpetas de trabajo concretas; ajústalas a tu sistema. Si tu cliente no expande `${HOME}`, usa rutas absolutas.

## Hoja de ruta

- [x] **01** `feat/01-core-types-and-workspace`: tipos, workspace, contratos y documentación
- [x] **02** `feat/02-cloud-llm-omniroute`: conector HTTP, parser SSE, ensamblador de tool calls
- [x] **03** `feat/03-mcp-orchestrator`: subprocesos JSON-RPC y ejecución de herramientas con namespaces
- [x] **04** `feat/04-local-voice-engine`: cpal, Silero VAD, Whisper STT, Piper TTS, rodio
- [x] **05** `feat/05-agent-runtime`: bucle Voz → LLM → herramientas MCP → Síntesis de voz
- [x] **06** `feat/06-tauri-desktop-ui`: UI de escritorio con Tauri v2, atajo global (`Ctrl+Shift+Espacio`) y visualizador reactivo de estado

## Contribuir

Lee [CONTRIBUTING.md](CONTRIBUTING.md). Licencia [MIT](LICENSE).

