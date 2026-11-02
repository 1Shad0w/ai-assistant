//! Motor de voz 100% local: captura, VAD, STT y TTS.
//!
//! - **Siempre disponible** (sin dependencias nativas): remuestreo, VAD por
//!   energía, segmentador de enunciados, troceado de frases, Piper TTS por
//!   subproceso y la orquestación ([`listen_utterance`], [`speak`]).
//! - **Features opcionales** (dependencias nativas pesadas):
//!   `mic` (cpal), `playback` (rodio), `silero` (ONNX Runtime) y
//!   `whisper` (whisper.cpp, requiere CMake). `full` activa todas.

#![forbid(unsafe_code)]

pub mod audio;
pub mod pipeline;
pub mod piper;
pub mod text;
pub mod vad;

#[cfg(feature = "mic")]
pub mod capture;
#[cfg(feature = "playback")]
pub mod playback;
#[cfg(feature = "silero")]
pub mod silero;
#[cfg(feature = "whisper")]
pub mod whisper;

pub use pipeline::{listen_utterance, speak, speak_sentence, AudioSink};
pub use piper::{PiperConfig, PiperSynthesizer};
pub use text::{split_sentences, SentenceChunker};
pub use vad::{
    EnergyVad, SegmenterConfig, UtteranceSegmenter, VadModel, FRAME_SAMPLES, VAD_SAMPLE_RATE,
};

#[cfg(feature = "mic")]
pub use capture::MicCapture;
#[cfg(feature = "playback")]
pub use playback::RodioSink;
#[cfg(feature = "silero")]
pub use silero::SileroVad;
#[cfg(feature = "whisper")]
pub use whisper::WhisperTranscriber;
