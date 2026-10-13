# AGENTS.md: Maya

## Definition of Done

A change is done when all of these hold:

1. Commit is conventional (feat:/fix:/refactor:/test:/docs:/chore:), atomic, branched from `origin/main` after a `git fetch`: a session that starts on a branch already merged there branches afresh as `feat/<issue>-<slug>`, whatever `/implement` says about the current branch.
2. Every commit body has a `BFL:` line, written by the steps in `docs/bfl-kontroll.md`. `.githooks/commit-msg` checks this and the conventional subject; enable it once per clone with `git config core.hooksPath .githooks`. CI runs the same check on every pull-request commit.

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues on hartwigcarlsson/maya, via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default triage labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`) are used as-is. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one root `GLOSSARY.md` plus `docs/adr/`. See `docs/agents/domain.md`.
