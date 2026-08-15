//! zaion-shadow integration tests — Sprint 5, Genesis v4.0
//!
//! 覆盖：
//!   queue.rs        — enqueue/dequeue/cancel/retry/stats/priority
//!   lifecycle.rs    — FSM 状态转移/invalid transitions
//!   executor.rs     — start/submit/list/get_stats/shutdown
//!   ACI integration — aci:write / aci:read / aci:syntax / aci:replace
//!   event channel   — ShadowEvent broadcast received by subscriber
#[cfg(test)]
mod queue_tests {
    use crate::{ShadowTask, TaskQueue, TaskResult, TaskStatus};
    use uuid::Uuid;

    fn make_task(name: &str) -> ShadowTask {
        ShadowTask::new(
            name.to_string(),
            "echo".to_string(),
            vec!["hello".to_string()],
        )
    }

    #[test]
    fn enqueue_and_stats() {
        let mut q = TaskQueue::new(10, 2);
        q.enqueue(make_task("t1")).unwrap();
        let stats = q.stats();
        assert_eq!(stats.queued, 1);
        assert_eq!(stats.total_tasks, 1);
    }

    #[test]
    fn queue_full_returns_error() {
        let mut q = TaskQueue::new(2, 2);
        q.enqueue(make_task("t1")).unwrap();
        q.enqueue(make_task("t2")).unwrap();
        assert!(q.enqueue(make_task("t3")).is_err());
    }

    #[test]
    fn dequeue_moves_to_running() {
        let mut q = TaskQueue::new(10, 2);
        q.enqueue(make_task("t1")).unwrap();
        let task = q.dequeue().unwrap();
        assert_eq!(task.status, TaskStatus::Running);
        assert_eq!(q.stats().running, 1);
        assert_eq!(q.stats().queued, 0);
    }

    #[test]
    fn dequeue_respects_concurrent_limit() {
        let mut q = TaskQueue::new(10, 1);
        q.enqueue(make_task("t1")).unwrap();
        q.enqueue(make_task("t2")).unwrap();
        assert!(q.dequeue().is_some());
        assert!(
            q.dequeue().is_none(),
            "should not dequeue beyond concurrent limit"
        );
    }

    #[test]
    fn complete_task_marks_completed() {
        let mut q = TaskQueue::new(10, 2);
        let t = make_task("t1");
        let id = t.id;
        q.enqueue(t).unwrap();
        q.dequeue().unwrap();
        let result = TaskResult {
            success: true,
            duration_ms: 10,
            ..Default::default()
        };
        q.complete_task(id, result).unwrap();
        assert_eq!(q.get_task(&id).unwrap().status, TaskStatus::Completed);
    }

    #[test]
    fn failed_task_with_retry_re_queues() {
        let mut q = TaskQueue::new(10, 2);
        let t = ShadowTask::new("t1".into(), "echo".into(), vec![]).with_retries(1);
        let id = t.id;
        q.enqueue(t).unwrap();
        q.dequeue().unwrap();
        let result = TaskResult {
            success: false,
            duration_ms: 5,
            ..Default::default()
        };
        q.complete_task(id, result).unwrap();
        let task = q.get_task(&id).unwrap();
        assert_eq!(task.status, TaskStatus::Queued);
        assert_eq!(task.retry_count, 1);
    }

    #[test]
    fn cancel_queued_task() {
        let mut q = TaskQueue::new(10, 2);
        let t = make_task("t1");
        let id = t.id;
        q.enqueue(t).unwrap();
        q.cancel_task(id).unwrap();
        assert_eq!(q.get_task(&id).unwrap().status, TaskStatus::Cancelled);
    }

    #[test]
    fn priority_ordering() {
        let mut q = TaskQueue::new(10, 1);
        let lo = ShadowTask::new("lo".into(), "echo".into(), vec![]).with_priority(0);
        let hi = ShadowTask::new("hi".into(), "echo".into(), vec![]).with_priority(10);
        q.enqueue(lo).unwrap();
        q.enqueue(hi).unwrap();
        assert_eq!(q.dequeue().unwrap().name, "hi");
    }

    #[test]
    fn not_found_returns_error() {
        let mut q = TaskQueue::new(10, 2);
        assert!(q.cancel_task(Uuid::new_v4()).is_err());
    }

    #[test]
    fn list_tasks_includes_all() {
        let mut q = TaskQueue::new(10, 2);
        q.enqueue(make_task("t1")).unwrap();
        q.enqueue(make_task("t2")).unwrap();
        q.dequeue();
        assert_eq!(q.list_tasks().len(), 2);
    }

    #[test]
    fn has_capacity_to_run() {
        let mut q = TaskQueue::new(10, 2);
        assert!(!q.has_capacity_to_run()); // no tasks
        q.enqueue(make_task("t1")).unwrap();
        assert!(q.has_capacity_to_run());
        q.dequeue().unwrap();
        assert!(!q.has_capacity_to_run()); // no more queued tasks
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use crate::{LifecycleEvent, LifecycleState, ShadowLifecycle};
    use uuid::Uuid;

    #[test]
    fn initial_state_is_idle() {
        assert_eq!(ShadowLifecycle::new().state, LifecycleState::Idle);
    }

    #[test]
    fn idle_to_starting() {
        let mut lc = ShadowLifecycle::new();
        lc.transition(LifecycleEvent::Start).unwrap();
        assert_eq!(lc.state, LifecycleState::Starting);
    }

    #[test]
    fn starting_to_running_on_task_queued() {
        let mut lc = ShadowLifecycle::new();
        lc.transition(LifecycleEvent::Start).unwrap();
        lc.transition(LifecycleEvent::TaskQueued {
            task_id: Uuid::new_v4(),
        })
        .unwrap();
        assert_eq!(lc.state, LifecycleState::Running);
    }

    #[test]
    fn running_task_started_stays_running() {
        let mut lc = ShadowLifecycle::new();
        lc.transition(LifecycleEvent::Start).unwrap();
        lc.transition(LifecycleEvent::TaskQueued {
            task_id: Uuid::new_v4(),
        })
        .unwrap();
        lc.transition(LifecycleEvent::TaskStarted {
            task_id: Uuid::new_v4(),
        })
        .unwrap();
        assert_eq!(lc.state, LifecycleState::Running);
    }

    #[test]
    fn task_completed_increments_counter() {
        let mut lc = ShadowLifecycle::new();
        lc.transition(LifecycleEvent::Start).unwrap();
        lc.transition(LifecycleEvent::TaskQueued {
            task_id: Uuid::new_v4(),
        })
        .unwrap();
        lc.transition(LifecycleEvent::TaskCompleted {
            task_id: Uuid::new_v4(),
            success: true,
        })
        .unwrap();
        assert_eq!(lc.tasks_processed, 1);
        assert_eq!(lc.tasks_failed, 0);
    }

    #[test]
    fn task_failed_increments_failed_counter() {
        let mut lc = ShadowLifecycle::new();
        lc.transition(LifecycleEvent::Start).unwrap();
        lc.transition(LifecycleEvent::TaskQueued {
            task_id: Uuid::new_v4(),
        })
        .unwrap();
        lc.transition(LifecycleEvent::TaskCompleted {
            task_id: Uuid::new_v4(),
            success: false,
        })
        .unwrap();
        assert_eq!(lc.tasks_failed, 1);
    }

    #[test]
    fn pause_resume_cycle() {
        let mut lc = ShadowLifecycle::new();
        lc.transition(LifecycleEvent::Start).unwrap();
        lc.transition(LifecycleEvent::TaskQueued {
            task_id: Uuid::new_v4(),
        })
        .unwrap();
        lc.transition(LifecycleEvent::Pause).unwrap();
        assert_eq!(lc.state, LifecycleState::Pausing);
        lc.transition(LifecycleEvent::TaskQueued {
            task_id: Uuid::new_v4(),
        })
        .unwrap();
        assert_eq!(lc.state, LifecycleState::Paused);
        lc.transition(LifecycleEvent::Resume).unwrap();
        assert_eq!(lc.state, LifecycleState::Resuming);
    }

    #[test]
    fn fail_from_any_state() {
        let mut lc = ShadowLifecycle::new();
        lc.transition(LifecycleEvent::Fail {
            reason: "crash".into(),
        })
        .unwrap();
        assert_eq!(lc.state, LifecycleState::Failed);
    }

    #[test]
    fn invalid_transition_returns_error() {
        let mut lc = ShadowLifecycle::new(); // Idle
        let res = lc.transition(LifecycleEvent::TaskStarted {
            task_id: Uuid::new_v4(),
        });
        assert!(res.is_err());
    }

    #[test]
    fn is_active_states() {
        let mut lc = ShadowLifecycle::new();
        assert!(!lc.is_active());
        lc.transition(LifecycleEvent::Start).unwrap();
        assert!(lc.is_active());
    }

    #[test]
    fn can_accept_tasks_only_when_running() {
        let mut lc = ShadowLifecycle::new();
        assert!(!lc.can_accept_tasks());
        lc.transition(LifecycleEvent::Start).unwrap();
        assert!(!lc.can_accept_tasks());
        lc.transition(LifecycleEvent::TaskQueued {
            task_id: Uuid::new_v4(),
        })
        .unwrap();
        assert!(lc.can_accept_tasks());
    }

    #[test]
    fn update_uptime_no_panic() {
        let mut lc = ShadowLifecycle::new();
        lc.transition(LifecycleEvent::Start).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        lc.update_uptime();
        assert!(lc.uptime_seconds < 5);
    }
}

#[cfg(test)]
mod executor_tests {
    use crate::{ExecutorConfig, ShadowEvent, ShadowExecutor, ShadowTask};
    use std::time::Duration;

    fn temp_db(suffix: &str) -> String {
        std::env::temp_dir()
            .join(format!("zaion_shex_{}_{}.db", suffix, uuid::Uuid::new_v4()))
            .to_string_lossy()
            .to_string()
    }

    fn test_config() -> ExecutorConfig {
        ExecutorConfig {
            ledger_db_path: temp_db("ledger"),
            aci_reality_db_path: temp_db("reality"),
            aci_toxic_db_path: temp_db("toxic"),
            heartbeat_interval_ms: 20,
            task_timeout_seconds: 10,
            // Allow "echo" so that existing executor smoke-tests can run.
            allowed_programs: vec!["echo".to_string()],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn executor_start_and_shutdown() {
        let mut ex = ShadowExecutor::new(test_config()).unwrap();
        ex.start().await.unwrap();
        ex.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn submit_and_list_task() {
        let mut ex = ShadowExecutor::new(test_config()).unwrap();
        ex.start().await.unwrap();
        ex.submit_task(ShadowTask::new(
            "list_test".into(),
            "echo".into(),
            vec!["hi".into()],
        ))
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        let tasks = ex.list_tasks().await.unwrap();
        assert!(!tasks.is_empty());
        ex.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn get_stats_reflects_queue() {
        let mut ex = ShadowExecutor::new(test_config()).unwrap();
        ex.start().await.unwrap();
        ex.submit_task(ShadowTask::new(
            "stats".into(),
            "echo".into(),
            vec!["x".into()],
        ))
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        let stats = ex.get_stats().await.unwrap();
        // At least 1 task should exist in some state
        assert!(stats.total_tasks > 0 || stats.completed > 0 || stats.running > 0);
        ex.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn task_completes_successfully() {
        let mut ex = ShadowExecutor::new(test_config()).unwrap();
        ex.start().await.unwrap();

        let task = ShadowTask::new("echo_task".into(), "echo".into(), vec!["hello".into()]);
        let task_id = task.id;
        ex.submit_task(task).await.unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if tokio::time::Instant::now() > deadline {
                panic!("task did not complete in time");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
            if let Ok(Some(t)) = ex.get_task(task_id).await {
                if t.is_terminal() {
                    assert!(t.result.as_ref().map(|r| r.success).unwrap_or(false));
                    break;
                }
            }
        }
        ex.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shadow_event_executor_started_received() {
        let mut ex = ShadowExecutor::new(test_config()).unwrap();
        let mut rx = ex.subscribe();
        ex.start().await.unwrap();

        let ev = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("timeout")
            .expect("recv error");
        assert!(matches!(ev, ShadowEvent::ExecutorStarted));

        ex.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shadow_event_task_spawned_received() {
        let mut ex = ShadowExecutor::new(test_config()).unwrap();
        let mut rx = ex.subscribe();
        ex.start().await.unwrap();

        // drain ExecutorStarted
        let _ = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await;

        ex.submit_task(ShadowTask::new("broadcast".into(), "echo".into(), vec![]))
            .await
            .unwrap();

        let ev = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("timeout")
            .expect("recv error");
        assert!(
            matches!(ev, ShadowEvent::TaskSpawned { .. }),
            "expected TaskSpawned, got {ev:?}"
        );

        ex.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shadow_event_task_completed_received() {
        let mut ex = ShadowExecutor::new(test_config()).unwrap();
        let mut rx = ex.subscribe();
        ex.start().await.unwrap();

        ex.submit_task(ShadowTask::new("done".into(), "echo".into(), vec![]))
            .await
            .unwrap();

        // Drain events until TaskCompleted or timeout
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let mut found = false;
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
                Ok(Ok(ShadowEvent::TaskCompleted { success, .. })) => {
                    assert!(success, "echo task should succeed");
                    found = true;
                    break;
                }
                Ok(Ok(_)) => {} // other events
                _ => break,
            }
        }
        assert!(found, "TaskCompleted event was not received");
        ex.shutdown().await.unwrap();
    }

    // ── allow-list security tests ─────────────────────────────────────────────

    /// A program absent from `allowed_programs` must NOT be executed; the task
    /// must fail with an error message that mentions the allow-list.
    #[tokio::test]
    async fn allowlist_miss_fails_task() {
        let mut config = test_config();
        config.allowed_programs = vec![]; // empty ⇒ fail-closed
        let mut ex = ShadowExecutor::new(config).unwrap();
        ex.start().await.unwrap();

        let task = ShadowTask::new(
            "blocked_echo".into(),
            "echo".into(), // "echo" NOT in allow-list
            vec!["secret".into()],
        );
        let task_id = task.id;
        ex.submit_task(task).await.unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if tokio::time::Instant::now() > deadline {
                panic!("blocked task did not complete in time");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
            if let Ok(Some(t)) = ex.get_task(task_id).await {
                if t.is_terminal() {
                    assert!(
                        !t.result.as_ref().map(|r| r.success).unwrap_or(true),
                        "blocked program must not succeed"
                    );
                    let err_msg = t
                        .result
                        .as_ref()
                        .and_then(|r| r.error.as_deref())
                        .unwrap_or("");
                    assert!(
                        err_msg.contains("allow-list"),
                        "error must mention allow-list, got: {err_msg}"
                    );
                    break;
                }
            }
        }
        ex.shutdown().await.unwrap();
    }

    /// Shell metacharacters inside `ShadowTask.args` must be passed literally.
    /// We submit `echo` with an arg containing `; rm -rf /` and verify the
    /// task completes successfully (echo itself just prints the literal string).
    #[tokio::test]
    async fn shell_metacharacter_in_args_is_literal_exec() {
        let mut config = test_config(); // "echo" is in allowed_programs
        config.task_timeout_seconds = 5;
        let mut ex = ShadowExecutor::new(config).unwrap();
        ex.start().await.unwrap();

        let task = ShadowTask::new(
            "safe_echo".into(),
            "echo".into(),
            vec!["hello; rm -rf /".into()], // metacharacter is a literal arg
        );
        let task_id = task.id;
        ex.submit_task(task).await.unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if tokio::time::Instant::now() > deadline {
                panic!("echo task did not complete");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
            if let Ok(Some(t)) = ex.get_task(task_id).await {
                if t.is_terminal() {
                    assert!(
                        t.result.as_ref().map(|r| r.success).unwrap_or(false),
                        "echo with literal arg must succeed"
                    );
                    break;
                }
            }
        }
        ex.shutdown().await.unwrap();
    }
}

#[cfg(test)]
mod aci_integration_tests {
    use crate::{ExecutorConfig, ShadowExecutor, ShadowTask};
    use std::time::Duration;

    fn temp_db(suffix: &str) -> String {
        std::env::temp_dir()
            .join(format!("zaion_aci_{}_{}.db", suffix, uuid::Uuid::new_v4()))
            .to_string_lossy()
            .to_string()
    }

    fn test_config() -> ExecutorConfig {
        ExecutorConfig {
            ledger_db_path: temp_db("ledger"),
            aci_reality_db_path: temp_db("reality"),
            aci_toxic_db_path: temp_db("toxic"),
            heartbeat_interval_ms: 20,
            task_timeout_seconds: 10,
            // ACI integration tasks use "aci:*" prefixes and skip the allow-list
            // path entirely, so the default empty list is fine here.
            ..Default::default()
        }
    }

    fn temp_path(ext: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("zaion_aci_{}.{}", uuid::Uuid::new_v4(), ext))
    }

    async fn wait_done(ex: &ShadowExecutor, id: uuid::Uuid) -> ShadowTask {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
        loop {
            if tokio::time::Instant::now() > deadline {
                panic!("ACI task {id} did not complete in time");
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
            if let Ok(Some(t)) = ex.get_task(id).await {
                if t.is_terminal() {
                    return t;
                }
            }
        }
    }

    #[tokio::test]
    async fn aci_write_and_read_file() {
        let mut ex = ShadowExecutor::new(test_config()).unwrap();
        ex.start().await.unwrap();

        let path = temp_path("txt");
        let path_str = path.to_string_lossy().to_string();

        let wt = ShadowTask::new(
            "write".into(),
            format!("aci:write:{}", path_str),
            vec!["hello from shadow".into()],
        );
        let wid = wt.id;
        ex.submit_task(wt).await.unwrap();
        let wr = wait_done(&ex, wid).await;
        assert!(
            wr.result.as_ref().map(|r| r.success).unwrap_or(false),
            "aci:write failed"
        );

        let rt = ShadowTask::new("read".into(), format!("aci:read:{}", path_str), vec![]);
        let rid = rt.id;
        ex.submit_task(rt).await.unwrap();
        let rr = wait_done(&ex, rid).await;
        assert!(
            rr.result.as_ref().map(|r| r.success).unwrap_or(false),
            "aci:read failed"
        );
        let output = rr
            .result
            .as_ref()
            .and_then(|r| r.output.as_deref())
            .unwrap_or("");
        assert!(
            output.contains("hello from shadow"),
            "read output mismatch: '{output}'"
        );

        ex.shutdown().await.unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn aci_write_invalid_toml_rejected() {
        let mut ex = ShadowExecutor::new(test_config()).unwrap();
        ex.start().await.unwrap();

        let path = temp_path("toml");
        std::fs::write(&path, "").unwrap();
        let task = ShadowTask::new(
            "bad_toml".into(),
            format!("aci:write:{}", path.display()),
            vec!["[broken\nkey".into()],
        );
        let tid = task.id;
        ex.submit_task(task).await.unwrap();
        let t = wait_done(&ex, tid).await;
        assert!(
            !t.result.as_ref().map(|r| r.success).unwrap_or(true),
            "invalid TOML should fail syntax gate"
        );

        ex.shutdown().await.unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn aci_syntax_check_valid_toml() {
        let mut ex = ShadowExecutor::new(test_config()).unwrap();
        ex.start().await.unwrap();

        let path = temp_path("toml");
        std::fs::write(&path, "[core]\nkey = \"value\"\n").unwrap();
        let task = ShadowTask::new(
            "syntax".into(),
            format!("aci:syntax:{}", path.display()),
            vec!["toml".into()],
        );
        let tid = task.id;
        ex.submit_task(task).await.unwrap();
        let t = wait_done(&ex, tid).await;
        assert!(
            t.result.as_ref().map(|r| r.success).unwrap_or(false),
            "syntax check should pass"
        );

        ex.shutdown().await.unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn aci_replace_ast_node() {
        let mut ex = ShadowExecutor::new(test_config()).unwrap();
        ex.start().await.unwrap();

        let path = temp_path("rs");
        std::fs::write(&path, "fn foo() { let x = 1; }").unwrap();
        let task = ShadowTask::new(
            "ast_replace".into(),
            format!("aci:replace:{}", path.display()),
            vec!["let x = 1;".into(), "let x = 42;".into(), "rust".into()],
        );
        let tid = task.id;
        ex.submit_task(task).await.unwrap();
        let t = wait_done(&ex, tid).await;
        assert!(
            t.result.as_ref().map(|r| r.success).unwrap_or(false),
            "AST replace should succeed"
        );
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("let x = 42;"), "AST replace did not apply");

        ex.shutdown().await.unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn aci_write_records_aci_op_count() {
        let mut ex = ShadowExecutor::new(test_config()).unwrap();
        ex.start().await.unwrap();

        let path = temp_path("txt");
        let task = ShadowTask::new(
            "aci_ops".into(),
            format!("aci:write:{}", path.display()),
            vec!["data".into()],
        );
        let tid = task.id;
        ex.submit_task(task).await.unwrap();
        let t = wait_done(&ex, tid).await;
        let ops = t.result.as_ref().map(|r| r.aci_operations).unwrap_or(0);
        assert_eq!(ops, 1, "should record 1 ACI operation");

        ex.shutdown().await.unwrap();
        let _ = std::fs::remove_file(&path);
    }
}
