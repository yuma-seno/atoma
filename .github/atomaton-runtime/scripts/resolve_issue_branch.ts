#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/resolve_issue_branch.ts
import { appendFileSync } from "fs";
import { parseArgs } from "util";

// src/adapters/github/gh.ts
function run(cmd) {
  const proc = Bun.spawnSync({
    cmd,
    stdout: "pipe",
    stderr: "pipe"
  });
  return {
    code: proc.exitCode ?? 1,
    stdout: proc.stdout ? proc.stdout.toString("utf8").trim() : "",
    stderr: proc.stderr ? proc.stderr.toString("utf8").trim() : ""
  };
}
function ghCommand() {
  const fake = (process.env.ATOMATON_FAKE_GH ?? "").trim();
  return fake ? [process.execPath, fake] : ["gh"];
}
function gh(...args) {
  return run([...ghCommand(), ...args]);
}

// src/adapters/github/branch-names.ts
var BRANCH_PREFIX = "atomaton/issue-";
var OWNED_SUFFIX = /^-(\d+)$/;
function ordinalOfBranch(name, issue) {
  const base = `${BRANCH_PREFIX}${issue}`;
  if (!name.startsWith(base))
    return 0;
  const rest = name.slice(base.length);
  if (rest === "")
    return 1;
  const match = OWNED_SUFFIX.exec(rest);
  return match ? Number(match[1]) : 0;
}
function matchingRefsPath(repo, issue) {
  return `repos/${repo}/git/matching-refs/heads/${BRANCH_PREFIX}${issue}`;
}

// src/domain/work/issue-branch.ts
function newestFirst(owned) {
  return [...owned].sort((a, b) => b.ordinal - a.ordinal);
}
function ordinalToResume(owned) {
  return newestFirst(owned).find((branch) => !branch.merged)?.ordinal;
}

// src/adapters/github/issue-branches.ts
function log(message) {
  console.error(`[atomaton-issue-branch] ${message}`);
}
function collectIssueBranches(repo, issueNumber) {
  const refs = gh("api", matchingRefsPath(repo, issueNumber));
  if (refs.code) {
    const why = `could not list the branches of #${issueNumber}: ${(refs.stderr || refs.stdout).trim()}`;
    log(`WARN ${why}`);
    return { known: false, why };
  }
  let listed;
  try {
    const parsed = JSON.parse(refs.stdout || "[]");
    listed = parsed.map((entry) => ({
      name: entry.ref.replace(/^refs\/heads\//, ""),
      tip: entry.object?.sha ?? ""
    }));
  } catch {
    const why = `the branch list for #${issueNumber} was not valid JSON`;
    log(`WARN ${why}`);
    return { known: false, why };
  }
  const owner = repo.split("/", 1)[0] ?? "";
  const branches = listed.map((entry) => ({ ...entry, ordinal: ordinalOfBranch(entry.name, issueNumber) })).filter((entry) => entry.ordinal > 0).map((entry) => ({
    name: entry.name,
    ordinal: entry.ordinal,
    merged: headBranchMerged(repo, owner, entry.name, entry.tip)
  }));
  return { known: true, branches };
}
function headBranchMerged(repo, owner, branch, tip) {
  if (!tip)
    return false;
  const prs = gh("api", `repos/${repo}/pulls?state=all&per_page=100&head=${owner}:${branch}`);
  if (prs.code) {
    log(`WARN could not read pull requests for ${branch}; treating it as unmerged`);
    return false;
  }
  try {
    const parsed = JSON.parse(prs.stdout || "[]");
    return parsed.some((pr) => Boolean(pr.merged_at) && (pr.head?.sha === tip || pr.merge_commit_sha === tip));
  } catch {
    log(`WARN pull request list for ${branch} was not valid JSON`);
    return false;
  }
}
function resumableBranch(branches) {
  const ordinal = ordinalToResume(branches);
  return branches.find((branch) => branch.ordinal === ordinal)?.name ?? "";
}

// src/entrypoints/machinery/lib/script-ref.ts
import { basename } from "path";
import { fileURLToPath } from "url";

// src/domain/machinery/machinery-layout.ts
var USER_ROOT = ".github/atomaton";
var RUNTIME_ROOT = ".github/atomaton-runtime";
var CONFIG_FILE = `${USER_ROOT}/config.yaml`;
var AGENT_DEFINITIONS_DIR = `${USER_ROOT}/agent-definitions`;
var PROMPT_TEMPLATE = `${USER_ROOT}/prompt-template.md`;
var SKILLS_DIR = `${USER_ROOT}/skills`;
var TOOLS_DIR = `${RUNTIME_ROOT}/tools`;
var TOOL_DEFAULTS_FILE = `${TOOLS_DIR}/defaults.yaml`;
var DELEGATES_DIR = `${USER_ROOT}/delegates`;
var DELEGATES_FILE = `${DELEGATES_DIR}/delegates.yaml`;
var TOOL_HOOKS_DIR = `${TOOLS_DIR}/hooks`;
var TOOL_PACKAGES_FILE = `${TOOLS_DIR}/packages.json`;
var RULESETS_DIR = `${USER_ROOT}/rulesets`;
var SCRIPTS_DIR = `${RUNTIME_ROOT}/scripts`;

// src/entrypoints/machinery/lib/script-ref.ts
function defineScript(importMetaUrl) {
  return { runtimePath: `${SCRIPTS_DIR}/${basename(fileURLToPath(importMetaUrl))}` };
}

// src/entrypoints/machinery/resolve_issue_branch.ts
var ref = defineScript(import.meta.url);
function log2(message) {
  console.error(`[atomaton-issue-branch] ${message}`);
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: { repo: { type: "string" }, issue: { type: "string" } }
  });
  const repo = (values.repo ?? "").trim();
  const issue = Number(values.issue);
  const githubOutput = process.env.GITHUB_OUTPUT;
  let branch = "";
  if (repo && Number.isInteger(issue) && issue > 0) {
    const listed = collectIssueBranches(repo, issue);
    if (listed.known)
      branch = resumableBranch(listed.branches);
    else
      log2(`${listed.why}; staying on the base branch`);
  } else {
    log2("missing --repo or --issue; staying on the base branch");
  }
  log2(branch ? `resuming ${branch}` : "no branch to resume; starting from the base branch");
  if (githubOutput)
    appendFileSync(githubOutput, `branch=${branch}
`);
}
if (import.meta.main)
  main();
export {
  ref
};
