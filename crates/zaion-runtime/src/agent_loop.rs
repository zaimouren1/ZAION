use crate::{
    agent_fsm::{AgentFsm, AgentState, FinishReason, FsmConfig, LlmOutcome},
    meta::MetaEngine,
    policy::{Policy, PolicyEngine},
    task::{Task, TaskEngine},
    RuntimeError,
};
use zaion_crypto::keypair::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_memory::skill::SkillStore;
use zaion_types::session::NamespaceKey;

pub struct AgentLoop {
    task_engine: TaskEngine,
    meta_engine: MetaEngine,
    policy_engine: PolicyEngine,
    fsm: AgentFsm,
    task_count: usize,
}

impl AgentLoop {
    pub fn new(
        ledger: EventLedger,
        skill_store: SkillStore,
        keypair: ZaionKeypair,
        namespace_key: NamespaceKey,
        policy: Policy,
    ) -> Self {
        let ledger2 = EventLedger::new(ledger.db_path());
        let skill_store2 = SkillStore::new(skill_store.db_path());
        Self {
            task_engine: TaskEngine::new(ledger, keypair.clone(), namespace_key.clone()),
            meta_engine: MetaEngine::new(ledger2, skill_store2, keypair, namespace_key),
            policy_engine: PolicyEngine::new(policy),
            fsm: AgentFsm::new(FsmConfig::default()),
            task_count: 0,
        }
    }

    /// Get the current FSM state.
    pub fn state(&self) -> &AgentState {
        self.fsm.state()
    }

    /// Get the FSM transition history.
    pub fn transitions(&self) -> &[crate::agent_fsm::StateTransition] {
        self.fsm.transitions()
    }

    pub fn run_task(
        &mut self,
        task_type: &str,
        input: serde_json::Value,
        handler: &dyn Fn(&crate::task::Task) -> Result<serde_json::Value, String>,
    ) -> Result<Task, RuntimeError> {
        // FSM: Idle → Thinking (simulating user message)
        let user_msg = zaion_adapters::ChatMessage {
            role: "user".into(),
            content: format!("task: {} with input: {}", task_type, input),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning_content: None,
        };
        self.fsm
            .on_user_message(user_msg)
            .map_err(|e| RuntimeError::Internal(format!("FSM error: {}", e)))?;

        // Policy checks
        self.policy_engine.check_task_type(task_type)?;
        self.policy_engine.check_task_count(self.task_count)?;

        // Execute task (simulates LLM call + execution)
        let (task, _event_id) = self.task_engine.execute(task_type, input, handler)?;

        // FSM: Thinking → Responding (simulating LLM text response)
        let outcome = LlmOutcome {
            finish_reason: FinishReason::Stop,
            tool_calls: vec![],
            text: format!("Task {} completed", task_type),
            tokens_used: 100,
        };
        self.fsm
            .on_llm_response(&outcome)
            .map_err(|e| RuntimeError::Internal(format!("FSM error: {}", e)))?;

        // FSM: Responding → Reflecting
        self.fsm
            .on_response_delivered()
            .map_err(|e| RuntimeError::Internal(format!("FSM error: {}", e)))?;

        // Meta-reflection
        self.meta_engine.reflect(&task)?;

        // FSM: Reflecting → Idle
        self.fsm
            .on_reflection_complete()
            .map_err(|e| RuntimeError::Internal(format!("FSM error: {}", e)))?;

        self.task_count += 1;
        Ok(task)
    }

    pub fn load_rules(&self, task_type: &str) -> Result<Vec<String>, RuntimeError> {
        self.meta_engine.load_rules(task_type, 10)
    }
}
