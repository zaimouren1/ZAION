use crate::RuntimeError;

#[derive(Debug, Clone)]
pub struct Policy {
    pub max_tasks_per_run: usize,
    pub allowed_task_types: Vec<String>,
    pub require_signature: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            max_tasks_per_run: 100,
            allowed_task_types: vec![],
            require_signature: true,
        }
    }
}

pub struct PolicyEngine {
    policy: Policy,
}

impl PolicyEngine {
    pub fn new(policy: Policy) -> Self {
        Self { policy }
    }

    pub fn check_task_type(&self, task_type: &str) -> Result<(), RuntimeError> {
        if self.policy.allowed_task_types.is_empty() {
            return Ok(());
        }
        if self
            .policy
            .allowed_task_types
            .iter()
            .any(|t| t == task_type)
        {
            Ok(())
        } else {
            Err(RuntimeError::PolicyViolation(format!(
                "task type '{}' not allowed",
                task_type
            )))
        }
    }

    pub fn check_task_count(&self, count: usize) -> Result<(), RuntimeError> {
        if count >= self.policy.max_tasks_per_run {
            Err(RuntimeError::PolicyViolation(format!(
                "task count {} exceeds max {}",
                count, self.policy.max_tasks_per_run
            )))
        } else {
            Ok(())
        }
    }
}
