# AGENTS.md: Maya

## Definition of Done

A change is done when all of these hold:

1. Commits are conventional (feat:/fix:/refactor:/test:/docs:/chore:) and atomic, on a branch `<type>/<issue>-<slug>` cut from `origin/main` after a `git fetch`. Stay on the current branch only when it was cut for this issue and is not yet merged, whatever `/implement` says.
2. Every commit body has a `BFL:` line, written by the steps in `docs/bfl-kontroll.md`. `.githooks/commit-msg` checks this and the conventional subject; enable it once per clone with `git config core.hooksPath .githooks`. CI runs the same check on every pull-request commit.

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues on hartwigcarlsson/maya, via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default triage labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`) are used as-is. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one root `GLOSSARY.md` plus `docs/adr/`. See `docs/agents/domain.md`.
