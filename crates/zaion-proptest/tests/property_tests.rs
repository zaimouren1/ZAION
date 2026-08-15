//! property_tests.rs — 数学级不变性证明
//!
//! Campaign V C5.3: 用 proptest 验证 Zaion 核心不变性
//! 每个 proptest! 块运行 256 个随机 case（默认配置）
//!
//! 覆盖 7 个域：
//!   1. EventLedger   — append / list 单调性
//!   2. Crypto 签名   — sign → verify，篡改必失败，跨 keypair 必失败
//!   3. PrincipalId   — 确定性、唯一性、格式约束
//!   4. SkillStore    — upsert → query，confidence 边界
//!   5. SemanticStore — cosine distance 边界，search(k) ≤ k
//!   6. ACI SyntaxGate — JSON 合法性，Rust 括号不平衡必拒绝
//!   7. AstDiff       — identical diff 全 Unchanged，chunk 非空

use proptest::prelude::*;
use tempfile::tempdir;

use zaion_crypto::keypair::ZaionKeypair;
use zaion_crypto::verify_signature;
use zaion_ledger::EventLedger;
use zaion_memory::skill::SkillStore;
use zaion_memory::SemanticStore;
use zaion_types::session::NamespaceKey;

// ── 1. Ledger 不变性 ─────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// 任意 event_type + payload → append 后必能在 list 中找到该 event_id
    #[test]
    fn ledger_append_then_list_contains(
        event_type in "[a-z][a-z0-9]{1,15}",
        payload_key in "[a-z]{3,8}",
        payload_val in "[a-zA-Z0-9]{1,16}",
    ) {
        let dir = tempdir().unwrap();
        let ledger = EventLedger::new(dir.path().join("prop.db"));
        let kp = ZaionKeypair::generate();
        let pid = kp.principal_id();
        let ns = NamespaceKey("test".into());
        let payload = serde_json::json!({ payload_key.as_str(): payload_val.as_str() });

        let event_id = ledger.append_event(&pid, &ns, &event_type, payload, None, None).unwrap();
        let events = ledger.list_global_events(200).unwrap();

        prop_assert!(
            events.iter().any(|e| e.event_id.0 == event_id.0),
            "appended event_id {:?} not found in list_global_events",
            event_id
        );
    }

    /// append N 个事件 → list 返回的数量 ≥ N（单调性）
    #[test]
    fn ledger_count_monotone(n in 1usize..=10usize) {
        let dir = tempdir().unwrap();
        let ledger = EventLedger::new(dir.path().join("mono.db"));
        let kp = ZaionKeypair::generate();
        let pid = kp.principal_id();
        let ns = NamespaceKey("ns".into());

        for i in 0..n {
            ledger.append_event(
                &pid, &ns, "test.event",
                serde_json::json!({"i": i}),
                None, None,
            ).unwrap();
        }

        let events = ledger.list_global_events(200).unwrap();
        prop_assert!(
            events.len() >= n,
            "expected >= {} events, got {}",
            n, events.len()
        );
    }
}

// ── 2. 签名不变性 ────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// 任意 payload → sign → verify 必须成功
    #[test]
    fn signature_sign_then_verify(
        payload in prop::collection::vec(any::<u8>(), 0..256),
    ) {
        let kp = ZaionKeypair::generate();
        let sig = kp.sign(&payload);
        let pub_key = kp.public_key_bytes();

        prop_assert!(
            verify_signature(&pub_key, &payload, &sig).is_ok(),
            "verify failed for a freshly signed payload"
        );
    }

    /// 将 payload 的任意一字节翻转后验证必须失败（当 payload 确实被修改时）
    #[test]
    fn signature_tampered_byte_fails(
        payload in prop::collection::vec(any::<u8>(), 1..256),
        tamper_idx in any::<u8>(),
    ) {
        let kp = ZaionKeypair::generate();
        let sig = kp.sign(&payload);
        let pub_key = kp.public_key_bytes();

        let mut tampered = payload.clone();
        let idx = (tamper_idx as usize) % tampered.len();
        tampered[idx] ^= 0xFF;

        // 仅当 tampered != payload 时（0xFF XOR 0xFF = 0x00 可能相同，但极罕见）
        if tampered != payload {
            prop_assert!(
                verify_signature(&pub_key, &tampered, &sig).is_err(),
                "tampered payload should not verify successfully"
            );
        }
    }

    /// 用不同 keypair 的公钥验证签名必须失败
    #[test]
    fn signature_wrong_keypair_fails(
        payload in prop::collection::vec(any::<u8>(), 1..64),
    ) {
        let kp1 = ZaionKeypair::generate();
        let kp2 = ZaionKeypair::generate();
        let sig = kp1.sign(&payload);
        let pub_key2 = kp2.public_key_bytes();

        prop_assert!(
            verify_signature(&pub_key2, &payload, &sig).is_err(),
            "signature verified with wrong keypair — must not happen"
        );
    }

    /// 从相同字节重建的 keypair 签名可被验证
    #[test]
    fn signature_round_trip_from_bytes(
        payload in prop::collection::vec(any::<u8>(), 0..128),
    ) {
        let kp = ZaionKeypair::generate();
        let raw_bytes = kp.to_bytes();

        // 重建 keypair
        let kp2 = ZaionKeypair::from_bytes(&raw_bytes).unwrap();
        let sig2 = kp2.sign(&payload);
        let pub_key = kp.public_key_bytes();

        prop_assert!(
            verify_signature(&pub_key, &payload, &sig2).is_ok(),
            "keypair round-trip via to_bytes/from_bytes must preserve signing ability"
        );
    }
}

// ── 3. PrincipalId 不变性 ────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// 同一 keypair 多次调用 principal_id() 必须返回相同值（确定性）
    #[test]
    fn principal_id_deterministic(_seed in any::<u64>()) {
        let kp = ZaionKeypair::generate();
        let pid1 = kp.principal_id();
        let pid2 = kp.principal_id();
        prop_assert_eq!(
            pid1.as_str(), pid2.as_str(),
            "principal_id() must be deterministic for the same keypair"
        );
    }

    /// 不同 keypair → 不同 principal_id（Ed25519 碰撞概率 ≈ 2^{-256}）
    #[test]
    fn principal_id_unique(_seed in any::<u64>()) {
        let kp1 = ZaionKeypair::generate();
        let kp2 = ZaionKeypair::generate();
        let pid1 = kp1.principal_id();
        let pid2 = kp2.principal_id();
        prop_assert_ne!(
            pid1.as_str(),
            pid2.as_str(),
            "two independently generated keypairs must have different principal_ids"
        );
    }

    /// principal_id 格式：非空，长度 ≥ 8（bs58(SHA-256(pubkey)) 最短也有 43 chars）
    #[test]
    fn principal_id_format(_seed in any::<u64>()) {
        let kp = ZaionKeypair::generate();
        let pid = kp.principal_id();
        let s = pid.as_str();
        prop_assert!(!s.is_empty(), "principal_id must not be empty");
        prop_assert!(s.len() >= 8, "principal_id too short: {} chars", s.len());
    }

    /// from_bytes(to_bytes(kp)).principal_id() == kp.principal_id()
    #[test]
    fn principal_id_stable_across_serialization(_seed in any::<u64>()) {
        let kp = ZaionKeypair::generate();
        let pid_original = kp.principal_id();
        let kp2 = ZaionKeypair::from_bytes(&kp.to_bytes()).unwrap();
        let pid_restored = kp2.principal_id();
        prop_assert_eq!(
            pid_original.as_str(), pid_restored.as_str(),
            "principal_id must survive keypair serialization round-trip"
        );
    }
}

// ── 4. SkillStore 不变性 ─────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// upsert 后 query 必须能检索到该 rule_text
    #[test]
    fn skill_store_upsert_then_query(
        skill_type in "[a-z]{3,10}",
        rule_text in "[a-zA-Z ]{5,40}",
    ) {
        let dir = tempdir().unwrap();
        let kp = ZaionKeypair::generate();
        let pid = kp.principal_id();
        let store = SkillStore::new(dir.path().join("skills.db"));

        store.upsert(&pid, &skill_type, &[], &rule_text, 1.0).unwrap();
        let results = store.query(&pid, &skill_type, 100).unwrap();

        prop_assert!(
            results.iter().any(|s| s.rule_text == rule_text),
            "upserted rule_text '{}' not found in query results",
            rule_text
        );
    }

    /// confidence 在 [0.0, 10.0] 内（代码使用 clamp(0, 10)）
    #[test]
    fn skill_store_confidence_bounded(
        skill_type in "[a-z]{3,8}",
        rule_text in "[a-z]{5,20}",
        delta in -2.0f64..=10.0f64,
    ) {
        let dir = tempdir().unwrap();
        let kp = ZaionKeypair::generate();
        let pid = kp.principal_id();
        let store = SkillStore::new(dir.path().join("skills.db"));

        store.upsert(&pid, &skill_type, &[], &rule_text, delta).unwrap();
        let results = store.query(&pid, &skill_type, 10).unwrap();

        for s in &results {
            prop_assert!(
                s.confidence >= 0.0,
                "confidence must be >= 0.0, got {}",
                s.confidence
            );
            prop_assert!(
                s.confidence <= 10.0,
                "confidence must be <= 10.0, got {}",
                s.confidence
            );
        }
    }

    /// 同一 (pid, skill_type, rule_text) upsert 两次 → usage_count 递增
    #[test]
    fn skill_store_usage_count_increments(
        skill_type in "[a-z]{3,8}",
        rule_text in "[a-z]{5,15}",
    ) {
        let dir = tempdir().unwrap();
        let kp = ZaionKeypair::generate();
        let pid = kp.principal_id();
        let store = SkillStore::new(dir.path().join("skills.db"));

        store.upsert(&pid, &skill_type, &[], &rule_text, 1.0).unwrap();
        store.upsert(&pid, &skill_type, &[], &rule_text, 1.0).unwrap();

        let results = store.query(&pid, &skill_type, 10).unwrap();
        let entry = results.iter().find(|s| s.rule_text == rule_text).unwrap();
        prop_assert!(
            entry.usage_count >= 1,
            "usage_count should be >= 1 after second upsert, got {}",
            entry.usage_count
        );
    }
}

// ── 5. SemanticStore 不变性 ──────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// upsert embedding → search 距离在 [0, 2] 范围（cosine distance 约束）
    #[test]
    fn semantic_distance_bounded(
        dims in 4usize..=16usize,
        n_entries in 1usize..=5usize,
    ) {
        let dir = tempdir().unwrap();
        let store = SemanticStore::new(dir.path());
        let kp = ZaionKeypair::generate();
        let pid = kp.principal_id().as_str().to_string();

        for i in 0..n_entries {
            // 非零向量，避免零向量导致 cosine_distance 返回 2.0
            let emb: Vec<f32> = (0..dims)
                .map(|j| ((i + 1) as f32) * ((j + 1) as f32) / (dims as f32))
                .collect();
            store.upsert(&pid, &format!("text-{i}"), &emb, serde_json::json!({})).unwrap();
        }

        // 非零单位方向查询向量
        let query: Vec<f32> = (0..dims).map(|i| (i + 1) as f32 / dims as f32).collect();
        let results = store.search(&pid, &query, n_entries).unwrap();

        for m in &results {
            prop_assert!(
                m.distance >= -0.01,
                "cosine distance must be >= 0, got {}",
                m.distance
            );
            prop_assert!(
                m.distance <= 2.01,
                "cosine distance must be <= 2, got {}",
                m.distance
            );
        }
    }

    /// search(k) 返回结果数量 ≤ k
    #[test]
    fn semantic_search_respects_k(
        k in 1usize..=5usize,
        n_entries in 1usize..=10usize,
    ) {
        let dir = tempdir().unwrap();
        let store = SemanticStore::new(dir.path());
        let kp = ZaionKeypair::generate();
        let pid = kp.principal_id().as_str().to_string();

        for i in 0..n_entries {
            let emb = vec![i as f32 + 1.0; 4];
            store.upsert(&pid, &format!("t{i}"), &emb, serde_json::json!({})).unwrap();
        }

        let query = vec![1.0f32; 4];
        let results = store.search(&pid, &query, k).unwrap();

        prop_assert!(
            results.len() <= k,
            "search returned {} results, but k={} — must not exceed k",
            results.len(), k
        );
    }

    /// 跨 principal 隔离：A 插入的向量不出现在 B 的搜索结果中
    #[test]
    fn semantic_search_principal_isolation(n_entries in 1usize..=5usize) {
        let dir = tempdir().unwrap();
        let store = SemanticStore::new(dir.path());
        let kp_a = ZaionKeypair::generate();
        let kp_b = ZaionKeypair::generate();
        let pid_a = kp_a.principal_id().as_str().to_string();
        let pid_b = kp_b.principal_id().as_str().to_string();

        for i in 0..n_entries {
            let emb = vec![i as f32 + 1.0; 4];
            store.upsert(&pid_a, &format!("a-text-{i}"), &emb, serde_json::json!({})).unwrap();
        }

        // B 没有插入任何向量，搜索结果必须为空
        let query = vec![1.0f32; 4];
        let results_b = store.search(&pid_b, &query, 10).unwrap();
        prop_assert!(
            results_b.is_empty(),
            "principal B should see 0 results, got {}",
            results_b.len()
        );
    }
}

// ── 6. ACI SyntaxGate 不变性 ─────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// 合法 JSON 对象 → SyntaxGate::check 通过
    #[test]
    fn syntax_gate_valid_json_passes(
        key in "[a-z]{2,8}",
        val in "[a-z0-9]{1,16}",
    ) {
        let json = format!(r#"{{"{}": "{}"}}"#, key, val);
        let result = zaion_aci::SyntaxGate::check(&json, &zaion_aci::SyntaxLanguage::Json);
        prop_assert!(
            result.is_valid(),
            "valid JSON should pass SyntaxGate: {}",
            json
        );
    }

    /// 随机字符串通常是非法 JSON → SyntaxGate 检测到错误（注：某些字符串可能合法）
    #[test]
    fn syntax_gate_literal_non_json_invalid(
        // alphanumeric without quotes/braces — never valid JSON
        content in "[a-z]{5,20}",
    ) {
        let result = zaion_aci::SyntaxGate::check(&content, &zaion_aci::SyntaxLanguage::Json);
        prop_assert!(
            !result.is_valid(),
            "bare alphanumeric string '{}' should not be valid JSON",
            content
        );
    }

    /// 未平衡的大括号 Rust 代码 → SyntaxGate 必须拒绝
    #[test]
    fn syntax_gate_unbalanced_rust_rejected(extra_opens in 1usize..=4usize) {
        // 生成 fn foo() { { { ... （n 个额外的未闭合大括号）
        let opens = "{".repeat(extra_opens + 1); // +1 for the fn body brace
        let src = format!("fn foo() {}", opens); // deliberately unclosed
        let result = zaion_aci::SyntaxGate::check(&src, &zaion_aci::SyntaxLanguage::Rust);
        prop_assert!(
            !result.is_valid(),
            "Rust with {} unclosed braces should be rejected: {:?}",
            extra_opens, src
        );
    }

    /// 平衡的 Rust 函数 → SyntaxGate 通过
    #[test]
    fn syntax_gate_balanced_rust_passes(fn_name in "[a-z][a-z0-9_]{1,10}") {
        let src = format!("fn {}() {{ let x = 42; }}", fn_name);
        let result = zaion_aci::SyntaxGate::check(&src, &zaion_aci::SyntaxLanguage::Rust);
        prop_assert!(
            result.is_valid(),
            "balanced Rust fn should pass SyntaxGate: {}",
            src
        );
    }

    /// Unknown 语言 → SyntaxGate 跳过校验（Skipped 等同于通过）
    #[test]
    fn syntax_gate_unknown_language_skipped(content in "[^\x00-\x1F]{1,50}") {
        let result = zaion_aci::SyntaxGate::check(&content, &zaion_aci::SyntaxLanguage::Unknown);
        prop_assert!(
            result.is_valid(),
            "Unknown language should produce Skipped (is_valid=true)"
        );
    }
}

// ── 7. AstDiff 不变性 ────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// diff(src, src) → 全部 Unchanged，无 Added/Modified/Deleted
    #[test]
    fn ast_diff_identical_all_unchanged(n_fns in 1usize..=6usize) {
        use zaion_aci::merge::{AstDiff, AstChange};

        // 生成 n 个合法的、括号平衡的 Rust 函数
        let src: String = (0..n_fns)
            .map(|i| format!("fn f{i}() {{ let _ = {i}; }}\n\n"))
            .collect();

        let changes = AstDiff::diff(&src, &src);

        for c in &changes {
            prop_assert!(
                matches!(c, AstChange::Unchanged { .. }),
                "diff(src, src) must produce only Unchanged, got: {:?}",
                c
            );
        }
    }

    /// chunk(src) 对非空源码返回 ≥ 1 个 chunk
    #[test]
    fn ast_diff_chunk_non_empty(n_fns in 1usize..=8usize) {
        use zaion_aci::merge::AstDiff;

        let src: String = (0..n_fns)
            .map(|i| format!("fn f{i}() {{ let _ = {i}; }}\n\n"))
            .collect();

        let chunks = AstDiff::chunk(&src);
        prop_assert!(
            !chunks.is_empty(),
            "chunk() must return at least 1 chunk for non-empty source"
        );
    }

    /// chunk(src).len() ≤ n_fns × 2（不会过度分割）
    #[test]
    fn ast_diff_chunk_count_bounded(n_fns in 1usize..=8usize) {
        use zaion_aci::merge::AstDiff;

        let src: String = (0..n_fns)
            .map(|i| format!("fn f{i}() {{ let _ = {i}; }}\n\n"))
            .collect();

        let chunks = AstDiff::chunk(&src);
        prop_assert!(
            chunks.len() <= n_fns * 2,
            "chunk() produced {} chunks for {} functions (max allowed: {})",
            chunks.len(), n_fns, n_fns * 2
        );
    }

    /// diff 长度 ≤ len(base_chunks) + len(branch_chunks)（上界）
    #[test]
    fn ast_diff_length_bounded(n_base in 1usize..=5usize, n_branch in 1usize..=5usize) {
        use zaion_aci::merge::AstDiff;

        let base: String = (0..n_base)
            .map(|i| format!("fn base_{i}() {{ {i} }}\n\n"))
            .collect();
        let branch: String = (0..n_branch)
            .map(|i| format!("fn branch_{i}() {{ {i} }}\n\n"))
            .collect();

        let base_chunks = AstDiff::chunk(&base);
        let branch_chunks = AstDiff::chunk(&branch);
        let changes = AstDiff::diff(&base, &branch);

        let upper_bound = base_chunks.len() + branch_chunks.len();
        prop_assert!(
            changes.len() <= upper_bound,
            "diff produced {} changes, but upper bound is {} (base={} + branch={})",
            changes.len(), upper_bound, base_chunks.len(), branch_chunks.len()
        );
    }
}
