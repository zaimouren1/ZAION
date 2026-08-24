#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use crate::keypair::ZaionKeypair;
    use crate::session::{current_unix_day, derive_session_id};
    use zaion_types::session::{
        ChannelId, MemoryNamespace, ProjectId, StyleLock, ThreadId, WorkspaceId,
    };

    #[test]
    fn test_keypair_generate_and_principal_id() {
        let kp = ZaionKeypair::generate();
        let pid = kp.principal_id();
        assert!(!pid.as_str().is_empty());
        let pid2 = kp.principal_id();
        assert_eq!(pid, pid2);
    }

    #[test]
    fn test_keypair_roundtrip() {
        let kp = ZaionKeypair::generate();
        let bytes = kp.to_bytes();
        let kp2 = ZaionKeypair::from_bytes(&bytes).unwrap();
        assert_eq!(kp.principal_id(), kp2.principal_id());
    }

    #[test]
    fn test_sign_and_verify() {
        use crate::verify::verify_signature;
        let kp = ZaionKeypair::generate();
        let message = b"zaion agentic process genesis";
        let sig = kp.sign(message);
        let pub_key = kp.public_key_bytes();
        assert!(verify_signature(&pub_key, message, &sig).is_ok());
    }


    #[test]
    fn tampered_message_fails_verification() {
        use crate::verify::verify_signature;
        let kp = ZaionKeypair::generate();
        let message = b"genesis event";
        let sig = kp.sign(message);
        let pub_key = kp.public_key_bytes();
        let tampered = b"genesis event (tampered)";
        assert!(verify_signature(&pub_key, tampered, &sig).is_err());
    }

    #[test]
    fn wrong_key_fails_verification() {
        use crate::verify::verify_signature;
        let kp1 = ZaionKeypair::generate();
        let kp2 = ZaionKeypair::generate();
        let message = b"shared message";
        let sig = kp1.sign(message);
        // kp2's public key must not verify kp1's signature.
        assert!(verify_signature(&kp2.public_key_bytes(), message, &sig).is_err());
    }

    #[test]
    fn tampered_signature_fails_verification() {
        use crate::verify::verify_signature;
        let kp = ZaionKeypair::generate();
        let message = b"event payload";
        let mut sig = kp.sign(message);
        sig.0[0] ^= 0xff; // flip one byte
        assert!(verify_signature(&kp.public_key_bytes(), message, &sig).is_err());
    }

    #[test]
    fn replayed_signature_on_different_message_fails() {
        use crate::verify::verify_signature;
        let kp = ZaionKeypair::generate();
        let message = b"event A";
        let sig = kp.sign(message);
        // Same signature replayed against a different message must fail.
        assert!(verify_signature(&kp.public_key_bytes(), b"event B", &sig).is_err());
    }

    #[test]
    fn test_session_key_no_style_fingerprint() {
        let kp = ZaionKeypair::generate();
        let principal_id = kp.principal_id();
        let channel_id = ChannelId("telegram".into());
        let thread_id = ThreadId("tg-001".into());
        let session_id =
            derive_session_id(&principal_id, &channel_id, &thread_id, current_unix_day());
        let ns = MemoryNamespace {
            principal_id: principal_id.clone(),
            workspace_id: WorkspaceId("ws-test".into()),
            project_id: ProjectId("proj-test".into()),
            channel_id: channel_id.clone(),
            thread_id: thread_id.clone(),
            session_id: session_id.clone(),
            run_id: None,
            style_lock: StyleLock::default(),
        };
        let sk = ns.session_key();
        assert!(
            !sk.0.contains("default-style"),
            "session_key must not contain style_fingerprint"
        );
        let ns2 = MemoryNamespace {
            style_lock: StyleLock {
                style_fingerprint: "custom-style".into(),
                ..StyleLock::default()
            },
            ..ns
        };
        assert_eq!(
            ns2.session_key(),
            sk,
            "session_key must be stable across style_fingerprint changes"
        );
    }

    #[test]
    fn test_derive_session_id_deterministic() {
        let kp = ZaionKeypair::generate();
        let pid = kp.principal_id();
        let channel_id = ChannelId("telegram".into());
        let thread_id = ThreadId("tg-001".into());
        let day = 20000u64;
        let s1 = derive_session_id(&pid, &channel_id, &thread_id, day);
        let s2 = derive_session_id(&pid, &channel_id, &thread_id, day);
        assert_eq!(s1, s2);
    }
}
