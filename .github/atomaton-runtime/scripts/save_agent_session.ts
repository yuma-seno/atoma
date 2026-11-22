#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/save_agent_session.ts
import { existsSync as existsSync2, readFileSync } from "fs";
import { parseArgs } from "util";

// src/entrypoints/machinery/lib/atomaton-data.ts
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readlinkSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync
} from "fs";
import { tmpdir } from "os";
import { dirname, join, relative } from "path";

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
function gitRun(...args) {
  return run(["git", ...args]);
}

// src/domain/machinery/data-layout.ts
var SESSIONS_TREE = "sessions/";
function sessionDir(type, number) {
  return `${SESSIONS_TREE}${type}-${number}`;
}

// src/entrypoints/machinery/lib/atomaton-data.ts
var DATA_BRANCH = "atomaton-data";
function sessionTargetPath(type, number, agent) {
  return `${sessionDir(type, number)}/${agent}.json`;
}
function gitIn(cwd, ...args) {
  const proc = Bun.spawnSync({ cmd: ["git", ...args], cwd, stdout: "pipe", stderr: "pipe" });
  return { code: proc.exitCode ?? 1, stdout: proc.stdout ? proc.stdout.toString("utf8").trim() : "" };
}
function saveSession(targetPath, content, commitMessage) {
  if (gitRun("ls-remote", "--exit-code", "origin", DATA_BRANCH).code !== 0) {
    gitRun("config", "user.email", "action@github.com");
    gitRun("config", "user.name", "GitHub Actions");
    const commit = gitRun("commit-tree", "4b825dc642cb6eb9a060e54bf8d69288fbee4904", "-m", `init: ${DATA_BRANCH} session store`).stdout;
    gitRun("push", "origin", `${commit}:refs/heads/${DATA_BRANCH}`);
  }
  gitRun("fetch", "origin", DATA_BRANCH);
  const worktreeDir = mkdtempSync(join(tmpdir(), "atomaton-data-wt-"));
  gitRun("worktree", "add", worktreeDir, `origin/${DATA_BRANCH}`);
  let saved = false;
  try {
    gitIn(worktreeDir, "config", "user.email", "action@github.com");
    gitIn(worktreeDir, "config", "user.name", "GitHub Actions");
    for (let attempt = 1;attempt <= 5; attempt++) {
      gitIn(worktreeDir, "fetch", "origin", DATA_BRANCH);
      gitIn(worktreeDir, "reset", "--hard", `origin/${DATA_BRANCH}`);
      const fullTarget = join(worktreeDir, targetPath);
      mkdirSync(dirname(fullTarget), { recursive: true });
      writeFileSync(fullTarget, content);
      gitIn(worktreeDir, "add", targetPath);
      if (gitIn(worktreeDir, "diff", "--cached", "--quiet").code === 0) {
        saved = true;
        break;
      }
      gitIn(worktreeDir, "commit", "-m", commitMessage);
      if (gitIn(worktreeDir, "push", "origin", `HEAD:${DATA_BRANCH}`).code === 0) {
        saved = true;
        break;
      }
      console.error(`Push attempt ${attempt} failed (concurrent push) -- resetting and retrying with a fresh pull...`);
      Bun.sleepSync(attempt * 2000);
    }
  } finally {
    gitRun("worktree", "remove", "--force", worktreeDir);
    rmSync(worktreeDir, { recursive: true, force: true });
  }
  return saved;
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

// src/entrypoints/machinery/save_agent_session.ts
var ref = defineScript(import.meta.url);
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      session: { type: "string" },
      type: { type: "string" },
      number: { type: "string" },
      agent: { type: "string" }
    }
  });
  if (!values.session || !values.type || !values.number || !values.agent) {
    console.error("usage: save_agent_session.ts --session FILE --type issue|pr --number N --agent NAME");
    process.exit(2);
  }
  if (!existsSync2(values.session)) {
    console.error("No session.json found, skipping save.");
    return;
  }
  const target = sessionTargetPath(values.type, values.number, values.agent);
  const content = readFileSync(values.session, "utf8");
  const runId = process.env.GITHUB_RUN_ID ?? "";
  const saved = saveSession(target, content, `session: ${values.agent} on ${values.type} ${values.number} (run ${runId})`);
  if (!saved) {
    console.log(`::warning::Failed to save session to atomaton-data:${target} after all retries.`);
  } else {
    console.error(`Session saved to atomaton-data:${target}`);
  }
}
if (import.meta.main)
  main();
export {
  ref
};
