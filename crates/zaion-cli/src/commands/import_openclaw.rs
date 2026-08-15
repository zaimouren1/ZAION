use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::fs;

/// OpenClaw migration configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationConfig {
    /// Source OpenClaw directory (default: ~/.openclaw)
    pub source_path: PathBuf,

    /// Target Zaion directory (default: ZAION_HOME or ~/.zaion)
    pub target_path: PathBuf,

    /// Migration preset: "user-data" or "full"
    pub preset: MigrationPreset,

    /// Overwrite existing files
    pub overwrite: bool,

    /// Migrate secrets (API keys)
    pub migrate_secrets: bool,

    /// Skill conflict resolution strategy
    pub skill_conflict: SkillConflictStrategy,

    /// Optional workspace directory that receives migrated instruction files
    pub workspace_target: Option<PathBuf>,

    /// Dry run (preview only)
    pub dry_run: bool,
}

/// Migration preset
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MigrationPreset {
    /// User data only (no secrets)
    UserData,
    /// Full migration including secrets
    Full,
}

/// Skill conflict resolution strategy
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SkillConflictStrategy {
    /// Skip conflicting skills
    Skip,
    /// Overwrite existing skills
    Overwrite,
    /// Rename imported skills
    Rename,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            source_path: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".openclaw"),
            target_path: zaion_paths::zaion_home(),
            preset: MigrationPreset::Full,
            overwrite: false,
            migrate_secrets: true,
            skill_conflict: SkillConflictStrategy::Skip,
            workspace_target: None,
            dry_run: false,
        }
    }
}

/// Migration report
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MigrationReport {
    /// Successfully migrated items
    pub migrated: Vec<MigrationItem>,

    /// Skipped items (conflicts or not found)
    pub skipped: Vec<MigrationItem>,

    /// Failed items (errors)
    pub failed: Vec<MigrationItem>,

    /// Total items processed
    pub total: usize,
}

/// A single migration item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationItem {
    /// Item type (e.g., "SOUL.md", "skill", "api_key")
    pub item_type: String,

    /// Source path
    pub source: PathBuf,

    /// Target path
    pub target: PathBuf,

    /// Status message
    pub message: String,
}

impl MigrationReport {
    /// Add a migrated item
    pub fn add_migrated(&mut self, item_type: String, source: PathBuf, target: PathBuf) {
        self.migrated.push(MigrationItem {
            item_type,
            source,
            target,
            message: "Migrated successfully".to_string(),
        });
        self.total += 1;
    }

    /// Add a skipped item
    pub fn add_skipped(&mut self, item_type: String, source: PathBuf, reason: String) {
        self.skipped.push(MigrationItem {
            item_type,
            source: source.clone(),
            target: source,
            message: reason,
        });
        self.total += 1;
    }

    /// Add a failed item
    pub fn add_failed(&mut self, item_type: String, source: PathBuf, error: String) {
        self.failed.push(MigrationItem {
            item_type,
            source: source.clone(),
            target: source,
            message: error,
        });
        self.total += 1;
    }

    /// Print summary
    pub fn print_summary(&self) {
        println!("\n=== Migration Report ===");
        println!("Total items: {}", self.total);
        println!("Migrated: {}", self.migrated.len());
        println!("Skipped: {}", self.skipped.len());
        println!("Failed: {}", self.failed.len());

        if !self.migrated.is_empty() {
            println!("\nMigrated items:");
            for item in &self.migrated {
                println!(
                    "  ok {} -> {}",
                    item.source.display(),
                    item.target.display()
                );
            }
        }

        if !self.skipped.is_empty() {
            println!("\nSkipped items:");
            for item in &self.skipped {
                println!("  skip {}: {}", item.source.display(), item.message);
            }
        }

        if !self.failed.is_empty() {
            println!("\nFailed items:");
            for item in &self.failed {
                println!("  fail {}: {}", item.source.display(), item.message);
            }
        }
    }
}

/// OpenClaw to Zaion migrator
pub struct OpenClawMigrator {
    config: MigrationConfig,
}

impl OpenClawMigrator {
    /// Create new migrator
    pub fn new(config: MigrationConfig) -> Self {
        Self { config }
    }

    /// Run migration
    pub async fn migrate(&self) -> Result<MigrationReport, std::io::Error> {
        let mut report = MigrationReport::default();

        eprintln!("Starting OpenClaw migration");
        eprintln!("Source: {}", self.config.source_path.display());
        eprintln!("Target: {}", self.config.target_path.display());
        eprintln!("Preset: {:?}", self.config.preset);
        eprintln!("Dry run: {}", self.config.dry_run);

        // Verify source exists
        if !self.config.source_path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "OpenClaw directory not found: {}",
                    self.config.source_path.display()
                ),
            ));
        }

        // Create target directory if needed
        if !self.config.dry_run {
            fs::create_dir_all(&self.config.target_path).await?;
        }

        // Migrate SOUL.md
        self.migrate_soul(&mut report).await?;

        // Migrate memory files
        self.migrate_memory(&mut report).await?;

        // Migrate skills
        self.migrate_skills(&mut report).await?;

        // Migrate exec approval patterns
        self.migrate_exec_approval(&mut report).await?;

        // Migrate workspace instruction files if explicitly requested
        self.migrate_workspace_instructions(&mut report).await?;

        // Migrate API keys (if full preset)
        if self.config.preset == MigrationPreset::Full && self.config.migrate_secrets {
            self.migrate_secrets(&mut report).await?;
        }

        eprintln!("Migration complete");
        Ok(report)
    }

    /// Migrate SOUL.md
    async fn migrate_soul(&self, report: &mut MigrationReport) -> Result<(), std::io::Error> {
        let source = self.config.source_path.join("workspace/SOUL.md");
        let target = self.config.target_path.join("SOUL.md");

        if !source.exists() {
            report.add_skipped("SOUL.md".to_string(), source, "Not found".to_string());
            return Ok(());
        }

        if target.exists() && !self.config.overwrite {
            report.add_skipped(
                "SOUL.md".to_string(),
                source,
                "Already exists (use --overwrite)".to_string(),
            );
            return Ok(());
        }

        if !self.config.dry_run {
            fs::copy(&source, &target).await?;
        }

        report.add_migrated("SOUL.md".to_string(), source, target);
        Ok(())
    }

    /// Migrate memory files
    async fn migrate_memory(&self, report: &mut MigrationReport) -> Result<(), std::io::Error> {
        let memory_files = vec!["MEMORY.md", "USER.md"];

        for file in memory_files {
            let source = self.config.source_path.join(format!("workspace/{}", file));
            let target = self.config.target_path.join(format!("memories/{}", file));

            if !source.exists() {
                report.add_skipped(file.to_string(), source, "Not found".to_string());
                continue;
            }

            if target.exists() && !self.config.overwrite {
                report.add_skipped(
                    file.to_string(),
                    source,
                    "Already exists (use --overwrite)".to_string(),
                );
                continue;
            }

            if !self.config.dry_run {
                let parent = target.parent().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("migration target has no parent dir: {}", target.display()),
                    )
                })?;
                fs::create_dir_all(parent).await?;
                fs::copy(&source, &target).await?;
            }

            report.add_migrated(file.to_string(), source, target);
        }

        Ok(())
    }

    /// Migrate skills
    async fn migrate_skills(&self, report: &mut MigrationReport) -> Result<(), std::io::Error> {
        let source_skills = self.config.source_path.join("workspace/skills");
        let target_skills = self.config.target_path.join("skills/openclaw-imports");

        if !source_skills.exists() {
            report.add_skipped(
                "skills".to_string(),
                source_skills,
                "Skills directory not found".to_string(),
            );
            return Ok(());
        }

        if !self.config.dry_run {
            fs::create_dir_all(&target_skills).await?;
        }

        // Traverse skills directory
        let mut entries = match fs::read_dir(&source_skills).await {
            Ok(entries) => entries,
            Err(e) => {
                report.add_failed(
                    "skills".to_string(),
                    source_skills,
                    format!("Failed to read directory: {}", e),
                );
                return Ok(());
            }
        };

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let skill_name = match path.file_name() {
                Some(name) => name.to_string_lossy().to_string(),
                None => continue,
            };

            let target_skill = target_skills.join(&skill_name);

            // Check for conflicts
            if target_skill.exists() {
                match self.config.skill_conflict {
                    SkillConflictStrategy::Skip => {
                        report.add_skipped(
                            format!("skill:{}", skill_name),
                            path,
                            "Already exists (use --skill-conflict overwrite or rename)".to_string(),
                        );
                        continue;
                    }
                    SkillConflictStrategy::Overwrite => {
                        if !self.config.dry_run {
                            fs::remove_dir_all(&target_skill).await?;
                        }
                    }
                    SkillConflictStrategy::Rename => {
                        // Find unique name
                        let mut counter = 1;
                        let mut renamed_target =
                            target_skills.join(format!("{}-imported", skill_name));
                        while renamed_target.exists() {
                            counter += 1;
                            renamed_target =
                                target_skills.join(format!("{}-imported-{}", skill_name, counter));
                        }

                        if !self.config.dry_run {
                            copy_dir_recursive(&path, &renamed_target).await?;
                        }

                        report.add_migrated(format!("skill:{}", skill_name), path, renamed_target);
                        continue;
                    }
                }
            }

            // Copy skill directory
            if !self.config.dry_run {
                copy_dir_recursive(&path, &target_skill).await?;
            }

            report.add_migrated(format!("skill:{}", skill_name), path, target_skill);
        }

        Ok(())
    }

    /// Migrate exec approval patterns
    async fn migrate_exec_approval(
        &self,
        report: &mut MigrationReport,
    ) -> Result<(), std::io::Error> {
        let source = self
            .config
            .source_path
            .join("workspace/exec_approval_patterns.yaml");
        let target = self
            .config
            .target_path
            .join("config/exec_approval_patterns.yaml");

        if !source.exists() {
            report.add_skipped(
                "exec_approval_patterns.yaml".to_string(),
                source,
                "Not found".to_string(),
            );
            return Ok(());
        }

        if target.exists() && !self.config.overwrite {
            report.add_skipped(
                "exec_approval_patterns.yaml".to_string(),
                source,
                "Already exists (use --overwrite)".to_string(),
            );
            return Ok(());
        }

        if !self.config.dry_run {
            let parent = target.parent().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("migration target has no parent dir: {}", target.display()),
                )
            })?;
            fs::create_dir_all(parent).await?;
            fs::copy(&source, &target).await?;
        }

        report.add_migrated("exec_approval_patterns.yaml".to_string(), source, target);
        Ok(())
    }

    /// Migrate workspace-scoped instruction files into a target workspace.
    async fn migrate_workspace_instructions(
        &self,
        report: &mut MigrationReport,
    ) -> Result<(), std::io::Error> {
        let Some(workspace_target) = &self.config.workspace_target else {
            return Ok(());
        };
        let source_workspace = self.config.source_path.join("workspace");
        let instruction_files = [
            "AGENTS.md",
            "CLAUDE.md",
            "SOUL.md",
            "README.md",
            "instructions.md",
            ".cursorrules",
        ];
        let mut found = false;

        for file in instruction_files {
            let source = source_workspace.join(file);
            if !source.exists() {
                continue;
            }
            found = true;
            let target = workspace_target.join(file);
            if target.exists() && !self.config.overwrite {
                report.add_skipped(
                    format!("workspace:{}", file),
                    source,
                    "Already exists (use --overwrite)".to_string(),
                );
                continue;
            }
            if !self.config.dry_run {
                fs::create_dir_all(workspace_target).await?;
                fs::copy(&source, &target).await?;
            }
            report.add_migrated(format!("workspace:{}", file), source, target);
        }

        if !found {
            report.add_skipped(
                "workspace-instructions".to_string(),
                source_workspace,
                "No workspace instruction files found".to_string(),
            );
        }

        Ok(())
    }

    /// Migrate API keys and secrets
    async fn migrate_secrets(&self, report: &mut MigrationReport) -> Result<(), std::io::Error> {
        // Migrate from OpenClaw config.yaml and .env
        let config_yaml = self.config.source_path.join("config.yaml");
        let env_file = self.config.source_path.join(".env");

        let mut secrets_found = HashMap::new();

        // Parse config.yaml if exists
        if config_yaml.exists() {
            match self.parse_openclaw_config(&config_yaml).await {
                Ok(secrets) => {
                    secrets_found.extend(secrets);
                }
                Err(e) => {
                    report.add_failed(
                        "config.yaml".to_string(),
                        config_yaml.clone(),
                        format!("Failed to parse: {}", e),
                    );
                }
            }
        }

        // Parse .env if exists
        if env_file.exists() {
            match self.parse_env_file(&env_file).await {
                Ok(secrets) => {
                    secrets_found.extend(secrets);
                }
                Err(e) => {
                    report.add_failed(
                        ".env".to_string(),
                        env_file.clone(),
                        format!("Failed to parse: {}", e),
                    );
                }
            }
        }

        if secrets_found.is_empty() {
            report.add_skipped(
                "secrets".to_string(),
                self.config.source_path.clone(),
                "No secrets found".to_string(),
            );
            return Ok(());
        }

        // Allowlist of secrets to migrate
        let allowlist = vec![
            "TELEGRAM_BOT_TOKEN",
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "ELEVENLABS_API_KEY",
            "GOOGLE_API_KEY",
        ];

        let target_env = self.config.target_path.join(".env");

        // Load existing .env if exists
        let mut existing_secrets = HashMap::new();
        if target_env.exists() {
            if let Ok(secrets) = self.parse_env_file(&target_env).await {
                existing_secrets = secrets;
            }
        }

        let mut migrated_count = 0;
        let mut skipped_count = 0;

        for key in allowlist {
            if let Some(value) = secrets_found.get(key) {
                // Skip if already exists and not overwriting
                if existing_secrets.contains_key(key) && !self.config.overwrite {
                    report.add_skipped(
                        format!("secret:{}", key),
                        self.config.source_path.clone(),
                        "Already exists (use --overwrite)".to_string(),
                    );
                    skipped_count += 1;
                    continue;
                }

                // Write to target .env
                if !self.config.dry_run {
                    existing_secrets.insert(key.to_string(), value.clone());
                }

                report.add_migrated(
                    format!("secret:{}", key),
                    self.config.source_path.clone(),
                    target_env.clone(),
                );
                migrated_count += 1;
            }
        }

        // Write updated .env file
        if !self.config.dry_run && migrated_count > 0 {
            let mut env_content = String::new();
            for (key, value) in existing_secrets {
                env_content.push_str(&format!("{}={}\n", key, value));
            }
            fs::write(&target_env, env_content).await?;
        }

        if migrated_count == 0 && skipped_count == 0 {
            report.add_skipped(
                "secrets".to_string(),
                self.config.source_path.clone(),
                "No allowlisted secrets found".to_string(),
            );
        }

        Ok(())
    }

    /// Parse OpenClaw config.yaml
    async fn parse_openclaw_config(
        &self,
        path: &Path,
    ) -> Result<HashMap<String, String>, std::io::Error> {
        let content = fs::read_to_string(path).await?;
        let mut secrets = HashMap::new();

        // Simple YAML parsing for key-value pairs
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim_matches('\'');

                // Extract known secret keys
                if key.contains("API_KEY") || key.contains("TOKEN") || key.contains("SECRET") {
                    secrets.insert(key.to_uppercase().replace('-', "_"), value.to_string());
                }
            }
        }

        Ok(secrets)
    }

    /// Parse .env file
    async fn parse_env_file(&self, path: &Path) -> Result<HashMap<String, String>, std::io::Error> {
        let content = fs::read_to_string(path).await?;
        let mut secrets = HashMap::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim_matches('\'');
                secrets.insert(key.to_string(), value.to_string());
            }
        }

        Ok(secrets)
    }
}

/// Recursively copy directory (standalone function to avoid async recursion)
async fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    use std::future::Future;
    use std::pin::Pin;

    fn copy_dir_inner<'a>(
        src: &'a Path,
        dst: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<(), std::io::Error>> + 'a>> {
        Box::pin(async move {
            fs::create_dir_all(dst).await?;

            let mut entries = fs::read_dir(src).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let file_name = match path.file_name() {
                    Some(name) => name,
                    None => continue,
                };
                let target_path = dst.join(file_name);

                if path.is_dir() {
                    copy_dir_inner(&path, &target_path).await?;
                } else {
                    fs::copy(&path, &target_path).await?;
                }
            }

            Ok(())
        })
    }

    copy_dir_inner(src, dst).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_config_default() {
        let config = MigrationConfig::default();
        assert_eq!(config.preset, MigrationPreset::Full);
        assert!(!config.overwrite);
        assert!(config.migrate_secrets);
    }

    #[test]
    fn test_migration_report() {
        let mut report = MigrationReport::default();
        report.add_migrated(
            "test".to_string(),
            PathBuf::from("/src"),
            PathBuf::from("/dst"),
        );
        assert_eq!(report.migrated.len(), 1);
        assert_eq!(report.total, 1);
    }

    #[test]
    fn test_skill_conflict_strategy() {
        assert_eq!(SkillConflictStrategy::Skip, SkillConflictStrategy::Skip);
        assert_ne!(
            SkillConflictStrategy::Skip,
            SkillConflictStrategy::Overwrite
        );
    }
}
