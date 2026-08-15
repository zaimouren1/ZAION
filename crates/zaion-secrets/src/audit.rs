use crate::SecretsError;
use zaion_crypto::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_types::session::NamespaceKey;

pub struct SecretsAuditor {
    ledger: EventLedger,
    keypair: ZaionKeypair,
    namespace_key: NamespaceKey,
}

impl SecretsAuditor {
    pub fn new(ledger: EventLedger, keypair: ZaionKeypair, namespace_key: NamespaceKey) -> Self {
        Self {
            ledger,
            keypair,
            namespace_key,
        }
    }

    pub fn log_operation(
        &self,
        operation: &str,
        secret_key: &str,
        detail: Option<&str>,
    ) -> Result<(), SecretsError> {
        let payload = serde_json::json!({
            "operation": operation,
            "secret_key": secret_key,
            "detail": detail,
        });
        self.ledger.append_signed_event(
            &self.keypair,
            &self.namespace_key,
            &format!("secrets.{}", operation),
            payload,
            None,
        )?;
        Ok(())
    }
}
