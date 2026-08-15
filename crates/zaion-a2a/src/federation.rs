use crate::{
    agent_card::AgentCard,
    protocol::{A2AMessage, MessageType},
    A2AError,
};
use std::collections::HashMap;
use zaion_crypto::keypair::ZaionKeypair;

pub struct FederationRegistry {
    cards: HashMap<String, AgentCard>,
}

impl FederationRegistry {
    pub fn new() -> Self {
        Self {
            cards: HashMap::new(),
        }
    }

    pub fn register(&mut self, card: AgentCard) -> Result<(), A2AError> {
        card.verify()?;
        self.cards.insert(card.principal_id.clone(), card);
        Ok(())
    }

    pub fn get(&self, principal_id: &str) -> Option<&AgentCard> {
        self.cards.get(principal_id)
    }

    pub fn list(&self) -> Vec<&AgentCard> {
        self.cards.values().collect()
    }

    pub fn delegate(
        &self,
        from_keypair: &ZaionKeypair,
        to_principal: &str,
        task_type: &str,
        input: serde_json::Value,
    ) -> Result<A2AMessage, A2AError> {
        if !self.cards.contains_key(to_principal) {
            return Err(A2AError::AgentNotFound(to_principal.to_string()));
        }
        let payload = serde_json::json!({
            "task_type": task_type,
            "input": input,
        });
        Ok(A2AMessage::new(
            from_keypair,
            to_principal,
            MessageType::Delegate,
            payload,
        ))
    }

    pub fn verify_message(&self, msg: &A2AMessage) -> Result<(), A2AError> {
        let card = self
            .cards
            .get(&msg.from_principal)
            .ok_or_else(|| A2AError::AgentNotFound(msg.from_principal.clone()))?;
        msg.verify(card)
    }
}

impl Default for FederationRegistry {
    fn default() -> Self {
        Self::new()
    }
}
