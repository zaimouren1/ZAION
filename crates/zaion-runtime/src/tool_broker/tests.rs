use chrono::Duration;

use super::*;

fn set(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn invocation_hash() -> String {
    format!("sha256:{}", "a".repeat(64))
}

fn manifest(effect: ToolEffect) -> ToolManifest {
    let network = if effect == ToolEffect::Network {
        NetworkPolicy::AllowHosts {
            hosts: set(&["api.example.com"]),
        }
    } else {
        NetworkPolicy::Denied
    };
    ToolManifest::new(ToolManifestInput {
        name: "test_tool".to_string(),
        version: "1.0.0".to_string(),
        effect,
        risk: ToolRisk::Low,
        required_scopes: vec!["tool:test".to_string()],
        approval: ToolApprovalRequirement::NotRequired,
        idempotency: ToolIdempotency::Idempotent,
        filesystem: FilesystemPolicy::Denied,
        network,
        environment: EnvironmentPolicy::Denied,
    })
    .unwrap()
}

fn invocation() -> ToolInvocation {
    ToolInvocation::new(ToolInvocationInput {
        invocation_hash: invocation_hash(),
        filesystem_paths: Vec::new(),
        network_hosts: Vec::new(),
        environment_variables: Vec::new(),
        idempotency_key: None,
    })
    .unwrap()
}

fn authorization(effects: Vec<ToolEffect>) -> ToolAuthorization {
    ToolAuthorization::new(ToolAuthorizationInput {
        subject_id: "subject-1".to_string(),
        tool_name: "test_tool".to_string(),
        tool_version: "1.0.0".to_string(),
        granted_scopes: vec!["tool:test".to_string()],
        granted_effects: effects,
        filesystem_roots: Vec::new(),
        network_hosts: Vec::new(),
        environment_variables: Vec::new(),
        approval: None,
    })
    .unwrap()
}

#[test]
fn pure_and_read_tools_can_be_scope_authorized_without_effect_grants() {
    let broker = ToolBroker;
    let now = Utc::now();
    for effect in [ToolEffect::Pure, ToolEffect::Read] {
        assert!(broker
            .decide(
                &manifest(effect),
                &invocation(),
                &authorization(vec![]),
                now
            )
            .is_allowed());
    }
}

#[test]
fn write_execute_and_network_are_default_deny_even_with_scope() {
    let broker = ToolBroker;
    let now = Utc::now();
    for effect in [ToolEffect::Write, ToolEffect::Execute, ToolEffect::Network] {
        assert_eq!(
            broker
                .decide(
                    &manifest(effect),
                    &invocation(),
                    &authorization(vec![]),
                    now
                )
                .denial_reason(),
            Some(&ToolDenyReason::EffectNotAuthorized(effect))
        );
    }
}

#[test]
fn sensitive_effects_require_the_exact_explicit_grant() {
    let broker = ToolBroker;
    let now = Utc::now();
    for effect in [ToolEffect::Write, ToolEffect::Execute, ToolEffect::Network] {
        let mut auth = authorization(vec![effect]);
        if effect == ToolEffect::Network {
            auth.network_hosts.insert("api.example.com".to_string());
        }
        assert!(broker
            .decide(&manifest(effect), &invocation(), &auth, now)
            .is_allowed());
    }
}

#[test]
fn required_approval_is_target_subject_and_expiry_bound() {
    let now = Utc::now();
    let manifest = ToolManifest::new(ToolManifestInput {
        risk: ToolRisk::High,
        approval: ToolApprovalRequirement::Required,
        ..manifest(ToolEffect::Read).into_input()
    })
    .unwrap();
    let broker = ToolBroker;
    let mut auth = authorization(vec![]);

    assert_eq!(
        broker
            .decide(&manifest, &invocation(), &auth, now)
            .denial_reason(),
        Some(&ToolDenyReason::ApprovalMissing)
    );

    auth.approval = Some(
        ToolApprovalGrant::new(
            "approval-1".to_string(),
            "subject-1".to_string(),
            "test_tool".to_string(),
            "1.0.0".to_string(),
            invocation_hash(),
            now,
            now + Duration::minutes(5),
        )
        .unwrap(),
    );
    assert!(broker
        .decide(&manifest, &invocation(), &auth, now)
        .is_allowed());

    assert_eq!(
        broker
            .decide(&manifest, &invocation(), &auth, now + Duration::minutes(6))
            .denial_reason(),
        Some(&ToolDenyReason::ApprovalExpired)
    );
}

#[test]
fn filesystem_network_and_environment_require_manifest_and_grant_intersection() {
    let manifest = ToolManifest::new(ToolManifestInput {
        name: "test_tool".to_string(),
        version: "1.0.0".to_string(),
        effect: ToolEffect::Execute,
        risk: ToolRisk::Low,
        required_scopes: vec!["tool:test".to_string()],
        approval: ToolApprovalRequirement::NotRequired,
        idempotency: ToolIdempotency::Idempotent,
        filesystem: FilesystemPolicy::ReadWrite {
            roots: set(&["workspace"]),
        },
        network: NetworkPolicy::AllowHosts {
            hosts: set(&["api.example.com"]),
        },
        environment: EnvironmentPolicy::AllowRead {
            variables: set(&["ZAION_PROFILE"]),
        },
    })
    .unwrap();
    let invocation = ToolInvocation::new(ToolInvocationInput {
        invocation_hash: invocation_hash(),
        filesystem_paths: vec!["workspace/src/lib.rs".to_string()],
        network_hosts: vec!["api.example.com".to_string()],
        environment_variables: vec!["ZAION_PROFILE".to_string()],
        idempotency_key: None,
    })
    .unwrap();
    let mut auth = authorization(vec![
        ToolEffect::Execute,
        ToolEffect::Write,
        ToolEffect::Network,
    ]);
    let broker = ToolBroker;
    let now = Utc::now();

    assert!(matches!(
        broker
            .decide(&manifest, &invocation, &auth, now)
            .denial_reason(),
        Some(ToolDenyReason::FilesystemPathNotAuthorized(_))
    ));
    auth.filesystem_roots.insert("workspace".to_string());
    auth.network_hosts.insert("api.example.com".to_string());
    auth.environment_variables
        .insert("ZAION_PROFILE".to_string());
    assert!(broker
        .decide(&manifest, &invocation, &auth, now)
        .is_allowed());
}

#[test]
fn key_required_tools_reject_missing_idempotency_key() {
    let manifest = ToolManifest::new(ToolManifestInput {
        idempotency: ToolIdempotency::KeyRequired,
        ..manifest(ToolEffect::Read).into_input()
    })
    .unwrap();
    let decision = ToolBroker.decide(&manifest, &invocation(), &authorization(vec![]), Utc::now());
    assert_eq!(
        decision.denial_reason(),
        Some(&ToolDenyReason::IdempotencyKeyMissing)
    );
}

#[test]
fn manifest_rejects_unsafe_invariant_combinations_table() {
    let high_without_approval = ToolManifestInput {
        risk: ToolRisk::High,
        ..manifest(ToolEffect::Read).into_input()
    };
    let pure_with_filesystem = ToolManifestInput {
        filesystem: FilesystemPolicy::ReadOnly { roots: set(&["."]) },
        ..manifest(ToolEffect::Pure).into_input()
    };
    let network_without_hosts = ToolManifestInput {
        network: NetworkPolicy::Denied,
        ..manifest(ToolEffect::Network).into_input()
    };

    let cases = [
        (
            high_without_approval,
            ToolManifestError::ApprovalRequiredByRisk,
        ),
        (
            pure_with_filesystem,
            ToolManifestError::PureToolDeclaresResources,
        ),
        (
            network_without_hosts,
            ToolManifestError::NetworkEffectWithoutHosts,
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(ToolManifest::new(input), Err(expected));
    }
}

#[test]
fn paths_cannot_escape_a_workspace_root() {
    for path in ["../secret", "/absolute", "a/../../b", "C:/windows"] {
        assert!(matches!(
            ToolInvocation::new(ToolInvocationInput {
                invocation_hash: invocation_hash(),
                filesystem_paths: vec![path.to_string()],
                network_hosts: Vec::new(),
                environment_variables: Vec::new(),
                idempotency_key: None,
            }),
            Err(ToolAuthorizationError::InvalidPath(_))
        ));
    }
}

#[test]
fn approval_is_bound_to_one_invocation_and_a_short_validity_window() {
    let now = Utc::now();
    let manifest = ToolManifest::new(ToolManifestInput {
        risk: ToolRisk::High,
        approval: ToolApprovalRequirement::Required,
        ..manifest(ToolEffect::Read).into_input()
    })
    .unwrap();
    let mut auth = authorization(vec![]);
    auth.approval = Some(
        ToolApprovalGrant::new(
            "approval-2".to_string(),
            "subject-1".to_string(),
            "test_tool".to_string(),
            "1.0.0".to_string(),
            format!("sha256:{}", "b".repeat(64)),
            now,
            now + Duration::minutes(5),
        )
        .unwrap(),
    );
    assert_eq!(
        ToolBroker
            .decide(&manifest, &invocation(), &auth, now)
            .denial_reason(),
        Some(&ToolDenyReason::ApprovalInvocationMismatch)
    );

    assert_eq!(
        ToolApprovalGrant::new(
            "approval-3".to_string(),
            "subject-1".to_string(),
            "test_tool".to_string(),
            "1.0.0".to_string(),
            invocation_hash(),
            now,
            now + Duration::minutes(16),
        ),
        Err(ToolAuthorizationError::InvalidApprovalWindow)
    );
}

#[test]
fn approval_issued_in_the_future_is_not_yet_valid() {
    let now = Utc::now();
    let manifest = ToolManifest::new(ToolManifestInput {
        risk: ToolRisk::High,
        approval: ToolApprovalRequirement::Required,
        ..manifest(ToolEffect::Read).into_input()
    })
    .unwrap();
    let mut auth = authorization(vec![]);
    auth.approval = Some(
        ToolApprovalGrant::new(
            "approval-4".to_string(),
            "subject-1".to_string(),
            "test_tool".to_string(),
            "1.0.0".to_string(),
            invocation_hash(),
            now + Duration::minutes(1),
            now + Duration::minutes(6),
        )
        .unwrap(),
    );
    assert_eq!(
        ToolBroker
            .decide(&manifest, &invocation(), &auth, now)
            .denial_reason(),
        Some(&ToolDenyReason::ApprovalNotYetValid)
    );
}

impl ToolManifest {
    fn into_input(self) -> ToolManifestInput {
        ToolManifestInput {
            name: self.name,
            version: self.version,
            effect: self.effect,
            risk: self.risk,
            required_scopes: self.required_scopes.into_iter().collect(),
            approval: self.approval,
            idempotency: self.idempotency,
            filesystem: self.filesystem,
            network: self.network,
            environment: self.environment,
        }
    }
}
