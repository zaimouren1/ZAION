use crate::{ShadowTask, TaskId, TaskStatus};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStats {
    pub total_tasks: usize,
    pub queued: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub max_capacity: usize,
}

#[derive(Debug)]
pub struct TaskQueue {
    tasks: HashMap<TaskId, ShadowTask>,
    queue: VecDeque<TaskId>,
    running: HashMap<TaskId, ShadowTask>,
    max_capacity: usize,
    max_concurrent: usize,
}

impl TaskQueue {
    pub fn new(max_capacity: usize, max_concurrent: usize) -> Self {
        Self {
            tasks: HashMap::new(),
            queue: VecDeque::new(),
            running: HashMap::new(),
            max_capacity,
            max_concurrent,
        }
    }

    pub fn enqueue(&mut self, mut task: ShadowTask) -> Result<TaskId, crate::ShadowError> {
        if self.tasks.len() >= self.max_capacity {
            return Err(crate::ShadowError::QueueFull {
                max: self.max_capacity,
            });
        }

        task.status = TaskStatus::Queued;
        let task_id = task.id;

        // Insert by priority (higher priority = front of queue)
        let insert_pos = self
            .queue
            .iter()
            .position(|&id| {
                self.tasks
                    .get(&id)
                    .map(|t| t.priority < task.priority)
                    .unwrap_or(false)
            })
            .unwrap_or(self.queue.len());

        self.queue.insert(insert_pos, task_id);
        self.tasks.insert(task_id, task);

        Ok(task_id)
    }

    pub fn dequeue(&mut self) -> Option<ShadowTask> {
        if self.running.len() >= self.max_concurrent {
            return None;
        }

        if let Some(task_id) = self.queue.pop_front() {
            if let Some(mut task) = self.tasks.remove(&task_id) {
                task.status = TaskStatus::Running;
                task.started_at = Some(chrono::Utc::now());
                self.running.insert(task_id, task.clone());
                return Some(task);
            }
        }
        None
    }

    pub fn complete_task(
        &mut self,
        task_id: TaskId,
        result: crate::TaskResult,
    ) -> Result<(), crate::ShadowError> {
        if let Some(mut task) = self.running.remove(&task_id) {
            task.status = if result.success {
                TaskStatus::Completed
            } else {
                TaskStatus::Failed
            };
            task.completed_at = Some(chrono::Utc::now());
            task.result = Some(result);

            // Handle retries for failed tasks
            if !task.result.as_ref().unwrap().success && task.can_retry() {
                task.retry_count += 1;
                task.status = TaskStatus::Queued;
                task.started_at = None;
                task.completed_at = None;
                task.result = None;

                // Re-queue with same priority
                let insert_pos = self
                    .queue
                    .iter()
                    .position(|&id| {
                        self.tasks
                            .get(&id)
                            .map(|t| t.priority < task.priority)
                            .unwrap_or(false)
                    })
                    .unwrap_or(self.queue.len());

                self.queue.insert(insert_pos, task_id);
            }

            self.tasks.insert(task_id, task);
            Ok(())
        } else {
            Err(crate::ShadowError::TaskNotFound(task_id.to_string()))
        }
    }

    pub fn cancel_task(&mut self, task_id: TaskId) -> Result<(), crate::ShadowError> {
        // Remove from queue if queued
        if let Some(pos) = self.queue.iter().position(|&id| id == task_id) {
            self.queue.remove(pos);
            if let Some(task) = self.tasks.get_mut(&task_id) {
                task.status = TaskStatus::Cancelled;
                task.completed_at = Some(chrono::Utc::now());
                return Ok(());
            }
        }

        // Remove from running if running
        if let Some(mut task) = self.running.remove(&task_id) {
            task.status = TaskStatus::Cancelled;
            task.completed_at = Some(chrono::Utc::now());
            self.tasks.insert(task_id, task);
            return Ok(());
        }

        Err(crate::ShadowError::TaskNotFound(task_id.to_string()))
    }

    pub fn get_task(&self, task_id: &TaskId) -> Option<&ShadowTask> {
        self.tasks
            .get(task_id)
            .or_else(|| self.running.get(task_id))
    }

    pub fn list_tasks(&self) -> Vec<&ShadowTask> {
        self.tasks.values().chain(self.running.values()).collect()
    }

    pub fn stats(&self) -> QueueStats {
        let mut stats = QueueStats {
            total_tasks: self.tasks.len() + self.running.len(),
            queued: 0,
            running: self.running.len(),
            completed: 0,
            failed: 0,
            cancelled: 0,
            max_capacity: self.max_capacity,
        };

        for task in self.tasks.values() {
            match task.status {
                TaskStatus::Queued => stats.queued += 1,
                TaskStatus::Completed => stats.completed += 1,
                TaskStatus::Failed => stats.failed += 1,
                TaskStatus::Cancelled => stats.cancelled += 1,
                _ => {}
            }
        }

        stats
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty() && self.running.is_empty()
    }

    pub fn can_accept_more(&self) -> bool {
        self.tasks.len() + self.running.len() < self.max_capacity
    }

    pub fn has_capacity_to_run(&self) -> bool {
        self.running.len() < self.max_concurrent && !self.queue.is_empty()
    }
}
