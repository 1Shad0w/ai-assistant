//! Síntesis de voz local con Piper TTS (binario externo, salida PCM cruda).
//!
//! Piper se invoca como subproceso: `piper --model <voz.onnx> --output-raw`
//! lee texto por stdin y escribe PCM `s16le` mono por stdout. Esto evita
//! enlazar `espeak-ng` y ONNX en el proceso del asistente.

use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use core_types::{AssistantError, BoxStream, Result, VoiceSynthesizer};
use futures_util::stream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdout, Command};

use crate::audio::s16le_to_f32;

/// Configuración de Piper.
#[derive(Debug, Clone)]
pub struct PiperConfig {
    /// Ejecutable de Piper (`piper` por defecto).
    pub command: String,
    /// Modelo de voz `.onnx` (junto a su `.onnx.json`).
    pub model_path: PathBuf,
    /// Frecuencia de salida de la voz (22 050 Hz en voces *medium*).
    pub sample_rate: u32,
    /// Argumentos extra (p. ej. `--length-scale 0.9`).
    pub extra_args: Vec<String>,
}

impl PiperConfig {
    /// Configuración por defecto para un modelo de voz.
    pub fn new(model_path: impl Into<PathBuf>) -> Self {
        Self {
            command: "piper".to_owned(),
            model_path: model_path.into(),
            sample_rate: 22_050,
            extra_args: Vec::new(),
        }
    }
}

/// Sintetizador Piper.
#[derive(Debug, Clone)]
pub struct PiperSynthesizer {
    config: PiperConfig,
}

impl PiperSynthesizer {
    /// Crea el sintetizador.
    pub fn new(config: PiperConfig) -> Self {
        Self { config }
    }

    /// Frecuencia de muestreo del audio producido.
    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }
}

struct ReadState {
    stdout: ChildStdout,
    // Mantiene el subproceso vivo mientras se consume el flujo.
    _child: Child,
    carry: Option<u8>,
}

#[async_trait]
impl VoiceSynthesizer for PiperSynthesizer {
    async fn synthesize(&self, text: &str) -> Result<BoxStream<'static, Result<Vec<f32>>>> {
        let mut child = Command::new(&self.config.command)
            .arg("--model")
            .arg(&self.config.model_path)
            .arg("--output-raw")
            .args(&self.config.extra_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                AssistantError::Audio(format!(
                    "no se pudo lanzar `{}` (¿está Piper instalado y en el PATH?): {e}",
                    self.config.command
                ))
            })?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AssistantError::Audio("Piper sin stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AssistantError::Audio("Piper sin stdout".into()))?;

        stdin.write_all(text.trim().as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        drop(stdin); // EOF: Piper sintetiza y termina.

        let state = ReadState {
            stdout,
            _child: child,
            carry: None,
        };
        Ok(Box::pin(stream::unfold(Some(state), |st| async move {
            let mut st = st?;
            let mut buf = vec![0u8; 8192];
            match st.stdout.read(&mut buf).await {
                Ok(0) => None,
                Ok(n) => {
                    buf.truncate(n);
                    // Reensambla una muestra de 2 bytes partida entre lecturas.
                    if let Some(b) = st.carry.take() {
                        buf.insert(0, b);
                    }
                    if buf.len() % 2 == 1 {
                        st.carry = buf.pop();
                    }
                    Some((Ok(s16le_to_f32(&buf)), Some(st)))
                }
                Err(e) => Some((Err(AssistantError::Io(e)), None)),
            }
        })))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    #[tokio::test]
    async fn reads_raw_pcm_from_subprocess() -> Result<()> {
        // Script que hace de "piper": ignora argumentos y emite 2 muestras.
        let dir = std::env::temp_dir().join(format!("fake-piper-{}.sh", std::process::id()));
        std::fs::write(
            &dir,
            "#!/bin/sh\ncat >/dev/null\nprintf '\\000\\100\\000\\300'\n",
        )?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))?;

        let mut cfg = PiperConfig::new("voz.onnx");
        cfg.command = dir.to_string_lossy().into_owned();
        let synth = PiperSynthesizer::new(cfg);
        let mut s = synth.synthesize("hola").await?;
        let mut samples = Vec::new();
        while let Some(chunk) = s.next().await {
            samples.extend(chunk?);
        }
        let _ = std::fs::remove_file(&dir);
        assert_eq!(samples, vec![0.5, -0.5]);
        Ok(())
    }

    #[tokio::test]
    async fn missing_binary_gives_audio_error() {
        let mut cfg = PiperConfig::new("voz.onnx");
        cfg.command = "__no_existe_piper__".to_owned();
        let r = PiperSynthesizer::new(cfg).synthesize("hola").await;
        assert!(matches!(r, Err(AssistantError::Audio(_))));
    }
}
