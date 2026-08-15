use crate::{
    agent_card::{AgentCard, AgentEndpoint, EndpointProtocol},
    federation::FederationRegistry,
    protocol::MessageType,
};
use zaion_crypto::keypair::ZaionKeypair;

#[test]
fn test_agent_card_create_and_verify() {
    let kp = ZaionKeypair::generate();
    let card = AgentCard::new(
        &kp,
        "zaion-alpha",
        vec!["task.echo".into(), "task.summarize".into()],
        vec![AgentEndpoint {
            protocol: EndpointProtocol::Local,
            url: "local://zaion".into(),
        }],
    );
    assert_eq!(card.principal_id, kp.principal_id().as_str());
    assert_eq!(card.capabilities.len(), 2);
    assert!(card.verify().is_ok());
}

#[test]
fn test_agent_card_tampered_fails_verify() {
    let kp = ZaionKeypair::generate();
    let mut card = AgentCard::new(&kp, "zaion-alpha", vec!["task.echo".into()], vec![]);
    card.display_name = "tampered".into();
    assert!(card.verify().is_err());
}

#[test]
fn test_agent_card_json_roundtrip() {
    let kp = ZaionKeypair::generate();
    let card = AgentCard::new(&kp, "test-agent", vec![], vec![]);
    let json = card.to_json().unwrap();
    let restored = AgentCard::from_json(&json).unwrap();
    assert_eq!(restored.principal_id, card.principal_id);
    assert!(restored.verify().is_ok());
}

#[test]
fn test_federation_register_and_delegate() {
    let kp_a = ZaionKeypair::generate();
    let kp_b = ZaionKeypair::generate();
    let card_a = AgentCard::new(&kp_a, "agent-a", vec!["task.echo".into()], vec![]);
    let card_b = AgentCard::new(&kp_b, "agent-b", vec!["task.summarize".into()], vec![]);
    let pid_b = card_b.principal_id.clone();
    let mut registry = FederationRegistry::new();
    registry.register(card_a).unwrap();
    registry.register(card_b).unwrap();
    assert_eq!(registry.list().len(), 2);
    let msg = registry
        .delegate(
            &kp_a,
            &pid_b,
            "task.summarize",
            serde_json::json!({ "text": "hello zaion" }),
        )
        .unwrap();
    assert_eq!(msg.message_type, MessageType::Delegate);
    assert_eq!(msg.from_principal, kp_a.principal_id().as_str());
    assert_eq!(msg.to_principal, pid_b);
    assert!(registry.verify_message(&msg).is_ok());
}

#[test]
fn test_delegate_to_unknown_fails() {
    let kp = ZaionKeypair::generate();
    let registry = FederationRegistry::new();
    let result = registry.delegate(&kp, "unknown-principal", "task.echo", serde_json::json!({}));
    assert!(result.is_err());
}
