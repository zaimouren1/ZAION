//! Integration tests for typed memory system
//!
//! Tests the complete end-to-end flow:
//! 1. Conversation turn → AutoMemoryExtractor
//! 2. Extracted candidates → TypedMemoryStore
//! 3. Prefetch from TypedMemoryStore
//! 4. Tool calls via BuiltinMemoryProvider

use std::sync::Arc;
use tempfile::tempdir;
use zaion_crypto::keypair::ZaionKeypair;
use zaion_memory::{
    AutoMemoryExtractor, BuiltinMemoryProvider, MemoryProvider, MemoryRuntimeConfig, MemoryType,
    PrincipalMemoryStore, SemanticStore, TypedMemoryStore,
};

#[test]
fn test_end_to_end_typed_memory_extraction() {
    let dir = tempdir().unwrap();
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();

    let typed_store = Arc::new(TypedMemoryStore::new(dir.path()));

    // Simulate a conversation turn
    let user_content = "I'm a senior Rust engineer. The deadline is next Friday.";
    let assistant_content = "[remember] tone: concise";

    // Extract memories
    let result =
        AutoMemoryExtractor::extract_from_turn(user_content, assistant_content, "session-1");

    assert!(
        !result.candidates.is_empty(),
        "Should extract at least one memory"
    );

    // Convert to entries and persist
    let entries = AutoMemoryExtractor::candidates_to_entries(
        result.candidates,
        principal_id.as_str(),
        "session-1",
        "test",
    );

    for entry in &entries {
        typed_store.upsert(entry).unwrap();
    }

    // Verify memories were stored
    let all_memories = typed_store.list_all(principal_id.as_str(), false).unwrap();
    assert!(!all_memories.is_empty(), "Should have stored memories");

    // Check memory types
    let types: Vec<_> = all_memories.iter().map(|e| e.memory_type).collect();
    assert!(
        types.contains(&MemoryType::User) || types.contains(&MemoryType::Project),
        "Should contain User or Project memories"
    );

    // Verify we can retrieve by type and key
    for entry in &all_memories {
        let retrieved = typed_store
            .get(principal_id.as_str(), entry.memory_type, &entry.key)
            .unwrap();
        assert!(retrieved.is_some(), "Should retrieve memory by key");
    }
}

#[test]
fn test_runtime_provider_prefetch_typed_memory() {
    let dir = tempdir().unwrap();
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();

    let typed_store = Arc::new(TypedMemoryStore::new(dir.path()));
    let semantic_store = Arc::new(SemanticStore::new(dir.path()));
    let principal_store = Arc::new(PrincipalMemoryStore::new(dir.path()));

    // Create a typed memory entry
    let entry = zaion_memory::TypedMemoryEntry::new(
        MemoryType::User,
        "role",
        "senior Rust engineer",
        "session-1",
        "test",
        &kp,
    );
    typed_store.upsert(&entry).unwrap();

    // Create provider
    let provider = BuiltinMemoryProvider::new(
        principal_id.to_string(),
        semantic_store,
        principal_store,
        typed_store,
        MemoryRuntimeConfig::default(),
    );

    // Prefetch should include typed memories
    let context = provider.prefetch("test query", "session-1").unwrap();

    assert!(
        context.contains("USER memories") || context.contains("role"),
        "Prefetch should include typed memories"
    );
}

#[test]
fn test_runtime_provider_sync_turn_extraction() {
    let dir = tempdir().unwrap();
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();

    let typed_store = Arc::new(TypedMemoryStore::new(dir.path()));
    let semantic_store = Arc::new(SemanticStore::new(dir.path()));
    let principal_store = Arc::new(PrincipalMemoryStore::new(dir.path()));

    let provider = BuiltinMemoryProvider::new(
        principal_id.to_string(),
        semantic_store,
        principal_store,
        typed_store.clone(),
        MemoryRuntimeConfig::default(),
    );

    // Sync a turn
    let user_content = "I prefer concise answers. My name is Alice.";
    let assistant_content = "Got it, I'll keep responses concise.";

    provider
        .sync_turn(user_content, assistant_content, "session-1")
        .unwrap();

    // Check that memories were extracted and stored
    let memories = typed_store.list_all(principal_id.as_str(), false).unwrap();

    assert!(
        !memories.is_empty(),
        "Should have extracted memories from turn"
    );

    // Should have user memories
    let user_memories: Vec<_> = memories
        .iter()
        .filter(|m| m.memory_type == MemoryType::User)
        .collect();

    assert!(
        !user_memories.is_empty(),
        "Should have extracted user memories"
    );
}

#[test]
fn test_tool_api_typed_memory() {
    let dir = tempdir().unwrap();
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();

    let typed_store = Arc::new(TypedMemoryStore::new(dir.path()));
    let semantic_store = Arc::new(SemanticStore::new(dir.path()));
    let principal_store = Arc::new(PrincipalMemoryStore::new(dir.path()));

    let provider = BuiltinMemoryProvider::new(
        principal_id.to_string(),
        semantic_store,
        principal_store,
        typed_store,
        MemoryRuntimeConfig::default(),
    );

    // Test memory_typed_set
    let set_result = provider
        .handle_tool_call(
            "memory_typed_set",
            &serde_json::json!({
                "memory_type": "user",
                "key": "role",
                "content": "senior engineer",
                "confidence": 0.9
            }),
        )
        .unwrap();

    let set_parsed: serde_json::Value = serde_json::from_str(&set_result).unwrap();
    assert_eq!(set_parsed["success"], true);

    // Test memory_typed_get
    let get_result = provider
        .handle_tool_call(
            "memory_typed_get",
            &serde_json::json!({
                "memory_type": "user",
                "key": "role"
            }),
        )
        .unwrap();

    let get_parsed: serde_json::Value = serde_json::from_str(&get_result).unwrap();
    assert_eq!(get_parsed["key"], "role");
    assert_eq!(get_parsed["content"], "senior engineer");

    // Test memory_typed_list
    let list_result = provider
        .handle_tool_call(
            "memory_typed_list",
            &serde_json::json!({
                "memory_type": "user"
            }),
        )
        .unwrap();

    let list_parsed: serde_json::Value = serde_json::from_str(&list_result).unwrap();
    assert!(list_parsed["count"].as_u64().unwrap() >= 1);
    assert!(list_parsed["results"].is_array());
}

#[test]
fn test_temporal_validity_in_prefetch() {
    let dir = tempdir().unwrap();
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();

    let typed_store = Arc::new(TypedMemoryStore::new(dir.path()));
    let semantic_store = Arc::new(SemanticStore::new(dir.path()));
    let principal_store = Arc::new(PrincipalMemoryStore::new(dir.path()));

    // Create an active memory
    let entry1 = zaion_memory::TypedMemoryEntry::new_unsigned(
        MemoryType::Project,
        "deadline",
        "June 15th",
        principal_id.as_str(),
        "session-1",
        "test",
    );
    typed_store.upsert(&entry1).unwrap();

    // Create an invalidated memory
    let entry2 = zaion_memory::TypedMemoryEntry::new_unsigned(
        MemoryType::Project,
        "old_deadline",
        "May 1st",
        principal_id.as_str(),
        "session-1",
        "test",
    );
    typed_store.upsert(&entry2).unwrap();
    typed_store
        .invalidate(principal_id.as_str(), MemoryType::Project, "old_deadline")
        .unwrap();

    let provider = BuiltinMemoryProvider::new(
        principal_id.to_string(),
        semantic_store,
        principal_store,
        typed_store,
        MemoryRuntimeConfig::default(),
    );

    // Prefetch should only include active memories
    let context = provider.prefetch("test", "session-1").unwrap();

    assert!(
        context.contains("June 15th"),
        "Should include active memory"
    );
    assert!(
        !context.contains("May 1st"),
        "Should not include invalidated memory"
    );
}

#[test]
fn test_confidence_in_extraction() {
    let user_content = "I am a senior engineer";
    let assistant_content = "";

    let result =
        AutoMemoryExtractor::extract_from_turn(user_content, assistant_content, "session-1");

    // Extracted user persona should have high confidence
    let user_candidates: Vec<_> = result
        .candidates
        .iter()
        .filter(|c| c.memory_type == MemoryType::User)
        .collect();

    assert!(!user_candidates.is_empty());

    for candidate in user_candidates {
        assert!(
            candidate.confidence >= 0.7,
            "User persona extraction should have high confidence"
        );
    }
}

#[test]
fn test_multiple_memory_types_in_one_turn() {
    let user_content = "I'm Alice, a senior engineer. The deadline is next week. Check https://github.com/zaion-ai/zaion";
    let assistant_content = "Got it!";

    let result =
        AutoMemoryExtractor::extract_from_turn(user_content, assistant_content, "session-1");

    let types: Vec<_> = result.candidates.iter().map(|c| c.memory_type).collect();

    // Should extract at least one memory type
    assert!(!types.is_empty(), "Should extract at least one memory type");

    // Likely to extract User memory from "I'm Alice, a senior engineer"
    // Or Project memory from "deadline is next week"
    // Or Reference memory from the URL
    let has_user_or_project_or_reference = types.contains(&MemoryType::User)
        || types.contains(&MemoryType::Project)
        || types.contains(&MemoryType::Reference);

    assert!(
        has_user_or_project_or_reference,
        "Should extract User, Project, or Reference memory. Got: {:?}",
        types
    );
}

#[test]
fn test_tool_schemas_include_typed_memory() {
    let dir = tempdir().unwrap();
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();

    let typed_store = Arc::new(TypedMemoryStore::new(dir.path()));
    let semantic_store = Arc::new(SemanticStore::new(dir.path()));
    let principal_store = Arc::new(PrincipalMemoryStore::new(dir.path()));

    let provider = BuiltinMemoryProvider::new(
        principal_id.to_string(),
        semantic_store,
        principal_store,
        typed_store,
        MemoryRuntimeConfig::default(),
    );

    let schemas = provider.get_tool_schemas();
    let tool_names: Vec<_> = schemas
        .iter()
        .filter_map(|s| s.get("name").and_then(|v| v.as_str()))
        .collect();

    assert!(tool_names.contains(&"memory_typed_get"));
    assert!(tool_names.contains(&"memory_typed_set"));
    assert!(tool_names.contains(&"memory_typed_list"));
}
