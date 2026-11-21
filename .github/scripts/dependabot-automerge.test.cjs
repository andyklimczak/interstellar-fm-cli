"use strict";
const assert = require("node:assert/strict");
const { test } = require("node:test");
const autoMerge = require("./dependabot-automerge.cjs");
const { isMinorOrPatch } = autoMerge;

function message(...types) {
  return `Update dependencies\n\n---\nupdated-dependencies:\n${types.map((type, i) =>
    `- dependency-name: dependency-${i}\n  dependency-type: direct:production\n  update-type: version-update:semver-${type}\n`).join("")}...\n\nSigned-off-by: dependabot[bot] <support@github.com>`;
}

function fixture() {
  const sha = "a".repeat(40);
  const repo = { full_name: "owner/radio" };
  const run = {
    id: 10, path: ".github/workflows/ci.yml", event: "pull_request",
    status: "completed", conclusion: "success", run_attempt: 1,
    head_sha: sha, head_repository: repo, pull_requests: [{ number: 42 }],
  };
  const pr = {
    state: "open", draft: false, user: { login: "dependabot[bot]" },
    head: { sha, repo }, base: { ref: "master", repo },
  };
  const commit = {
    sha, author: { login: "dependabot[bot]" },
    commit: { verification: { verified: true }, message: message("minor", "patch") },
  };
  const calls = [];
  const github = {
    rest: {
      actions: { getWorkflowRun: async () => ({ data: run }) },
      repos: {
        getCommit: async () => ({ data: commit }),
        listPullRequestsAssociatedWithCommit: () => {},
      },
      pulls: {
        get: async () => ({ data: pr }),
        createReview: async (args) => { calls.push(["review", args]); },
        merge: async (args) => { calls.push(["merge", args]); return { data: { merged: true } }; },
      },
    },
    paginate: async () => [{ number: 42 }],
  };
  const context = {
    repo: { owner: "owner", repo: "radio" },
    payload: { workflow_run: structuredClone(run), repository: { default_branch: "master" } },
  };
  const core = { info: () => {} };
  return { github, context, core, run, pr, commit, calls };
}

test("only complete minor/patch metadata is eligible, including grouped updates", () => {
  assert.equal(isMinorOrPatch(message("patch")), true);
  assert.equal(isMinorOrPatch(message("minor", "patch")), true);
  assert.equal(isMinorOrPatch(message("minor").replace("version-update:semver-minor", '"version-update:semver-minor"')), true);
  for (const text of ["", "not Dependabot metadata", message(), message("major"),
    message("patch", "major"), message("unknown"),
    message("minor", "patch").replace("  update-type: version-update:semver-patch\n", "")]) {
    assert.equal(isMinorOrPatch(text), false);
  }
});

test("approves and squash-merges exactly the verified, passing head SHA", async () => {
  const f = fixture();
  await autoMerge(f);
  assert.deepEqual(f.calls.map(([action]) => action), ["review", "merge"]);
  assert.equal(f.calls[0][1].event, "APPROVE");
  assert.equal(f.calls[0][1].commit_id, f.pr.head.sha);
  assert.equal(f.calls[1][1].sha, f.pr.head.sha);
  assert.equal(f.calls[1][1].merge_method, "squash");
});

test("resolves PRs by commit when workflow_run omits its PR list", async () => {
  const f = fixture(); f.run.pull_requests = [];
  await autoMerge(f);
  assert.equal(f.calls.length, 2);
});

for (const [reason, modify] of [
  ["failed CI", (f) => { f.run.conclusion = "failure"; }],
  ["incomplete CI", (f) => { f.run.status = "in_progress"; }],
  ["a stale successful event before a rerun", (f) => { f.run.run_attempt = 2; }],
  ["another workflow", (f) => { f.run.path = ".github/workflows/unrelated.yml"; }],
  ["a push rather than a PR run", (f) => { f.run.event = "push"; }],
  ["a fork CI run", (f) => { f.run.head_repository = { full_name: "fork/radio" }; }],
  ["a human PR", (f) => { f.pr.user.login = "human"; }],
  ["a fork PR", (f) => { f.pr.head.repo = { full_name: "fork/radio" }; }],
  ["another base branch", (f) => { f.pr.base.ref = "other"; }],
  ["a draft", (f) => { f.pr.draft = true; }],
  ["a closed PR", (f) => { f.pr.state = "closed"; }],
  ["an updated head after CI", (f) => { f.pr.head.sha = "b".repeat(40); }],
  ["a major update", (f) => { f.commit.commit.message = message("major"); }],
  ["missing metadata", (f) => { f.commit.commit.message = "Update dependencies"; }],
  ["an unsigned commit", (f) => { f.commit.commit.verification.verified = false; }],
  ["a non-bot commit", (f) => { f.commit.author.login = "human"; }],
]) {
  test(`does not approve or merge ${reason}`, async () => {
    const f = fixture(); modify(f);
    await autoMerge(f);
    assert.deepEqual(f.calls, []);
  });
}

test("surfaces a rejected merge instead of claiming success", async () => {
  const f = fixture();
  f.github.rest.pulls.merge = async () => ({ data: { merged: false, message: "Head changed" } });
  await assert.rejects(() => autoMerge(f), /Head changed/);
});
