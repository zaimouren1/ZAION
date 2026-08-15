use crate::RuntimeError;
use serde::{Deserialize, Serialize};
use zaion_crypto::keypair::ZaionKeypair;
use zaion_ledger::EventLedger;
pub use zaion_types::task::TaskStatus;
use zaion_types::{event::EventId, session::NamespaceKey};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    pub principal_id: String,
    pub session_key: String,
    pub task_type: String,
    pub input: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub status: TaskStatus,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub type TaskHandler = Box<dyn Fn(&Task) -> Result<serde_json::Value, String> + Send + Sync>;

pub struct TaskEngine {
    ledger: EventLedger,
    keypair: ZaionKeypair,
    namespace_key: NamespaceKey,
}

impl TaskEngine {
    pub fn new(ledger: EventLedger, keypair: ZaionKeypair, namespace_key: NamespaceKey) -> Self {
        Self {
            ledger,
            keypair,
            namespace_key,
        }
    }

    pub fn execute(
        &self,
        task_type: &str,
        input: serde_json::Value,
        handler: &dyn Fn(&Task) -> Result<serde_json::Value, String>,
    ) -> Result<(Task, EventId), RuntimeError> {
        let now = chrono::Utc::now().to_rfc3339();
        let task_id = format!("tsk-{}", uuid::Uuid::new_v4());
        let pid = self.keypair.principal_id();
        let mut task = Task {
            task_id: task_id.clone(),
            principal_id: pid.as_str().to_string(),
            session_key: self.namespace_key.0.clone(),
            task_type: task_type.to_string(),
            input: input.clone(),
            output: None,
            status: TaskStatus::Running,
            error: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        let start_payload = serde_json::json!({
            "task_id": task_id,
            "task_type": task_type,
            "input": input,
        });
        self.ledger.append_signed_event(
            &self.keypair,
            &self.namespace_key,
            "task.started",
            start_payload,
            None,
        )?;
        let (status, output, error, event_type) = match handler(&task) {
            Ok(result) => (
                TaskStatus::Completed,
                Some(result.clone()),
                None,
                "task.completed",
            ),
            Err(e) => (TaskStatus::Failed, None, Some(e.clone()), "task.failed"),
        };
        task.status = status;
        task.output = output.clone();
        task.error = error.clone();
        task.updated_at = chrono::Utc::now().to_rfc3339();
        let end_payload = serde_json::json!({
            "task_id": task_id,
            "task_type": task_type,
            "output": output,
            "error": error,
        });
        let event_id = self.ledger.append_signed_event(
            &self.keypair,
            &self.namespace_key,
            event_type,
            end_payload,
            None,
        )?;
        Ok((task, event_id))
    }
}
