# Replacement / Deprecation Awareness

Detect when a Docker image or Helm chart has been deprecated or renamed, and propose migration MRs.

## Why
- Container registries migrate (e.g., `gcr.io/google-containers` → `registry.k8s.io`)
- Images get deprecated in favor of successors (e.g., `nginx` → `nginxinc/nginx-unprivileged`)
- Without awareness, teams pin to abandoned images and miss security patches

## What's needed
- **Replacement database**: A local or remote data source mapping deprecated images/charts to their replacements (could start as a TOML file in the repo, evolve to a remote feed)
- **Migration MR generation**: When a replacement is known, create an MR that swaps the image/chart reference entirely, not just the version
- **Deprecation warnings**: If no replacement is known, annotate the dependency dashboard and MR descriptions with deprecation notices
- **Registry header detection**: Some registries return deprecation headers or redirect to new locations — detect and surface these
- **Community data**: Optionally pull from Renovate's open-source replacement rules as a data source

## Estimated scope
~400-500 lines. Replacement database schema, migration diff generation (different from version bump diffs), detection logic.
