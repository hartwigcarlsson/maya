# Issue tracker: GitHub

Issues and specs for this repo live as GitHub issues. Use the `gh` CLI for all operations.

## Conventions

- **Create an issue**: `gh issue create --title "..." --body "..."`. Use a heredoc for multi-line bodies.
- **Read an issue**: `gh issue view <number> --json number,title,body,labels,comments`.
- **List issues**: `gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'` with appropriate `--label` and `--state` filters.
- **Make an issue a sub-issue of a parent**: `gh issue create --parent <parent> ...`, or `gh issue edit <parent> --add-sub-issue <child>` afterwards (`gh` 2.94+). Older `gh`: `gh api --method POST repos/<owner>/<repo>/issues/<parent>/sub_issues -F sub_issue_id=<child-db-id>` (database id, as in **Blocking** below). Without sub-issues, put `Part of #<parent>` at the top of the child body.
- **Comment on an issue**: `gh issue comment <number> --body "..."`
- **Apply / remove labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **Close**: `gh issue close <number> --comment "..."`

Infer the repo from `git remote -v`; `gh` does this automatically when run inside a clone.

## Pull requests as a triage surface

**PRs as a request surface: no.** _(Set to `yes` if this repo treats external PRs as feature requests; `/triage` reads this flag.)_

When set to `yes`, PRs run through the same labels and states as issues, using the `gh pr` equivalents:

- **Read a PR**: `gh pr view <number> --comments` and `gh pr diff <number>` for the diff.
- **List external PRs for triage**: `gh api --paginate 'repos/{owner}/{repo}/pulls?state=open' --jq '.[] | select(.author_association | IN("OWNER","MEMBER","COLLABORATOR") | not) | {number, title, author: .user.login, author_association, labels: [.labels[].name]}'`.
- **Comment / label / close**: `gh pr comment`, `gh pr edit --add-label`/`--remove-label`, `gh pr close`.

GitHub shares one number space across issues and PRs, so a bare `#42` may be either: resolve with `gh pr view 42` and fall back to `gh issue view 42`.

## When a skill says "publish to the issue tracker"

Create a GitHub issue.

## When a skill says "fetch the relevant ticket"

Read it as in **Read an issue** above.

## Wayfinding operations

Used by `/wayfinder`. The **map** is a single issue with **child** issues as tickets.

- **Map**: a single issue labelled `wayfinder:map`, holding the Notes / Decisions-so-far / Fog body. `gh issue create --label wayfinder:map`.
- **Child ticket**: an issue linked to the map as a GitHub sub-issue (see **Make an issue a sub-issue of a parent**). Where sub-issues aren't enabled, add the child to a task list in the map body and put `Part of #<map>` at the top of the child body. Labels: `wayfinder:<type>` (`research`/`prototype`/`grilling`/`task`). Once claimed, the ticket is assigned to the driving dev.
- **Blocking**: GitHub's **native issue dependencies**, the canonical, UI-visible representation. Add an edge with `gh api --method POST repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`, where `<blocker-db-id>` is the blocker's numeric **database id** (`gh api repos/<owner>/<repo>/issues/<n> --jq .id`, _not_ the `#number` or `node_id`). GitHub reports `issue_dependencies_summary.blocked_by` (open blockers only, the live gate). Where dependencies aren't available, fall back to a `Blocked by: #<n>, #<n>` line at the top of the child body. A ticket is unblocked when every blocker is closed.
- **Frontier query**: the map's open sub-issues with no assignee and no open blocker, in map order; the first row wins. One call (set `MAP`):

  ```sh
  gh api graphql -F map=$MAP -f query='query($map:Int!){repository(owner:"hartwigcarlsson",name:"maya"){issue(number:$map){subIssues(first:100){nodes{number title state assignees(first:1){totalCount} labels(first:5){nodes{name}} blockedBy(first:20){nodes{state}}}}}}}' \
    --jq '.data.repository.issue.subIssues.nodes[] | select(.state=="OPEN" and .assignees.totalCount==0 and ([.blockedBy.nodes[]|select(.state=="OPEN")]|length)==0) | "\(.number)\t\(.title)\t\([.labels.nodes[].name]|join(","))"'
  ```

- **HITL task**: a `wayfinder:task` only a human can do (send an email, sign an agreement) starts its body with `HITL: <what the human does>`. Read the first line of a task before claiming it, and take the next frontier row when the human cannot do it now.
- **Claim**: `gh issue edit <n> --add-assignee @me`, the session's first write.
- **Resolution branch**: right after claiming, `git fetch` and put a fresh `<type>/<issue>-<slug>` branch from `origin/main` in a worktree: in a session that already runs in its own worktree (the desktop app refuses edits to any other), check the branch out there with `git checkout -b <branch> origin/main`; otherwise add a worktree on it. Read `GLOSSARY.md`, `docs/adr/` and `docs/bfl-kontroll.md` on that branch: the session's starting checkout may sit on a stale branch, and its glossary and krav-id:n with it. Then read the diff of every open PR that changes `GLOSSARY.md`: since merge is not a gate, earlier resolutions' terms may live only there.

  ```sh
  gh pr list --state open --json number,title,files --jq '.[] | select(any(.files[]; .path == "GLOSSARY.md")) | "\(.number)\t\(.title)"'
  ```

  To change a term such a PR adds, cut the branch from that PR's branch and open the PR against it; GitHub retargets it to `main` when the base PR merges and its branch is deleted.
- **Resolve**: `gh issue comment <n> --body "<answer>"`, then `gh issue close <n>`, then append a context pointer (gist + link) to the map's Decisions-so-far. Write the comment in Swedish under `## Beslut`, like the map. Run `scripts/check-bfl-line --comment <file>` and `scripts/check-links <file>` on it before posting it: link a file a resolution PR adds as `blob/main/<path>`, which resolves once the PR merges, since merged branches are deleted. Name in the comment the constraints later decisions depend on (what a source lacks, what a licence forbids), not only the conclusion: the map carries only the gist, and later sessions decide from it. Before posting, search closed tickets' comments for each term the decision changes, since the map only gists earlier decisions: `gh search issues "<term>" --repo hartwigcarlsson/maya --state closed --match comments`. Where an earlier decision says otherwise, comment on that ticket with a link to the new one. Edit the map body only through `scripts/wayfinder-map` (`add`, `replace`, `remove`; `--help` for usage): other sessions edit it concurrently, and the script refetches before the write and verifies after it.
- **Files a resolution changes** (`GLOSSARY.md`, an ADR, docs): one PR from the resolution branch, opened before the resolution comment, which links it. Close the ticket once the PR is open; merge is not a gate. Then `git worktree remove` a worktree you added for it: the branch on origin carries the PR.
- **Research worktree**: once the subagent has pushed its `research/<slug>` branch, `git worktree remove` its worktree; the branch on origin is the record.
- **Prototype**: a single `prototypes/<slug>/index.html` on an unmerged `prototype/<issue>-<slug>` branch, linked from the resolution comment. Preview it with `preview_start` on the launch configuration `prototyp` (served at `http://localhost:8724/<slug>/`); a `file://` URL only gives a static snapshot. Page prototypes build on variant D of [the interface prototype](https://github.com/hartwigcarlsson/maya/tree/prototype/24-granssnitt/prototypes/granssnitt): `git show origin/prototype/24-granssnitt:prototypes/granssnitt/index.html`.
- **Research findings**: live on unmerged `research/<slug>` branches, linked from the ticket's resolution comment. Read the headings first, then only the sections you need: `git fetch && git show origin/research/<slug>:docs/research/<slug>.md | grep -n '^#'`, then the line range with `sed -n`.
