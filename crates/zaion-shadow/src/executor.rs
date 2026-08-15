//! ShadowExecutor — 后台并发任务执行器（Sprint 5, Genesis v4.0）
//!
//! 架构：
//!   ShadowExecutor  — 主控器（mpsc 命令环路 + Tokio 并发槽位）
//!   AciDispatcher   — 所有文件写操作必须经过 ACI 三道门（SyntaxGate + FileOpsGate + Toxic）
//!   EventLedger     — 每个任务生命周期事件 append_event 写入
//!   ShadowEventTx   — broadcast channel，供 TopoPane 实时订阅任务事件
use crate::command_spec::{AllowList, CommandSpec};
use crate::{
    LifecycleEvent, ShadowError, ShadowLifecycle, ShadowTask, TaskId, TaskQueue, TaskResult,
};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{error, warn};
use zaion_aci::{AciAction, AciDispatcher};
use zaion_crypto::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_types::session::NamespaceKey;

// ── ShadowEvent (broadcast channel) ──────────────────────────────────────────

/// 实时事件，广播给 TopoPane 等订阅者
#[derive(Debug, Clone)]
pub enum ShadowEvent {
    TaskSpawned {
        task_id: TaskId,
        name: String,
    },
    TaskStarted {
        task_id: TaskId,
        name: String,
    },
    TaskCompleted {
        task_id: TaskId,
        name: String,
        success: bool,
        duration_ms: u64,
    },
    TaskCancelled {
        task_id: TaskId,
    },
    AciOperation {
        task_id: TaskId,
        op: String,
        ok: bool,
    },
    ExecutorStarted,
    ExecutorStopped,
}

pub type ShadowEventTx = broadcast::Sender<ShadowEvent>;
pub type ShadowEventRx = broadcast::Receiver<ShadowEvent>;

// ── ExecutorConfig ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub max_queue_size: usize,
    pub max_concurrent_tasks: usize,
    pub task_timeout_seconds: u64,
    pub heartbeat_interval_ms: u64,
    pub ledger_db_path: String,
    pub aci_reality_db_path: String,
    pub aci_toxic_db_path: String,
    pub principal_id: String,
    /// Explicit allow-list of programs that `ShadowTask` may spawn.
    ///
    /// Default is **empty** (fail-closed): any task whose `command` is not in
    /// this list will be rejected before a process is created.  Add program
    /// names (bare names or full paths) as required by the deployment context.
    pub allowed_programs: Vec<String>,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            max_queue_size: 1000,
            max_concurrent_tasks: 4,
            task_timeout_seconds: 300,
            heartbeat_interval_ms: 100,
            ledger_db_path: "shadow_events.db".to_string(),
            aci_reality_db_path: "aci_reality.db".to_string(),
            aci_toxic_db_path: "aci_toxic.db".to_string(),
            principal_id: "shadow-executor".to_string(),
            // Default empty ⇒ fail-closed: no shell commands permitted unless
            // the operator explicitly populates this list.
            allowed_programs: Vec::new(),
        }
    }
}

// ── ExecutorCommand ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ExecutorCommand {
    /// Boxed to keep the enum variant size bounded; ShadowTask is ~344 bytes
    /// and would otherwise dominate every other variant.
    SubmitTask(Box<ShadowTask>),
    CancelTask(TaskId),
    GetTask(TaskId, tokio::sync::oneshot::Sender<Option<ShadowTask>>),
    ListTasks(tokio::sync::oneshot::Sender<Vec<ShadowTask>>),
    GetStats(tokio::sync::oneshot::Sender<crate::QueueStats>),
    Shutdown,
}

// ── ShadowExecutor ────────────────────────────────────────────────────────────

pub struct ShadowExecutor {
    config: ExecutorConfig,
    queue: Arc<RwLock<TaskQueue>>,
    lifecycle: Arc<RwLock<ShadowLifecycle>>,
    aci_dispatcher: Arc<AciDispatcher>,
    ledger: Arc<EventLedger>,
    keypair: Arc<ZaionKeypair>,
    command_tx: Option<mpsc::UnboundedSender<ExecutorCommand>>,
    running_tasks: Arc<RwLock<HashMap<TaskId, tokio::task::JoinHandle<()>>>>,
    event_tx: ShadowEventTx,
}

impl ShadowExecutor {
    #[cfg(test)]
    pub fn new(mut config: ExecutorConfig) -> Result<Self, ShadowError> {
        let keypair = Arc::new(ZaionKeypair::generate());
        config.principal_id = keypair.principal_id().as_str().to_string();
        Self::new_with_key(config, keypair)
    }

    pub fn new_with_key(
        config: ExecutorConfig,
        keypair: Arc<ZaionKeypair>,
    ) -> Result<Self, ShadowError> {
        let signing_principal = keypair.principal_id();
        if config.principal_id.trim().is_empty() {
            return Err(ShadowError::InvalidIdentity(
                "shadow executor requires a persisted principal_id".to_string(),
            ));
        }
        if config.principal_id != signing_principal.as_str() {
            return Err(ShadowError::InvalidIdentity(format!(
                "configured principal {} does not match signing key {}",
                config.principal_id,
                signing_principal.as_str()
            )));
        }

        let queue = Arc::new(RwLock::new(TaskQueue::new(
            config.max_queue_size,
            config.max_concurrent_tasks,
        )));

        let lifecycle = Arc::new(RwLock::new(ShadowLifecycle::new()));

        // ACI dispatcher: (toxic_db, reality_db) — ensure schemas exist
        zaion_watchdog::toxic::ToxicHashRegistry::new(&config.aci_toxic_db_path)
            .ensure()
            .map_err(|e: zaion_watchdog::WatchdogError| {
                ShadowError::Io(std::io::Error::other(e.to_string()))
            })?;
        zaion_watchdog::reality_sync::RealitySyncStore::new(&config.aci_reality_db_path)
            .ensure()
            .map_err(|e: zaion_watchdog::WatchdogError| {
                ShadowError::Io(std::io::Error::other(e.to_string()))
            })?;
        let aci_dispatcher = Arc::new(AciDispatcher::new(
            &config.aci_toxic_db_path,
            &config.aci_reality_db_path,
        ));

        let ledger = Arc::new(EventLedger::new(&config.ledger_db_path));
        ledger.ensure()?;

        let (event_tx, _) = broadcast::channel(64);

        Ok(Self {
            config,
            queue,
            lifecycle,
            aci_dispatcher,
            ledger,
            keypair,
            command_tx: None,
            running_tasks: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
        })
    }

    /// 订阅实时事件（TopoPane / CLI 用）
    pub fn subscribe(&self) -> ShadowEventRx {
        self.event_tx.subscribe()
    }

    pub async fn start(&mut self) -> Result<mpsc::UnboundedSender<ExecutorCommand>, ShadowError> {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        self.command_tx = Some(command_tx.clone());

        {
            let mut lc = self.lifecycle.write().await;
            lc.transition(LifecycleEvent::Start)?;
        }

        let ns = NamespaceKey(self.config.principal_id.clone());
        Self::ledger_append(
            &self.ledger,
            self.keypair.as_ref(),
            &ns,
            "shadow.executor.started",
            serde_json::json!({ "max_concurrent": self.config.max_concurrent_tasks }),
        );
        let _ = self.event_tx.send(ShadowEvent::ExecutorStarted);

        let queue = Arc::clone(&self.queue);
        let lifecycle = Arc::clone(&self.lifecycle);
        let aci_dispatcher = Arc::clone(&self.aci_dispatcher);
        let ledger = Arc::clone(&self.ledger);
        let keypair = Arc::clone(&self.keypair);
        let running_tasks = Arc::clone(&self.running_tasks);
        let config = self.config.clone();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let mut tick = tokio::time::interval(tokio::time::Duration::from_millis(
                config.heartbeat_interval_ms,
            ));
            let ns = NamespaceKey(config.principal_id.clone());

            loop {
                tokio::select! {
                    cmd_res = command_rx.recv() => {
                        // Explicit None handling: when the sender half is
                        // dropped (ShadowExecutor drops without `shutdown()`),
                        // recv() returns None. Using `Some(cmd) =` in a select
                        // arm silently disables the arm and the loop runs
                        // forever on the tick branch, leaking the task and
                        // every captured Arc (queue/lifecycle/ledger/event_tx).
                        // (HIGH H-N6 fix.)
                        let Some(cmd) = cmd_res else { break };
                        match cmd {
                            ExecutorCommand::SubmitTask(task) => {
                                Self::handle_submit(
                                    &queue, &lifecycle, &ledger,
                                    &keypair, &ns, &event_tx, *task,
                                ).await;
                            }
                            ExecutorCommand::CancelTask(tid) => {
                                Self::handle_cancel(&queue, &running_tasks, &event_tx, tid).await;
                            }
                            ExecutorCommand::GetTask(tid, tx) => {
                                let t = queue.read().await.get_task(&tid).cloned();
                                let _ = tx.send(t);
                            }
                            ExecutorCommand::ListTasks(tx) => {
                                let tasks = queue.read().await
                                    .list_tasks().into_iter().cloned().collect();
                                let _ = tx.send(tasks);
                            }
                            ExecutorCommand::GetStats(tx) => {
                                let stats = queue.read().await.stats();
                                let _ = tx.send(stats);
                            }
                            ExecutorCommand::Shutdown => break,
                        }
                    }

                    _ = tick.tick() => {
                        Self::drain_queue(
                            &queue, &lifecycle, &aci_dispatcher,
                            &ledger, &keypair, &ns,
                            &running_tasks, &config, &event_tx,
                        ).await;
                        lifecycle.write().await.update_uptime();
                    }
                }
            }

            let mut guard = running_tasks.write().await;
            for (_, handle) in guard.drain() {
                handle.abort();
            }
            let _ = event_tx.send(ShadowEvent::ExecutorStopped);
        });

        Ok(command_tx)
    }

    // ── Internal handlers ────────────────────────────────────────────────────

    async fn handle_submit(
        queue: &Arc<RwLock<TaskQueue>>,
        lifecycle: &Arc<RwLock<ShadowLifecycle>>,
        ledger: &Arc<EventLedger>,
        keypair: &Arc<ZaionKeypair>,
        ns: &NamespaceKey,
        event_tx: &ShadowEventTx,
        task: ShadowTask,
    ) {
        let task_id = task.id;
        let name = task.name.clone();

        match queue.write().await.enqueue(task) {
            Ok(_) => {
                let mut lc = lifecycle.write().await;
                let _ = lc.transition(LifecycleEvent::TaskQueued { task_id });
                drop(lc);
                Self::ledger_append(
                    ledger,
                    keypair.as_ref(),
                    ns,
                    "shadow.task.queued",
                    serde_json::json!({ "task_id": task_id.to_string(), "name": name }),
                );
                let _ = event_tx.send(ShadowEvent::TaskSpawned { task_id, name });
            }
            Err(e) => warn!(task_id = %task_id, error = %e, "enqueue failed"),
        }
    }

    async fn handle_cancel(
        queue: &Arc<RwLock<TaskQueue>>,
        running_tasks: &Arc<RwLock<HashMap<TaskId, tokio::task::JoinHandle<()>>>>,
        event_tx: &ShadowEventTx,
        task_id: TaskId,
    ) {
        let _ = queue.write().await.cancel_task(task_id);
        if let Some(handle) = running_tasks.write().await.remove(&task_id) {
            handle.abort();
        }
        let _ = event_tx.send(ShadowEvent::TaskCancelled { task_id });
    }

    /// Drains tasks from the queue while capacity allows.
    ///
    /// Nine parameters is intentional here — this is a purely private helper
    /// that threads the executor's shared state into the worker loop. Packing
    /// them into a `struct Ctx<'a>` would only push the complexity one level
    /// deeper without any readability win, so we silence the lint locally.
    #[allow(clippy::too_many_arguments)]
    async fn drain_queue(
        queue: &Arc<RwLock<TaskQueue>>,
        lifecycle: &Arc<RwLock<ShadowLifecycle>>,
        aci_dispatcher: &Arc<AciDispatcher>,
        ledger: &Arc<EventLedger>,
        keypair: &Arc<ZaionKeypair>,
        ns: &NamespaceKey,
        running_tasks: &Arc<RwLock<HashMap<TaskId, tokio::task::JoinHandle<()>>>>,
        config: &ExecutorConfig,
        event_tx: &ShadowEventTx,
    ) {
        // H21 fix: single write lock scope to avoid race between capacity check
        // and dequeue — another task could modify the queue between separate
        // read-then-write locks.
        let task = {
            let mut q = queue.write().await;
            if !q.has_capacity_to_run() {
                return;
            }
            q.dequeue()
        };
        let Some(task) = task else { return };

        let task_id = task.id;
        let name = task.name.clone();

        {
            let mut lc = lifecycle.write().await;
            let _ = lc.transition(LifecycleEvent::TaskStarted { task_id });
        }

        let _ = event_tx.send(ShadowEvent::TaskStarted {
            task_id,
            name: name.clone(),
        });
        Self::ledger_append(
            ledger,
            keypair.as_ref(),
            ns,
            "shadow.task.started",
            serde_json::json!({ "task_id": task_id.to_string(), "name": name }),
        );

        let queue_c = Arc::clone(queue);
        let lifecycle_c = Arc::clone(lifecycle);
        let aci_c = Arc::clone(aci_dispatcher);
        let ledger_c = Arc::clone(ledger);
        let rt_c = Arc::clone(running_tasks);
        let keypair_c = Arc::clone(keypair);
        let ev_tx = event_tx.clone();
        let timeout = config.task_timeout_seconds;
        let ns_c = ns.clone();
        // Clone the allow-list so the spawned task owns it independently.
        let allowed_programs_c = config.allowed_programs.clone();

        let handle = tokio::spawn(async move {
            let result = Self::run_task_aci_gated(
                task.clone(),
                &aci_c,
                &ev_tx,
                timeout,
                &allowed_programs_c,
            )
            .await;
            let success = result.success;
            let duration_ms = result.duration_ms;

            let _ = queue_c.write().await.complete_task(task_id, result);

            {
                let mut lc = lifecycle_c.write().await;
                let _ = lc.transition(LifecycleEvent::TaskCompleted { task_id, success });
            }

            Self::ledger_append(
                &ledger_c,
                keypair_c.as_ref(),
                &ns_c,
                "shadow.task.completed",
                serde_json::json!({
                    "task_id": task_id.to_string(),
                    "name": name,
                    "success": success,
                    "duration_ms": duration_ms,
                }),
            );
            let _ = ev_tx.send(ShadowEvent::TaskCompleted {
                task_id,
                name,
                success,
                duration_ms,
            });

            rt_c.write().await.remove(&task_id);
        });

        running_tasks.write().await.insert(task_id, handle);
    }

    // ── ACI-gated execution ───────────────────────────────────────────────────

    /// ShadowTask.command 路由语义：
    ///   "aci:write:<path>"   → ACI WriteFile（内容取自 args[0]）
    ///   "aci:read:<path>"    → ACI ReadFile
    ///   "aci:syntax:<path>"  → ACI SyntaxCheck（语言取自 args[0]，默认 rust）
    ///   "aci:replace:<path>" → ACI ReplaceAstNode（old=args[0], new=args[1], lang=args[2]）
    ///   "aci:reality:<path>" → ACI RealityCheck
    ///   其他                 → 经 allow-list 校验的直接 exec（绝不经过 sh -c）
    async fn run_task_aci_gated(
        task: ShadowTask,
        aci: &Arc<AciDispatcher>,
        ev_tx: &ShadowEventTx,
        timeout_secs: u64,
        allowed_programs: &[String],
    ) -> TaskResult {
        let t0 = std::time::Instant::now();
        let r = tokio::time::timeout(
            tokio::time::Duration::from_secs(timeout_secs),
            Self::execute_inner(task.clone(), aci, ev_tx, allowed_programs),
        )
        .await;
        match r {
            Ok(mut res) => {
                res.duration_ms = t0.elapsed().as_millis() as u64;
                res
            }
            Err(_) => TaskResult {
                success: false,
                error: Some(format!("timed out after {timeout_secs}s")),
                duration_ms: t0.elapsed().as_millis() as u64,
                ..Default::default()
            },
        }
    }

    async fn execute_inner(
        task: ShadowTask,
        aci: &Arc<AciDispatcher>,
        ev_tx: &ShadowEventTx,
        allowed_programs: &[String],
    ) -> TaskResult {
        let task_id = task.id;
        let cmd = task.command.trim().to_string();

        // ── ACI WriteFile ────────────────────────────────────────────────────
        if let Some(rest) = cmd.strip_prefix("aci:write:") {
            let path = PathBuf::from(rest);
            let content = task.args.first().cloned().unwrap_or_default();
            let op_desc = format!("WriteFile {}", path.display());
            let r = aci.dispatch(AciAction::WriteFile {
                path: path.clone(),
                content,
                update_anchor: true,
            });
            let ok = r.is_ok();
            let _ = ev_tx.send(ShadowEvent::AciOperation {
                task_id,
                op: op_desc,
                ok,
            });
            return TaskResult {
                success: ok,
                output: r.data.map(|d| d.to_string()),
                error: if ok { None } else { r.error },
                files_written: if ok {
                    vec![path.display().to_string()]
                } else {
                    vec![]
                },
                aci_operations: 1,
                ..Default::default()
            };
        }

        // ── ACI ReadFile ─────────────────────────────────────────────────────
        if let Some(rest) = cmd.strip_prefix("aci:read:") {
            let path = PathBuf::from(rest);
            let op_desc = format!("ReadFile {}", path.display());
            let r = aci.dispatch(AciAction::ReadFile { path: path.clone() });
            let ok = r.is_ok();
            let _ = ev_tx.send(ShadowEvent::AciOperation {
                task_id,
                op: op_desc,
                ok,
            });
            return TaskResult {
                success: ok,
                output: r
                    .data
                    .as_ref()
                    .and_then(|d| d["content"].as_str())
                    .map(|s| s.to_string()),
                error: if ok { None } else { r.error },
                files_read: if ok {
                    vec![path.display().to_string()]
                } else {
                    vec![]
                },
                aci_operations: 1,
                ..Default::default()
            };
        }

        // ── ACI SyntaxCheck ──────────────────────────────────────────────────
        if let Some(rest) = cmd.strip_prefix("aci:syntax:") {
            let path = PathBuf::from(rest);
            let lang = task.args.first().cloned().unwrap_or_else(|| "rust".into());
            let op_desc = format!("SyntaxCheck {}", path.display());
            let r = aci.dispatch(AciAction::SyntaxCheck {
                path,
                language: lang,
            });
            let ok = r.is_ok()
                && r.data
                    .as_ref()
                    .and_then(|d| d["valid"].as_bool())
                    .unwrap_or(false);
            let _ = ev_tx.send(ShadowEvent::AciOperation {
                task_id,
                op: op_desc,
                ok,
            });
            return TaskResult {
                success: ok,
                output: r.data.map(|d| d.to_string()),
                error: if ok {
                    None
                } else {
                    Some("syntax invalid".into())
                },
                aci_operations: 1,
                ..Default::default()
            };
        }

        // ── ACI ReplaceAstNode ───────────────────────────────────────────────
        if let Some(rest) = cmd.strip_prefix("aci:replace:") {
            let path = PathBuf::from(rest);
            let old_text = task.args.first().cloned().unwrap_or_default();
            let new_text = task.args.get(1).cloned().unwrap_or_default();
            let lang = task.args.get(2).cloned().unwrap_or_else(|| "rust".into());
            let op_desc = format!("ReplaceAstNode {}", path.display());
            let r = aci.dispatch(AciAction::ReplaceAstNode {
                path,
                old_text,
                new_text,
                language: lang,
            });
            let ok = r.is_ok();
            let _ = ev_tx.send(ShadowEvent::AciOperation {
                task_id,
                op: op_desc,
                ok,
            });
            return TaskResult {
                success: ok,
                output: r.data.map(|d| d.to_string()),
                error: if ok { None } else { r.error },
                aci_operations: 1,
                ..Default::default()
            };
        }

        // ── ACI RealityCheck ─────────────────────────────────────────────────
        if let Some(rest) = cmd.strip_prefix("aci:reality:") {
            let path = PathBuf::from(rest);
            let op_desc = format!("RealityCheck {}", path.display());
            let r = aci.dispatch(AciAction::RealityCheck { path });
            let ok = r.is_ok();
            let _ = ev_tx.send(ShadowEvent::AciOperation {
                task_id,
                op: op_desc,
                ok,
            });
            return TaskResult {
                success: ok,
                output: r.data.map(|d| d.to_string()),
                error: if ok { None } else { r.error },
                aci_operations: 1,
                ..Default::default()
            };
        }

        // ── Direct exec（非 ACI 路径）— 经 allow-list 校验，绝不经过 sh -c ────
        //
        // Security: task.command is the program name (already parsed into
        // program + args by the caller); we NEVER concatenate into a shell
        // string.  The AllowList provides fail-closed enforcement: any program
        // absent from config.allowed_programs is rejected before fork().
        let spec = CommandSpec {
            program: task.command.clone(),
            args: task.args.clone(),
            env: task
                .env_vars
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<BTreeMap<_, _>>(),
            cwd: task.working_dir.as_deref().map(PathBuf::from),
        };
        let allow_list = AllowList::from_programs(allowed_programs.iter().cloned());
        let mut proc = match spec.into_tokio_command(&allow_list) {
            Ok(cmd) => cmd,
            Err(e) => {
                return TaskResult {
                    success: false,
                    error: Some(e.to_string()),
                    ..Default::default()
                };
            }
        };

        match proc.output().await {
            Ok(out) => {
                let success = out.status.success();
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                TaskResult {
                    success,
                    output: Some(if stderr.is_empty() {
                        stdout
                    } else if success {
                        format!("{stdout}\n--- stderr ---\n{stderr}")
                    } else {
                        stdout
                    }),
                    error: if success { None } else { Some(stderr) },
                    aci_operations: 0,
                    ..Default::default()
                }
            }
            Err(e) => TaskResult {
                success: false,
                error: Some(format!("spawn error: {e}")),
                ..Default::default()
            },
        }
    }

    // ── Ledger helper ─────────────────────────────────────────────────────────

    fn ledger_append(
        ledger: &Arc<EventLedger>,
        keypair: &ZaionKeypair,
        ns: &NamespaceKey,
        kind: &str,
        data: serde_json::Value,
    ) {
        if let Err(e) = ledger.append_signed_event(keypair, ns, kind, data, None) {
            error!(error = %e, kind = kind, "ledger append error");
        }
    }

    // ── Public API ────────────────────────────────────────────────────────────

    pub async fn submit_task(&self, task: ShadowTask) -> Result<(), ShadowError> {
        self.command_tx
            .as_ref()
            .ok_or(ShadowError::ExecutorNotRunning)?
            .send(ExecutorCommand::SubmitTask(Box::new(task)))
            .map_err(|_| ShadowError::ExecutorNotRunning)
    }

    pub async fn cancel_task(&self, task_id: TaskId) -> Result<(), ShadowError> {
        self.command_tx
            .as_ref()
            .ok_or(ShadowError::ExecutorNotRunning)?
            .send(ExecutorCommand::CancelTask(task_id))
            .map_err(|_| ShadowError::ExecutorNotRunning)
    }

    pub async fn get_task(&self, task_id: TaskId) -> Result<Option<ShadowTask>, ShadowError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .as_ref()
            .ok_or(ShadowError::ExecutorNotRunning)?
            .send(ExecutorCommand::GetTask(task_id, tx))
            .map_err(|_| ShadowError::ExecutorNotRunning)?;
        rx.await.map_err(|_| ShadowError::ExecutorNotRunning)
    }

    pub async fn list_tasks(&self) -> Result<Vec<ShadowTask>, ShadowError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .as_ref()
            .ok_or(ShadowError::ExecutorNotRunning)?
            .send(ExecutorCommand::ListTasks(tx))
            .map_err(|_| ShadowError::ExecutorNotRunning)?;
        rx.await.map_err(|_| ShadowError::ExecutorNotRunning)
    }

    pub async fn get_stats(&self) -> Result<crate::QueueStats, ShadowError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .as_ref()
            .ok_or(ShadowError::ExecutorNotRunning)?
            .send(ExecutorCommand::GetStats(tx))
            .map_err(|_| ShadowError::ExecutorNotRunning)?;
        rx.await.map_err(|_| ShadowError::ExecutorNotRunning)
    }

    pub async fn shutdown(&self) -> Result<(), ShadowError> {
        self.command_tx
            .as_ref()
            .ok_or(ShadowError::ExecutorNotRunning)?
            .send(ExecutorCommand::Shutdown)
            .map_err(|_| ShadowError::ExecutorNotRunning)
    }
}
