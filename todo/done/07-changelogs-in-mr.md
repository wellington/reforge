# Changelogs and Release Notes in MR Body

Fetch and embed release notes or changelog entries in update MR descriptions.

## Why
- The #1 factor in whether teams actually review and merge update MRs
- Saves reviewers from manually looking up what changed between versions
- Surfaces breaking changes and migration guides

## What's needed
- **GitHub Releases API**: Fetch release notes for Docker images and Helm charts hosted on GitHub
- **GitLab Releases API**: Same for GitLab-hosted projects
- **CHANGELOG.md parsing**: Fall back to parsing CHANGELOG.md from the source repo
- **Diff range**: Show notes for all versions between current and target, not just the latest
- **Truncation**: Cap changelog length in MR body to avoid massive descriptions
- **Caching**: Cache fetched changelogs to avoid redundant API calls across dependencies

## Estimated scope
~400-500 lines. New changelog fetcher module, GitHub/GitLab release API clients, MR body template expansion.
