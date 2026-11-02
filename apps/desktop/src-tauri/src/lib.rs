//! Interfaz de escritorio (Tauri v2) del asistente.
//!
//! La UI está desacoplada del backend: el runtime publica
//! [`agent_runtime::AgentEvent`] y esta capa solo los reenvía al frontend
//! como el evento `agent-event`. Las órdenes llegan por comandos IPC.

use std::path::PathBuf;
use std::sync::Arc;

use agent_runtime::{event_channel, Agent, AgentConfig, AgentEvent, VoiceControl};
use llm_client::{OmniRouteClient, OmniRouteConfig};
use mcp_client::{McpConfig, McpOrchestrator, DEFAULT_TIMEOUT};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tokio::sync::{broadcast::error::RecvError, mpsc};

/// Estado compartido accesible desde los comandos IPC.
struct AppState {
    control: VoiceControl,
    text_tx: mpsc::UnboundedSender<String>,
}

#[derive(Serialize)]
struct Capabilities {
    voice: bool,
}

/// Envía un mensaje de texto al agente (modo texto).
#[tauri::command]
fn send_message(state: State<'_, AppState>, text: String) -> Result<(), String> {
    state.text_tx.send(text).map_err(|e| e.to_string())
}

/// Activa o desactiva la escucha del micrófono.
#[tauri::command]
fn set_listening(app: AppHandle, state: State<'_, AppState>, on: bool) {
    state.control.set_listening(on);
    let _ = app.emit("listening-changed", on);
}

/// Alterna la escucha y devuelve el nuevo valor.
#[tauri::command]
fn toggle_listening(app: AppHandle, state: State<'_, AppState>) -> bool {
    let on = state.control.toggle_listening();
    let _ = app.emit("listening-changed", on);
    on
}

/// Capacidades con las que se compiló la aplicación.
#[tauri::command]
fn capabilities() -> Capabilities {
    Capabilities {
        voice: cfg!(feature = "voice"),
    }
}

/// Busca `relative` en el directorio actual y sus ancestros (y los del ejecutable).
fn locate(relative: &str) -> Option<PathBuf> {
    let starts = [
        std::env::current_dir().ok(),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from)),
    ];
    starts
        .into_iter()
        .flatten()
        .flat_map(|start| start.ancestors().map(PathBuf::from).collect::<Vec<_>>())
        .map(|dir| dir.join(relative))
        .find(|p| p.exists())
}

fn report(app: &AppHandle, message: String) {
    tracing::warn!("{message}");
    let _ = app.emit("agent-event", AgentEvent::Error { message });
}

/// Construye el runtime y lo mantiene en marcha; los errores llegan a la UI.
async fn backend(
    app: AppHandle,
    control: VoiceControl,
    text_rx: mpsc::UnboundedReceiver<String>,
) {
    if let Err(e) = backend_inner(&app, control, text_rx).await {
        report(&app, format!("el backend se detuvo: {e:#}"));
    }
}

async fn backend_inner(
    app: &AppHandle,
    control: VoiceControl,
    mut text_rx: mpsc::UnboundedReceiver<String>,
) -> anyhow::Result<()> {
    let mcp_config = match std::env::var_os("ASSISTANT_MCP_CONFIG")
        .map(PathBuf::from)
        .or_else(|| locate(".agents/mcp_config.json"))
    {
        Some(path) => McpConfig::from_file(&path)?,
        None => {
            report(app, "no se encontró .agents/mcp_config.json; sin herramientas MCP".into());
            McpConfig {
                servers: Default::default(),
            }
        }
    };
    let mcp = McpOrchestrator::start(&mcp_config, DEFAULT_TIMEOUT).await;
    for (name, err) in mcp.failures() {
        report(app, format!("servidor MCP `{name}` no disponible: {err}"));
    }

    let llm = OmniRouteClient::new(OmniRouteConfig::from_env())?;
    let (events, _keep_alive) = event_channel();
    let mut agent = Agent::new(Arc::new(llm), Arc::new(mcp), AgentConfig::default(), events);

    // Reenvío de eventos del runtime al frontend.
    let mut rx = agent.subscribe();
    let forward = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let _ = forward.emit("agent-event", &ev);
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });

    #[cfg(feature = "voice")]
    {
        let models = locate("models").unwrap_or_else(|| PathBuf::from("models"));
        let stt = voice_engine::WhisperTranscriber::new(
            models.join("ggml-base-q5_1.bin"),
            Some("es".to_owned()),
        )?;
        drop(text_rx);
        agent_runtime::voice_mode::run(&mut agent, stt, &models, &control).await
    }

    #[cfg(not(feature = "voice"))]
    {
        let _ = control;
        agent.emit_state(agent_runtime::AgentState::Idle);
        while let Some(text) = text_rx.recv().await {
            // Los errores del turno ya se publican como evento `Error`.
            let _ = agent.run_turn(&text, |_| {}).await;
        }
        Ok(())
    }
}

/// Punto de entrada de la aplicación.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Cmd+Shift+Espacio en macOS; Ctrl+Shift+Espacio en el resto.
    let primary = if cfg!(target_os = "macos") {
        Modifiers::SUPER
    } else {
        Modifiers::CONTROL
    };
    let shortcut = Shortcut::new(Some(primary | Modifiers::SHIFT), Code::Space);
    let (text_tx, text_rx) = mpsc::unbounded_channel();
    let control = VoiceControl::new(false);
    let backend_control = control.clone();
    let mut text_rx = Some(text_rx);

    let result = tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let on = app.state::<AppState>().control.toggle_listening();
                        let _ = app.emit("listening-changed", on);
                    }
                })
                .build(),
        )
        .manage(AppState { control, text_tx })
        .setup(move |app| {
            // En algunos entornos (p. ej. Wayland) el atajo global no está disponible.
            if let Err(e) = app.global_shortcut().register(shortcut) {
                tracing::warn!("no se pudo registrar el atajo global: {e}");
            }
            if let Some(rx) = text_rx.take() {
                tauri::async_runtime::spawn(backend(
                    app.handle().clone(),
                    backend_control.clone(),
                    rx,
                ));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_message,
            set_listening,
            toggle_listening,
            capabilities
        ])
        .run(tauri::generate_context!());

    if let Err(e) = result {
        eprintln!("error al ejecutar la aplicación Tauri: {e}");
        std::process::exit(1);
    }
}
