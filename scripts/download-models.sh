#!/usr/bin/env bash
# Descarga los modelos ligeros de voz local en ./models (ignorado por git).
#   Silero VAD (~2 MB) · Whisper base q5_1 multilingüe (~57 MB) · Piper es_ES davefx medium (~60 MB)
# Uso: scripts/download-models.sh [tiny|base]   (Whisper; por defecto base)
set -euo pipefail

WHISPER_SIZE="${1:-base}"
DIR="$(cd "$(dirname "$0")/.." && pwd)/models"
mkdir -p "$DIR"

fetch() { # url destino
  if [ -s "$2" ]; then echo "ya existe: $2"; else echo "descargando $2"; curl -fL --retry 3 -o "$2.part" "$1" && mv "$2.part" "$2"; fi
}

fetch "https://raw.githubusercontent.com/snakers4/silero-vad/master/src/silero_vad/data/silero_vad.onnx" "$DIR/silero_vad.onnx"
fetch "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-${WHISPER_SIZE}-q5_1.bin" "$DIR/ggml-${WHISPER_SIZE}-q5_1.bin"
V="https://huggingface.co/rhasspy/piper-voices/resolve/main/es/es_ES/davefx/medium"
fetch "$V/es_ES-davefx-medium.onnx" "$DIR/es_ES-davefx-medium.onnx"
fetch "$V/es_ES-davefx-medium.onnx.json" "$DIR/es_ES-davefx-medium.onnx.json"

echo "Listo. Piper se instala aparte: pip install piper-tts  (o descarga el binario de su release)"
