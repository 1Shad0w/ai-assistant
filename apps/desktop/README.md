# ai-assistant-desktop

**Rama:** `feat/06-tauri-desktop-ui`

Interfaz gráfica de escritorio de alto rendimiento para el asistente autónomo, construida con **Tauri v2** y frontend reactivo ultraligero con **Svelte 5** + **Vite**.

## Arquitectura y Desacoplamiento

La interfaz gráfica actúa únicamente como cliente de visualización y control, completamente desacoplada de la lógica del asistente:
- **Comunicación Asíncrona (Eventos)**: El backend (`agent-runtime`) emite eventos mediante un bus de difusión (`tokio::sync::broadcast`) que Tauri retransmite al frontend a través del canal de eventos `agent-event`.
- **Comandos IPC**: La ventana frontend invoca comandos nativos de Tauri (`send_message`, `set_listening`, `toggle_listening`, `capabilities`) sin bloquear el hilo principal.
- **Atajo Global de Teclado**: Soporte para activación/desactivación del micrófono mediante `Ctrl+Shift+Espacio` (Linux/Windows) o `Cmd+Shift+Espacio` (macOS) a través de `tauri-plugin-global-shortcut`.
- **Indicador Visual de Estado**: Un visualizador ("orbe") pulsante en tiempo real que refleja el ciclo de vida del agente:
  - Reposo (`idle`)
  - Escuchando micrófono (`listening`)
  - Transcribiendo audio STT (`transcribing`)
  - Razonando LLM / Cloud (`thinking`)
  - Ejecutando herramienta MCP (`using_tool`)
  - Sintetizando respuesta de voz TTS (`speaking`)

## Requisitos del Sistema

- **Node.js**: >= 18 (para desarrollo web y bundler Vite)
- **Librerías de sistema (Linux)**:
  - `webkit2gtk-4.1`
  - `libsoup-3.0`
  - `gtk+-3.0`
  - `librsvg-2.0`
- **Modelos locales de voz**: Descargados previamente mediante `scripts/download-models.sh` (si se usa la característica `--features voice`).

## Ejecución en Desarrollo

```bash
# 1. Asegurar dependencias de Node
cd apps/desktop
npm install

# 2. Ejecutar entorno de desarrollo con hot-reload
npm run tauri dev
```

Para compilar con soporte de voz completo:
```bash
npm run tauri dev -- --features voice
```

## Compilación para Producción (Instalador / Binario)

```bash
cd apps/desktop
npm run tauri build
```
Los ejecutables e instaladores específicos de plataforma (`.deb`, `.AppImage`, `.msi`, `.dmg`) se generan en `apps/desktop/src-tauri/target/release/bundle/`.
