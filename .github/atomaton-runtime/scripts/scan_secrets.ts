#!/usr/bin/env bun
// @bun

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
function gitRun(...args) {
  return run(["git", ...args]);
}

// src/adapters/runner/config.ts
import { readFileSync } from "fs";

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

// src/adapters/runner/machinery.ts
function machineryRoot() {
  return process.env[MACHINERY_ROOT_VAR]?.trim() || undefined;
}
function machineryPath(relative) {
  const root = machineryRoot();
  return root ? `${root}/${relative}` : relative;
}

// src/adapters/runner/config.ts
function configPath() {
  return machineryPath(CONFIG_FILE);
}
var cached;
function loadConfig() {
  if (!cached) {
    cached = Bun.YAML.parse(readFileSync(configPath(), "utf8"));
  }
  return cached;
}
function getBaseBranch(fallback = "") {
  try {
    return loadConfig().base_branch?.trim() || fallback;
  } catch (error) {
    if (error.code === "ENOENT")
      return fallback;
    throw error;
  }
}

// src/adapters/github/branch-reads.ts
function log(message) {
  console.error(`[atomaton-branch-reads] ${message}`);
}
function defaultBranchFrom(lsRemoteSymref) {
  return /^ref:\s+refs\/heads\/([^\s]+)\s+HEAD$/m.exec(lsRemoteSymref)?.[1];
}
function readRemoteDefaultBranch() {
  const symref = gitRun("ls-remote", "--symref", "origin", "HEAD");
  if (symref.code !== 0) {
    const why = `could not read origin, so its default branch is unknown: ${symref.stderr || symref.stdout}`;
    log(`WARN ${why}`);
    return { known: false, why };
  }
  const branch = defaultBranchFrom(symref.stdout);
  if (!branch) {
    const why = "origin named no default branch (its HEAD is not a symbolic ref)";
    log(`WARN ${why}`);
    return { known: false, why };
  }
  return { known: true, branch };
}
function baseBranch() {
  const configured = getBaseBranch().trim();
  if (configured)
    return { known: true, branch: configured };
  return readRemoteDefaultBranch();
}

// src/entrypoints/machinery/lib/script-ref.ts
import { basename } from "path";
import { fileURLToPath } from "url";
function defineScript(importMetaUrl) {
  return { runtimePath: `${SCRIPTS_DIR}/${basename(fileURLToPath(importMetaUrl))}` };
}

// src/entrypoints/machinery/scan_secrets.ts
var ref = defineScript(import.meta.url);
function log2(message) {
  console.error(`[scan-secrets] ${message}`);
}
function baseBranch2() {
  const fromEvent = (process.env.GITHUB_BASE_REF ?? "").trim();
  if (fromEvent)
    return fromEvent;
  const base = baseBranch();
  if (base.known)
    return base.branch;
  log2(`${base.why}; no base branch could be determined`);
  return "";
}
function branchRange() {
  gitRun("fetch", "--quiet", "--unshallow", "origin");
  const base = baseBranch2();
  if (!base) {
    log2("no base branch could be determined; scanning nothing rather than guessing at a range");
    return;
  }
  if (gitRun("fetch", "--quiet", "origin", base).code !== 0) {
    log2(`could not fetch ${base}; scanning nothing rather than guessing at a range`);
    return;
  }
  const mergeBase = gitRun("merge-base", "FETCH_HEAD", "HEAD");
  if (mergeBase.code !== 0 || !mergeBase.stdout) {
    log2(`no merge base with ${base}; scanning nothing rather than guessing at a range`);
    return;
  }
  return `${mergeBase.stdout.trim()}..HEAD`;
}
function run2(cmd) {
  return Bun.spawnSync({ cmd, stdout: "inherit", stderr: "inherit" }).exitCode ?? 1;
}
function main() {
  const range = branchRange();
  if (range === undefined)
    return;
  console.log(`Scanning ${range}`);
  const release = gh("api", "repos/gitleaks/gitleaks/releases/latest", "--jq", ".tag_name");
  if (release.code !== 0 || !release.stdout.trim()) {
    console.log("::warning::could not resolve the latest gitleaks release, so this branch was not scanned for credentials");
    return;
  }
  const tag = release.stdout.trim();
  const url = `https://github.com/gitleaks/gitleaks/releases/download/${tag}/gitleaks_${tag.replace(/^v/, "")}_linux_x64.tar.gz`;
  if (run2(["bash", "-c", `curl -sSfL "${url}" | tar -xz -C /tmp gitleaks`]) !== 0) {
    console.log(`::warning::could not download gitleaks ${tag}, so this branch was not scanned for credentials`);
    return;
  }
  const found = run2(["/tmp/gitleaks", "git", `--log-opts=${range}`, "--redact", "--verbose", "--no-banner", "."]);
  if (found !== 0) {
    console.error("::error::gitleaks found a credential in this branch's commits. The output above says where.");
    process.exit(found);
  }
}
if (import.meta.main)
  main();
export {
  ref
};
