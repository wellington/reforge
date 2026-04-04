# Regex Manager

A generic custom manager where users define regex patterns to extract versions from arbitrary files.

## Why
- Covers the long tail of version pinning that isn't Dockerfiles or Helm charts (Terraform, Makefiles, CI configs, JSON, shell scripts)
- Eliminates the need to write a new manager for every file format
- Matches one of Renovate's most powerful features

## What's needed
- **Config schema**: Users define regex managers in `reforge.toml` with file patterns, extraction regex (with named groups for `depName`, `currentValue`, `datasource`, `registryUrl`), and versioning strategy
- **RegexManager struct**: Implements `PackageManager` trait using user-provided patterns
- **Datasource mapping**: Map extracted `datasource` field to the appropriate `RegistrySource` variant
- **Validation**: Verify regex compiles at config load time, provide clear errors for missing named groups

## Estimated scope
~400-500 lines. New manager implementation, config schema extension, documentation for regex patterns.
