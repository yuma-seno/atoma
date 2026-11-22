#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/resolve_pr_branch.ts
import { appendFileSync } from "fs";
import { parseArgs } from "util";

// src/domain/work/mention.ts
var LOGIN_PATTERN = "[A-Za-z0-9](?:[A-Za-z0-9]|-(?=[A-Za-z0-9])){0,38}";
var MENTION = new RegExp(`(^|[^\\w@/-])@(${LOGIN_PATTERN})\\b(?!\\/)`, "g");

// src/domain/work/pr-head-branch.ts
function describePrHeadRefusal(number, refusal) {
  switch (refusal.kind) {
    case "unreadable":
      return `pull request #${number} could not be read: ${refusal.detail}`;
    case "fork":
      return `pull request #${number} comes from a fork, so its head branch is a branch of ` + "the fork rather than of this repository";
    case "no-branch":
      return `pull request #${number} names no head branch`;
    case "not-a-branch-name":
      return `pull request #${number} names '${refusal.branch}', which is not a branch name`;
    case "not-on-origin":
      return `pull request #${number}'s head branch '${refusal.branch}' is not a branch of this repository`;
    case "origin-unreadable":
      return `pull request #${number}'s head branch could not be checked against this repository: ${refusal.detail}`;
  }
}

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
function ghRead(...args) {
  let result = gh(...args);
  for (const delay of [2000, 6000]) {
    if (result.code === 0 || !looksTransient(result))
      return result;
    console.error(`::warning::gh ${args.slice(0, 2).join(" ")} failed transiently, retrying: ${result.stderr || result.stdout}`);
    Bun.sleepSync(delay);
    result = gh(...args);
  }
  return result;
}
function looksTransient(result) {
  const text = `${result.stderr} ${result.stdout}`;
  if (/HTTP (429|5[0-9][0-9])(?![0-9])/.test(text))
    return true;
  return /(timeout|timed out|connection reset|unexpected EOF|TLS handshake|temporary failure)/i.test(text);
}
function gitRun(...args) {
  return run(["git", ...args]);
}

// src/domain/delivery/merge-readiness.ts
var CI_WOULD_BE_WASTED = new Set([
  "not-open",
  "draft",
  "conflicting",
  "behind",
  "mergeability-unknown",
  "checks-pending",
  "checks-failing"
]);
var PASSING = new Set(["success", "neutral", "skipped"]);

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
var MACHINERY_ROOT_VAR = "ATOMATON_MACHINERY_ROOT";

// src/domain/delivery/declared-secrets.ts
var RUN_CREDENTIALS = [
  "OPENAI_API_KEY",
  "OPENROUTER_API_KEY",
  "ORCAROUTER_API_KEY",
  "ANTHROPIC_API_KEY",
  "ATOMA_COPILOT_TOKEN",
  "GH_TOKEN"
];
var AGENT_ENV_NAMES = [
  "HOME",
  "PATH",
  "AGENT",
  "ATOMATON_UNTRUSTED_USER",
  MACHINERY_ROOT_VAR,
  "GITHUB_REPOSITORY",
  "BRANCH",
  "NUMBER",
  "NOTIFY",
  "TYPE",
  "ATOMATON_RELOAD_COUNT",
  "ATOMATON_OPS_LOG",
  "ATOMATON_RUN_DIR",
  "ATOMATON_RUN_URL",
  "ATOMATON_DISPATCHED_BY",
  "ATOMATON_ASK_ID",
  "GITHUB_RUN_ID",
  "XDG_CACHE_HOME",
  "XDG_CONFIG_HOME",
  "XDG_DATA_HOME",
  "BUN_INSTALL_CACHE_DIR",
  "npm_config_cache",
  "PIP_CACHE_DIR",
  "CARGO_HOME",
  "OPENAI_BASE_URL",
  "ATOMA_PROVIDER"
];
var RUN_STEP_NAMES = [
  "OPENROUTER_BASE_URL",
  "ORCAROUTER_BASE_URL",
  "ANTHROPIC_BASE_URL",
  "COPILOT_BASE_URL",
  "ATOMA_PROVIDER_IN",
  "OPENAI_BASE_URL_IN"
];
var TOOL_SECRETS = {
  field: "tools.secrets",
  reserved: new Set([...RUN_CREDENTIALS, ...AGENT_ENV_NAMES, ...RUN_STEP_NAMES])
};
var JOB_ENV = ["ATOMATON_COMMANDS", "GH_TOKEN"];
var CHECK_JOB_RESERVED = new Set([...JOB_ENV, "ATOMATON_PR_TREE"]);
var DEPLOY_JOB_RESERVED = new Set([...JOB_ENV, "ATOMATON_DEPLOY_TARGET"]);

// src/domain/delivery/check-jobs.ts
var CHECKS_FROM_PULL_REQUEST = {
  where: "checks.from_pull_request",
  secrets: {
    refused: "These commands come from the pull request, which may rewrite them, so a credential " + "named beside them is one the change being judged can read. Move the check to " + "`checks.from_default_branch`, where the commands come from a branch a person approved."
  }
};
var NO_PULL_REQUEST_CHECKS = "This check verified nothing: `checks.from_pull_request` in .github/atomaton/config.yaml is empty, " + "so a pull request satisfying it has not been tested. Add the commands that check this project, " + "or point `checks.your_workflow` at a workflow of your own.";

// src/adapters/github/branch-reads.ts
function log(message) {
  console.error(`[atomaton-branch-reads] ${message}`);
}
function readOriginBranch(branch) {
  const { code, stdout, stderr } = gitRun("ls-remote", "--heads", "origin", `refs/heads/${branch}`);
  if (code !== 0) {
    const why = `could not read '${branch}' from origin: ${stderr || stdout || `git ls-remote exited ${code}`}`;
    log(`WARN ${why}`);
    return { known: false, why };
  }
  const wanted = `refs/heads/${branch}`;
  for (const line of stdout.split(`
`)) {
    const [sha, ref] = line.trim().split(/\s+/);
    if (ref === wanted && sha)
      return { known: true, present: true, sha };
  }
  return { known: true, present: false };
}

// src/adapters/github/pull-request.ts
function viewArgs(repo, number, fields) {
  const json = typeof fields === "string" ? fields : fields.join(",");
  return ["pr", "view", String(number), "--repo", repo, "--json", json];
}
function whyNot(what, result) {
  return `${what}: ${result.stderr || result.stdout || `gh exited ${result.code}`}`;
}
function readPullRequest(repo, number, fields) {
  const what = `gh pr view ${number} --repo ${repo}`;
  const result = ghRead(...viewArgs(repo, number, fields));
  if (result.code !== 0)
    return { known: false, why: whyNot(what, result) };
  if (!result.stdout.trim())
    return { known: false, why: `${what}: gh printed nothing` };
  try {
    return { known: true, value: JSON.parse(result.stdout) };
  } catch {
    return { known: false, why: `${what}: the answer was not JSON` };
  }
}

// src/entrypoints/machinery/lib/pr-head-branch.ts
var PR_HEAD_FIELDS = "headRefName,isCrossRepository";
function prHead(pr) {
  return {
    branch: typeof pr?.headRefName === "string" ? pr.headRefName.trim() : "",
    crossRepository: pr?.isCrossRepository === true
  };
}
function isBranchName(name) {
  if (!name)
    return false;
  return gitRun("check-ref-format", "--branch", name).code === 0;
}
function resolvePrHeadBranch(repo, number) {
  const read = readPullRequest(repo, number, PR_HEAD_FIELDS);
  if (!read.known) {
    return { branch: "", refusal: { kind: "unreadable", detail: read.why } };
  }
  const head = prHead(read.value);
  if (head.crossRepository)
    return { branch: "", refusal: { kind: "fork" } };
  if (!head.branch)
    return { branch: "", refusal: { kind: "no-branch" } };
  if (!isBranchName(head.branch)) {
    return { branch: "", refusal: { kind: "not-a-branch-name", branch: head.branch } };
  }
  const onOrigin = readOriginBranch(head.branch);
  if (!onOrigin.known) {
    return { branch: "", refusal: { kind: "origin-unreadable", detail: onOrigin.why } };
  }
  if (!onOrigin.present) {
    return { branch: "", refusal: { kind: "not-on-origin", branch: head.branch } };
  }
  return { branch: head.branch, refusal: null };
}

// src/entrypoints/machinery/lib/script-ref.ts
import { basename } from "path";
import { fileURLToPath } from "url";
function defineScript(importMetaUrl) {
  return { runtimePath: `${SCRIPTS_DIR}/${basename(fileURLToPath(importMetaUrl))}` };
}

// src/entrypoints/machinery/resolve_pr_branch.ts
var ref = defineScript(import.meta.url);
function log2(message) {
  console.error(`[atomaton-pr-branch] ${message}`);
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: { repo: { type: "string" }, number: { type: "string" } }
  });
  const repo = (values.repo ?? "").trim();
  const number = Number((values.number ?? "").trim());
  const githubOutput = process.env.GITHUB_OUTPUT;
  let branch = "";
  if (!repo || !Number.isInteger(number) || number <= 0) {
    log2("missing --repo or --number; this run has no branch to check out");
  } else {
    const resolved = resolvePrHeadBranch(repo, number);
    if (resolved.refusal)
      log2(describePrHeadRefusal(number, resolved.refusal));
    branch = resolved.branch;
  }
  log2(branch ? `the pull request's head branch is ${branch}` : "no head branch to check out; staying on the detached checkout");
  if (githubOutput)
    appendFileSync(githubOutput, `branch=${branch}
`);
}
if (import.meta.main)
  main();
export {
  ref
};
