use crate::RuntimeError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zaion_crypto::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_types::session::NamespaceKey;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub job_id: String,
    pub name: String,
    pub schedule: String,
    pub command: String,
    #[serde(default)]
    pub deliver: Option<String>,
    #[serde(default)]
    pub repeat: Option<u32>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub script: Option<String>,
    pub principal_id: String,
    pub enabled: bool,
    pub last_run: Option<String>,
    pub next_run: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CronStore {
    jobs: Vec<CronJob>,
}

pub struct CronEngine {
    store_path: PathBuf,
    ledger: EventLedger,
    keypair: ZaionKeypair,
    namespace_key: NamespaceKey,
}

impl CronEngine {
    pub fn new(
        store_path: impl AsRef<Path>,
        ledger: EventLedger,
        keypair: ZaionKeypair,
        namespace_key: NamespaceKey,
    ) -> Self {
        Self {
            store_path: store_path.as_ref().to_path_buf(),
            ledger,
            keypair,
            namespace_key,
        }
    }

    fn load(&self) -> CronStore {
        std::fs::read_to_string(&self.store_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, store: &CronStore) -> Result<(), RuntimeError> {
        if let Some(parent) = self.store_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| RuntimeError::Internal(e.to_string()))?;
        }
        let data = serde_json::to_string_pretty(store)
            .map_err(|e| RuntimeError::Internal(e.to_string()))?;
        std::fs::write(&self.store_path, data).map_err(|e| RuntimeError::Internal(e.to_string()))
    }

    pub fn add(&self, name: &str, schedule: &str, command: &str) -> Result<CronJob, RuntimeError> {
        let now = chrono::Utc::now().to_rfc3339();
        let job = CronJob {
            job_id: format!("cron-{}", uuid::Uuid::new_v4()),
            name: name.to_string(),
            schedule: schedule.to_string(),
            command: command.to_string(),
            deliver: None,
            repeat: None,
            skills: Vec::new(),
            script: None,
            principal_id: self.keypair.principal_id().as_str().to_string(),
            enabled: true,
            last_run: None,
            next_run: Some(now.clone()),
            created_at: now,
        };
        let mut store = self.load();
        store.jobs.push(job.clone());
        self.save(&store)?;
        let payload = serde_json::json!({
            "job_id": job.job_id,
            "name": name,
            "schedule": schedule,
            "command": command,
        });
        self.ledger
            .append_signed_event(
                &self.keypair,
                &self.namespace_key,
                "cron.added",
                payload,
                None,
            )
            .map_err(|e| RuntimeError::Internal(e.to_string()))?;
        Ok(job)
    }

    pub fn remove(&self, job_id: &str) -> Result<(), RuntimeError> {
        let mut store = self.load();
        let before = store.jobs.len();
        store.jobs.retain(|j| j.job_id != job_id);
        if store.jobs.len() == before {
            return Err(RuntimeError::Internal(format!("job not found: {}", job_id)));
        }
        self.save(&store)?;
        let payload = serde_json::json!({ "job_id": job_id });
        self.ledger
            .append_signed_event(
                &self.keypair,
                &self.namespace_key,
                "cron.removed",
                payload,
                None,
            )
            .map_err(|e| RuntimeError::Internal(e.to_string()))?;
        Ok(())
    }

    pub fn set_enabled(&self, job_id: &str, enabled: bool) -> Result<CronJob, RuntimeError> {
        let mut store = self.load();
        let job = store
            .jobs
            .iter_mut()
            .find(|j| j.job_id == job_id)
            .ok_or_else(|| RuntimeError::Internal(format!("job not found: {}", job_id)))?;
        job.enabled = enabled;
        let job_clone = job.clone();
        self.save(&store)?;
        let payload = serde_json::json!({
            "job_id": job_id,
            "enabled": enabled,
        });
        self.ledger
            .append_signed_event(
                &self.keypair,
                &self.namespace_key,
                if enabled {
                    "cron.resumed"
                } else {
                    "cron.paused"
                },
                payload,
                None,
            )
            .map_err(|e| RuntimeError::Internal(e.to_string()))?;
        Ok(job_clone)
    }

    pub fn edit(
        &self,
        job_id: &str,
        name: Option<&str>,
        schedule: Option<&str>,
        command: Option<&str>,
    ) -> Result<CronJob, RuntimeError> {
        let mut store = self.load();
        let job = store
            .jobs
            .iter_mut()
            .find(|j| j.job_id == job_id)
            .ok_or_else(|| RuntimeError::Internal(format!("job not found: {}", job_id)))?;
        if let Some(name) = name {
            job.name = name.to_string();
        }
        if let Some(schedule) = schedule {
            job.schedule = schedule.to_string();
        }
        if let Some(command) = command {
            job.command = command.to_string();
        }
        let job_clone = job.clone();
        self.save(&store)?;
        let payload = serde_json::json!({
            "job_id": job_id,
            "name": job_clone.name,
            "schedule": job_clone.schedule,
        });
        self.ledger
            .append_signed_event(
                &self.keypair,
                &self.namespace_key,
                "cron.edited",
                payload,
                None,
            )
            .map_err(|e| RuntimeError::Internal(e.to_string()))?;
        Ok(job_clone)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_metadata(
        &self,
        job_id: &str,
        deliver: Option<&str>,
        repeat: Option<u32>,
        skills: Option<Vec<String>>,
        add_skills: Vec<String>,
        remove_skills: Vec<String>,
        clear_skills: bool,
        script: Option<&str>,
    ) -> Result<CronJob, RuntimeError> {
        let mut store = self.load();
        let job = store
            .jobs
            .iter_mut()
            .find(|j| j.job_id == job_id)
            .ok_or_else(|| RuntimeError::Internal(format!("job not found: {}", job_id)))?;
        if let Some(deliver) = deliver {
            job.deliver = Some(deliver.to_string());
        }
        if let Some(repeat) = repeat {
            job.repeat = Some(repeat);
        }
        if let Some(skills) = skills {
            job.skills = skills;
        }
        if clear_skills {
            job.skills.clear();
        }
        for skill in add_skills {
            if !job.skills.iter().any(|existing| existing == &skill) {
                job.skills.push(skill);
            }
        }
        if !remove_skills.is_empty() {
            job.skills
                .retain(|skill| !remove_skills.iter().any(|remove| remove == skill));
        }
        if let Some(script) = script {
            if script.is_empty() {
                job.script = None;
            } else {
                job.script = Some(script.to_string());
            }
        }
        let job_clone = job.clone();
        self.save(&store)?;
        let payload = serde_json::json!({
            "job_id": job_id,
            "deliver": job_clone.deliver,
            "repeat": job_clone.repeat,
            "skills": job_clone.skills,
            "script": job_clone.script,
        });
        self.ledger
            .append_signed_event(
                &self.keypair,
                &self.namespace_key,
                "cron.metadata",
                payload,
                None,
            )
            .map_err(|e| RuntimeError::Internal(e.to_string()))?;
        Ok(job_clone)
    }

    pub fn list(&self) -> Vec<CronJob> {
        self.load().jobs
    }

    pub fn run_now(&self, job_id: &str) -> Result<CronJob, RuntimeError> {
        let mut store = self.load();
        let job = store
            .jobs
            .iter_mut()
            .find(|j| j.job_id == job_id)
            .ok_or_else(|| RuntimeError::Internal(format!("job not found: {}", job_id)))?;
        let now = chrono::Utc::now().to_rfc3339();
        job.last_run = Some(now.clone());
        let job_clone = job.clone();
        self.save(&store)?;
        let payload = serde_json::json!({
            "job_id": job_id,
            "triggered_at": now,
            "command": job_clone.command,
        });
        self.ledger
            .append_signed_event(
                &self.keypair,
                &self.namespace_key,
                "cron.triggered",
                payload,
                None,
            )
            .map_err(|e| RuntimeError::Internal(e.to_string()))?;
        Ok(job_clone)
    }

    pub fn tick(&self) -> Result<Vec<CronJob>, RuntimeError> {
        let jobs = self
            .list()
            .into_iter()
            .filter(|job| job.enabled)
            .collect::<Vec<_>>();
        let mut triggered = Vec::new();
        for job in jobs {
            triggered.push(self.run_now(&job.job_id)?);
        }
        Ok(triggered)
    }

    pub fn logs(
        &self,
        job_id: &str,
        limit: usize,
    ) -> Result<Vec<zaion_types::event::LedgerEvent>, RuntimeError> {
        let events = self
            .ledger
            .list_global_events(limit * 10)
            .map_err(|e| RuntimeError::Internal(e.to_string()))?;
        Ok(events
            .into_iter()
            .filter(|e| {
                e.event_type.starts_with("cron.")
                    && e.payload.get("job_id").and_then(|v| v.as_str()) == Some(job_id)
            })
            .take(limit)
            .collect())
    }
}
