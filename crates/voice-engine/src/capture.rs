//! Captura de micrófono con `cpal`. Feature `mic`.

use std::sync::mpsc as std_mpsc;
use std::thread::JoinHandle;

use core_types::{AssistantError, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use tokio::sync::mpsc;

use crate::audio::{downmix_to_mono, resample_linear};
use crate::vad::VAD_SAMPLE_RATE;

fn audio_err(what: &str, e: impl std::fmt::Display) -> AssistantError {
    AssistantError::Audio(format!("micrófono ({what}): {e}"))
}

/// Captura continua del micrófono por defecto, entregada como 16 kHz mono.
///
/// El `Stream` de cpal no es `Send` en todas las plataformas, así que vive en
/// un hilo dedicado que termina al soltar este manejador.
pub struct MicCapture {
    stop: Option<std_mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl MicCapture {
    /// Abre el micrófono y devuelve el receptor de audio (16 kHz mono `f32`).
    ///
    /// Si el consumidor va lento se descartan bloques en lugar de bloquear el
    /// callback de audio en tiempo real.
    pub fn start() -> Result<(Self, mpsc::Receiver<Vec<f32>>)> {
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>(256);
        let (ready_tx, ready_rx) = std_mpsc::channel::<Result<()>>();
        let (stop_tx, stop_rx) = std_mpsc::channel::<()>();

        let thread = std::thread::Builder::new()
            .name("mic-capture".into())
            .spawn(move || match build_stream(audio_tx) {
                Ok(stream) => {
                    let _ = ready_tx.send(Ok(()));
                    // Bloquea hasta que se pida parar (o se suelte el manejador).
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            })
            .map_err(|e| audio_err("hilo", e))?;

        ready_rx
            .recv()
            .map_err(|_| audio_err("inicio", "el hilo terminó inesperadamente"))??;

        Ok((
            Self {
                stop: Some(stop_tx),
                thread: Some(thread),
            },
            audio_rx,
        ))
    }
}

impl Drop for MicCapture {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn build_stream(tx: mpsc::Sender<Vec<f32>>) -> Result<cpal::Stream> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| audio_err("dispositivo", "no hay micrófono por defecto"))?;
    let config = device
        .default_input_config()
        .map_err(|e| audio_err("configuración", e))?;

    let channels = usize::from(config.channels());
    let rate = config.sample_rate();
    let format = config.sample_format();

    let forward = move |mono_f32: Vec<f32>| {
        let mono = downmix_to_mono(&mono_f32, channels);
        let out = resample_linear(&mono, rate, VAD_SAMPLE_RATE);
        let _ = tx.try_send(out);
    };
    let on_error = |e: cpal::Error| tracing::warn!("error en el flujo del micrófono: {e}");

    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(
            config.into(),
            move |data: &[f32], _: &_| forward(data.to_vec()),
            on_error,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            config.into(),
            move |data: &[i16], _: &_| {
                forward(data.iter().map(|s| f32::from(*s) / 32768.0).collect());
            },
            on_error,
            None,
        ),
        other => {
            return Err(audio_err(
                "formato",
                format!("formato de muestra no soportado: {other:?}"),
            ))
        }
    }
    .map_err(|e| audio_err("abrir flujo", e))?;

    stream.play().map_err(|e| audio_err("iniciar", e))?;
    Ok(stream)
}
