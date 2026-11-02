# voice-engine

**Rama:** `feat/04-local-voice-engine`

Pipeline de voz 100% local: micrófono → VAD → STT, y texto → TTS → altavoz.

## Modelos ligeros elegidos

| Etapa | Modelo | Tamaño | Notas |
|---|---|---|---|
| VAD | Silero VAD v5 (ONNX) | ~2 MB | Vía ONNX Runtime (`ort`) |
| STT | Whisper `base` q5_1 (whisper.cpp) | ~57 MB | Multilingüe (español). `tiny` (~31 MB) si hay poca CPU |
| TTS | Piper `es_ES-davefx-medium` | ~60 MB | Voz en español, 22 050 Hz |

Descarga con `scripts/download-models.sh [tiny|base]` (van a `models/`, ignorado por git).
Piper se instala aparte (`pip install piper-tts` o el binario de su release) y se invoca como subproceso.

## Arquitectura

| Módulo | Feature | Contenido |
|---|---|---|
| `audio`, `vad`, `text`, `piper`, `pipeline` | siempre | Remuestreo, VAD por energía, segmentador de enunciados, troceado en frases, Piper, `listen_utterance` / `speak` |
| `capture` | `mic` | Micrófono con `cpal` (entrega 16 kHz mono) |
| `playback` | `playback` | Altavoz con `rodio` (cola, `clear` para interrumpir, `drain`) |
| `silero` | `silero` | `SileroVad` (ONNX Runtime) |
| `whisper` | `whisper` | `WhisperTranscriber` (whisper.cpp, requiere CMake) |

`cargo check` sin features no necesita dependencias nativas. `--features full` activa todo.

> [!NOTE]
> El módulo `whisper` aún no se ha podido compilar en CI local por falta de CMake: valídalo con `cargo check -p voice-engine --features whisper` tras instalar CMake.

## Plataforma objetivo
Windows, Linux (ALSA/PulseAudio/PipeWire; requiere `libasound2-dev`) y macOS (CoreAudio).

## Ejemplo

```rust,ignore
let (_mic, mut frames) = MicCapture::start()?;
let mut seg = UtteranceSegmenter::new(SileroVad::new("models/silero_vad.onnx")?, Default::default());
let stt = WhisperTranscriber::new("models/ggml-base-q5_1.bin", Some("es".into()))?;
if let Some(audio) = listen_utterance(&mut frames, &mut seg).await? {
    let text = stt.transcribe(audio, VAD_SAMPLE_RATE).await?;
}
```
