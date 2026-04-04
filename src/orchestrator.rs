use chrono::Utc;
use futures::stream::{self, StreamExt};
use std::collections::{HashMap, HashSet};
use tracing::{debug, error, info, warn};

use crate::automerge::{AutomergeEvaluator, UpdateType};
use crate::changelog::{self, ChangelogFetcher};
use crate::config::Config;
use crate::dashboard;
use crate::error::Result;
use crate::grouping::{group_candidates, Group};
use crate::lockfile;
use crate::manager::{Dependency, PackageManager, RegistrySource};
use crate::platform::gitlab::{CreateMrParams, GitLabClient};
use crate::platform::{FileSource, GitLabSource, LocalGitSource};
use crate::rebase::{handle_stale_mrs, StalenessChecker};
use crate::registry::docker::DockerRegistryClient;
use crate::registry::helm::HelmRegistryClient;
use crate::registry::{parse_version_lenient, RegistryClient, VersionInfo};
use crate::replacement::{
    check_dependencies, render_replacement_mr_body, ReplacementAction, ReplacementDatabase,
};
use crate::scheduling::{is_within_schedule_window, RateLimiter};
use crate::updater;
use crate::versioning::{PinStrategy, VersionPolicy};
use crate::vulnerability::{
    ecosystem_for_manager, render_vulnerability_section, VulnerabilityChecker, VulnerabilityInfo,
};

const CONCURRENCY_LIMIT: usize = 5;

pub struct Orchestrator {
    config: Config,
    /// Only used in GitLab mode for MR creation.
    gitlab: Option<GitLabClient>,
    docker_registry: DockerRegistryClient,
    helm_registry: HelmRegistryClient,
    http_client: reqwest::Client,
    managers: Vec<Box<dyn PackageManager>>,
    /// Dynamic file-pattern sets for regex managers (parallel to the regex manager
    /// entries appended to `managers`). Each entry is the list of glob patterns for
    /// one regex manager.
    regex_manager_patterns: Vec<Vec<String>>,
    version_policy: VersionPolicy,
    dry_run: bool,
    dashboard_enabled: bool,
    changelog_fetcher: ChangelogFetcher,
    vuln_checker: VulnerabilityChecker,
    replacement_db: ReplacementDatabase,
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
            Some(GitLabClient::with_options(&config.gitlab.url, token, config.gitlab.insecure)?)
        } else {
            None
        };

        let docker_registry = DockerRegistryClient::new(config.registries.clone());
        let helm_registry = HelmRegistryClient::new(config.registries.clone());
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("Failed to build HTTP client");

        let mut managers: Vec<Box<dyn PackageManager>> = Vec::new();
        for mgr_name in &config.managers.enabled {
            match mgr_name.as_str() {
                "docker" => managers.push(Box::new(crate::manager::docker::DockerManager::new())),
                "helm" => managers.push(Box::new(crate::manager::helm::HelmManager::new())),
                other => warn!("Unknown manager: {}", other),
            }
        }

        let mut regex_manager_patterns: Vec<Vec<String>> = Vec::new();
        for rm_config in &config.regex_managers {
            match crate::manager::regex::RegexManager::new(rm_config.clone()) {
                Ok(mgr) => {
                    info!("Loaded regex manager '{}'", rm_config.name);
                    regex_manager_patterns.push(rm_config.file_patterns.clone());
                    managers.push(Box::new(mgr));
                }
                Err(e) => {
                    warn!("Skipping regex manager '{}': {}", rm_config.name, e);
                }
            }
        }

        let strategy = PinStrategy::from_str(&config.versioning.pin_strategy);
        let version_policy = VersionPolicy::new(strategy);

        let changelog_fetcher =
            ChangelogFetcher::new(config.changelog.github_token.clone());

        let vuln_checker = VulnerabilityChecker::new();

        // Build the replacement database: start with built-ins, then layer any
        // user-supplied rules file.
        let mut replacement_db = ReplacementDatabase::load_builtin();
        if config.replacement.enabled {
            if let Some(rules_path) = &config.replacement.rules_file {
                match ReplacementDatabase::load_from_toml(rules_path) {
                    Ok(extra) => {
                        replacement_db.rules.extend(extra.rules);
                        info!("Loaded {} extra replacement rule(s) from {}", replacement_db.rules.len(), rules_path);
                    }
                    Err(e) => warn!("Failed to load replacement rules from {}: {}", rules_path, e),
                }
            }
        }

        Ok(Self {
            config,
            gitlab,
            docker_registry,
            helm_registry,
            http_client,
            managers,
            regex_manager_patterns,
            version_policy,
            dry_run,
            dashboard_enabled,
            changelog_fetcher,
            vuln_checker,
            replacement_db,
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
                client: GitLabClient::with_options(&self.config.gitlab.url, self.config.gitlab.token.as_deref().unwrap_or(""), self.config.gitlab.insecure)?,
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

        // Check for deprecated / renamed images before doing version checks.
        if self.config.replacement.enabled {
            let dep_list: Vec<Dependency> = all_deps.iter().map(|(d, _)| d.clone()).collect();
            let replacement_actions = check_dependencies(&dep_list, &self.replacement_db);
            if !replacement_actions.is_empty() {
                self.handle_replacement_actions(source, label, &all_deps, &replacement_actions)
                    .await;
            }
        }

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

        // Partition candidates into groups according to the configured rules.
        let groups = group_candidates(
            candidates,
            &self.config.merge_request.grouping_rules,
            &self.config.merge_request.grouping,
        );

        // Reconstruct a flat list for dashboard/dry-run reporting.
        let flat_candidates: Vec<UpdateCandidate> = groups
            .iter()
            .flat_map(|g| g.candidates.iter().map(|c| UpdateCandidate {
                dependency: c.dependency.clone(),
                new_version: c.new_version.clone(),
                file_content: c.file_content.clone(),
            }))
            .collect();

        // In local mode we commit directly; in GitLab mode we create MRs.
        if self.config.local_path.is_some() {
            self.apply_local_updates(source, &default_branch, &groups)
                .await?;

            if self.dashboard_enabled && self.config.dashboard.enabled {
                let statuses = dashboard::build_statuses(&all_deps, &flat_candidates, &[], &self.config.merge_request.branch_prefix);
                let body = dashboard::render_dashboard(&statuses, label);
                let path = &self.config.dashboard.local_path;
                if let Err(e) = dashboard::write_local_dashboard(&body, path) {
                    error!("Failed to write local dashboard: {}", e);
                } else {
                    info!("Dashboard written to {}", path);
                }
            }
        } else {
            self.create_gitlab_mrs(source, label, &default_branch, &groups)
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
                    let statuses = dashboard::build_statuses(&all_deps, &flat_candidates, &open_mrs, &self.config.merge_request.branch_prefix);
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
        groups: &[Group],
    ) -> Result<()> {
        // Before processing new updates, rebase any existing reforge branches
        // in local mode (if the strategy calls for it).
        if self.config.merge_request.rebase_enabled {
            if let Some(local_path) = &self.config.local_path {
                use crate::platform::git::GitRepo;
                let repo = GitRepo::new(local_path.clone());
                self.rebase_local_branches(&repo, default_branch).await;
            }
        }

        for group in groups {
            if group.candidates.is_empty() {
                continue;
            }

            let is_grouped = group.candidates.len() > 1;
            let branch_name = if is_grouped {
                self.branch_name_for_group(&group.name)
            } else {
                self.branch_name_for(
                    &group.candidates[0].dependency,
                    &group.candidates[0].new_version,
                )
            };

            let already_exists = source.branch_exists(&branch_name).await?;
            if already_exists {
                info!("Branch already exists (branch: {}), skipping", branch_name);
                continue;
            }

            if let Err(e) = source.create_branch(&branch_name, default_branch).await {
                error!("Failed to create branch {}: {}", branch_name, e);
                continue;
            }

            // Group candidates by file so we can apply all updates for a given
            // file in a single commit.
            let mut by_file: HashMap<String, Vec<&UpdateCandidate>> = HashMap::new();
            for candidate in &group.candidates {
                by_file
                    .entry(candidate.dependency.file_path.clone())
                    .or_default()
                    .push(candidate);
            }

            let mut committed_names: Vec<String> = Vec::new();

            for (file_path, file_candidates) in &by_file {
                let original_content = &file_candidates[0].file_content;
                let updates: Vec<(&crate::manager::Dependency, &str)> = file_candidates
                    .iter()
                    .map(|c| (&c.dependency, c.new_version.original_tag.as_str()))
                    .collect();

                let (file_update, errors) =
                    updater::apply_updates(updates, original_content, file_path);

                for e in &errors {
                    error!("Failed to apply update in group '{}': {}", group.name, e);
                }

                let commit_msg = if file_candidates.len() == 1 {
                    let c = file_candidates[0];
                    format!(
                        "chore(deps): update {} from {} to {}",
                        c.dependency.name,
                        c.dependency.current_version,
                        c.new_version.original_tag,
                    )
                } else {
                    format!("chore(deps): grouped update for '{}'", group.name)
                };

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
                            "Committed {} update(s) on branch {} ({})",
                            file_candidates.len(),
                            branch_name,
                            commit
                        );
                        for c in file_candidates {
                            committed_names.push(c.dependency.name.clone());
                        }
                    }
                    Err(e) => error!("Failed to commit to {}: {}", branch_name, e),
                }

                // If this is a Chart.yaml file and lockfile maintenance is enabled,
                // update the sibling Chart.lock when one exists.
                if self.config.lockfile.enabled
                    && file_path.ends_with("Chart.yaml")
                {
                    let lock_path = chart_lock_path(file_path);
                    if let Ok(lock_content) = source.get_file(&lock_path, &branch_name).await {
                        let mut updated_lock = lock_content.clone();
                        for c in file_candidates {
                            let dep_name = &c.dependency.name;
                            let new_ver = c.new_version.original_tag.as_str();
                            let new_digest = self
                                .fetch_dep_digest(&c.dependency.registry, new_ver)
                                .await;
                            updated_lock = lockfile::update_chart_lock(
                                &updated_lock,
                                dep_name,
                                new_ver,
                                &new_digest,
                            );
                        }
                        let lock_commit_msg = format!(
                            "chore(deps): update Chart.lock for {}",
                            file_candidates
                                .iter()
                                .map(|c| c.dependency.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                        match source
                            .commit_file(&branch_name, &lock_path, &updated_lock, &lock_commit_msg)
                            .await
                        {
                            Ok(_) => info!("Updated Chart.lock at {}", lock_path),
                            Err(e) => warn!("Failed to update Chart.lock at {}: {}", lock_path, e),
                        }
                    }
                }
            }

            // Automerge hint for single-dependency groups.
            if group.candidates.len() == 1 {
                let candidate = &group.candidates[0];
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
        }
        Ok(())
    }

    async fn create_gitlab_mrs(
        &self,
        source: &dyn FileSource,
        project: &str,
        default_branch: &str,
        groups: &[Group],
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

        for group in groups {
            if group.candidates.is_empty() {
                continue;
            }

            let slots = rate_limiter.remaining_slots().saturating_sub(created_count);
            if slots == 0 {
                info!(
                    "Open MR limit reached — skipping remaining groups for project {}",
                    project
                );
                break;
            }

            let is_grouped = group.candidates.len() > 1;
            let branch_name = if is_grouped {
                self.branch_name_for_group(&group.name)
            } else {
                self.branch_name_for(
                    &group.candidates[0].dependency,
                    &group.candidates[0].new_version,
                )
            };

            if existing_branches.contains(&branch_name) {
                info!("MR already exists for group '{}' (branch: {})", group.name, branch_name);
                continue;
            }

            match self
                .create_group_mr(source, gitlab, project, default_branch, group, &branch_name)
                .await
            {
                Ok(()) => created_count += 1,
                Err(e) => error!("Failed to create MR for group '{}': {}", group.name, e),
            }
        }

        // After creating new MRs, check existing ones for staleness.
        if self.config.merge_request.rebase_enabled {
            let checker = StalenessChecker::new();
            let stale_mrs = checker
                .check_stale_mrs(gitlab, project, &self.config.merge_request.branch_prefix)
                .await;

            if !stale_mrs.is_empty() {
                info!(
                    "Found {} stale MR(s) for project {} — applying strategy '{:?}'",
                    stale_mrs.len(),
                    project,
                    self.config.merge_request.stale_mr_strategy,
                );
                handle_stale_mrs(
                    gitlab,
                    project,
                    &stale_mrs,
                    &self.config.merge_request.stale_mr_strategy,
                )
                .await;
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

    async fn create_group_mr(
        &self,
        source: &dyn FileSource,
        gitlab: &GitLabClient,
        project: &str,
        default_branch: &str,
        group: &Group,
        branch_name: &str,
    ) -> Result<()> {
        source.create_branch(branch_name, default_branch).await?;

        // Group candidates by file for multi-file support.
        let mut by_file: HashMap<String, Vec<&UpdateCandidate>> = HashMap::new();
        for candidate in &group.candidates {
            by_file
                .entry(candidate.dependency.file_path.clone())
                .or_default()
                .push(candidate);
        }

        for (file_path, file_candidates) in &by_file {
            let original_content = &file_candidates[0].file_content;
            let updates: Vec<(&crate::manager::Dependency, &str)> = file_candidates
                .iter()
                .map(|c| (&c.dependency, c.new_version.original_tag.as_str()))
                .collect();

            let (file_update, errors) =
                updater::apply_updates(updates, original_content, file_path);

            for e in &errors {
                error!("Failed to apply update in group '{}': {}", group.name, e);
            }

            let commit_msg = if file_candidates.len() == 1 {
                let c = file_candidates[0];
                format!(
                    "chore(deps): update {} from {} to {}",
                    c.dependency.name,
                    c.dependency.current_version,
                    c.new_version.original_tag,
                )
            } else {
                format!("chore(deps): grouped update for '{}'", group.name)
            };

            source
                .commit_file(
                    branch_name,
                    &file_update.file_path,
                    &file_update.updated_content,
                    &commit_msg,
                )
                .await?;

            // Update sibling Chart.lock when lockfile maintenance is enabled.
            if self.config.lockfile.enabled && file_path.ends_with("Chart.yaml") {
                let lock_path = chart_lock_path(file_path);
                if let Ok(lock_content) = source.get_file(&lock_path, branch_name).await {
                    let mut updated_lock = lock_content.clone();
                    for c in file_candidates {
                        let dep_name = &c.dependency.name;
                        let new_ver = c.new_version.original_tag.as_str();
                        let new_digest =
                            self.fetch_dep_digest(&c.dependency.registry, new_ver).await;
                        updated_lock = lockfile::update_chart_lock(
                            &updated_lock,
                            dep_name,
                            new_ver,
                            &new_digest,
                        );
                    }
                    let lock_commit_msg = format!(
                        "chore(deps): update Chart.lock for {}",
                        file_candidates
                            .iter()
                            .map(|c| c.dependency.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    match source
                        .commit_file(branch_name, &lock_path, &updated_lock, &lock_commit_msg)
                        .await
                    {
                        Ok(_) => info!("Updated Chart.lock at {}", lock_path),
                        Err(e) => warn!("Failed to update Chart.lock at {}: {}", lock_path, e),
                    }
                }
            }
        }

        let changelog_notes = self.maybe_fetch_changelog(group).await;
        let vulns = self.maybe_check_vulnerabilities(group).await;
        let (mr_title, mr_body) =
            self.build_group_mr_content(group, changelog_notes.as_deref(), &vulns);

        // Automerge: only enable when the group contains a single dependency.
        let use_automerge = if group.candidates.len() == 1 {
            let candidate = &group.candidates[0];
            let update_type = UpdateType::classify(
                &candidate.dependency.current_version,
                &candidate.new_version.original_tag,
            );
            let evaluator = AutomergeEvaluator::new(&self.config.merge_request.automerge_policies);
            let policy_automerge = update_type.as_ref().map_or(false, |ut| {
                evaluator.should_automerge(&candidate.dependency.name, ut, None)
            });
            self.config.merge_request.auto_merge || policy_automerge
        } else {
            self.config.merge_request.auto_merge
        };

        // Append security labels when the update fixes known vulnerabilities.
        let mut labels = self.config.merge_request.labels.clone();
        if !vulns.is_empty() {
            for lbl in &self.config.vulnerability.security_labels {
                if !labels.contains(lbl) {
                    labels.push(lbl.clone());
                }
            }
        }

        let mr = gitlab
            .create_mr(
                project,
                CreateMrParams {
                    source_branch: branch_name.to_string(),
                    target_branch: default_branch.to_string(),
                    title: mr_title,
                    description: mr_body,
                    labels,
                    assignee_ids: self.config.merge_request.assignees.clone(),
                    merge_when_pipeline_succeeds: if use_automerge { Some(true) } else { None },
                },
            )
            .await?;

        info!("Created MR !{}: {}", mr.iid, mr.web_url);
        Ok(())
    }

    /// Build the MR title and body for a group of update candidates.
    fn build_group_mr_content(
        &self,
        group: &Group,
        changelog_notes: Option<&str>,
        vulns: &[VulnerabilityInfo],
    ) -> (String, String) {
        let title = if group.candidates.len() == 1 {
            let c = &group.candidates[0];
            format!(
                "chore(deps): update {} to {}",
                c.dependency.name, c.new_version.original_tag,
            )
        } else {
            format!("chore(deps): grouped update — {}", group.name)
        };

        let mut rows = String::new();
        for candidate in &group.candidates {
            let manager = match &candidate.dependency.registry {
                RegistrySource::DockerRegistry { .. } => "docker",
                RegistrySource::HelmRepository { .. } => "helm",
                RegistrySource::OciHelmRegistry { .. } => "helm",
            };
            rows.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                candidate.dependency.name,
                manager,
                candidate.dependency.file_path,
                candidate.dependency.current_version,
                candidate.new_version.original_tag,
            ));
        }

        let changelog_section = match changelog_notes {
            Some(notes) if !notes.is_empty() => {
                let truncated =
                    changelog::truncate_changelog(notes, self.config.changelog.max_length);
                format!("\n\n{}", changelog::render_changelog_section(&truncated))
            }
            _ => String::new(),
        };

        let vuln_section = if !vulns.is_empty() {
            format!("\n\n{}", render_vulnerability_section(vulns))
        } else {
            String::new()
        };

        let body = format!(
            "## Dependency Update{}\n\n\
             | Package | Manager | File | Current | New |\n\
             |---------|---------|------|---------|-----|\n\
             {}\n\
             {}{}---\n\n\
             *This MR was automatically created by reforge.*",
            if group.candidates.len() > 1 { "s" } else { "" },
            rows,
            changelog_section,
            vuln_section,
        );

        (title, body)
    }

    /// Attempt to fetch changelog/release notes for a single-dependency group.
    /// Returns `None` for grouped updates or when the changelog feature is disabled.
    async fn maybe_fetch_changelog(&self, group: &Group) -> Option<String> {
        if !self.config.changelog.enabled {
            return None;
        }
        // Only fetch for single-dependency updates.
        if group.candidates.len() != 1 {
            return None;
        }
        let candidate = &group.candidates[0];
        let registry_source_str = match &candidate.dependency.registry {
            RegistrySource::DockerRegistry { image, .. } => image.clone(),
            RegistrySource::HelmRepository { repo_url, chart_name, .. } => {
                format!("{}/{}", repo_url, chart_name)
            }
            RegistrySource::OciHelmRegistry { registry, image } => {
                match registry {
                    Some(r) => format!("{}/{}", r, image),
                    None => image.clone(),
                }
            }
        };
        self.changelog_fetcher
            .fetch_release_notes(
                &candidate.dependency.name,
                Some(&registry_source_str),
                &candidate.dependency.current_version,
                &candidate.new_version.original_tag,
            )
            .await
    }

    /// Query OSV for vulnerabilities affecting the current version of each
    /// dependency in the group. Returns an empty vec when the feature is
    /// disabled or no vulnerabilities are found.
    async fn maybe_check_vulnerabilities(&self, group: &Group) -> Vec<VulnerabilityInfo> {
        if !self.config.vulnerability.enabled {
            return vec![];
        }

        let mut all_vulns: Vec<VulnerabilityInfo> = Vec::new();

        for candidate in &group.candidates {
            let manager_name = match &candidate.dependency.registry {
                RegistrySource::DockerRegistry { .. } => "docker",
                RegistrySource::HelmRepository { .. } => "helm",
                RegistrySource::OciHelmRegistry { .. } => "helm",
            };
            let ecosystem = ecosystem_for_manager(manager_name);
            if ecosystem.is_empty() {
                continue;
            }

            let mut vulns = self
                .vuln_checker
                .check_dependency(
                    &candidate.dependency.name,
                    ecosystem,
                    &candidate.dependency.current_version,
                )
                .await;

            all_vulns.append(&mut vulns);
        }

        all_vulns
    }

    /// In local mode, iterate local branches with the reforge prefix and rebase
    /// any that are behind the default branch or have conflicts.
    async fn rebase_local_branches(
        &self,
        repo: &crate::platform::git::GitRepo,
        default_branch: &str,
    ) {
        use crate::rebase::StaleMrStrategy;

        let strategy = &self.config.merge_request.stale_mr_strategy;
        if *strategy == StaleMrStrategy::Ignore {
            return;
        }

        let prefix = &self.config.merge_request.branch_prefix;

        // Enumerate local branches.
        let branches: Vec<String> = match repo.run(&["branch", "--list", &format!("{}*", prefix)]).await {
            Ok(out) => out
                .lines()
                .map(|l: &str| l.trim().trim_start_matches("* ").to_string())
                .filter(|b: &String| !b.is_empty())
                .collect(),
            Err(e) => {
                warn!("Failed to list local reforge branches: {}", e);
                return;
            }
        };

        for branch in &branches {
            // Check for conflicts by dry-run merging into default_branch.
            let original = match repo.current_branch().await {
                Ok(b) => b,
                Err(_) => continue,
            };

            // Check if branch is behind: compare merge-base with default_branch tip.
            let is_behind = match repo
                .run(&["merge-base", "--is-ancestor", default_branch, branch])
                .await
            {
                // exit 0 means default_branch is an ancestor of branch (branch is up to date)
                Ok(_) => false,
                // exit 1 means not an ancestor (branch is behind)
                Err(_) => true,
            };

            // Check conflicts by checking out default and doing a dry-run merge.
            let _ = repo.checkout(default_branch).await;
            let has_conflicts = match repo.has_conflicts(branch).await {
                Ok(c) => c,
                Err(e) => {
                    warn!("Could not check conflicts for branch '{}': {}", branch, e);
                    let _ = repo.checkout(&original).await;
                    continue;
                }
            };
            let _ = repo.checkout(&original).await;

            if !is_behind && !has_conflicts {
                continue;
            }

            info!(
                "Local branch '{}' is stale (behind={}, conflicts={}) — applying strategy '{:?}'",
                branch, is_behind, has_conflicts, strategy
            );

            match strategy {
                StaleMrStrategy::Rebase => {
                    if let Err(e) = repo.rebase(branch, default_branch).await {
                        warn!("Failed to rebase local branch '{}': {}", branch, e);
                    } else {
                        info!("Rebased local branch '{}' onto '{}'", branch, default_branch);
                    }
                }
                StaleMrStrategy::Recreate => {
                    // For local mode, recreate = delete branch and re-create from default.
                    // The update will be re-applied on the next scan.
                    if let Err(e) = repo
                        .run(&["branch", "-D", branch])
                        .await
                    {
                        warn!("Failed to delete local branch '{}': {}", branch, e);
                    } else {
                        info!("Deleted stale local branch '{}' (will be recreated on next scan)", branch);
                    }
                }
                StaleMrStrategy::Ignore => {}
            }
        }
    }

    /// Handle replacement/deprecation actions found after scanning dependencies.
    ///
    /// * `Replace` actions  → create a separate migration MR (or log in local mode).
    /// * `DeprecationWarning` → log a warning (and note in dry-run output).
    async fn handle_replacement_actions(
        &self,
        source: &dyn FileSource,
        label: &str,
        all_deps: &[(Dependency, String)],
        actions: &[ReplacementAction],
    ) {
        for action in actions {
            match action {
                ReplacementAction::DeprecationWarning { dep_name, file_path, reason } => {
                    warn!(
                        "[replacement] DEPRECATED: {} in {} — {}",
                        dep_name,
                        file_path,
                        reason.as_deref().unwrap_or("no details"),
                    );
                }
                ReplacementAction::Replace { dep_name, file_path, from_ref, to_ref, reason } => {
                    if self.config.replacement.warn_only {
                        warn!(
                            "[replacement] {} in {} should be migrated: {} → {}",
                            dep_name, file_path, from_ref, to_ref,
                        );
                        continue;
                    }

                    info!(
                        "[replacement] Creating migration MR: {} → {} ({})",
                        from_ref, to_ref, file_path,
                    );

                    // Find the file content for this dependency.
                    let file_content = all_deps
                        .iter()
                        .find(|(d, _)| d.name == *dep_name && d.file_path == *file_path)
                        .map(|(_, content)| content.as_str())
                        .unwrap_or("");

                    let branch_name = format!(
                        "{}replace-{}-{}",
                        self.config.merge_request.branch_prefix,
                        dep_name.replace('/', "-"),
                        // short deterministic suffix
                        {
                            use std::collections::hash_map::DefaultHasher;
                            use std::hash::{Hash, Hasher};
                            let mut h = DefaultHasher::new();
                            from_ref.hash(&mut h);
                            to_ref.hash(&mut h);
                            format!("{:08x}", h.finish() as u32)
                        }
                    );

                    // Skip if branch already exists.
                    match source.branch_exists(&branch_name).await {
                        Ok(true) => {
                            info!("[replacement] Branch {} already exists, skipping", branch_name);
                            continue;
                        }
                        Ok(false) => {}
                        Err(e) => {
                            warn!("[replacement] Could not check branch existence: {}", e);
                            continue;
                        }
                    }

                    let default_branch = match source.default_branch().await {
                        Ok(b) => b,
                        Err(e) => {
                            warn!("[replacement] Could not get default branch: {}", e);
                            continue;
                        }
                    };

                    if let Err(e) = source.create_branch(&branch_name, &default_branch).await {
                        warn!("[replacement] Failed to create branch {}: {}", branch_name, e);
                        continue;
                    }

                    let file_update = updater::apply_replacement(
                        file_content,
                        file_path,
                        from_ref,
                        to_ref,
                    );

                    let commit_msg = format!(
                        "chore(deps): migrate {} from {} to {}",
                        dep_name, from_ref, to_ref,
                    );

                    if let Err(e) = source
                        .commit_file(
                            &branch_name,
                            &file_update.file_path,
                            &file_update.updated_content,
                            &commit_msg,
                        )
                        .await
                    {
                        warn!("[replacement] Failed to commit migration for {}: {}", dep_name, e);
                        continue;
                    }

                    let mr_body = render_replacement_mr_body(action);
                    let mr_title = format!(
                        "chore(deps): migrate {} to {}",
                        dep_name, to_ref,
                    );

                    if let Some(gitlab) = &self.gitlab {
                        let mut labels = self.config.merge_request.labels.clone();
                        labels.push("replacement".to_string());

                        match gitlab
                            .create_mr(
                                label,
                                CreateMrParams {
                                    source_branch: branch_name.clone(),
                                    target_branch: default_branch.clone(),
                                    title: mr_title,
                                    description: mr_body,
                                    labels,
                                    assignee_ids: self.config.merge_request.assignees.clone(),
                                    merge_when_pipeline_succeeds: None,
                                },
                            )
                            .await
                        {
                            Ok(mr) => info!("[replacement] Created migration MR !{}: {}", mr.iid, mr.web_url),
                            Err(e) => warn!("[replacement] Failed to create migration MR: {}", e),
                        }
                    } else {
                        // Local mode: the branch and commit are sufficient.
                        info!(
                            "[replacement] Local mode: migration branch '{}' created for {} → {}",
                            branch_name, from_ref, to_ref,
                        );
                    }
                }
            }
        }
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

    fn branch_name_for_group(&self, group_name: &str) -> String {
        let sanitized = group_name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
            .collect::<String>();
        // Use a short hash of the name to guarantee uniqueness even after sanitization.
        let hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            group_name.hash(&mut h);
            format!("{:08x}", h.finish() as u32)
        };
        format!("{}group-{}-{}", self.config.merge_request.branch_prefix, sanitized, hash)
    }

    fn matches_any_pattern(&self, path: &str) -> bool {
        self.managers
            .iter()
            .enumerate()
            .any(|(idx, m)| self.file_matches_manager_at(path, m.as_ref(), idx))
    }

    fn file_matches_manager(&self, path: &str, manager: &dyn PackageManager) -> bool {
        // Find the index of this manager by pointer comparison.
        let idx = self
            .managers
            .iter()
            .position(|m| std::ptr::eq(m.as_ref() as *const _, manager as *const _))
            .unwrap_or(usize::MAX);
        self.file_matches_manager_at(path, manager, idx)
    }

    fn file_matches_manager_at(&self, path: &str, manager: &dyn PackageManager, idx: usize) -> bool {
        let static_patterns = manager.file_patterns();

        if !static_patterns.is_empty() {
            // Built-in manager with static patterns.
            let filename = path.rsplit('/').next().unwrap_or(path);
            return static_patterns.iter().any(|pattern| {
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
            });
        }

        // Regex manager: look up dynamic patterns by index offset.
        // Regex managers are appended after built-in managers.
        let builtin_count = self.managers.len() - self.regex_manager_patterns.len();
        if idx >= builtin_count {
            let rm_idx = idx - builtin_count;
            if let Some(patterns) = self.regex_manager_patterns.get(rm_idx) {
                return patterns
                    .iter()
                    .any(|p| crate::manager::regex::file_matches_pattern(path, p));
            }
        }

        false
    }

    /// Fetch the digest for a helm dependency. Returns a placeholder on failure
    /// so the rest of the update can proceed.
    async fn fetch_dep_digest(&self, registry: &RegistrySource, version: &str) -> String {
        match lockfile::fetch_chart_digest(&self.http_client, registry, version).await {
            Ok(d) => d,
            Err(e) => {
                warn!("Could not fetch chart digest: {}", e);
                "sha256:unknown".to_string()
            }
        }
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

/// Derive the Chart.lock path that sits alongside a Chart.yaml path.
fn chart_lock_path(chart_yaml_path: &str) -> String {
    if let Some(dir) = chart_yaml_path.rfind('/') {
        format!("{}/Chart.lock", &chart_yaml_path[..dir])
    } else {
        "Chart.lock".to_string()
    }
}
