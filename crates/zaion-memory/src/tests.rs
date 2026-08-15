use crate::projection::ProjectionStore;
use crate::skill::SkillStore;
use crate::slimmer::{ContextLayer, ContextSlimmer};
use tempfile::tempdir;
use zaion_crypto::keypair::ZaionKeypair;
use zaion_types::session::SessionKey;

#[test]
fn test_skill_upsert_and_query() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("skills.db");
    let store = SkillStore::new(&db);
    let kp = ZaionKeypair::generate();
    let pid = kp.principal_id();
    let id1 = store
        .upsert(
            &pid,
            "code_review",
            &["rust", "safety"],
            "always check unwrap calls",
            1.0,
        )
        .unwrap();
    assert!(id1.starts_with("skl-"));
    let id2 = store
        .upsert(
            &pid,
            "code_review",
            &["rust", "safety"],
            "always check unwrap calls",
            0.5,
        )
        .unwrap();
    assert_eq!(id1, id2, "same rule should update, not insert");
    let entry = store.get(&id1).unwrap().unwrap();
    assert!((entry.confidence - 1.5).abs() < 1e-9);
    assert_eq!(entry.usage_count, 1);
    let results = store.query(&pid, "code_review", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].rule_text, "always check unwrap calls");
}

#[test]
fn test_skill_multiple_types() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("skills2.db");
    let store = SkillStore::new(&db);
    let kp = ZaionKeypair::generate();
    let pid = kp.principal_id();
    store
        .upsert(
            &pid,
            "task_planning",
            &["telegram"],
            "break tasks into subtasks",
            1.0,
        )
        .unwrap();
    store
        .upsert(
            &pid,
            "code_review",
            &["rust"],
            "prefer Result over panic",
            1.0,
        )
        .unwrap();
    let planning = store.query(&pid, "task_planning", 10).unwrap();
    assert_eq!(planning.len(), 1);
    let review = store.query(&pid, "code_review", 10).unwrap();
    assert_eq!(review.len(), 1);
}

#[test]
fn test_skill_delete() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("skills_del.db");
    let store = SkillStore::new(&db);
    let kp = ZaionKeypair::generate();
    let pid = kp.principal_id();
    let id = store.upsert(&pid, "chat", &[], "test rule", 1.0).unwrap();
    let before = store.query(&pid, "chat", 10).unwrap();
    assert_eq!(before.len(), 1);
    store.delete(&pid, &id).unwrap();
    let after = store.query(&pid, "chat", 10).unwrap();
    assert_eq!(after.len(), 0);
}

#[test]
fn test_skill_search_text() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("skills_search.db");
    let store = SkillStore::new(&db);
    let kp = ZaionKeypair::generate();
    let pid = kp.principal_id();
    store
        .upsert(&pid, "chat", &[], "always validate user input", 1.0)
        .unwrap();
    store
        .upsert(&pid, "chat", &[], "prefer immutable data structures", 1.0)
        .unwrap();
    store
        .upsert(&pid, "chat", &[], "validate before processing", 0.8)
        .unwrap();
    let results = store.search_text(&pid, "validate", 10).unwrap();
    assert_eq!(results.len(), 2);
    let results2 = store.search_text(&pid, "immutable", 10).unwrap();
    assert_eq!(results2.len(), 1);
}

#[test]
fn test_projection_upsert_and_get() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("proj.db");
    let store = ProjectionStore::new(&db);
    let kp = ZaionKeypair::generate();
    let pid = kp.principal_id();
    let sk = SessionKey("test__ws__proj__tg__thread__sess".into());
    let content = serde_json::json!({ "goal": "build zaion", "status": "in_progress" });
    let id1 = store
        .upsert(&pid, &sk, 5, content.clone(), "evt-000")
        .unwrap();
    assert!(id1.starts_with("prj-"));
    let fetched = store.get(&sk, 5).unwrap().unwrap();
    assert_eq!(fetched.content_json["goal"], "build zaion");
    let updated_content = serde_json::json!({ "goal": "build zaion", "status": "completed" });
    let id2 = store
        .upsert(&pid, &sk, 5, updated_content, "evt-001")
        .unwrap();
    assert_eq!(id1, id2, "upsert must update existing projection");
    let fetched2 = store.get(&sk, 5).unwrap().unwrap();
    assert_eq!(fetched2.content_json["status"], "completed");
    assert_eq!(fetched2.event_cursor, "evt-001");
}

#[test]
fn test_projection_list_layers() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("proj2.db");
    let store = ProjectionStore::new(&db);
    let kp = ZaionKeypair::generate();
    let pid = kp.principal_id();
    let sk = SessionKey("test__ws__proj__tg__thread__sess".into());
    for layer in 1u8..=7 {
        store
            .upsert(
                &pid,
                &sk,
                layer,
                serde_json::json!({ "layer": layer }),
                "evt-000",
            )
            .unwrap();
    }
    let all = store.list(&pid, &sk).unwrap();
    assert_eq!(all.len(), 7);
    assert_eq!(all[0].layer, 1);
    assert_eq!(all[6].layer, 7);
}

#[test]
fn test_context_slimmer() {
    let slimmer = ContextSlimmer::new(8192);
    let layers: Vec<ContextLayer> = (1u8..=7)
        .map(|i| ContextLayer {
            layer: i,
            content: serde_json::json!({ "data": format!("layer {} content", i) }),
            compressed: false,
        })
        .collect();
    let sk = SessionKey("test__session".into());
    let slimmed = slimmer.slim(layers);
    assert_eq!(slimmed.layers.len(), 7);
    for l in &slimmed.layers {
        if l.layer <= 4 {
            assert!(!l.compressed, "L1-L4 must not be compressed");
        } else {
            assert!(l.compressed, "L5-L7 should be compressed");
        }
    }
    let messages = slimmer.build_context_messages(&sk, &slimmed);
    assert_eq!(messages.len(), 7);
    assert!(messages[4]["content"]
        .as_str()
        .unwrap()
        .contains("(compressed)"));
    assert!(!messages[0]["content"]
        .as_str()
        .unwrap()
        .contains("(compressed)"));
}
