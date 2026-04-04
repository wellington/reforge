use chrono::Utc;
use futures::stream::{self, StreamExt};
use std::collections::HashSet;
use tracing::{debug, error, info, warn};

use crate::automerge::{AutomergeEvaluator, UpdateType};
use crate::config::Config;
use crate::dashboard;
use crate::error::Result;
use crate::manager::{Dependency, PackageManager, RegistrySource};
use crate::platform::gitlab::{CreateMrParams, GitLabClient};
use crate::platform::{FileSource, GitLabSource, LocalGitSource};
use crate::registry::docker::DockerRegistryClient;
use crate::registry::helm::HelmRegistryClient;
use crate::registry::{parse_version_lenient, RegistryClient, VersionInfo};
use crate::scheduling::{is_within_schedule_window, RateLimiter};
use crate::updater;
use crate::versioning::{PinStrategy, VersionPolicy};

const CONCURRENCY_LIMIT: usize = 5;

pub struct Orchestrator {
    config: Config,
    /// Only used in GitLab mode for MR creation.
    gitlab: Option<GitLabClient>,
    docker_registry: DockerRegistryClient,
    helm_registry: HelmRegistryClient,
    managers: Vec<Box<dyn PackageManager>>,
    version_policy: VersionPolicy,
    dry_run: bool,
    dashboard_enabled: bool,
}

#[derive(Debug)]
pub struct UpdateCandidate {
    pub dependency: Dependency,
    pub new_version: VersionInfo,
    pub file_content: String,
}

impl Orchestrator {
    pub fn new(config: Config, dry_run: bool, dashboard_enabled: bool) -> Result<Self> {
        // Only construct a GitLab client when not in local mode.
        let gitlab = if config.local_path.is_none() {
            let token = config.gitlab.token.as_deref().unwrap_or("");
            Some(GitLabClient::new(&config.gitlab.url, token)?)
        } else {
            None
        };

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
            dashboard_enabled,
        })
    }

    pub async fn run(&self) -> Result<()> {
        if let Some(local_path) = &self.config.local_path {
            info!("Running in local mode against {:?}", local_path);
            let source = LocalGitSource::new(local_path.clone());
            source.repo.validate().await?;
            return self.process_with_source(&source, local_path.display().to_string().as_str()).await;
        }

        if self.config.scan.projects.is_empty() {
            warn!("No projects configured to scan");
            return Ok(());
        }

        let gitlab = self.gitlab.as_ref().expect("GitLab client required in API mode");

        for project in &self.config.scan.projects {
            info!("Scanning project: {}", project);
            let source = GitLabSource {
                client: GitLabClient::new(&self.config.gitlab.url, self.config.gitlab.token.as_deref().unwrap_or(""))?,
                project: project.clone(),
            };
            if let Err(e) = self.process_with_source(&source, project).await {
                error!("Error processing project {}: {}", project, e);
            }
            let _ = gitlab; // ensure borrow extends to here
        }

        Ok(())
    }

    async fn process_with_source(&self, source: &dyn FileSource, label: &str) -> Result<()> {
        let default_branch = source.default_branch().await?;
        info!("Default branch: {}", default_branch);

        let entries = source.list_files(&default_branch).await?;
        let file_paths: Vec<String> = entries
            .into_iter()
            .filter(|e| self.matches_any_pattern(&e.path))
            .map(|e| e.path)
            .collect();

        info!("Found {} matching files", file_paths.len());

        let mut all_deps: Vec<(Dependency, String)> = Vec::new();

        for file_path in &file_paths {
            debug!("Fetching file: {}", file_path);
            let contents = match source.get_file(file_path, &default_branch).await {
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

        let candidates: Vec<UpdateCandidate> = stream::iter(all_deps.clone())
            .map(|(dep, content)| async move { self.check_for_update(dep, content).await })
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

        // In local mode we commit directly; in GitLab mode we create MRs.
        if self.config.local_path.is_some() {
            self.apply_local_updates(source, &default_branch, &candidates)
                .await?;

            if self.dashboard_enabled && self.config.dashboard.enabled {
                let statuses = dashboard::build_statuses(&all_deps, &candidates, &[], &self.config.merge_request.branch_prefix);
                let body = dashboard::render_dashboard(&statuses, label);
                let path = &self.config.dashboard.local_path;
                if let Err(e) = dashboard::write_local_dashboard(&body, path) {
                    error!("Failed to write local dashboard: {}", e);
                } else {
                    info!("Dashboard written to {}", path);
                }
            }
        } else {
            self.create_gitlab_mrs(source, label, &default_branch, &candidates)
                .await?;

            if self.dashboard_enabled && self.config.dashboard.enabled {
                if let Some(gitlab) = &self.gitlab {
                    let open_mrs = gitlab
                        .list_open_mrs(label, Some(&self.config.merge_request.branch_prefix))
                        .await
                        .unwrap_or_else(|e| {
                            warn!("Failed to fetch open MRs for dashboard: {}", e);
                            vec![]
                        });
                    let statuses = dashboard::build_statuses(&all_deps, &candidates, &open_mrs, &self.config.merge_request.branch_prefix);
                    let body = dashboard::render_dashboard(&statuses, label);
                    match dashboard::upsert_gitlab_dashboard(
                        gitlab,
                        label,
                        &body,
                        &self.config.dashboard.labels,
                    )
                    .await
                    {
                        Ok(issue) => info!("Dashboard issue updated: {}", issue.web_url),
                        Err(e) => error!("Failed to upsert dashboard issue: {}", e),
                    }
                }
            }
        }

        Ok(())
    }

    async fn apply_local_updates(
        &self,
        source: &dyn FileSource,
        default_branch: &str,
        candidates: &[UpdateCandidate],
    ) -> Result<()> {
        for candidate in candidates {
            let branch_name =
                self.branch_name_for(&candidate.dependency, &candidate.new_version);

            let already_exists = source.branch_exists(&branch_name).await?;
            if already_exists {
                info!(
                    "Branch already exists for {} -> {} (branch: {}), skipping",
                    candidate.dependency.name,
                    candidate.new_version.original_tag,
                    branch_name
                );
                continue;
            }

            let file_update = match updater::apply_update(
                &candidate.dependency,
                &candidate.new_version.original_tag,
                &candidate.file_content,
            ) {
                Ok(u) => u,
                Err(e) => {
                    error!(
                        "Failed to apply update for {}: {}",
                        candidate.dependency.name, e
                    );
                    continue;
                }
            };

            if let Err(e) = source.create_branch(&branch_name, default_branch).await {
                error!("Failed to create branch {}: {}", branch_name, e);
                continue;
            }

            let commit_msg = format!(
                "chore(deps): update {} from {} to {}",
                candidate.dependency.name,
                candidate.dependency.current_version,
                candidate.new_version.original_tag,
            );

            match source
                .commit_file(
                    &branch_name,
                    &file_update.file_path,
                    &file_update.updated_content,
                    &commit_msg,
                )
                .await
            {
                Ok(commit) => {
                    info!(
                        "Committed update for {} on branch {} ({})",
                        candidate.dependency.name, branch_name, commit
                    );

                    let update_type = UpdateType::classify(
                        &candidate.dependency.current_version,
                        &candidate.new_version.original_tag,
                    );
                    let evaluator =
                        AutomergeEvaluator::new(&self.config.merge_request.automerge_policies);
                    let policy_automerge = update_type.as_ref().map_or(false, |ut| {
                        evaluator.should_automerge(&candidate.dependency.name, ut, None)
                    });

                    if self.config.merge_request.auto_merge || policy_automerge {
                        info!(
                            "Automerge would be applied for {} ({:?}) [local mode]",
                            candidate.dependency.name, update_type,
                        );
                    }
                }
                Err(e) => error!(
                    "Failed to commit update for {}: {}",
                    candidate.dependency.name, e
                ),
            }
        }
        Ok(())
    }

    async fn create_gitlab_mrs(
        &self,
        source: &dyn FileSource,
        project: &str,
        default_branch: &str,
        candidates: &[UpdateCandidate],
    ) -> Result<()> {
        let gitlab = self.gitlab.as_ref().expect("GitLab client required");

        // Check schedule window before attempting to create any MRs.
        if let Some(window) = &self.config.merge_request.schedule_window {
            let now = Utc::now();
            if !is_within_schedule_window(now, window) {
                info!(
                    "Outside schedule window — skipping MR creation for project {}",
                    project
                );
                return Ok(());
            }
        }

        let existing_mrs = gitlab
            .list_open_mrs(project, Some(&self.config.merge_request.branch_prefix))
            .await?;
        let existing_branches: HashSet<String> = existing_mrs
            .iter()
            .map(|mr| mr.source_branch.clone())
            .collect();

        // Sort candidates by priority (highest first) so that when we're at the
        // rate limit we favour more impactful updates.
        let mut sorted_candidates: Vec<&UpdateCandidate> = candidates.iter().collect();
        sorted_candidates.sort_by_key(|c| {
            let ut = UpdateType::classify(
                &c.dependency.current_version,
                &c.new_version.original_tag,
            );
            crate::scheduling::PriorityOrder::from_update_type_pub(ut.as_ref())
        });

        // Rate limiter counts only the MRs that already exist; new ones we
        // create this run are tracked via `created_this_run`.
        let rate_limiter = RateLimiter::new(
            self.config.merge_request.max_open_mrs,
            existing_mrs.len(),
        );

        if !rate_limiter.can_create_mr() {
            info!(
                "Open MR limit reached ({} open, max {:?}) — skipping MR creation for project {}",
                existing_mrs.len(),
                self.config.merge_request.max_open_mrs,
                project,
            );
            return Ok(());
        }

        let mut created_count: usize = 0;

        for candidate in sorted_candidates {
            let slots = rate_limiter.remaining_slots().saturating_sub(created_count);
            if slots == 0 {
                info!(
                    "Open MR limit reached — skipping remaining candidates for project {}",
                    project
                );
                break;
            }

            let branch_name =
                self.branch_name_for(&candidate.dependency, &candidate.new_version);

            if existing_branches.contains(&branch_name) {
                info!(
                    "MR already exists for {} -> {} (branch: {})",
                    candidate.dependency.name,
                    candidate.new_version.original_tag,
                    branch_name
                );
                continue;
            }

            match self
                .create_update_mr(source, gitlab, project, default_branch, candidate, &branch_name)
                .await
            {
                Ok(()) => created_count += 1,
                Err(e) => error!(
                    "Failed to create MR for {} -> {}: {}",
                    candidate.dependency.name, candidate.new_version.original_tag, e
                ),
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
        source: &dyn FileSource,
        gitlab: &GitLabClient,
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

        source.create_branch(branch_name, default_branch).await?;

        let commit_msg = format!(
            "chore(deps): update {} from {} to {}",
            candidate.dependency.name,
            candidate.dependency.current_version,
            candidate.new_version.original_tag,
        );

        source
            .commit_file(
                branch_name,
                &file_update.file_path,
                &file_update.updated_content,
                &commit_msg,
            )
            .await?;

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

        let update_type = UpdateType::classify(
            &candidate.dependency.current_version,
            &candidate.new_version.original_tag,
        );

        let evaluator = AutomergeEvaluator::new(&self.config.merge_request.automerge_policies);

        // Determine automerge intent before creating the MR so we can set
        // merge_when_pipeline_succeeds at creation time when possible.
        let policy_automerge = update_type.as_ref().map_or(false, |ut| {
            evaluator.should_automerge(&candidate.dependency.name, ut, None)
        });

        let use_automerge = self.config.merge_request.auto_merge || policy_automerge;

        let mr = gitlab
            .create_mr(
                project,
                CreateMrParams {
                    source_branch: branch_name.to_string(),
                    target_branch: default_branch.to_string(),
                    title: mr_title,
                    description: mr_body,
                    labels: self.config.merge_request.labels.clone(),
                    assignee_ids: self.config.merge_request.assignees.clone(),
                    merge_when_pipeline_succeeds: if use_automerge { Some(true) } else { None },
                },
            )
            .await?;

        info!("Created MR !{}: {}", mr.iid, mr.web_url);

        if policy_automerge {
            info!(
                "Automerge policy matched for {} ({:?}): merge_when_pipeline_succeeds enabled on MR !{}",
                candidate.dependency.name,
                update_type,
                mr.iid,
            );
        }

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
