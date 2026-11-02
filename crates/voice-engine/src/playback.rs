//! Reproducción con `rodio`. Feature `playback`.

use std::num::NonZero;
use std::sync::mpsc as std_mpsc;
use std::thread::JoinHandle;

use async_trait::async_trait;
use core_types::{AssistantError, Result};
use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, Player};

use crate::pipeline::AudioSink;

fn closed(what: &str) -> AssistantError {
    AssistantError::Audio(format!("{what}: el hilo de audio terminó"))
}

enum Command {
    Play(Vec<f32>, u32),
    Clear,
    Drain(std_mpsc::Sender<()>),
}

/// Altavoz por defecto. El dispositivo de salida vive en un hilo propio.
pub struct RodioSink {
    tx: Option<std_mpsc::Sender<Command>>,
    thread: Option<JoinHandle<()>>,
}

impl RodioSink {
    /// Abre el dispositivo de salida por defecto.
    pub fn new() -> Result<Self> {
        let (tx, rx) = std_mpsc::channel::<Command>();
        let (ready_tx, ready_rx) = std_mpsc::channel::<Result<()>>();

        let thread = std::thread::Builder::new()
            .name("audio-playback".into())
            .spawn(move || {
                let device = match DeviceSinkBuilder::open_default_sink() {
                    Ok(d) => d,
                    Err(e) => {
                        let _ = ready_tx.send(Err(AssistantError::Audio(format!(
                            "no se pudo abrir el altavoz: {e}"
                        ))));
                        return;
                    }
                };
                let player = Player::connect_new(device.mixer());
                let _ = ready_tx.send(Ok(()));

                let channels = NonZero::<u16>::MIN;
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        Command::Play(samples, rate) => {
                            if let Some(rate) = NonZero::new(rate) {
                                player.append(SamplesBuffer::new(channels, rate, samples));
                            }
                        }
                        Command::Clear => player.clear(),
                        Command::Drain(done) => {
                            player.sleep_until_end();
                            let _ = done.send(());
                        }
                    }
                }
            })
            .map_err(|e| AssistantError::Audio(format!("hilo de reproducción: {e}")))?;

        ready_rx.recv().map_err(|_| closed("reproducción"))??;
        Ok(Self {
            tx: Some(tx),
            thread: Some(thread),
        })
    }

    fn send(&self, cmd: Command) -> Result<()> {
        self.tx
            .as_ref()
            .ok_or_else(|| closed("reproducción"))?
            .send(cmd)
            .map_err(|_| closed("reproducción"))
    }
}

#[async_trait]
impl AudioSink for RodioSink {
    fn play(&self, samples: Vec<f32>, sample_rate: u32) -> Result<()> {
        self.send(Command::Play(samples, sample_rate))
    }

    fn clear(&self) {
        let _ = self.send(Command::Clear);
    }

    async fn drain(&self) -> Result<()> {
        let (done_tx, done_rx) = std_mpsc::channel();
        self.send(Command::Drain(done_tx))?;
        tokio::task::spawn_blocking(move || done_rx.recv())
            .await
            .map_err(|e| AssistantError::Audio(format!("espera de reproducción: {e}")))?
            .map_err(|_| closed("reproducción"))
    }
}

impl Drop for RodioSink {
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
