use crate::error::Result;
use crate::manager::{Dependency, UpdateContext};

/// Apply a sequence of updates to a single file, chaining the output of each
/// update as the input for the next. Returns the final `FileUpdate` whose
/// `updated_content` incorporates all changes.
///
/// If an individual update fails, it is skipped and the error is returned
/// alongside the partial result so the caller can note the failure.
pub fn apply_updates<'a>(
    updates: impl IntoIterator<Item = (&'a Dependency, &'a str)>,
    file_content: &str,
    file_path: &str,
) -> (FileUpdate, Vec<crate::error::ReforgeError>) {
    let mut current_content = file_content.to_string();
    let mut errors: Vec<crate::error::ReforgeError> = Vec::new();

    for (dep, new_version) in updates {
        match apply_update(dep, new_version, &current_content) {
            Ok(update) => {
                current_content = update.updated_content;
            }
            Err(e) => {
                errors.push(e);
            }
        }
    }

    (
        FileUpdate {
            file_path: file_path.to_string(),
            original_content: file_content.to_string(),
            updated_content: current_content,
        },
        errors,
    )
}

#[derive(Debug)]
pub struct FileUpdate {
    pub file_path: String,
    pub original_content: String,
    pub updated_content: String,
}

pub fn apply_update(
    dependency: &Dependency,
    new_version: &str,
    file_content: &str,
) -> Result<FileUpdate> {
    let updated = match &dependency.update_context {
        UpdateContext::DockerFrom {
            line_number,
            full_reference: _,
        } => {
            update_line_based(file_content, *line_number, &dependency.current_version, new_version)
        }
        UpdateContext::YamlKeyPath { keys: _ } => {
            update_yaml_value(file_content, &dependency.current_version, new_version)
        }
        UpdateContext::DockerComposeImage {
            full_reference, ..
        } => {
            let old_ref = full_reference;
            let new_ref = old_ref.replace(&dependency.current_version, new_version);
            file_content.replace(old_ref, &new_ref)
        }
    };

    Ok(FileUpdate {
        file_path: dependency.file_path.clone(),
        original_content: file_content.to_string(),
        updated_content: updated,
    })
}

/// Replace the version on a specific line (for Dockerfiles).
fn update_line_based(
    content: &str,
    line_number: usize,
    old_version: &str,
    new_version: &str,
) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::with_capacity(lines.len());

    for (idx, line) in lines.iter().enumerate() {
        if idx == line_number {
            result.push(line.replace(old_version, new_version));
        } else {
            result.push(line.to_string());
        }
    }

    // Preserve trailing newline
    let mut output = result.join("\n");
    if content.ends_with('\n') {
        output.push('\n');
    }
    output
}

/// For YAML files, do a targeted string replacement of the version value.
/// This preserves comments and formatting.
fn update_yaml_value(content: &str, old_version: &str, new_version: &str) -> String {
    // We do a careful replacement: find lines containing the old version
    // and replace only the version part. This works because version strings
    // are typically unique enough in context.
    //
    // For extra safety, we look for patterns like:
    //   tag: "1.25.3"    or    tag: 1.25.3
    //   version: "1.25.3" or   version: 1.25.3
    let patterns = [
        format!(": \"{}\"", old_version),
        format!(": '{}'", old_version),
        format!(": {}", old_version),
    ];

    let replacements = [
        format!(": \"{}\"", new_version),
        format!(": '{}'", new_version),
        format!(": {}", new_version),
    ];

    let mut result = content.to_string();

    // Replace only the first match found (to handle one dependency at a time)
    for (pattern, replacement) in patterns.iter().zip(replacements.iter()) {
        if result.contains(pattern.as_str()) {
            result = result.replacen(pattern, replacement, 1);
            break;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager::{Dependency, RegistrySource, UpdateContext};

    #[test]
    fn test_dockerfile_line_update() {
        let content = "FROM golang:1.22 AS builder\nRUN go build\nFROM alpine:3.19\n";
        let result = update_line_based(content, 0, "1.22", "1.23");
        assert!(result.starts_with("FROM golang:1.23 AS builder\n"));
        assert!(result.contains("FROM alpine:3.19"));
    }

    #[test]
    fn test_yaml_quoted_update() {
        let content = "image:\n  repository: nginx\n  tag: \"1.25.3\"\n";
        let result = update_yaml_value(content, "1.25.3", "1.26.0");
        assert!(result.contains("tag: \"1.26.0\""));
    }

    #[test]
    fn test_yaml_unquoted_update() {
        let content = "image:\n  repository: nginx\n  tag: 1.25.3\n";
        let result = update_yaml_value(content, "1.25.3", "1.26.0");
        assert!(result.contains("tag: 1.26.0"));
    }

    #[test]
    fn test_compose_image_update() {
        let dep = Dependency {
            name: "nginx".to_string(),
            current_version: "1.25.3".to_string(),
            registry: RegistrySource::DockerRegistry {
                image: "nginx".to_string(),
                registry: None,
            },
            file_path: "docker-compose.yaml".to_string(),
            update_context: UpdateContext::DockerComposeImage {
                service_path: vec![
                    "services".to_string(),
                    "web".to_string(),
                    "image".to_string(),
                ],
                full_reference: "nginx:1.25.3".to_string(),
            },
        };

        let content = "services:\n  web:\n    image: nginx:1.25.3\n";
        let result = apply_update(&dep, "1.26.0", content).unwrap();
        assert!(result.updated_content.contains("nginx:1.26.0"));
    }

    #[test]
    fn test_preserves_trailing_newline() {
        let content = "FROM nginx:1.25.3\n";
        let result = update_line_based(content, 0, "1.25.3", "1.26.0");
        assert!(result.ends_with('\n'));
        assert_eq!(result, "FROM nginx:1.26.0\n");
    }
}
