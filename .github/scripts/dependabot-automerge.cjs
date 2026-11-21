"use strict";

// Dependabot puts machine-readable metadata in its signed commit message.
// Fail closed for missing, incomplete, or unfamiliar metadata (including majors).
function isMinorOrPatch(message) {
  const metadata = message.match(/^updated-dependencies:\s*\r?\n([\s\S]*?)^\.\.\.\s*$/m)?.[1];
  if (!metadata) return false;
  const dependencies = [...metadata.matchAll(/^\s*- dependency-name:/gm)];
  const types = [...metadata.matchAll(/^\s+update-type:\s*([^\r\n]+)$/gm)]
    .map((match) => match[1].trim().replace(/^(['"])(.*)\1$/, "$2"));
  return dependencies.length > 0 && types.length === dependencies.length && types.every(
    (type) => ["version-update:semver-minor", "version-update:semver-patch"].includes(type),
  );
}

async function autoMerge({ github, context, core }) {
  const { owner, repo } = context.repo;
  const fullName = `${owner}/${repo}`;
  const eventRun = context.payload.workflow_run;
  if (!eventRun) throw new Error("This workflow must run after Rust CI, not against PR code");

  // Re-read the run so an older successful event cannot bypass a failed rerun.
  const { data: run } = await github.rest.actions.getWorkflowRun({ owner, repo, run_id: eventRun.id });
  if (run.path !== ".github/workflows/ci.yml" || run.event !== "pull_request" ||
      run.status !== "completed" || run.conclusion !== "success" ||
      run.head_repository?.full_name !== fullName || run.head_sha !== eventRun.head_sha ||
      run.run_attempt !== eventRun.run_attempt) {
    core.info("Skipping: the current Rust CI run did not pass for this repository/attempt");
    return;
  }

  let candidates = run.pull_requests || [];
  if (candidates.length === 0) {
    candidates = await github.paginate(github.rest.repos.listPullRequestsAssociatedWithCommit, {
      owner, repo, commit_sha: run.head_sha, per_page: 100,
    });
  }

  for (const number of new Set(candidates.map((pr) => pr.number))) {
    const { data: pr } = await github.rest.pulls.get({ owner, repo, pull_number: number });
    if (pr.state !== "open" || pr.draft || pr.user.login !== "dependabot[bot]" ||
        pr.head.repo?.full_name !== fullName || pr.base.repo?.full_name !== fullName ||
        pr.base.ref !== context.payload.repository.default_branch || pr.head.sha !== run.head_sha) {
      core.info(`Skipping #${number}: not a current, same-repository Dependabot PR for the default branch`);
      continue;
    }

    const { data: commit } = await github.rest.repos.getCommit({ owner, repo, ref: pr.head.sha });
    if (commit.sha !== pr.head.sha || commit.author?.login !== "dependabot[bot]" ||
        commit.commit.verification?.verified !== true || !isMinorOrPatch(commit.commit.message)) {
      core.info(`Skipping #${number}: requires a verified Dependabot minor/patch commit; majors stay manual`);
      continue;
    }

    await github.rest.pulls.createReview({
      owner, repo, pull_number: number, commit_id: pr.head.sha, event: "APPROVE",
      body: "Automatically approved: verified Dependabot minor/patch update; Rust CI passed for this commit.",
    });
    // SHA matching prevents merging an update that arrived after the passing CI run.
    // Native GitHub auto-merge / branch protection is not required for this policy.
    const { data: result } = await github.rest.pulls.merge({
      owner, repo, pull_number: number, sha: pr.head.sha, merge_method: "squash",
    });
    if (!result.merged) throw new Error(`Could not merge #${number}: ${result.message}`);
    core.info(`Squash-merged Dependabot PR #${number} after passing Rust CI`);
  }
}

module.exports = autoMerge;
module.exports.isMinorOrPatch = isMinorOrPatch;
