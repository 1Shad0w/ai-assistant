//! CLI del asistente: modo texto (por defecto) o modo voz (`--voice`).
//!
//! Uso: `assistant [--voice] [--mcp-config RUTA] [--models-dir RUTA]`

use std::path::PathBuf;
use std::sync::Arc;

use agent_runtime::{event_channel, Agent, AgentConfig, AgentEvent};
use anyhow::Context;
use llm_client::{OmniRouteClient, OmniRouteConfig};
use mcp_client::{McpConfig, McpOrchestrator, DEFAULT_TIMEOUT};
use tokio::io::{AsyncBufReadExt, BufReader};

struct Args {
    voice: bool,
    mcp_config: PathBuf,
    models_dir: PathBuf,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut args = Args {
        voice: false,
        mcp_config: PathBuf::from(".agents/mcp_config.json"),
        models_dir: PathBuf::from("models"),
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--voice" => args.voice = true,
            "--mcp-config" => {
                args.mcp_config = it.next().context("--mcp-config requiere una ruta")?.into();
            }
            "--models-dir" => {
                args.models_dir = it.next().context("--models-dir requiere una ruta")?.into();
            }
            "-h" | "--help" => {
                println!("assistant [--voice] [--mcp-config RUTA] [--models-dir RUTA]");
                std::process::exit(0);
            }
            other => anyhow::bail!("argumento desconocido: {other}"),
        }
    }
    Ok(args)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let args = parse_args()?;

    let mcp_config = McpConfig::from_file(&args.mcp_config)
        .with_context(|| format!("leyendo {}", args.mcp_config.display()))?;
    let mcp = McpOrchestrator::start(&mcp_config, DEFAULT_TIMEOUT).await;
    for (name, err) in mcp.failures() {
        eprintln!("aviso: servidor MCP `{name}` no disponible: {err}");
    }

    let llm = OmniRouteClient::new(OmniRouteConfig::from_env())?;
    let (events, _rx) = event_channel();
    let mut agent = Agent::new(Arc::new(llm), Arc::new(mcp), AgentConfig::default(), events);

    // Traza de herramientas por stderr.
    let mut rx = agent.subscribe();
    tokio::spawn(async move {
        while let Ok(ev) = rx.recv().await {
            match ev {
                AgentEvent::ToolStarted { name, .. } => eprintln!("[herramienta] {name}…"),
                AgentEvent::ToolFinished { name, ok, .. } => {
                    eprintln!("[herramienta] {name} → {}", if ok { "ok" } else { "error" });
                }
                AgentEvent::Transcript { text } => eprintln!("[tú] {text}"),
                _ => {}
            }
        }
    });

    if args.voice {
        return run_voice(&mut agent, &args).await;
    }

    println!("Modo texto. Escribe un mensaje (Ctrl-D para salir).");
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let result = agent
            .run_turn(line, |t| {
                use std::io::Write;
                print!("{t}");
                let _ = std::io::stdout().flush();
            })
            .await;
        match result {
            Ok(_) => println!(),
            Err(e) => eprintln!("\nerror: {e}"),
        }
    }
    Ok(())
}

#[cfg(feature = "voice")]
async fn run_voice(agent: &mut Agent, args: &Args) -> anyhow::Result<()> {
    use agent_runtime::VoiceControl;

    let model = args.models_dir.join("ggml-base-q5_1.bin");
    let stt = voice_engine::WhisperTranscriber::new(&model, Some("es".to_owned()))
        .with_context(|| format!("cargando {}", model.display()))?;

    let control = VoiceControl::new(true);
    let c = control.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        c.shutdown();
    });
    println!("Modo voz activo. Ctrl-C para salir.");
    agent_runtime::voice_mode::run(agent, stt, &args.models_dir, &control).await
}

#[cfg(not(feature = "voice"))]
async fn run_voice(_agent: &mut Agent, _args: &Args) -> anyhow::Result<()> {
    anyhow::bail!(
        "este binario se compiló sin voz; usa `cargo run -p agent-runtime --features voice`"
    )
}
