use crate::{
    context::ContextEngine,
    meta::MetaEngine,
    policy::{Policy, PolicyEngine},
    task::{Task, TaskEngine},
    task_async::{AsyncTask, AsyncTaskEngine, AsyncTaskHandler},
};
use std::sync::Arc;
use tempfile::tempdir;
use zaion_crypto::keypair::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_memory::skill::SkillStore;
use zaion_types::session::NamespaceKey;
use zaion_types::task::TaskStatus;

#[test]
fn test_task_engine_success() {
    let dir = tempdir().unwrap();
    let engine = TaskEngine::new(
        EventLedger::new(dir.path().join("rt.db")),
        ZaionKeypair::generate(),
        NamespaceKey("ns".into()),
    );
    let (task, event_id) = engine
        .execute(
            "test.echo",
            serde_json::json!({ "msg": "hi" }),
            &|_: &Task| Ok(serde_json::json!({ "result": "done" })),
        )
        .unwrap();
    assert_eq!(task.status, TaskStatus::Completed);
    assert!(task.output.is_some());
    assert!(event_id.0.starts_with("evt-"));
}

#[test]
fn test_task_engine_failure() {
    let dir = tempdir().unwrap();
    let engine = TaskEngine::new(
        EventLedger::new(dir.path().join("rt_fail.db")),
        ZaionKeypair::generate(),
        NamespaceKey("ns".into()),
    );
    let (task, _) = engine
        .execute("test.fail", serde_json::json!({}), &|_: &Task| {
            Err("something went wrong".into())
        })
        .unwrap();
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(task.error.as_deref(), Some("something went wrong"));
}

#[test]
fn test_meta_engine_distills_skill() {
    let dir = tempdir().unwrap();
    let meta = MetaEngine::new(
        EventLedger::new(dir.path().join("meta.db")),
        SkillStore::new(dir.path().join("skills.db")),
        ZaionKeypair::generate(),
        NamespaceKey("ns".into()),
    );
    let kp = ZaionKeypair::generate();
    let task = Task {
        task_id: "tsk-001".into(),
        principal_id: kp.principal_id().as_str().to_string(),
        session_key: "test".into(),
        task_type: "code_review".into(),
        input: serde_json::json!({}),
        output: Some(serde_json::json!({ "result": "ok" })),
        status: TaskStatus::Completed,
        error: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    let skill_id = meta.reflect(&task).unwrap();
    assert!(skill_id.is_some());
    let rules = meta.load_rules("code_review", 10).unwrap();
    assert_eq!(rules.len(), 1);
}

#[test]
fn test_policy_engine() {
    let policy = Policy {
        allowed_task_types: vec!["allowed.task".into()],
        ..Policy::default()
    };
    let engine = PolicyEngine::new(policy);
    assert!(engine.check_task_type("allowed.task").is_ok());
    assert!(engine.check_task_type("blocked.task").is_err());
    assert!(engine.check_task_count(50).is_ok());
    assert!(engine.check_task_count(100).is_err());
}

#[test]
fn test_context_engine_respects_budget() {
    let dir = tempdir().unwrap();
    let ledger = EventLedger::new(dir.path().join("ledger.db"));
    let engine = ContextEngine::new(dir.path(), "test-principal");
    let ctx = engine.build("rust memory", 500, &ledger).unwrap();
    assert!(
        ctx.budget_used <= 500,
        "budget_used must not exceed token_budget"
    );
    assert!(ctx.total_tokens <= 500);
    assert!(!ctx.system_prompt.is_empty());
}

#[test]
fn test_context_engine_always_includes_principal() {
    let dir = tempdir().unwrap();
    let ledger = EventLedger::new(dir.path().join("ledger.db"));
    let engine = ContextEngine::new(dir.path(), "my-principal-id-xyz");
    let ctx = engine.build("query", 8000, &ledger).unwrap();
    assert!(ctx.system_prompt.contains("my-principal-id-xyz"));
    assert!(ctx.chunks.iter().any(|c| c.layer == 6));
}

#[test]
fn test_context_engine_zero_budget_has_principal() {
    let dir = tempdir().unwrap();
    let ledger = EventLedger::new(dir.path().join("ledger.db"));
    let ctx = ContextEngine::new(dir.path(), "pid-zero")
        .build("q", 0, &ledger)
        .unwrap();
    assert!(ctx.chunks.iter().any(|c| c.layer == 6));
}

// ── Async task engine tests ───────────────────────────────────────────────────

#[test]
fn test_context_engine_large_history_keeps_budget_and_event_lineage() {
    let dir = tempdir().unwrap();
    let ledger = EventLedger::new(dir.path().join("ledger.db"));
    let kp = ZaionKeypair::generate();
    let ns = NamespaceKey("large-history".into());
    let principal_id = kp.principal_id().as_str().to_string();

    for i in 0..300 {
        ledger
            .append_signed_event(
                &kp,
                &ns,
                "channel.received",
                serde_json::json!({
                    "content": format!("large historical turn {i}: traceable context compression evidence repeated for budget pressure")
                }),
                None,
            )
            .unwrap();
    }

    let ctx = ContextEngine::new(dir.path(), principal_id)
        .build("traceable context compression", 4000, &ledger)
        .unwrap();
    assert!(ctx.total_tokens <= 4000);
    assert!(ctx.budget_used <= 4000);
    let recent = ctx
        .chunks
        .iter()
        .find(|chunk| chunk.label == "recent_events")
        .expect("recent events chunk");
    assert!(!recent.lineage.is_empty());
    assert!(
        recent
            .lineage
            .iter()
            .all(|entry| entry.starts_with("ledger:event:evt-")),
        "recent lineage must contain exact ledger event ids: {:?}",
        recent.lineage
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_task_engine_basic_success() {
    let dir = tempdir().unwrap();
    let engine = AsyncTaskEngine::new(
        EventLedger::new(dir.path().join("async.db")),
        ZaionKeypair::generate(),
        NamespaceKey("ns".into()),
    );

    let handler: AsyncTaskHandler = Arc::new(|_task: AsyncTask| {
        Box::pin(async { Ok(serde_json::json!({ "result": "async_done" })) })
    });

    let (task, event_id) = engine
        .execute("test.async".into(), serde_json::json!({ "x": 1 }), handler)
        .await
        .unwrap();

    assert_eq!(task.status, TaskStatus::Completed);
    assert!(task.output.is_some());
    assert!(event_id.0.starts_with("evt-"));
    engine.shutdown();
}

/// C1.1 批量 spawn benchmark:
/// 50 个任务同时提交，全部并发执行，验证吞吐量与正确性。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn async_task_engine_batch_spawn_50() {
    let dir = tempdir().unwrap();
    let engine = AsyncTaskEngine::new(
        EventLedger::new(dir.path().join("batch.db")),
        ZaionKeypair::generate(),
        NamespaceKey("batch".into()),
    );

    const N: usize = 50;

    let handler: AsyncTaskHandler = Arc::new(|task: AsyncTask| {
        Box::pin(async move {
            // Simulate lightweight async work
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            Ok(serde_json::json!({ "echo": task.input }))
        })
    });

    let start = std::time::Instant::now();

    // Submit all N tasks concurrently
    let mut futures = Vec::with_capacity(N);
    for i in 0..N {
        let h = Arc::clone(&handler);
        let f = engine.execute(format!("bench.task.{i}"), serde_json::json!({ "i": i }), h);
        futures.push(f);
    }

    let results = futures::future::join_all(futures).await;
    let elapsed = start.elapsed();

    let ok_count = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(ok_count, N, "{} of {} tasks failed", N - ok_count, N);

    // 50 tasks × 5ms each = 250ms serial.
    // Concurrent execution + WAL ledger writes: finish well under 3s in debug builds.
    // (Connection-open overhead dominates in debug; release builds are ~10× faster.)
    assert!(
        elapsed.as_millis() < 3000,
        "batch of {N} tasks took {}ms — possible regression to serial execution",
        elapsed.as_millis()
    );

    engine.shutdown();
}
