//! Silero VAD (v5) sobre ONNX Runtime. Feature `silero`.

use std::path::Path;

use core_types::{AssistantError, Result};
use ort::session::Session;
use ort::value::Tensor;

use crate::vad::{VadModel, FRAME_SAMPLES, VAD_SAMPLE_RATE};

/// Muestras de contexto que Silero v5 antepone a cada frame.
const CONTEXT: usize = 64;
const STATE_LEN: usize = 2 * 128;

fn ort_err(what: &str, e: impl std::fmt::Display) -> AssistantError {
    AssistantError::Audio(format!("Silero VAD ({what}): {e}"))
}

/// Modelo Silero VAD con estado recurrente.
pub struct SileroVad {
    session: Session,
    state: Vec<f32>,
    context: Vec<f32>,
}

impl SileroVad {
    /// Carga `silero_vad.onnx`.
    pub fn new(model_path: impl AsRef<Path>) -> Result<Self> {
        let env = ort::init()
            .build()
            .map_err(|e| ort_err("entorno ONNX Runtime", e))?;
        let session = Session::builder(&env)
            .and_then(|mut b| b.commit_from_file(model_path.as_ref()))
            .map_err(|e| ort_err("cargar modelo", e))?;
        Ok(Self {
            session,
            state: vec![0.0; STATE_LEN],
            context: vec![0.0; CONTEXT],
        })
    }
}

impl VadModel for SileroVad {
    fn speech_probability(&mut self, frame: &[f32]) -> Result<f32> {
        if frame.len() != FRAME_SAMPLES {
            return Err(AssistantError::Audio(format!(
                "Silero VAD exige frames de {FRAME_SAMPLES} muestras, recibió {}",
                frame.len()
            )));
        }
        let mut input = Vec::with_capacity(CONTEXT + FRAME_SAMPLES);
        input.extend_from_slice(&self.context);
        input.extend_from_slice(frame);

        let input_t = Tensor::<f32>::from_array(([1usize, input.len()], input.clone()))
            .map_err(|e| ort_err("tensor input", e))?;
        let state_t = Tensor::<f32>::from_array(([2usize, 1, 128], self.state.clone()))
            .map_err(|e| ort_err("tensor state", e))?;
        let sr_t = Tensor::<i64>::from_array(([1usize], vec![i64::from(VAD_SAMPLE_RATE)]))
            .map_err(|e| ort_err("tensor sr", e))?;

        let outputs = self
            .session
            .run(ort::inputs! { "input" => input_t, "state" => state_t, "sr" => sr_t })
            .map_err(|e| ort_err("inferencia", e))?;

        let (_, prob) = outputs["output"]
            .try_extract_tensor::<f32>()
            .map_err(|e| ort_err("leer output", e))?;
        let (_, new_state) = outputs["stateN"]
            .try_extract_tensor::<f32>()
            .map_err(|e| ort_err("leer stateN", e))?;

        let p = prob.first().copied().unwrap_or(0.0);
        if new_state.len() == STATE_LEN {
            self.state.copy_from_slice(new_state);
        }
        self.context = input[input.len() - CONTEXT..].to_vec();
        Ok(p)
    }

    fn reset(&mut self) {
        self.state.fill(0.0);
        self.context.fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Requiere `models/silero_vad.onnx` (scripts/download-models.sh); si falta, se omite.
    #[test]
    fn silence_is_not_speech() -> Result<()> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../models/silero_vad.onnx");
        if !Path::new(path).exists() {
            return Ok(());
        }
        let mut vad = SileroVad::new(path)?;
        for _ in 0..5 {
            let p = vad.speech_probability(&[0.0; FRAME_SAMPLES])?;
            assert!((0.0..0.5).contains(&p), "p = {p}");
        }
        assert!(vad.speech_probability(&[0.0; 10]).is_err());
        Ok(())
    }
}
