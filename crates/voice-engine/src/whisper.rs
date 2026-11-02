//! Transcripción local con whisper.cpp (vía `whisper-rs`). Feature `whisper`.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use core_types::{AssistantError, Result, VoiceTranscriber};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::resample_linear;
use crate::vad::VAD_SAMPLE_RATE;

/// Transcriptor Whisper. Cargar el modelo es costoso: crea una sola instancia.
#[derive(Clone)]
pub struct WhisperTranscriber {
    ctx: Arc<WhisperContext>,
    language: Option<String>,
    threads: i32,
}

impl WhisperTranscriber {
    /// Carga un modelo `ggml-*.bin`. `language`: `Some("es")` o `None` (auto).
    pub fn new(model_path: impl AsRef<Path>, language: Option<String>) -> Result<Self> {
        let path = model_path.as_ref().to_string_lossy().into_owned();
        let ctx = WhisperContext::new_with_params(&path, WhisperContextParameters::default())
            .map_err(|e| {
                AssistantError::Audio(format!("no se pudo cargar el modelo Whisper `{path}`: {e}"))
            })?;
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4);
        Ok(Self {
            ctx: Arc::new(ctx),
            language,
            threads: i32::try_from(threads).unwrap_or(4),
        })
    }
}

#[async_trait]
impl VoiceTranscriber for WhisperTranscriber {
    async fn transcribe(&self, samples: Vec<f32>, sample_rate: u32) -> Result<String> {
        let ctx = Arc::clone(&self.ctx);
        let language = self.language.clone();
        let threads = self.threads;

        tokio::task::spawn_blocking(move || -> Result<String> {
            let audio = resample_linear(&samples, sample_rate, VAD_SAMPLE_RATE);
            let mut state = ctx
                .create_state()
                .map_err(|e| AssistantError::Audio(format!("estado Whisper: {e}")))?;

            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            params.set_n_threads(threads);
            params.set_language(language.as_deref());
            params.set_translate(false);
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);

            state
                .full(params, &audio)
                .map_err(|e| AssistantError::Audio(format!("transcripción Whisper: {e}")))?;

            let mut text = String::new();
            for segment in state.as_iter() {
                let piece = segment
                    .to_str_lossy()
                    .map_err(|e| AssistantError::Audio(format!("segmento Whisper: {e}")))?;
                text.push_str(&piece);
            }
            Ok(text.trim().to_owned())
        })
        .await
        .map_err(|e| AssistantError::Audio(format!("tarea de transcripción: {e}")))?
    }
}
