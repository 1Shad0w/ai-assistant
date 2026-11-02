# voice-engine

**Rama:** `feat/04-local-voice-engine`

## Objetivos técnicos
Pipeline de voz 100% local:
- Captura de micrófono con `cpal`.
- VAD con Silero sobre ONNX Runtime (`ort`).
- STT con whisper.cpp (`whisper-rs`) → `VoiceTranscriber`.
- TTS con Piper por chunks de texto + reproducción con `rodio` → `VoiceSynthesizer`.

## Dependencias previstas
`cpal`, `ort`, `whisper-rs` (requiere CMake), `rodio`, `tokio`, `core-types`.

## Plataforma objetivo
Windows, Linux (ALSA/PulseAudio/PipeWire) y macOS (CoreAudio).
