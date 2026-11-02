//! Detección de actividad de voz (VAD) y segmentación de enunciados.

use std::collections::VecDeque;

use core_types::Result;

/// Frecuencia de muestreo que esperan VAD y STT.
pub const VAD_SAMPLE_RATE: u32 = 16_000;
/// Tamaño de frame del VAD (32 ms a 16 kHz, el que exige Silero).
pub const FRAME_SAMPLES: usize = 512;
const FRAME_MS: u32 = 32;

/// Modelo que estima la probabilidad de voz de un frame de [`FRAME_SAMPLES`].
pub trait VadModel: Send {
    /// Probabilidad `[0, 1]` de que el frame contenga voz.
    fn speech_probability(&mut self, frame: &[f32]) -> Result<f32>;

    /// Reinicia el estado interno entre enunciados.
    fn reset(&mut self) {}
}

/// VAD por energía RMS: sin dependencias, útil como alternativa ligera.
#[derive(Debug, Clone)]
pub struct EnergyVad {
    threshold_rms: f32,
}

impl EnergyVad {
    /// `threshold_rms` es el RMS a partir del cual se considera voz (≈0.01–0.02).
    pub fn new(threshold_rms: f32) -> Self {
        Self {
            threshold_rms: threshold_rms.max(f32::EPSILON),
        }
    }
}

impl Default for EnergyVad {
    fn default() -> Self {
        Self::new(0.015)
    }
}

impl VadModel for EnergyVad {
    fn speech_probability(&mut self, frame: &[f32]) -> Result<f32> {
        if frame.is_empty() {
            return Ok(0.0);
        }
        let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
        // El umbral cae en p = 0.5.
        Ok((rms / (2.0 * self.threshold_rms)).min(1.0))
    }
}

/// Parámetros del segmentador.
#[derive(Debug, Clone)]
pub struct SegmenterConfig {
    /// Probabilidad mínima para considerar un frame como voz.
    pub speech_threshold: f32,
    /// Silencio continuo que cierra un enunciado (ms).
    pub min_silence_ms: u32,
    /// Voz mínima para aceptar un enunciado y descartar clics (ms).
    pub min_speech_ms: u32,
    /// Audio previo al inicio de la voz que se conserva (ms).
    pub preroll_ms: u32,
    /// Duración máxima de un enunciado (ms).
    pub max_utterance_ms: u32,
}

impl Default for SegmenterConfig {
    fn default() -> Self {
        Self {
            speech_threshold: 0.5,
            min_silence_ms: 700,
            min_speech_ms: 250,
            preroll_ms: 200,
            max_utterance_ms: 30_000,
        }
    }
}

/// Convierte un flujo continuo de audio 16 kHz mono en enunciados completos.
pub struct UtteranceSegmenter<V: VadModel> {
    vad: V,
    cfg: SegmenterConfig,
    pending: Vec<f32>,
    preroll: VecDeque<Vec<f32>>,
    utterance: Vec<f32>,
    in_speech: bool,
    speech_frames: u32,
    silence_frames: u32,
}

impl<V: VadModel> UtteranceSegmenter<V> {
    /// Crea el segmentador.
    pub fn new(vad: V, cfg: SegmenterConfig) -> Self {
        Self {
            vad,
            cfg,
            pending: Vec::new(),
            preroll: VecDeque::new(),
            utterance: Vec::new(),
            in_speech: false,
            speech_frames: 0,
            silence_frames: 0,
        }
    }

    /// Alimenta audio y devuelve los enunciados completados (normalmente 0 o 1).
    pub fn push(&mut self, samples: &[f32]) -> Result<Vec<Vec<f32>>> {
        self.pending.extend_from_slice(samples);
        let mut done = Vec::new();
        while self.pending.len() >= FRAME_SAMPLES {
            let frame: Vec<f32> = self.pending.drain(..FRAME_SAMPLES).collect();
            if let Some(u) = self.process_frame(frame)? {
                done.push(u);
            }
        }
        Ok(done)
    }

    /// Cierra el enunciado en curso (p. ej. al terminar la captura).
    pub fn flush(&mut self) -> Option<Vec<f32>> {
        self.pending.clear();
        self.finish()
    }

    fn frames(ms: u32) -> u32 {
        ms.div_ceil(FRAME_MS)
    }

    fn process_frame(&mut self, frame: Vec<f32>) -> Result<Option<Vec<f32>>> {
        let is_speech = self.vad.speech_probability(&frame)? >= self.cfg.speech_threshold;

        if !self.in_speech {
            if is_speech {
                self.in_speech = true;
                self.speech_frames = 1;
                self.silence_frames = 0;
                self.utterance = self.preroll.drain(..).flatten().collect();
                self.utterance.extend_from_slice(&frame);
            } else {
                self.preroll.push_back(frame);
                while self.preroll.len() > Self::frames(self.cfg.preroll_ms) as usize {
                    self.preroll.pop_front();
                }
            }
            return Ok(None);
        }

        self.utterance.extend_from_slice(&frame);
        if is_speech {
            self.speech_frames += 1;
            self.silence_frames = 0;
        } else {
            self.silence_frames += 1;
        }
        let too_long = self.utterance.len() >= (self.cfg.max_utterance_ms as usize) * 16;
        if self.silence_frames >= Self::frames(self.cfg.min_silence_ms) || too_long {
            return Ok(self.finish());
        }
        Ok(None)
    }

    fn finish(&mut self) -> Option<Vec<f32>> {
        let valid = self.in_speech && self.speech_frames >= Self::frames(self.cfg.min_speech_ms);
        let utterance = std::mem::take(&mut self.utterance);
        self.in_speech = false;
        self.speech_frames = 0;
        self.silence_frames = 0;
        self.preroll.clear();
        self.vad.reset();
        valid.then_some(utterance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize) -> Vec<f32> {
        (0..frames * FRAME_SAMPLES)
            .map(|i| 0.3 * (i as f32 * 0.2).sin())
            .collect()
    }

    fn silence(frames: usize) -> Vec<f32> {
        vec![0.0; frames * FRAME_SAMPLES]
    }

    #[test]
    fn segments_speech_between_silences() -> Result<()> {
        let mut seg = UtteranceSegmenter::new(EnergyVad::default(), SegmenterConfig::default());
        let mut audio = silence(20);
        audio.extend(tone(30)); // ~960 ms de voz
        audio.extend(silence(40)); // > 700 ms de silencio
        let out = seg.push(&audio)?;
        assert_eq!(out.len(), 1);
        // Contiene preroll + voz + silencio de cierre.
        assert!(out[0].len() >= 30 * FRAME_SAMPLES);
        Ok(())
    }

    #[test]
    fn discards_very_short_noise() -> Result<()> {
        let mut seg = UtteranceSegmenter::new(EnergyVad::default(), SegmenterConfig::default());
        let mut audio = silence(10);
        audio.extend(tone(2)); // 64 ms
        audio.extend(silence(40));
        assert!(seg.push(&audio)?.is_empty());
        Ok(())
    }

    #[test]
    fn works_with_arbitrary_chunk_sizes_and_flush() -> Result<()> {
        let mut seg = UtteranceSegmenter::new(EnergyVad::default(), SegmenterConfig::default());
        let mut audio = silence(5);
        audio.extend(tone(30));
        let mut got = Vec::new();
        for chunk in audio.chunks(333) {
            got.extend(seg.push(chunk)?);
        }
        assert!(got.is_empty());
        assert!(seg.flush().is_some());
        Ok(())
    }
}
