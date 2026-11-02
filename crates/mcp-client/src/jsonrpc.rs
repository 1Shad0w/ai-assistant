//! Conexión JSON-RPC 2.0 sobre líneas (transporte stdio de MCP).
//!
//! Es genérica sobre `AsyncRead`/`AsyncWrite`, de modo que se puede probar
//! sin subprocesos reales.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use core_types::{AssistantError, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;

type Pending = Arc<StdMutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;
type SharedWriter = Arc<Mutex<Box<dyn AsyncWrite + Send + Unpin>>>;

fn lock_pending(
    p: &Pending,
) -> std::sync::MutexGuard<'_, HashMap<u64, oneshot::Sender<Result<Value>>>> {
    p.lock().unwrap_or_else(|e| e.into_inner())
}

async fn write_line(writer: &SharedWriter, msg: &Value) -> Result<()> {
    let mut line = serde_json::to_string(msg)?;
    line.push('\n');
    let mut w = writer.lock().await;
    w.write_all(line.as_bytes()).await?;
    w.flush().await?;
    Ok(())
}

/// Conexión JSON-RPC con correlación de respuestas por `id`.
pub struct McpConnection {
    writer: SharedWriter,
    pending: Pending,
    next_id: AtomicU64,
    closed: Arc<AtomicBool>,
    timeout: Duration,
    reader: JoinHandle<()>,
}

impl Drop for McpConnection {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl McpConnection {
    /// Crea la conexión y lanza la tarea lectora.
    pub fn new<R, W>(reader: R, writer: W, timeout: Duration) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let writer: SharedWriter = Arc::new(Mutex::new(Box::new(writer)));
        let pending: Pending = Arc::new(StdMutex::new(HashMap::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let reader = tokio::spawn(read_loop(
            reader,
            Arc::clone(&writer),
            Arc::clone(&pending),
            Arc::clone(&closed),
        ));
        Self {
            writer,
            pending,
            next_id: AtomicU64::new(1),
            closed,
            timeout,
            reader,
        }
    }

    /// Envía una petición y espera su resultado (con timeout).
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(AssistantError::Mcp("el servidor MCP está cerrado".into()));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        lock_pending(&self.pending).insert(id, tx);

        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if let Err(e) = write_line(&self.writer, &msg).await {
            lock_pending(&self.pending).remove(&id);
            return Err(e);
        }
        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AssistantError::Mcp(
                "el servidor MCP cerró la conexión".into(),
            )),
            Err(_) => {
                lock_pending(&self.pending).remove(&id);
                Err(AssistantError::Mcp(format!(
                    "timeout esperando respuesta a `{method}`"
                )))
            }
        }
    }

    /// Envía una notificación (sin respuesta).
    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        write_line(&self.writer, &msg).await
    }
}

async fn read_loop<R: AsyncRead + Unpin>(
    reader: R,
    writer: SharedWriter,
    pending: Pending,
    closed: Arc<AtomicBool>,
) {
    let mut lines = BufReader::new(reader).lines();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(l)) => l,
            Ok(None) => break,
            Err(e) => {
                tracing::warn!("error leyendo del servidor MCP: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!("línea no JSON del servidor MCP ignorada: {e}");
                continue;
            }
        };
        let id = msg.get("id").cloned();
        let has_method = msg.get("method").is_some();

        match (id, has_method) {
            // Petición del servidor al cliente: no soportamos ninguna.
            (Some(id), true) => {
                let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
                let reply = if method == "ping" {
                    json!({ "jsonrpc": "2.0", "id": id, "result": {} })
                } else {
                    json!({ "jsonrpc": "2.0", "id": id,
                            "error": { "code": -32601, "message": "method not found" } })
                };
                if let Err(e) = write_line(&writer, &reply).await {
                    tracing::warn!("no se pudo responder al servidor MCP: {e}");
                }
            }
            // Respuesta a una petición nuestra.
            (Some(id), false) => {
                let Some(id) = id.as_u64() else { continue };
                let Some(tx) = lock_pending(&pending).remove(&id) else {
                    continue;
                };
                let outcome = match msg.get("error") {
                    Some(err) => {
                        let m = err
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("error JSON-RPC");
                        let code = err.get("code").and_then(Value::as_i64).unwrap_or(0);
                        Err(AssistantError::Mcp(format!("{m} (código {code})")))
                    }
                    None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = tx.send(outcome);
            }
            // Notificación del servidor (logs, progreso...).
            (None, _) => tracing::debug!("notificación MCP: {line}"),
        }
    }
    closed.store(true, Ordering::SeqCst);
    // Al soltar los emisores pendientes, los receptores fallan con "cerrado".
    lock_pending(&pending).clear();
}
