//! Bucle de voz: micrófono → VAD → STT → agente → TTS → altavoz.

use std::sync::Arc;

use core_types::{AssistantError, Result, VoiceSynthesizer, VoiceTranscriber};
use tokio::sync::{mpsc, watch};
use voice_engine::{
    listen_utterance, speak_sentence, AudioSink, SentenceChunker, UtteranceSegmenter, VadModel,
    VAD_SAMPLE_RATE,
};

use crate::agent::Agent;
use crate::events::{AgentEvent, AgentState};

/// Mando a distancia del bucle de voz, clonable (la UI lo usará por IPC).
#[derive(Clone)]
pub struct VoiceControl {
    shutdown: Arc<watch::Sender<bool>>,
    listening: Arc<watch::Sender<bool>>,
}

impl VoiceControl {
    /// Crea el mando; `start_listening` indica si arranca escuchando.
    pub fn new(start_listening: bool) -> Self {
        Self {
            shutdown: Arc::new(watch::channel(false).0),
            listening: Arc::new(watch::channel(start_listening).0),
        }
    }

    /// Activa o desactiva la escucha (p. ej. con un atajo global).
    pub fn set_listening(&self, on: bool) {
        self.listening.send_replace(on);
    }

    /// Alterna la escucha y devuelve el nuevo valor.
    pub fn toggle_listening(&self) -> bool {
        let next = !*self.listening.borrow();
        self.listening.send_replace(next);
        next
    }

    /// Pide al bucle que termine.
    pub fn shutdown(&self) {
        self.shutdown.send_replace(true);
    }
}

/// Componentes de voz que necesita el bucle.
pub struct VoiceComponents<V: VadModel> {
    /// Segmentador de enunciados (con su VAD).
    pub segmenter: UtteranceSegmenter<V>,
    /// Audio 16 kHz mono del micrófono.
    pub frames: mpsc::Receiver<Vec<f32>>,
    /// Reconocedor de voz.
    pub transcriber: Arc<dyn VoiceTranscriber>,
    /// Sintetizador de voz.
    pub synthesizer: Arc<dyn VoiceSynthesizer>,
    /// Salida de audio.
    pub sink: Arc<dyn AudioSink>,
    /// Frecuencia de muestreo del sintetizador.
    pub tts_sample_rate: u32,
}

fn drain_frames(frames: &mut mpsc::Receiver<Vec<f32>>) {
    while frames.try_recv().is_ok() {}
}

/// Ejecuta el bucle hasta `control.shutdown()` o hasta que se cierre el micrófono.
pub async fn run_voice_loop<V: VadModel>(
    agent: &mut Agent,
    mut voice: VoiceComponents<V>,
    control: &VoiceControl,
) -> Result<()> {
    let mut shutdown = control.shutdown.subscribe();
    let mut listening = control.listening.subscribe();

    loop {
        if *shutdown.borrow() {
            break;
        }

        // Escucha desactivada: espera a que se active y descarta audio acumulado.
        if !*listening.borrow() {
            agent.emit_state(AgentState::Idle);
            tokio::select! {
                _ = shutdown.changed() => {}
                _ = listening.changed() => {}
            }
            if *listening.borrow() {
                drain_frames(&mut voice.frames);
                let _ = voice.segmenter.flush();
            }
            continue;
        }

        agent.emit_state(AgentState::Listening);
        let utterance = tokio::select! {
            r = listen_utterance(&mut voice.frames, &mut voice.segmenter) => r?,
            _ = shutdown.changed() => break,
            _ = listening.changed() => continue,
        };
        let Some(audio) = utterance else { break };

        if let Err(e) = handle_utterance(agent, &mut voice, audio).await {
            tracing::warn!("turno de voz fallido: {e}");
            agent.emit(AgentEvent::Error {
                message: e.to_string(),
            });
        }
        // Descarta lo captado mientras el asistente hablaba (eco).
        drain_frames(&mut voice.frames);
        let _ = voice.segmenter.flush();
    }
    agent.emit_state(AgentState::Idle);
    Ok(())
}

async fn handle_utterance<V: VadModel>(
    agent: &mut Agent,
    voice: &mut VoiceComponents<V>,
    audio: Vec<f32>,
) -> Result<()> {
    agent.emit_state(AgentState::Transcribing);
    let text = voice.transcriber.transcribe(audio, VAD_SAMPLE_RATE).await?;
    let text = text.trim().to_owned();
    // Whisper emite marcas como "[BLANK_AUDIO]" cuando solo hay ruido.
    if text.is_empty() || (text.starts_with('[') && text.ends_with(']')) {
        return Ok(());
    }
    agent.emit(AgentEvent::Transcript { text: text.clone() });

    // El audio se sintetiza en paralelo mientras el LLM sigue generando.
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let synthesizer = Arc::clone(&voice.synthesizer);
    let sink = Arc::clone(&voice.sink);
    let rate = voice.tts_sample_rate;
    let speaker = tokio::spawn(async move {
        while let Some(sentence) = rx.recv().await {
            if let Err(e) = speak_sentence(&*synthesizer, &*sink, rate, &sentence).await {
                tracing::warn!("fallo al sintetizar: {e}");
            }
        }
    });

    let mut chunker = SentenceChunker::new();
    let result = agent
        .run_turn(&text, |delta| {
            for sentence in chunker.push(delta) {
                let _ = tx.send(sentence);
            }
        })
        .await;
    if let Some(rest) = chunker.finish() {
        let _ = tx.send(rest);
    }
    drop(tx);

    agent.emit_state(AgentState::Speaking);
    speaker
        .await
        .map_err(|e| AssistantError::Audio(format!("tarea de voz: {e}")))?;
    voice.sink.drain().await?;
    result.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentConfig;
    use crate::events::event_channel;
    use async_trait::async_trait;
    use core_types::{BoxStream, ChatMessage, LlmGateway, McpClient, StreamChunk, ToolDefinition};
    use futures_util::stream;
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use voice_engine::{EnergyVad, SegmenterConfig, FRAME_SAMPLES};

    struct Llm;
    #[async_trait]
    impl LlmGateway for Llm {
        async fn stream_chat(
            &self,
            _m: &[ChatMessage],
            _t: &[ToolDefinition],
        ) -> Result<BoxStream<'static, Result<StreamChunk>>> {
            let chunks = vec![
                Ok(StreamChunk::Text("Hola. Qué tal.".into())),
                Ok(StreamChunk::Done),
            ];
            Ok(Box::pin(stream::iter(chunks)))
        }
    }

    struct NoTools;
    #[async_trait]
    impl McpClient for NoTools {
        async fn list_tools(&self) -> Result<Vec<ToolDefinition>> {
            Ok(vec![])
        }
        async fn call_tool(&self, _n: &str, _a: Value) -> Result<String> {
            Ok(String::new())
        }
    }

    struct Stt;
    #[async_trait]
    impl VoiceTranscriber for Stt {
        async fn transcribe(&self, _s: Vec<f32>, _r: u32) -> Result<String> {
            Ok("hola asistente".into())
        }
    }

    struct Tts;
    #[async_trait]
    impl VoiceSynthesizer for Tts {
        async fn synthesize(
            &self,
            _t: &str,
        ) -> Result<core_types::BoxStream<'static, Result<Vec<f32>>>> {
            Ok(Box::pin(stream::iter(vec![Ok(vec![0.1; 10])])))
        }
    }

    #[derive(Default)]
    struct CountingSink(AtomicUsize);
    #[async_trait::async_trait]
    impl AudioSink for CountingSink {
        fn play(&self, _s: Vec<f32>, _r: u32) -> Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn full_voice_turn_with_fakes() -> Result<()> {
        let (events, _keep) = event_channel();
        let mut agent = Agent::new(
            Arc::new(Llm),
            Arc::new(NoTools),
            AgentConfig::default(),
            events,
        );
        let mut rx = agent.subscribe();

        let (tx, frames) = mpsc::channel(8);
        let tone: Vec<f32> = (0..30 * FRAME_SAMPLES)
            .map(|i| 0.3 * (i as f32 * 0.2).sin())
            .collect();
        tx.send(tone)
            .await
            .map_err(|_| AssistantError::Audio("canal".into()))?;
        tx.send(vec![0.0; 40 * FRAME_SAMPLES])
            .await
            .map_err(|_| AssistantError::Audio("canal".into()))?;

        let sink = Arc::new(CountingSink::default());
        let voice = VoiceComponents {
            segmenter: UtteranceSegmenter::new(EnergyVad::default(), SegmenterConfig::default()),
            frames,
            transcriber: Arc::new(Stt),
            synthesizer: Arc::new(Tts),
            sink: sink.clone(),
            tts_sample_rate: 22_050,
        };
        let control = VoiceControl::new(true);
        let stopper = control.clone();
        // Apaga el bucle cuando llegue la respuesta final.
        tokio::spawn(async move {
            while let Ok(ev) = rx.recv().await {
                if matches!(ev, AgentEvent::Response { .. }) {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    stopper.shutdown();
                    break;
                }
            }
        });

        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            run_voice_loop(&mut agent, voice, &control),
        )
        .await
        .map_err(|_| AssistantError::Audio("timeout del bucle".into()))??;

        assert_eq!(sink.0.load(Ordering::SeqCst), 2); // dos frases
        Ok(())
    }
}
