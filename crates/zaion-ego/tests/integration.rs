//! Integration tests for System I: Ego-Matrix
//!
//! Tests the complete ego system workflow including manifest loading,
//! compilation, signature verification, and response filtering.

use zaion_crypto::keypair::ZaionKeypair;
use zaion_ego::{
    BaffleConfig, BehaviorConfig, DynamicLexicalBaffle, EgoCompiler, EgoManifest, EgoStore,
    ImmuneSystem, SoulConfig, SoulHash,
};

#[test]
fn test_ego_manifest_roundtrip() {
    let manifest = EgoManifest {
        soul: SoulConfig {
            name: "TestBot".to_string(),
            core_tone: "friendly, concise".to_string(),
        },
        baffle: BaffleConfig {
            immune_system: ImmuneSystem {
                banned_exact: vec!["bad_word".to_string()],
                banned_regex: vec!["(?i)spam.*".to_string()],
            },
            behavior: BehaviorConfig {
                proactive_rate: 0.7,
                max_words_per_reply: 150,
                max_retries: 5,
            },
        },
    };

    // Serialize and deserialize
    let toml_str = toml::to_string(&manifest).unwrap();
    let parsed: EgoManifest = toml::from_str(&toml_str).unwrap();

    assert_eq!(parsed.soul.name, "TestBot");
    assert_eq!(parsed.soul.core_tone, "friendly, concise");
    assert_eq!(parsed.baffle.behavior.proactive_rate, 0.7);
    assert_eq!(parsed.baffle.behavior.max_words_per_reply, 150);
}

#[test]
fn test_ego_store_save_and_load() {
    let temp_dir = tempfile::tempdir().unwrap();
    let store = EgoStore::new(temp_dir.path());

    let manifest = EgoManifest {
        soul: SoulConfig {
            name: "Zaion".to_string(),
            core_tone: "helpful".to_string(),
        },
        baffle: BaffleConfig::default(),
    };

    // Save
    store.save(&manifest).unwrap();
    assert!(store.exists());

    // Load
    let loaded = store.load().unwrap();
    assert_eq!(loaded.soul.name, "Zaion");
    assert_eq!(loaded.soul.core_tone, "helpful");
}

#[test]
fn test_soul_hash_compute_and_verify() {
    let manifest = EgoManifest::default();
    let keypair = ZaionKeypair::generate();

    // Compute hash
    let soul_hash = SoulHash::compute(&manifest, &keypair).unwrap();
    assert!(!soul_hash.manifest_hash.is_empty());
    assert!(!soul_hash.signature_hex.is_empty());
    assert!(!soul_hash.created_at.is_empty());

    // Verify with correct keypair
    soul_hash.verify(&keypair).unwrap();
}

#[test]
fn test_soul_hash_verify_fails_with_wrong_keypair() {
    let manifest = EgoManifest::default();
    let keypair1 = ZaionKeypair::generate();
    let keypair2 = ZaionKeypair::generate();

    // Compute with keypair1
    let soul_hash = SoulHash::compute(&manifest, &keypair1).unwrap();

    // Verify with keypair2 should fail
    assert!(soul_hash.verify(&keypair2).is_err());
}

#[test]
fn test_ego_compiler_xml_generation() {
    let manifest = EgoManifest {
        soul: SoulConfig {
            name: "Zaion".to_string(),
            core_tone: "concise, direct".to_string(),
        },
        baffle: BaffleConfig {
            immune_system: ImmuneSystem {
                banned_exact: vec!["sorry".to_string(), "apologize".to_string()],
                banned_regex: vec!["(?i)i am an ai.*".to_string()],
            },
            behavior: BehaviorConfig {
                proactive_rate: 0.5,
                max_words_per_reply: 200,
                max_retries: 3,
            },
        },
    };

    let xml = EgoCompiler::compile(&manifest);

    // Verify XML structure
    assert!(xml.contains("<Zaion_Protocol>"));
    assert!(xml.contains("</Zaion_Protocol>"));
    assert!(xml.contains("<Identity>"));
    assert!(xml.contains("<Name>Zaion</Name>"));
    assert!(xml.contains("<CoreTone>concise, direct</CoreTone>"));
    assert!(xml.contains("<Constraints>"));
    assert!(xml.contains("<MaxWords>200</MaxWords>"));
    assert!(xml.contains("<ForbiddenPatterns>"));
    assert!(xml.contains("sorry|apologize"));
}

#[test]
fn test_ego_compiler_xml_escaping() {
    let manifest = EgoManifest {
        soul: SoulConfig {
            name: "<Script>Alert</Script>".to_string(),
            core_tone: "test & \"quoted\"".to_string(),
        },
        baffle: BaffleConfig::default(),
    };

    let xml = EgoCompiler::compile(&manifest);

    // Verify special characters are escaped
    assert!(xml.contains("&lt;Script&gt;Alert&lt;/Script&gt;"));
    assert!(xml.contains("test &amp; &quot;quoted&quot;"));
    assert!(!xml.contains("<Script>"));
}

#[test]
fn test_lexical_baffle_filters_exact_matches() {
    let manifest = EgoManifest {
        soul: SoulConfig::default(),
        baffle: BaffleConfig {
            immune_system: ImmuneSystem {
                banned_exact: vec!["作为一名AI".to_string(), "我是一个人工智能".to_string()],
                banned_regex: vec![],
            },
            behavior: BehaviorConfig::default(),
        },
    };

    let baffle = DynamicLexicalBaffle::new(&manifest).unwrap();

    // Should block exact matches
    assert!(!baffle.is_allowed("作为一名AI助手"));
    assert!(!baffle.is_allowed("我是一个人工智能"));

    // Should allow other text
    assert!(baffle.is_allowed("你好，我可以帮助你"));
    assert!(baffle.is_allowed("这是一个测试"));
}

#[test]
fn test_lexical_baffle_filters_regex_patterns() {
    let manifest = EgoManifest {
        soul: SoulConfig::default(),
        baffle: BaffleConfig {
            immune_system: ImmuneSystem {
                banned_exact: vec![],
                banned_regex: vec!["(?i)sorry.*".to_string(), "(?i)i cannot.*".to_string()],
            },
            behavior: BehaviorConfig::default(),
        },
    };

    let baffle = DynamicLexicalBaffle::new(&manifest).unwrap();

    // Should block regex matches (case insensitive)
    assert!(!baffle.is_allowed("Sorry, I cannot help"));
    assert!(!baffle.is_allowed("SORRY about that"));
    assert!(!baffle.is_allowed("I cannot do this"));

    // Should allow non-matching text
    assert!(baffle.is_allowed("Hello, how can I help?"));
    assert!(baffle.is_allowed("I can help you"));
}

#[test]
fn test_lexical_baffle_filter_response() {
    let manifest = EgoManifest {
        soul: SoulConfig::default(),
        baffle: BaffleConfig {
            immune_system: ImmuneSystem {
                banned_exact: vec!["banned".to_string()],
                banned_regex: vec!["(?i)bad.*".to_string()],
            },
            behavior: BehaviorConfig::default(),
        },
    };

    let baffle = DynamicLexicalBaffle::new(&manifest).unwrap();

    let response = "This is a banned word and bad things here";
    let filtered = baffle.filter_response(response);

    // Should remove banned tokens
    assert!(!filtered.contains("banned"));
    assert!(!filtered.contains("bad"));
    // Should keep allowed tokens
    assert!(filtered.contains("This"));
    assert!(filtered.contains("is"));
    assert!(filtered.contains("word"));
}

#[test]
fn test_lexical_baffle_allows_empty_bans() {
    let manifest = EgoManifest {
        soul: SoulConfig::default(),
        baffle: BaffleConfig {
            immune_system: ImmuneSystem {
                banned_exact: vec![],
                banned_regex: vec![],
            },
            behavior: BehaviorConfig::default(),
        },
    };

    let baffle = DynamicLexicalBaffle::new(&manifest).unwrap();

    // Should allow everything when no bans
    assert!(baffle.is_allowed("Any text"));
    assert!(baffle.is_allowed("Including special chars: @#$%"));
}

#[test]
fn test_lexical_baffle_invalid_regex() {
    let manifest = EgoManifest {
        soul: SoulConfig::default(),
        baffle: BaffleConfig {
            immune_system: ImmuneSystem {
                banned_exact: vec![],
                banned_regex: vec!["[invalid".to_string()], // Invalid regex
            },
            behavior: BehaviorConfig::default(),
        },
    };

    // Should return error for invalid regex
    let result = DynamicLexicalBaffle::new(&manifest);
    assert!(result.is_err());
}

#[test]
fn test_end_to_end_ego_workflow() {
    // 1. Create manifest
    let manifest = EgoManifest {
        soul: SoulConfig {
            name: "Zaion".to_string(),
            core_tone: "concise, helpful".to_string(),
        },
        baffle: BaffleConfig {
            immune_system: ImmuneSystem {
                banned_exact: vec!["sorry".to_string()],
                banned_regex: vec!["(?i)i am an ai.*".to_string()],
            },
            behavior: BehaviorConfig {
                proactive_rate: 0.5,
                max_words_per_reply: 200,
                max_retries: 3,
            },
        },
    };

    // 2. Save to disk
    let temp_dir = tempfile::tempdir().unwrap();
    let store = EgoStore::new(temp_dir.path());
    store.save(&manifest).unwrap();

    // 3. Load from disk
    let loaded = store.load().unwrap();
    assert_eq!(loaded.soul.name, "Zaion");

    // 4. Generate cryptographic hash
    let keypair = ZaionKeypair::generate();
    let soul_hash = SoulHash::compute(&loaded, &keypair).unwrap();
    soul_hash.verify(&keypair).unwrap();

    // 5. Compile to XML
    let xml = EgoCompiler::compile(&loaded);
    assert!(xml.contains("<Name>Zaion</Name>"));
    assert!(xml.contains("<MaxWords>200</MaxWords>"));

    // 6. Create lexical baffle
    let baffle = DynamicLexicalBaffle::new(&loaded).unwrap();
    assert!(!baffle.is_allowed("sorry about that"));
    assert!(!baffle.is_allowed("I am an AI assistant"));
    assert!(baffle.is_allowed("Hello, I can help you"));

    // 7. Filter a response
    let response = "Hello, sorry I am an AI but I can help";
    let filtered = baffle.filter_response(response);
    assert!(filtered.contains("Hello"));
    assert!(filtered.contains("can"));
    assert!(filtered.contains("help"));
    assert!(!filtered.contains("sorry"));
}
