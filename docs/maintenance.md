# Repository automation

## Dependabot

[`.github/dependabot.yml`](../.github/dependabot.yml) checks Cargo dependencies and
GitHub Actions every Monday at 09:00 UTC, targeting `master`. Minor/patch updates
are grouped per ecosystem; major updates remain separate PRs for manual review.

## Automatic merging

Set **Settings → Secrets and variables → Actions → New repository secret**:

```text
DEPENDABOT_AUTOMERGE_PAT
```

Use a fine-grained PAT with access to this repository and these repository permissions:

- **Actions: Read** — verify the completed CI run.
- **Contents: Read and write** — inspect commits and squash-merge.
- **Pull requests: Read and write** — inspect PRs and approve them.

A classic PAT with `repo` scope also works. Use a trusted account that can approve
and merge Dependabot PRs; rotate the token before it expires. Store it as an
**Actions repository secret**, not a plain variable or a checked-in value. This
trusted `workflow_run` workflow does not require duplicating it in Dependabot secrets.

The [auto-merge workflow](../.github/workflows/dependabot-automerge.yml) runs after
**Rust CI** succeeds. It only approves and squash-merges open, non-draft,
same-repository Dependabot PRs targeting the default branch. It checks the signed
Dependabot commit metadata and permits **minor/patch updates only**, including groups
where every dependency qualifies. Missing/unknown metadata and major updates are
left for manual review.

Before merging, it rechecks the CI run/attempt and matches the PR's current head SHA
to the tested SHA. The merge API also pins that SHA, so a subsequent update cannot
bypass CI. Only trusted default-branch workflow code is checked out; PR code is never
executed with the PAT. The automation policy has offline Node.js tests.

This merges directly **after passing CI**, rather than merely enabling GitHub's native
"auto-merge" setting. It therefore also works for private repositories whose GitHub
plan does not provide native auto-merge/branch protection. Any available branch
protection and additional approval requirements still apply; they are never bypassed.

If a token is missing/expired, the merge fails safely and the PR stays open. After
fixing the secret, rerun the failed auto-merge job. A newer PR head needs a fresh
passing CI run before it can merge.

## Rust CI

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) runs on pushes and PRs:
formatting, Clippy with warnings denied, locked tests/release build, the local
MP3/AAC/Ogg/HLS playback smoke test, and the automation policy tests. Public radio
search tests and README screenshot capture remain opt-in, not CI requirements.

If renaming **Rust CI** or its workflow path, also update the auto-merge trigger and
policy check so CI gating remains effective.
