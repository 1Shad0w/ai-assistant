//! Montaje del modo de voz con componentes reales (feature `voice-io`).

use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use core_types::VoiceTranscriber;
use voice_engine::{
    MicCapture, PiperConfig, PiperSynthesizer, RodioSink, SegmenterConfig, SileroVad,
    UtteranceSegmenter,
};

use crate::agent::Agent;
use crate::voice_loop::{run_voice_loop, VoiceComponents, VoiceControl};

/// Arranca captura, VAD, Piper y altavoz, y ejecuta el bucle de voz.
///
/// Espera en `models_dir`: `silero_vad.onnx` y `es_ES-davefx-medium.onnx`
/// (ver `scripts/download-models.sh`). `PIPER_BIN` y `PIPER_VOICE` permiten
/// cambiar el ejecutable y la voz.
pub async fn run<T>(
    agent: &mut Agent,
    transcriber: T,
    models_dir: &Path,
    control: &VoiceControl,
) -> anyhow::Result<()>
where
    T: VoiceTranscriber + 'static,
{
    let vad = SileroVad::new(models_dir.join("silero_vad.onnx"))
        .context("cargando Silero VAD (¿ejecutaste scripts/download-models.sh?)")?;
    let segmenter = UtteranceSegmenter::new(vad, SegmenterConfig::default());

    let (_mic, frames) = MicCapture::start().context("abriendo el micrófono")?;
    let sink = RodioSink::new().context("abriendo el altavoz")?;

    let voice_model = std::env::var("PIPER_VOICE")
        .map(Into::into)
        .unwrap_or_else(|_| models_dir.join("es_ES-davefx-medium.onnx"));
    let mut piper_cfg = PiperConfig::new(voice_model);
    if let Ok(bin) = std::env::var("PIPER_BIN") {
        piper_cfg.command = bin;
    }
    let synthesizer = PiperSynthesizer::new(piper_cfg);
    let tts_sample_rate = synthesizer.sample_rate();

    let voice = VoiceComponents {
        segmenter,
        frames,
        transcriber: Arc::new(transcriber),
        synthesizer: Arc::new(synthesizer),
        sink: Arc::new(sink),
        tts_sample_rate,
    };
    run_voice_loop(agent, voice, control).await?;
    Ok(())
}
