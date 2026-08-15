use crate::auth::AuthManager;
use crate::store::{EncryptedStore, SecretSource};

#[test]
fn test_encrypted_store_set_get() {
    let dir = tempfile::tempdir().unwrap();
    let key = EncryptedStore::generate_key();
    let store = EncryptedStore::new(dir.path().join("s.json"), &key);
    store
        .set("MY_KEY", "sk-test-1234", SecretSource::Inline)
        .unwrap();
    assert_eq!(store.get("MY_KEY").unwrap(), "sk-test-1234");
}

#[test]
fn test_encrypted_store_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let key = EncryptedStore::generate_key();
    let store = EncryptedStore::new(dir.path().join("s.json"), &key);
    assert!(store.get("MISSING").is_err());
}

#[test]
fn test_encrypted_store_delete() {
    let dir = tempfile::tempdir().unwrap();
    let key = EncryptedStore::generate_key();
    let store = EncryptedStore::new(dir.path().join("s.json"), &key);
    store.set("FOO", "bar", SecretSource::Inline).unwrap();
    store.delete("FOO").unwrap();
    assert!(store.get("FOO").is_err());
}

#[test]
fn test_encrypted_store_list() {
    let dir = tempfile::tempdir().unwrap();
    let key = EncryptedStore::generate_key();
    let store = EncryptedStore::new(dir.path().join("s.json"), &key);
    store.set("KEY_A", "val_a", SecretSource::Env).unwrap();
    store.set("KEY_B", "val_b", SecretSource::File).unwrap();
    let list = store.list().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].key, "KEY_A");
    assert_eq!(list[1].key, "KEY_B");
}

#[test]
fn test_encrypted_store_tamper_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.json");
    let key = EncryptedStore::generate_key();
    let store = EncryptedStore::new(&path, &key);
    store
        .set("SECRET", "mysecret", SecretSource::Inline)
        .unwrap();
    let mut data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    data["entries"]["SECRET"]["ciphertext_hex"] = serde_json::json!("deadbeef");
    std::fs::write(&path, serde_json::to_string(&data).unwrap()).unwrap();
    assert!(store.get("SECRET").is_err());
}

#[test]
fn test_key_never_written_to_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.json");
    let key = EncryptedStore::generate_key();
    let store = EncryptedStore::new(&path, &key);
    store.set("K", "v", SecretSource::Inline).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(
        json.get("key_hex").is_none(),
        "master key must never be on disk"
    );
    assert!(
        !content.contains(&hex::encode(key)),
        "raw key hex must not appear in file"
    );
}

#[test]
fn test_auth_manager_add_list_get() {
    let dir = tempfile::tempdir().unwrap();
    let master_key = EncryptedStore::generate_key();
    let mgr = AuthManager::new(dir.path(), &master_key);
    let p = mgr
        .add("main", "openai", "sk-test-xxx", Some("gpt-4o"), None, true)
        .unwrap();
    assert_eq!(p.name, "main");
    assert!(p.is_default);
    assert_eq!(mgr.get_key("main").unwrap(), "sk-test-xxx");
    assert_eq!(mgr.list().unwrap().len(), 1);
}

#[test]
fn test_auth_manager_switch_default() {
    let dir = tempfile::tempdir().unwrap();
    let master_key = EncryptedStore::generate_key();
    let mgr = AuthManager::new(dir.path(), &master_key);
    mgr.add("a", "anthropic", "sk-a", None, None, true).unwrap();
    mgr.add("b", "openai", "sk-b", None, None, false).unwrap();
    mgr.switch("b").unwrap();
    assert_eq!(mgr.default_profile().unwrap().unwrap().name, "b");
    let a = mgr
        .list()
        .unwrap()
        .into_iter()
        .find(|p| p.name == "a")
        .unwrap();
    assert!(!a.is_default);
}

#[test]
fn test_auth_manager_remove() {
    let dir = tempfile::tempdir().unwrap();
    let master_key = EncryptedStore::generate_key();
    let mgr = AuthManager::new(dir.path(), &master_key);
    mgr.add("tmp", "openai", "sk-tmp", None, None, false)
        .unwrap();
    mgr.remove("tmp").unwrap();
    assert!(mgr.list().unwrap().is_empty());
    assert!(mgr.remove("tmp").is_err());
}
