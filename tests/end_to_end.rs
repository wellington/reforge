use std::path::PathBuf;
use tempfile::TempDir;
use tokio::process::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .expect("git command failed to start");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

async fn create_test_repo() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();

    git(dir, &["init", "-b", "main"]).await;
    git(dir, &["config", "user.email", "test@reforge.dev"]).await;
    git(dir, &["config", "user.name", "Reforge Test"]).await;

    tokio::fs::create_dir_all(dir.join("values/speedtest")).await.unwrap();
    tokio::fs::create_dir_all(dir.join("values/vault-unseal")).await.unwrap();
    tokio::fs::create_dir_all(dir.join("apps/app")).await.unwrap();
    tokio::fs::create_dir_all(dir.join("charts/login")).await.unwrap();

    tokio::fs::write(
        dir.join("Dockerfile"),
        "FROM nginx:1.25.0\nCOPY html/ /usr/share/nginx/html/\n",
    )
    .await
    .unwrap();

    tokio::fs::write(
        dir.join("values/speedtest/values.yaml"),
        r#"server:
  image:
    repository: nginx
    tag: "1.25.0"
client:
  image:
    repository: curlimages/curl
    tag: "8.7.0"
"#,
    )
    .await
    .unwrap();

    tokio::fs::write(
        dir.join("values/vault-unseal/values.yaml"),
        r#"image:
  repository: hashicorp/vault
  tag: "1.16.0"
"#,
    )
    .await
    .unwrap();

    tokio::fs::write(
        dir.join("charts/login/Chart.yaml"),
        r#"apiVersion: v2
name: login
version: 1.0.0
"#,
    )
    .await
    .unwrap();

    // helmChart / helmVersion in app YAML — detected by regex manager
    tokio::fs::write(
        dir.join("apps/app/login.yaml"),
        r#"appName: login
namespace: login
helmChart: "oci://MOCK_HOST/developer-excellence/app-charts/stateless-http-service"
helmVersion: "14.1.0"
"#,
    )
    .await
    .unwrap();

    git(dir, &["add", "-A"]).await;
    git(dir, &["commit", "-m", "initial commit"]).await;

    tmp
}

/// Patch MOCK_HOST placeholders in all YAML files.
async fn patch_mock_host(dir: &std::path::Path, mock_host: &str) {
    for entry in walkdir(dir).await {
        let name = entry.to_string_lossy();
        if name.ends_with(".yaml") || name.ends_with(".yml") {
            let content = tokio::fs::read_to_string(&entry).await.unwrap();
            if content.contains("MOCK_HOST") {
                let patched = content.replace("MOCK_HOST", mock_host);
                tokio::fs::write(&entry, patched).await.unwrap();
            }
        }
    }
    git(dir, &["add", "-A"]).await;
    git(dir, &["commit", "--amend", "--no-edit"]).await;
}

async fn walkdir(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let mut rd = tokio::fs::read_dir(&d).await.unwrap();
        while let Some(entry) = rd.next_entry().await.unwrap() {
            let p = entry.path();
            if p.is_dir() {
                if p.file_name().unwrap_or_default() != ".git" {
                    stack.push(p);
                }
            } else {
                files.push(p);
            }
        }
    }
    files
}

async fn setup_docker_registry_mocks(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/v2/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/v2/library/nginx/tags/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "library/nginx",
            "tags": ["1.24.0", "1.25.0", "1.25.1", "1.25.2", "1.25.3", "1.26.0",
                     "1.27.0", "1.27.1", "latest", "alpine", "1.27-alpine"]
        })))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/v2/curlimages/curl/tags/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "curlimages/curl",
            "tags": ["8.7.0", "8.7.1", "8.8.0", "8.9.0", "8.10.0", "8.10.1",
                     "8.11.0", "8.12.0", "8.12.1"]
        })))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/v2/hashicorp/vault/tags/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "hashicorp/vault",
            "tags": ["1.15.0", "1.16.0", "1.16.1", "1.16.2", "1.17.0",
                     "1.17.1", "1.17.2", "1.18.0"]
        })))
        .mount(server)
        .await;
}

async fn setup_oci_helm_registry_mocks(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/v2/developer-excellence/app-charts/stateless-http-service/tags/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "developer-excellence/app-charts/stateless-http-service",
            "tags": ["13.0.0", "14.0.0", "14.1.0", "14.2.0", "15.0.0",
                     "15.4.0", "15.5.0", "15.6.0", "15.7.0"]
        })))
        .mount(server)
        .await;
}

/// Build reforge.toml with registries overridden to point to the mock server.
fn build_config(
    repo_path: &str,
    mock_host: &str,
    mock_url: &str,
    managers: &[&str],
    extras: &str,
) -> String {
    let mgrs = managers
        .iter()
        .map(|m| format!("\"{}\"", m))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        r#"
local_path = "{repo_path}"

[managers]
enabled = [{mgrs}]

[versioning]
pin_strategy = "semver-minor"

[merge_request]
branch_prefix = "reforge/"
grouping = "per-dependency"
rebase_enabled = false

[dashboard]
enabled = true
local_path = "{repo_path}/DEPENDENCY_DASHBOARD.md"

[changelog]
enabled = false

[vulnerability]
enabled = false

[replacement]
enabled = false

[lockfile]
enabled = false

[registries."{mock_host}"]
base_url = "{mock_url}"

[registries."registry-1.docker.io"]
base_url = "{mock_url}"

{extras}
"#,
    )
}

/// Helper: write config, commit, run reforge, return (stdout, stderr, exit status).
async fn run_reforge(
    repo_path: &std::path::Path,
    config_toml: &str,
    extra_args: &[&str],
) -> (String, String, bool) {
    let config_path = repo_path.join("reforge.toml");
    tokio::fs::write(&config_path, config_toml).await.unwrap();
    git(repo_path, &["add", "reforge.toml"]).await;
    // Only commit if there are staged changes
    let status = git(repo_path, &["status", "--porcelain"]).await;
    if !status.is_empty() {
        git(repo_path, &["commit", "-m", "add/update reforge config"]).await;
    }

    let binary = env!("CARGO_BIN_EXE_reforge");
    let mut cmd = Command::new(binary);
    cmd.arg("--config")
        .arg(config_path.to_str().unwrap())
        .arg("--local-path")
        .arg(repo_path.to_str().unwrap())
        .arg("--log-level")
        .arg("debug");
    for arg in extra_args {
        cmd.arg(arg);
    }

    let output = cmd.output().await.expect("Failed to run reforge");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (stdout, stderr, output.status.success())
}

fn list_reforge_branches(branches_output: &str) -> Vec<String> {
    branches_output
        .lines()
        .map(|l| l.trim().trim_start_matches("* ").to_string())
        .filter(|b| b.starts_with("reforge/"))
        .collect()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_full_local_scan_with_mocked_registries() {
    let mock_server = MockServer::start().await;
    setup_docker_registry_mocks(&mock_server).await;
    setup_oci_helm_registry_mocks(&mock_server).await;

    let mock_host = mock_server.address().to_string();
    let mock_url = format!("http://{}", mock_host);

    let repo = create_test_repo().await;
    let repo_path = repo.path();
    patch_mock_host(repo_path, &mock_host).await;

    let regex_section = format!(
        r#"
[[regex_managers]]
name = "helm-app-version"
file_patterns = ["apps/**/*.yaml"]
match_pattern = 'helmChart:\s*"[^"]*?/(?P<depName>[^"/]+)"\nhelmVersion:\s*"(?P<currentValue>[^"]+)"'
datasource = "helm-oci"
registry_url = "{mock_host}/developer-excellence/app-charts"
"#,
    );

    let config = build_config(
        &repo_path.display().to_string(),
        &mock_host,
        &mock_url,
        &["helm", "docker"],
        &regex_section,
    );

    let (stdout, stderr, success) = run_reforge(repo_path, &config, &[]).await;
    eprintln!("=== STDOUT ===\n{}", stdout);
    eprintln!("=== STDERR ===\n{}", stderr);
    assert!(success, "reforge failed: {}", stderr);

    let branches = git(repo_path, &["branch", "--list"]).await;
    eprintln!("=== BRANCHES ===\n{}", branches);

    let reforge_branches = list_reforge_branches(&branches);
    assert!(
        !reforge_branches.is_empty(),
        "Expected reforge/ branches. Got:\n{}",
        branches,
    );

    // Verify dashboard was created
    let dashboard_path = repo_path.join("DEPENDENCY_DASHBOARD.md");
    assert!(dashboard_path.exists(), "Dashboard file should exist");
    let dashboard = tokio::fs::read_to_string(&dashboard_path).await.unwrap();
    assert!(dashboard.contains("Dependency Dashboard"));
}

#[tokio::test]
async fn test_dry_run_does_not_create_branches() {
    let mock_server = MockServer::start().await;
    setup_docker_registry_mocks(&mock_server).await;

    let mock_host = mock_server.address().to_string();
    let mock_url = format!("http://{}", mock_host);

    let repo = create_test_repo().await;
    let repo_path = repo.path();

    let config = build_config(
        &repo_path.display().to_string(),
        &mock_host,
        &mock_url,
        &["helm"],
        "",
    );

    let (stdout, stderr, success) = run_reforge(repo_path, &config, &["--dry-run"]).await;
    eprintln!("=== DRY-RUN STDOUT ===\n{}", stdout);
    eprintln!("=== DRY-RUN STDERR ===\n{}", stderr);
    assert!(success);

    assert!(
        stdout.contains("update(s) available") || stdout.contains("No updates"),
        "Expected dry-run report",
    );

    let branches = git(repo_path, &["branch", "--list"]).await;
    assert!(
        !branches.contains("reforge/"),
        "Dry-run should NOT create branches",
    );
}

#[tokio::test]
async fn test_regex_manager_detects_helm_version() {
    let mock_server = MockServer::start().await;
    setup_docker_registry_mocks(&mock_server).await;
    setup_oci_helm_registry_mocks(&mock_server).await;

    let mock_host = mock_server.address().to_string();
    let mock_url = format!("http://{}", mock_host);

    let repo = create_test_repo().await;
    let repo_path = repo.path();
    patch_mock_host(repo_path, &mock_host).await;

    let regex_section = format!(
        r#"
[[regex_managers]]
name = "helm-app-version"
file_patterns = ["apps/**/*.yaml"]
match_pattern = 'helmChart:\s*"[^"]*?/(?P<depName>[^"/]+)"\nhelmVersion:\s*"(?P<currentValue>[^"]+)"'
datasource = "helm-oci"
registry_url = "{mock_host}/developer-excellence/app-charts"
"#,
    );

    let config = build_config(
        &repo_path.display().to_string(),
        &mock_host,
        &mock_url,
        &[],
        &regex_section,
    );

    let (stdout, stderr, success) =
        run_reforge(repo_path, &config, &["--no-dashboard"]).await;
    eprintln!("=== REGEX STDOUT ===\n{}", stdout);
    eprintln!("=== REGEX STDERR ===\n{}", stderr);
    assert!(success, "reforge failed: {}", stderr);

    let branches = git(repo_path, &["branch", "--list"]).await;
    let reforge_branches = list_reforge_branches(&branches);
    eprintln!("=== BRANCHES ===\n{:?}", reforge_branches);

    assert!(
        !reforge_branches.is_empty(),
        "Expected a reforge/ branch for regex manager",
    );

    // Verify the update was applied
    let update_branch = &reforge_branches[0];
    git(repo_path, &["checkout", update_branch]).await;
    let updated = tokio::fs::read_to_string(repo_path.join("apps/app/login.yaml"))
        .await
        .unwrap();
    eprintln!("=== UPDATED login.yaml ===\n{}", updated);

    assert!(!updated.contains("14.1.0"), "Should no longer have 14.1.0");
    // semver-minor strategy: 14.1.0 -> 14.2.0 (same major, highest minor)
    assert!(updated.contains("14.2.0"), "Should have 14.2.0 (semver-minor)");

    git(repo_path, &["checkout", "main"]).await;
}

#[tokio::test]
async fn test_idempotent_no_duplicate_branches() {
    let mock_server = MockServer::start().await;
    setup_docker_registry_mocks(&mock_server).await;

    let mock_host = mock_server.address().to_string();
    let mock_url = format!("http://{}", mock_host);

    let repo = create_test_repo().await;
    let repo_path = repo.path();

    // Simplify: only Dockerfile
    let _ = tokio::fs::remove_dir_all(repo_path.join("values")).await;
    let _ = tokio::fs::remove_dir_all(repo_path.join("charts")).await;
    let _ = tokio::fs::remove_dir_all(repo_path.join("apps")).await;
    // Use mock_host as explicit registry in Dockerfile to avoid Docker Hub redirect
    let dockerfile = format!("FROM {mock_host}/library/nginx:1.25.0\nRUN echo hello\n");
    tokio::fs::write(repo_path.join("Dockerfile"), &dockerfile).await.unwrap();

    git(repo_path, &["add", "-A"]).await;
    git(repo_path, &["commit", "-m", "simplify"]).await;

    let config = build_config(
        &repo_path.display().to_string(),
        &mock_host,
        &mock_url,
        &["docker"],
        "",
    );

    // First run
    let (_, stderr1, ok1) = run_reforge(repo_path, &config, &["--no-dashboard"]).await;
    assert!(ok1, "Run 1 failed: {}", stderr1);

    let b1 = git(repo_path, &["branch", "--list"]).await;
    let count1 = list_reforge_branches(&b1).len();
    assert!(count1 > 0, "First run should create branches");

    // Second run
    let (_, stderr2, ok2) = run_reforge(repo_path, &config, &["--no-dashboard"]).await;
    assert!(ok2, "Run 2 failed: {}", stderr2);

    let b2 = git(repo_path, &["branch", "--list"]).await;
    let count2 = list_reforge_branches(&b2).len();

    assert_eq!(count1, count2, "Second run should NOT create new branches");
}

#[tokio::test]
async fn test_dockerfile_update_content_is_correct() {
    let mock_server = MockServer::start().await;
    setup_docker_registry_mocks(&mock_server).await;

    let mock_host = mock_server.address().to_string();
    let mock_url = format!("http://{}", mock_host);

    let repo = create_test_repo().await;
    let repo_path = repo.path();

    let dockerfile = format!("FROM {mock_host}/library/nginx:1.25.0\nRUN echo hello\n");
    tokio::fs::write(repo_path.join("Dockerfile"), &dockerfile).await.unwrap();
    let _ = tokio::fs::remove_dir_all(repo_path.join("values")).await;
    let _ = tokio::fs::remove_dir_all(repo_path.join("charts")).await;
    let _ = tokio::fs::remove_dir_all(repo_path.join("apps")).await;

    git(repo_path, &["add", "-A"]).await;
    git(repo_path, &["commit", "-m", "dockerfile only"]).await;

    let config = build_config(
        &repo_path.display().to_string(),
        &mock_host,
        &mock_url,
        &["docker"],
        "",
    );

    let (stdout, stderr, ok) = run_reforge(repo_path, &config, &["--no-dashboard"]).await;
    eprintln!("=== DOCKERFILE TEST STDOUT ===\n{}", stdout);
    eprintln!("=== DOCKERFILE TEST STDERR ===\n{}", stderr);
    assert!(ok, "reforge failed: {}", stderr);

    let branches = git(repo_path, &["branch", "--list"]).await;
    let reforge_branches = list_reforge_branches(&branches);
    eprintln!("=== BRANCHES ===\n{:?}", reforge_branches);

    let update_branch = reforge_branches
        .iter()
        .find(|b| b.contains("nginx"))
        .expect("Should find a reforge branch for nginx");

    git(repo_path, &["checkout", update_branch]).await;
    let updated = tokio::fs::read_to_string(repo_path.join("Dockerfile")).await.unwrap();

    assert!(!updated.contains(":1.25.0"), "Should no longer have :1.25.0");
    // Mock has tags up to 1.27.1; semver-minor permits any 1.x update
    assert!(updated.contains(":1.27.1"), "Should have :1.27.1. Got:\n{}", updated);
    assert!(updated.contains("RUN echo hello"), "Non-FROM lines preserved");

    git(repo_path, &["checkout", "main"]).await;
}

#[tokio::test]
async fn test_values_yaml_image_updates() {
    let mock_server = MockServer::start().await;
    setup_docker_registry_mocks(&mock_server).await;

    let mock_host = mock_server.address().to_string();
    let mock_url = format!("http://{}", mock_host);

    let repo = create_test_repo().await;
    let repo_path = repo.path();

    // Remove non-values files
    let _ = tokio::fs::remove_file(repo_path.join("Dockerfile")).await;
    let _ = tokio::fs::remove_dir_all(repo_path.join("charts")).await;
    let _ = tokio::fs::remove_dir_all(repo_path.join("apps")).await;

    git(repo_path, &["add", "-A"]).await;
    git(repo_path, &["commit", "-m", "values only"]).await;

    let config = build_config(
        &repo_path.display().to_string(),
        &mock_host,
        &mock_url,
        &["helm"],
        "",
    );

    let (stdout, stderr, ok) = run_reforge(repo_path, &config, &["--no-dashboard"]).await;
    eprintln!("=== STDOUT ===\n{}", stdout);
    eprintln!("=== STDERR ===\n{}", stderr);
    assert!(ok, "reforge failed: {}", stderr);

    let branches = git(repo_path, &["branch", "--list"]).await;
    let reforge_branches = list_reforge_branches(&branches);
    eprintln!("=== BRANCHES ===\n{:?}", reforge_branches);

    // Should have branches for nginx, curl, and vault updates
    assert!(
        reforge_branches.len() >= 3,
        "Expected >= 3 update branches for 3 images. Got {}:\n{:?}",
        reforge_branches.len(),
        reforge_branches,
    );

    // Verify one update: vault 1.16.0 -> 1.18.0
    if let Some(vault_branch) = reforge_branches.iter().find(|b| b.contains("vault")) {
        git(repo_path, &["checkout", vault_branch]).await;
        let updated = tokio::fs::read_to_string(repo_path.join("values/vault-unseal/values.yaml"))
            .await
            .unwrap();
        assert!(!updated.contains("1.16.0"), "vault should be updated from 1.16.0");
        // semver-minor: 1.16.0 -> 1.17.2 (or 1.18.0 depending on strategy).
        // The mock has 1.17.0, 1.17.1, 1.17.2, 1.18.0; best minor update is 1.18.0.
        assert!(
            updated.contains("1.18.0") || updated.contains("1.17."),
            "vault should be updated to a newer version. Got:\n{}", updated,
        );
        git(repo_path, &["checkout", "main"]).await;
    }
}
