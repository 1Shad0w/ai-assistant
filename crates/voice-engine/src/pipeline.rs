//! Orquestación de alto nivel: escuchar un enunciado y hablar por frases.

use core_types::{Result, VoiceSynthesizer};
use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::text::split_sentences;
use crate::vad::{UtteranceSegmenter, VadModel};

/// Destino de audio sintetizado (altavoz, fichero, mock de test...).
pub trait AudioSink: Send + Sync {
    /// Encola muestras `f32` mono a `sample_rate` Hz sin bloquear.
    fn play(&self, samples: Vec<f32>, sample_rate: u32) -> Result<()>;

    /// Descarta el audio encolado (interrupción del usuario / *barge-in*).
    fn clear(&self) {}
}

/// Espera hasta completar un enunciado del usuario.
///
/// `frames` entrega audio mono a 16 kHz (ver [`crate::VAD_SAMPLE_RATE`]).
/// Devuelve `None` si el canal se cierra sin enunciado pendiente.
pub async fn listen_utterance<V: VadModel>(
    frames: &mut mpsc::Receiver<Vec<f32>>,
    segmenter: &mut UtteranceSegmenter<V>,
) -> Result<Option<Vec<f32>>> {
    while let Some(chunk) = frames.recv().await {
        if let Some(utterance) = segmenter.push(&chunk)?.into_iter().next() {
            return Ok(Some(utterance));
        }
    }
    Ok(segmenter.flush())
}

/// Sintetiza `text` frase a frase y envía cada fragmento al `sink` según llega.
pub async fn speak<S: VoiceSynthesizer + ?Sized>(
    synthesizer: &S,
    sink: &dyn AudioSink,
    sample_rate: u32,
    text: &str,
) -> Result<()> {
    for sentence in split_sentences(text) {
        speak_sentence(synthesizer, sink, sample_rate, &sentence).await?;
    }
    Ok(())
}

/// Sintetiza y reproduce una única frase (para uso con `SentenceChunker`).
pub async fn speak_sentence<S: VoiceSynthesizer + ?Sized>(
    synthesizer: &S,
    sink: &dyn AudioSink,
    sample_rate: u32,
    sentence: &str,
) -> Result<()> {
    let mut audio = synthesizer.synthesize(sentence).await?;
    while let Some(chunk) = audio.next().await {
        let samples = chunk?;
        if !samples.is_empty() {
            sink.play(samples, sample_rate)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vad::{EnergyVad, SegmenterConfig, FRAME_SAMPLES};
    use async_trait::async_trait;
    use core_types::AssistantError;
    use core_types::BoxStream;
    use futures_util::stream;
    use std::sync::Mutex;

    struct FakeTts;

    #[async_trait]
    impl VoiceSynthesizer for FakeTts {
        async fn synthesize(&self, text: &str) -> Result<BoxStream<'static, Result<Vec<f32>>>> {
            let n = text.len();
            Ok(Box::pin(stream::iter(vec![
                Ok(vec![0.1; n]),
                Ok(vec![0.2; n]),
            ])))
        }
    }

    #[derive(Default)]
    struct Recorder(Mutex<Vec<usize>>);

    impl AudioSink for Recorder {
        fn play(&self, samples: Vec<f32>, _rate: u32) -> Result<()> {
            self.0
                .lock()
                .map_err(|_| AssistantError::Audio("mutex envenenado".into()))?
                .push(samples.len());
            Ok(())
        }
    }

    #[tokio::test]
    async fn speaks_sentence_by_sentence() -> Result<()> {
        let sink = Recorder::default();
        speak(&FakeTts, &sink, 22_050, "Hola. Adiós.").await?;
        let got = sink
            .0
            .lock()
            .map_err(|_| AssistantError::Audio("mutex envenenado".into()))?
            .clone();
        assert_eq!(got, vec![5, 5, 7, 7]);
        Ok(())
    }

    #[tokio::test]
    async fn listens_until_utterance_completes() -> Result<()> {
        let (tx, mut rx) = mpsc::channel(64);
        let mut seg = UtteranceSegmenter::new(EnergyVad::default(), SegmenterConfig::default());
        let tone: Vec<f32> = (0..30 * FRAME_SAMPLES)
            .map(|i| 0.3 * (i as f32 * 0.2).sin())
            .collect();
        tx.send(tone)
            .await
            .map_err(|_| AssistantError::Audio("canal cerrado".into()))?;
        tx.send(vec![0.0; 40 * FRAME_SAMPLES])
            .await
            .map_err(|_| AssistantError::Audio("canal cerrado".into()))?;
        let u = listen_utterance(&mut rx, &mut seg).await?;
        assert!(u.is_some());
        Ok(())
    }
}
