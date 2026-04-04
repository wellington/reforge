# Persistence Context Database

This directory is the AI assistant's persistent memory for the reforge project. It is **not** application code — it is a structured knowledge store that the assistant reads and writes across sessions to accumulate learning: solutions to problems, effective workflows, codebase observations, and project-specific decisions.

## Why This Exists

The assistant starts each session without memory of prior sessions. Without a persistence mechanism, the same issues get diagnosed repeatedly, effective workflows are rediscovered from scratch, and non-obvious codebase facts have to be re-inferred. This directory is the remedy.

## How the Assistant Should Use This

### At Session Start (mandatory)

1. Read `.claude/index.md` to load the table of contents.
2. Identify any entries relevant to the current task or domain.
3. Read those knowledge files before proceeding.

This prevents re-discovering solutions, re-experiencing dead ends, and re-learning project conventions.

### When to Write a New Entry

Write a new entry (or update an existing one) whenever the assistant learns something worth preserving across sessions:

| Write when... | Entry type |
|---------------|-----------|
| A problem required multiple attempts to solve | **Issue** |
| A workflow or approach proved consistently effective | **Pattern** |
| A faster/better way to do something was discovered | **Optimization** |
| A non-obvious fact about the codebase was discovered | **Observation** |
| An architectural or stylistic decision was made for this project | **Decision** |
| The user corrected an approach or confirmed one worked | **Feedback** |

**Do not** write entries for trivial single-step tasks or things already clearly stated in project documentation.

### When to Update an Existing Entry

- A new method was tried and the status changed
- A workaround was superseded by a proper fix
- A codebase observation became outdated
- Additional edge cases or caveats were discovered

## Entry Format

All entries use this structure. Sections marked _(Issue only)_ are omitted for non-Issue types; _(non-Issue)_ sections replace them.

```markdown
# [Topic Name]

**Last updated:** YYYY-MM-DD
**Type:** Issue | Pattern | Optimization | Observation | Decision | Feedback
**Status:** Resolved | Active | Ongoing | Superseded

## Description
[What this entry is about. One paragraph.]

## Context
[When/where this applies — which tools, workflows, CI jobs, codebase areas]

## Methods Tried  <- Issue only
1. **[Approach]** -> FAILED
   Reason: [why]
2. **[Approach]** -> WORKED

## Approach  <- Pattern / Optimization only
[What to do. Actionable steps or commands.]

## Details  <- Observation / Decision / Feedback only
[The fact, finding, or decision and its rationale.]

## Solution / Summary
[For Issues: exact working commands. For others: key takeaway in one paragraph.]

## Notes
- [Caveats, edge cases, related entries, links to project docs]
```

**Status values:**
- `Resolved` — Issue fixed; no longer a problem
- `Active` — Pattern/Optimization/Observation currently in use
- `Ongoing` — Issue or situation that recurs and is being managed
- `Superseded` — Entry replaced by a better approach (keep for history)

## Cross-References

When an entry directly relates to another, add a `**See also:**` line in the Notes section:

```markdown
## Notes
- **See also:** [topic description](filename.md)
```

## Rules

1. **The assistant owns these files** — written by assistant sessions; humans may correct factual errors.
2. **Keep entries factual** — document what actually happened, not hypotheticals.
3. **Commit changes** — context files are tracked in git so future sessions benefit.
4. **Update the index** — whenever you add or update a knowledge file, update `index.md`.
5. **Prefer updating over creating** — if an existing entry covers the topic, extend it.
6. **Cross-reference related entries** — add `See also:` links when entries are meaningfully related.

## Directory Structure

```
.claude/
├── README.md                              # This file — system overview
├── index.md                               # Master index — READ THIS AT SESSION START
└── knowledge/
    ├── project-architecture.md            # [Observation] Module layout, design decisions
    ├── initial-build-session.md           # [Observation] First build session details
    └── ...
```
