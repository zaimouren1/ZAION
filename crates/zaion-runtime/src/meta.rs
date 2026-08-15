use crate::{
    task::{Task, TaskStatus},
    RuntimeError,
};
use zaion_crypto::keypair::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_memory::skill::SkillStore;
use zaion_types::identity::PrincipalId;
use zaion_types::session::NamespaceKey;

pub struct MetaEngine {
    ledger: EventLedger,
    skill_store: SkillStore,
    keypair: ZaionKeypair,
    namespace_key: NamespaceKey,
}

impl MetaEngine {
    pub fn new(
        ledger: EventLedger,
        skill_store: SkillStore,
        keypair: ZaionKeypair,
        namespace_key: NamespaceKey,
    ) -> Self {
        Self {
            ledger,
            skill_store,
            keypair,
            namespace_key,
        }
    }

    pub fn reflect(&self, task: &Task) -> Result<Option<String>, RuntimeError> {
        let pid = self.keypair.principal_id();
        match &task.status {
            TaskStatus::Completed => self.distill_success(&pid, task),
            TaskStatus::Failed => self.distill_failure(&pid, task),
            _ => Ok(None),
        }
    }

    fn distill_success(
        &self,
        pid: &PrincipalId,
        task: &Task,
    ) -> Result<Option<String>, RuntimeError> {
        let rule = format!(
            "on {}: completed successfully with input pattern {:?}",
            task.task_type,
            task.input
                .get("pattern")
                .unwrap_or(&serde_json::Value::Null)
        );
        let skill_id = self
            .skill_store
            .upsert(pid, &task.task_type, &["success"], &rule, 1.0)
            .map_err(RuntimeError::Memory)?;
        let payload = serde_json::json!({
            "task_id": task.task_id,
            "task_type": task.task_type,
            "skill_id": skill_id,
            "rule": rule,
            "outcome": "success",
        });
        self.ledger.append_signed_event(
            &self.keypair,
            &self.namespace_key,
            "skill.distilled",
            payload,
            None,
        )?;
        Ok(Some(skill_id))
    }

    fn distill_failure(
        &self,
        pid: &PrincipalId,
        task: &Task,
    ) -> Result<Option<String>, RuntimeError> {
        let error_msg = task.error.as_deref().unwrap_or("unknown error");
        let rule = format!("on {}: avoid - error was: {}", task.task_type, error_msg);
        let skill_id = self
            .skill_store
            .upsert(pid, &task.task_type, &["failure", "avoidance"], &rule, 0.5)
            .map_err(RuntimeError::Memory)?;
        let payload = serde_json::json!({
            "task_id": task.task_id,
            "task_type": task.task_type,
            "skill_id": skill_id,
            "rule": rule,
            "outcome": "failure",
        });
        self.ledger.append_signed_event(
            &self.keypair,
            &self.namespace_key,
            "skill.distilled",
            payload,
            None,
        )?;
        Ok(Some(skill_id))
    }

    pub fn load_rules(&self, task_type: &str, limit: usize) -> Result<Vec<String>, RuntimeError> {
        let pid = self.keypair.principal_id();
        let skills = self
            .skill_store
            .query(&pid, task_type, limit)
            .map_err(RuntimeError::Memory)?;
        Ok(skills.into_iter().map(|s| s.rule_text).collect())
    }
}
