use crate::{
    controller::ProcessController,
    pairing::PairingStore,
    process::{ProcessState, ProcessStore},
};
use tempfile::tempdir;

#[test]
fn test_process_create_and_load() {
    let dir = tempdir().unwrap();
    let store = ProcessStore::new(dir.path());
    let (process, kp) = store.create("ws-test", "proj-test").unwrap();
    assert_eq!(process.state, ProcessState::Created);
    assert_eq!(process.workspace_id, "ws-test");
    assert!(process.principal_id.len() > 10);
    let (loaded, kp2) = store.load(&process.principal_id).unwrap();
    assert_eq!(loaded.principal_id, process.principal_id);
    assert_eq!(kp.principal_id(), kp2.principal_id());
}

#[test]
fn test_process_dir_exists_after_create() {
    let dir = tempdir().unwrap();
    let store = ProcessStore::new(dir.path());
    let (process, _kp) = store.create("ws-test", "proj-test").unwrap();
    assert!(store.process_dir(&process.principal_id).exists());
}

#[test]
fn test_process_migrate_roundtrip() {
    let dir = tempdir().unwrap();
    let store = ProcessStore::new(dir.path());
    let (process, _kp) = store.create("ws-a", "proj-a").unwrap();
    let export_path = dir.path().join("exported.bin");
    store
        .export_keypair(&process.principal_id, &export_path)
        .unwrap();
    assert!(export_path.exists());
    let dir2 = tempdir().unwrap();
    let store2 = ProcessStore::new(dir2.path());
    let (migrated, kp_m) = store2
        .import_keypair(&export_path, "ws-b", "proj-b")
        .unwrap();
    assert_eq!(migrated.principal_id, process.principal_id);
    assert_eq!(migrated.state, ProcessState::Migrating);
    assert_eq!(kp_m.principal_id().as_str(), process.principal_id);
}

#[test]
fn test_process_encrypted_migrate_roundtrip() {
    let dir = tempdir().unwrap();
    let store = ProcessStore::new(dir.path());
    let (process, _kp) = store.create("ws-a", "proj-a").unwrap();
    let export_path = dir.path().join("exported.zaion-key");
    store
        .export_keypair_encrypted(&process.principal_id, &export_path, "correct horse")
        .unwrap();
    assert!(ProcessStore::key_export_is_encrypted(&export_path));
    let exported = std::fs::read(&export_path).unwrap();
    assert_ne!(
        exported.len(),
        32,
        "encrypted export must not be raw key bytes"
    );

    let dir2 = tempdir().unwrap();
    let store2 = ProcessStore::new(dir2.path());
    let (migrated, kp_m) = store2
        .import_keypair_encrypted(&export_path, "ws-b", "proj-b", "correct horse")
        .unwrap();
    assert_eq!(migrated.principal_id, process.principal_id);
    assert_eq!(kp_m.principal_id().as_str(), process.principal_id);
}

#[test]
fn test_process_encrypted_import_rejects_wrong_passphrase() {
    let dir = tempdir().unwrap();
    let store = ProcessStore::new(dir.path());
    let (process, _kp) = store.create("ws-a", "proj-a").unwrap();
    let export_path = dir.path().join("exported.zaion-key");
    store
        .export_keypair_encrypted(&process.principal_id, &export_path, "correct horse")
        .unwrap();

    let dir2 = tempdir().unwrap();
    let store2 = ProcessStore::new(dir2.path());
    let result = store2.import_keypair_encrypted(&export_path, "ws-b", "proj-b", "wrong");
    assert!(result.is_err());
}

#[test]
fn test_controller_create_and_sleep() {
    let dir = tempdir().unwrap();
    let ctrl = ProcessController::new(dir.path());
    let process = ctrl.create("ws-test", "proj-test").unwrap();
    ctrl.sleep(&process.principal_id).unwrap();
    let store = ProcessStore::new(dir.path());
    let (loaded, _) = store.load(&process.principal_id).unwrap();
    assert_eq!(loaded.state, ProcessState::Sleeping);
}

#[test]
fn test_pairing_challenge_verify_roundtrip() {
    let dir = tempdir().unwrap();
    let store = ProcessStore::new(dir.path());
    let (process, kp) = store.create("ws-pair", "proj-pair").unwrap();
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
    let ns = zaion_types::session::NamespaceKey(process.principal_id.clone());
    let pairing = PairingStore::new(dir.path().join("pairings.db"));
    let ch = pairing.generate_challenge(&kp).unwrap();
    assert_eq!(ch.code.len(), 6);
    assert!(ch.code.chars().all(|c| c.is_ascii_digit()));
    let rec = pairing
        .verify(&ch.code, "test-device", &kp, &ledger, &ns)
        .unwrap();
    assert!(!rec.pairing_id.is_empty());
    assert_eq!(rec.remote_label, "test-device");
    assert!(!rec.revoked);
}

#[test]
fn test_pairing_invalid_code_fails() {
    let dir = tempdir().unwrap();
    let store = ProcessStore::new(dir.path());
    let (process, kp) = store.create("ws-inv", "proj-inv").unwrap();
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
    let ns = zaion_types::session::NamespaceKey(process.principal_id.clone());
    let pairing = PairingStore::new(dir.path().join("pairings_inv.db"));
    assert!(pairing.verify("000000", "dev", &kp, &ledger, &ns).is_err());
}

#[test]
fn test_pairing_list_and_revoke() {
    let dir = tempdir().unwrap();
    let store = ProcessStore::new(dir.path());
    let (process, kp) = store.create("ws-rev", "proj-rev").unwrap();
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
    let ns = zaion_types::session::NamespaceKey(process.principal_id.clone());
    let pairing = PairingStore::new(dir.path().join("pairings_rev.db"));
    let ch = pairing.generate_challenge(&kp).unwrap();
    let rec = pairing
        .verify(&ch.code, "laptop", &kp, &ledger, &ns)
        .unwrap();
    assert_eq!(pairing.list().unwrap().len(), 1);
    pairing.revoke(&rec.pairing_id, &kp, &ledger, &ns).unwrap();
    assert!(pairing.list().unwrap()[0].revoked);
}
