# Renovate API Compatibility

**Last updated:** 2026-04-04
**Type:** Decision
**Status:** Active

## Description

Reforge must support Renovate's parameters and features to maintain compatibility with codebases already using the Renovate API. The upstream reference is https://github.com/renovatebot/renovate.

## Context

Existing projects may use Renovate configuration (`renovate.json`, `.renovaterc`, etc.) and depend on its behavior for dependency management. Reforge aims to be a drop-in replacement or compatible subset so teams can migrate without rewriting their configs.

## Next Steps

- Investigate API gaps between reforge's current config surface and Renovate's config options
- Identify which Renovate features are actively used
- Prioritize compatibility for the most-used Renovate parameters
- Support reading `renovate.json` / `.renovaterc` / `.renovaterc.json` config formats
- Map Renovate config keys to reforge equivalents where possible
- Document any intentional divergences

## Notes
- Reforge already accepts `RENOVATE_TOKEN` and `RENOVATE_GITLAB_URL` env vars as migration fallbacks (see `project-naming.md`)
