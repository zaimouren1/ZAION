//! task_async.rs — Tokio-native AsyncTaskEngine
//!
//! 重构重点 (C1.1)：
//!   旧实现：TaskWorker::run() 对每个 Execute 命令调用 await，串行执行任务。
//!   新实现：每个任务通过 tokio::spawn 独立并发执行，命令循环不再阻塞，
//!           支持任意数量的并发任务。
use crate::RuntimeError;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use zaion_crypto::keypair::ZaionKeypair;
use zaion_ledger::EventLedger;
pub use zaion_types::task::TaskStatus;
use zaion_types::{event::EventId, session::NamespaceKey};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncTask {
    pub task_id: String,
    pub principal_id: String,
    pub session_key: String,
    pub task_type: String,
    pub input: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub status: TaskStatus,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub type AsyncTaskHandler = Arc<
    dyn Fn(AsyncTask) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, String>> + Send>>
        + Send
        + Sync,
>;

enum TaskCommand {
    Execute {
        task_type: String,
        input: serde_json::Value,
        handler: AsyncTaskHandler,
        response: tokio::sync::oneshot::Sender<Result<(AsyncTask, EventId), RuntimeError>>,
    },
    Shutdown,
}

/// Shared state threaded into each spawned task.
///
/// `EventLedger` is already internally `Sync` via its own
/// `Mutex<Option<Connection>>`. An outer async `Mutex<EventLedger>` would
/// be redundant double-locking and would serialize every task's ledger
/// write on a Tokio worker. Instead we share `Arc<EventLedger>` directly
/// and push the blocking SQLite write into `spawn_blocking`.
struct WorkerState {
    ledger: Arc<EventLedger>,
    keypair: Arc<ZaionKeypair>,
    namespace_key: NamespaceKey,
    active_tasks: Arc<RwLock<std::collections::HashMap<String, AsyncTask>>>,
}

pub struct AsyncTaskEngine {
    tx: mpsc::UnboundedSender<TaskCommand>,
}

impl AsyncTaskEngine {
    pub fn new(ledger: EventLedger, keypair: ZaionKeypair, namespace_key: NamespaceKey) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let state = Arc::new(WorkerState {
            ledger: Arc::new(ledger),
            keypair: Arc::new(keypair),
            namespace_key,
            active_tasks: Arc::new(RwLock::new(std::collections::HashMap::new())),
        });
        tokio::spawn(dispatcher_loop(rx, state));
        Self { tx }
    }

    pub async fn execute(
        &self,
        task_type: String,
        input: serde_json::Value,
        handler: AsyncTaskHandler,
    ) -> Result<(AsyncTask, EventId), RuntimeError> {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(TaskCommand::Execute {
                task_type,
                input,
                handler,
                response: response_tx,
            })
            .map_err(|_| RuntimeError::Internal("task engine shutdown".into()))?;
        response_rx
            .await
            .map_err(|_| RuntimeError::Internal("task response lost".into()))?
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(TaskCommand::Shutdown);
    }
}

/// 命令分发循环 — 永不 await 任务本体，只 spawn 并继续。
async fn dispatcher_loop(mut rx: mpsc::UnboundedReceiver<TaskCommand>, state: Arc<WorkerState>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            TaskCommand::Execute {
                task_type,
                input,
                handler,
                response,
            } => {
                let s = Arc::clone(&state);
                // ★ 核心变化：spawn 独立 task，dispatcher 立即返回继续接收命令
                tokio::spawn(async move {
                    let result = execute_task(&s, task_type, input, handler).await;
                    let _ = response.send(result);
                });
            }
            TaskCommand::Shutdown => break,
        }
    }
}

/// 单任务执行体 — 在独立 tokio task 中运行。
async fn execute_task(
    state: &WorkerState,
    task_type: String,
    input: serde_json::Value,
    handler: AsyncTaskHandler,
) -> Result<(AsyncTask, EventId), RuntimeError> {
    let now = chrono::Utc::now().to_rfc3339();
    let task_id = format!("tsk-{}", uuid::Uuid::new_v4());
    let pid = state.keypair.principal_id();

    let mut task = AsyncTask {
        task_id: task_id.clone(),
        principal_id: pid.as_str().to_string(),
        session_key: state.namespace_key.0.clone(),
        task_type: task_type.clone(),
        input: input.clone(),
        output: None,
        status: TaskStatus::Running,
        error: None,
        created_at: now.clone(),
        updated_at: now,
    };

    state
        .active_tasks
        .write()
        .await
        .insert(task_id.clone(), task.clone());

    let start_payload = serde_json::json!({
        "task_id":   &task_id,
        "task_type": &task_type,
        "input":     &input,
    });
    {
        let ledger = state.ledger.clone();
        let keypair = state.keypair.clone();
        let ns = state.namespace_key.clone();
        let payload = start_payload;
        tokio::task::spawn_blocking(move || {
            ledger.append_signed_event(&keypair, &ns, "task.started", payload, None)
        })
        .await
        .map_err(|e| RuntimeError::Internal(format!("ledger join error: {}", e)))??;
    }

    let result = handler(task.clone()).await;

    let (status, output, error, event_type) = match result {
        Ok(res) => (TaskStatus::Completed, Some(res), None, "task.completed"),
        Err(e) => (TaskStatus::Failed, None, Some(e), "task.failed"),
    };

    task.status = status;
    task.output = output.clone();
    task.error = error.clone();
    task.updated_at = chrono::Utc::now().to_rfc3339();

    let end_payload = serde_json::json!({
        "task_id":   &task_id,
        "task_type": &task_type,
        "output":    &output,
        "error":     &error,
    });
    let event_id = {
        let ledger = state.ledger.clone();
        let keypair = state.keypair.clone();
        let ns = state.namespace_key.clone();
        let payload = end_payload;
        let ev_type = event_type.to_string();
        tokio::task::spawn_blocking(move || {
            ledger.append_signed_event(&keypair, &ns, &ev_type, payload, None)
        })
        .await
        .map_err(|e| RuntimeError::Internal(format!("ledger join error: {}", e)))??
    };

    state.active_tasks.write().await.remove(&task_id);
    Ok((task, event_id))
}
