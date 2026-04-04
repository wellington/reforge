use futures::stream::{self, StreamExt};
use std::collections::HashSet;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::error::Result;
use crate::manager::{Dependency, PackageManager, RegistrySource};
use crate::platform::gitlab::{CommitAction, CreateMrParams, GitLabClient};
use crate::registry::docker::DockerRegistryClient;
use crate::registry::helm::HelmRegistryClient;
use crate::registry::{parse_version_lenient, RegistryClient, VersionInfo};
use crate::updater;
use crate::versioning::{PinStrategy, VersionPolicy};

const CONCURRENCY_LIMIT: usize = 5;

pub struct Orchestrator {
    config: Config,
    gitlab: GitLabClient,
    docker_registry: DockerRegistryClient,
    helm_registry: HelmRegistryClient,
    managers: Vec<Box<dyn PackageManager>>,
    version_policy: VersionPolicy,
    dry_run: bool,
}

#[derive(Debug)]
pub struct UpdateCandidate {
    pub dependency: Dependency,
    pub new_version: VersionInfo,
    pub file_content: String,
}

impl Orchestrator {
    pub fn new(config: Config, dry_run: bool) -> Result<Self> {
        let token = config
            .gitlab
            .token
            .as_deref()
            .unwrap_or("");
        let gitlab = GitLabClient::new(&config.gitlab.url, token)?;

        let docker_registry = DockerRegistryClient::new(config.registries.clone());
        let helm_registry = HelmRegistryClient::new(config.registries.clone());

        let mut managers: Vec<Box<dyn PackageManager>> = Vec::new();
        for mgr_name in &config.managers.enabled {
            match mgr_name.as_str() {
                "docker" => managers.push(Box::new(crate::manager::docker::DockerManager::new())),
                "helm" => managers.push(Box::new(crate::manager::helm::HelmManager::new())),
                other => warn!("Unknown manager: {}", other),
            }
        }

        let strategy = PinStrategy::from_str(&config.versioning.pin_strategy);
        let version_policy = VersionPolicy::new(strategy);

        Ok(Self {
            config,
            gitlab,
            docker_registry,
            helm_registry,
            managers,
            version_policy,
            dry_run,
        })
    }

    pub async fn run(&self) -> Result<()> {
        if self.config.scan.projects.is_empty() {
            warn!("No projects configured to scan");
            return Ok(());
        }

        for project in &self.config.scan.projects {
            info!("Scanning project: {}", project);
            if let Err(e) = self.process_project(project).await {
                error!("Error processing project {}: {}", project, e);
            }
        }

        Ok(())
    }

    async fn process_project(&self, project: &str) -> Result<()> {
        let default_branch = self.gitlab.get_default_branch(project).await?;
        info!("Default branch: {}", default_branch);

        let tree = self
            .gitlab
            .list_tree(project, &default_branch, None, true)
            .await?;

        let file_paths: Vec<String> = tree
            .iter()
            .filter(|entry| entry.entry_type == "blob")
            .filter(|entry| self.matches_any_pattern(&entry.path))
            .map(|entry| entry.path.clone())
            .collect();

        info!("Found {} matching files", file_paths.len());

        // Extract dependencies from all matching files
        let mut all_deps: Vec<(Dependency, String)> = Vec::new();

        for file_path in &file_paths {
            debug!("Fetching file: {}", file_path);
            let contents = match self.gitlab.get_file(project, file_path, &default_branch).await {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to fetch {}: {}", file_path, e);
                    continue;
                }
            };

            for manager in &self.managers {
                if self.file_matches_manager(file_path, manager.as_ref()) {
                    match manager.extract_dependencies(file_path, &contents) {
                        Ok(deps) => {
                            debug!(
                                "Found {} dependencies in {} ({})",
                                deps.len(),
                                file_path,
                                manager.name()
                            );
                            for dep in deps {
                                all_deps.push((dep, contents.clone()));
                            }
                        }
                        Err(e) => {
                            warn!(
                                "Failed to extract dependencies from {} ({}): {}",
                                file_path,
                                manager.name(),
                                e
                            );
                        }
                    }
                }
            }
        }

        info!("Extracted {} total dependencies", all_deps.len());

        // Fetch available versions concurrently
        let candidates: Vec<UpdateCandidate> = stream::iter(all_deps)
            .map(|(dep, content)| async move {
                self.check_for_update(dep, content).await
            })
            .buffer_unordered(CONCURRENCY_LIMIT)
            .filter_map(|result| async move {
                match result {
                    Ok(Some(candidate)) => Some(candidate),
                    Ok(None) => None,
                    Err(e) => {
                        warn!("Error checking for update: {}", e);
                        None
                    }
                }
            })
            .collect()
            .await;

        info!("Found {} available updates", candidates.len());

        if self.dry_run {
            self.print_dry_run_report(&candidates);
            return Ok(());
        }

        // Check existing MRs to avoid duplicates
        let existing_mrs = self
            .gitlab
            .list_open_mrs(project, Some(&self.config.merge_request.branch_prefix))
            .await?;
        let existing_branches: HashSet<String> = existing_mrs
            .iter()
            .map(|mr| mr.source_branch.clone())
            .collect();

        for candidate in &candidates {
            let branch_name = self.branch_name_for(&candidate.dependency, &candidate.new_version);

            if existing_branches.contains(&branch_name) {
                info!(
                    "MR already exists for {} -> {} (branch: {})",
                    candidate.dependency.name,
                    candidate.new_version.original_tag,
                    branch_name
                );
                continue;
            }

            if let Err(e) = self
                .create_update_mr(project, &default_branch, candidate, &branch_name)
                .await
            {
                error!(
                    "Failed to create MR for {} -> {}: {}",
                    candidate.dependency.name, candidate.new_version.original_tag, e
                );
            }
        }

        Ok(())
    }

    async fn check_for_update(
        &self,
        dep: Dependency,
        file_content: String,
    ) -> Result<Option<UpdateCandidate>> {
        let current = match parse_version_lenient(&dep.current_version) {
            Some(v) => v,
            None => {
                debug!(
                    "Cannot parse version '{}' for {}, skipping",
                    dep.current_version, dep.name
                );
                return Ok(None);
            }
        };

        let versions = match &dep.registry {
            RegistrySource::DockerRegistry { .. } => {
                self.docker_registry.fetch_versions(&dep.registry).await?
            }
            RegistrySource::HelmRepository { .. } => {
                self.helm_registry.fetch_versions(&dep.registry).await?
            }
            RegistrySource::OciHelmRegistry { .. } => {
                self.helm_registry.fetch_versions(&dep.registry).await?
            }
        };

        match self.version_policy.best_update(&current, &versions) {
            Some(new_version) => {
                info!(
                    "Update available: {} {} -> {}",
                    dep.name, dep.current_version, new_version.original_tag
                );
                Ok(Some(UpdateCandidate {
                    dependency: dep,
                    new_version,
                    file_content,
                }))
            }
            None => {
                debug!("{} is up to date at {}", dep.name, dep.current_version);
                Ok(None)
            }
        }
    }

    async fn create_update_mr(
        &self,
        project: &str,
        default_branch: &str,
        candidate: &UpdateCandidate,
        branch_name: &str,
    ) -> Result<()> {
        let file_update = updater::apply_update(
            &candidate.dependency,
            &candidate.new_version.original_tag,
            &candidate.file_content,
        )?;

        // Create branch
        self.gitlab
            .create_branch(project, branch_name, default_branch)
            .await?;

        // Commit updated file
        let commit_msg = format!(
            "chore(deps): update {} from {} to {}",
            candidate.dependency.name,
            candidate.dependency.current_version,
            candidate.new_version.original_tag,
        );

        self.gitlab
            .commit_files(
                project,
                branch_name,
                &commit_msg,
                vec![CommitAction {
                    action: "update".to_string(),
                    file_path: file_update.file_path.clone(),
                    content: file_update.updated_content,
                }],
            )
            .await?;

        // Determine manager name for MR body
        let manager_name = match &candidate.dependency.registry {
            RegistrySource::DockerRegistry { .. } => "docker",
            RegistrySource::HelmRepository { .. } => "helm",
            RegistrySource::OciHelmRegistry { .. } => "helm",
        };

        let mr_body = format!(
            "## Dependency Update\n\n\
             | Package | Manager | Current | New |\n\
             |---------|---------|---------|-----|\n\
             | {} | {} | {} | {} |\n\n\
             ---\n\n\
             *This MR was automatically created by reforge.*",
            candidate.dependency.name,
            manager_name,
            candidate.dependency.current_version,
            candidate.new_version.original_tag,
        );

        let mr_title = format!(
            "chore(deps): update {} to {}",
            candidate.dependency.name, candidate.new_version.original_tag,
        );

        let mr = self
            .gitlab
            .create_mr(
                project,
                CreateMrParams {
                    source_branch: branch_name.to_string(),
                    target_branch: default_branch.to_string(),
                    title: mr_title,
                    description: mr_body,
                    labels: self.config.merge_request.labels.clone(),
                    assignee_ids: self.config.merge_request.assignees.clone(),
                    merge_when_pipeline_succeeds: if self.config.merge_request.auto_merge {
                        Some(true)
                    } else {
                        None
                    },
                },
            )
            .await?;

        info!("Created MR !{}: {}", mr.iid, mr.web_url);
        Ok(())
    }

    fn branch_name_for(&self, dep: &Dependency, new_version: &VersionInfo) -> String {
        let manager_name = match &dep.registry {
            RegistrySource::DockerRegistry { .. } => "docker",
            RegistrySource::HelmRepository { .. } => "helm",
            RegistrySource::OciHelmRegistry { .. } => "helm",
        };
        let sanitized_name = dep.name.replace('/', "-");
        format!(
            "{}{}-{}-{}",
            self.config.merge_request.branch_prefix,
            manager_name,
            sanitized_name,
            new_version.original_tag,
        )
    }

    fn matches_any_pattern(&self, path: &str) -> bool {
        self.managers
            .iter()
            .any(|m| self.file_matches_manager(path, m.as_ref()))
    }

    fn file_matches_manager(&self, path: &str, manager: &dyn PackageManager) -> bool {
        let filename = path.rsplit('/').next().unwrap_or(path);
        manager.file_patterns().iter().any(|pattern| {
            if pattern.contains('*') {
                // Simple glob: "Dockerfile.*" or "values-*.yaml"
                let parts: Vec<&str> = pattern.split('*').collect();
                if parts.len() == 2 {
                    filename.starts_with(parts[0]) && filename.ends_with(parts[1])
                } else {
                    filename == *pattern
                }
            } else {
                filename == *pattern
            }
        })
    }

    fn print_dry_run_report(&self, candidates: &[UpdateCandidate]) {
        if candidates.is_empty() {
            println!("\nNo updates available.");
            return;
        }

        println!("\n{}", "=".repeat(72));
        println!("  Dependency Update Report (dry-run)");
        println!("{}", "=".repeat(72));
        println!(
            "\n{:<30} {:<10} {:<15} {:<15}",
            "Package", "Manager", "Current", "Available"
        );
        println!("{}", "-".repeat(72));

        for candidate in candidates {
            let manager = match &candidate.dependency.registry {
                RegistrySource::DockerRegistry { .. } => "docker",
                RegistrySource::HelmRepository { .. } => "helm",
                RegistrySource::OciHelmRegistry { .. } => "helm",
            };
            println!(
                "{:<30} {:<10} {:<15} {:<15}",
                candidate.dependency.name,
                manager,
                candidate.dependency.current_version,
                candidate.new_version.original_tag,
            );
        }

        println!("{}", "-".repeat(72));
        println!("Total: {} update(s) available\n", candidates.len());
    }
}
