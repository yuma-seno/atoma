#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/save_workspace.ts
import { existsSync as existsSync2, readdirSync as readdirSync2 } from "fs";
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
var WORKSPACE_TREE = "workspace/";
function workspaceDir(rootIssue) {
  return `${WORKSPACE_TREE}issue-${rootIssue}`;
}

// src/entrypoints/machinery/lib/atomaton-data.ts
var DATA_BRANCH = "atomaton-data";
function gitIn(cwd, ...args) {
  const proc = Bun.spawnSync({ cmd: ["git", ...args], cwd, stdout: "pipe", stderr: "pipe" });
  return { code: proc.exitCode ?? 1, stdout: proc.stdout ? proc.stdout.toString("utf8").trim() : "" };
}
function workspaceTargetPrefix(rootIssue) {
  return workspaceDir(rootIssue);
}
var DIRECTORY_MODE = 493;
var FILE_MODE = 420;
var EXECUTABLE_FILE_MODE = 493;
function copyWorkspace(source, destination) {
  const skipped = [];
  const copy = (from, to) => {
    let entries;
    try {
      entries = readdirSync(from, { withFileTypes: true });
    } catch {
      skipped.push(relative(source, from) || ".");
      return false;
    }
    mkdirSync(to, { recursive: true });
    chmodSync(to, DIRECTORY_MODE);
    for (const entry of entries) {
      if (entry.name === ".git")
        continue;
      const sourcePath = join(from, entry.name);
      const targetPath = join(to, entry.name);
      try {
        if (entry.isDirectory()) {
          copy(sourcePath, targetPath);
        } else if (entry.isSymbolicLink()) {
          symlinkSync(readlinkSync(sourcePath), targetPath);
        } else if (entry.isFile()) {
          copyFileSync(sourcePath, targetPath);
          const executable = (statSync(sourcePath).mode & 73) !== 0;
          chmodSync(targetPath, executable ? EXECUTABLE_FILE_MODE : FILE_MODE);
        } else {
          skipped.push(relative(source, sourcePath));
        }
      } catch {
        rmSync(targetPath, { recursive: true, force: true });
        skipped.push(relative(source, sourcePath));
      }
    }
    return true;
  };
  return copy(source, destination) ? skipped : undefined;
}
function saveWorkspace(prefix, sourceDir, commitMessage) {
  if (!existsSync(sourceDir))
    return true;
  if (gitRun("ls-remote", "--exit-code", "origin", DATA_BRANCH).code !== 0) {
    console.error(`[${DATA_BRANCH}] ${DATA_BRANCH} does not exist yet; skipping workspace save`);
    return false;
  }
  gitRun("fetch", "origin", DATA_BRANCH);
  const worktreeDir = mkdtempSync(join(tmpdir(), "atomaton-data-ws-"));
  gitRun("worktree", "add", worktreeDir, `origin/${DATA_BRANCH}`);
  let saved = false;
  try {
    gitIn(worktreeDir, "config", "user.email", "action@github.com");
    gitIn(worktreeDir, "config", "user.name", "GitHub Actions");
    for (let attempt = 1;attempt <= 5; attempt++) {
      gitIn(worktreeDir, "fetch", "origin", DATA_BRANCH);
      gitIn(worktreeDir, "reset", "--hard", `origin/${DATA_BRANCH}`);
      const target = join(worktreeDir, prefix);
      rmSync(target, { recursive: true, force: true });
      mkdirSync(dirname(target), { recursive: true });
      const skipped = copyWorkspace(sourceDir, target);
      if (skipped === undefined) {
        console.error(`[${DATA_BRANCH}] cannot read ${sourceDir}; leaving the previous workspace snapshot alone`);
        break;
      }
      for (const path of skipped) {
        console.error(`::warning::workspace snapshot: left ${path} out, because the user saving it cannot read it`);
      }
      gitIn(worktreeDir, "rm", "-r", "--cached", "--quiet", "--ignore-unmatch", "--", prefix);
      gitIn(worktreeDir, "add", "--all", "--", prefix);
      if (gitIn(worktreeDir, "diff", "--cached", "--quiet").code === 0) {
        saved = true;
        break;
      }
      gitIn(worktreeDir, "commit", "-m", commitMessage);
      if (gitIn(worktreeDir, "push", "origin", `HEAD:${DATA_BRANCH}`).code === 0) {
        saved = true;
        break;
      }
      console.error(`[${DATA_BRANCH}] workspace push attempt ${attempt} failed (concurrent push) -- retrying`);
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

// src/entrypoints/machinery/save_workspace.ts
var ref = defineScript(import.meta.url);
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      "root-issue": { type: "string" },
      source: { type: "string" },
      agent: { type: "string" }
    }
  });
  const root = values["root-issue"];
  const source = values.source;
  if (!root || !source) {
    console.error("usage: save_workspace.ts --root-issue N --source PATH [--agent NAME]");
    process.exit(2);
  }
  if (!existsSync2(source)) {
    console.error(`[atomaton-workspace] ${source} does not exist; nothing to save`);
    return;
  }
  const prefix = workspaceTargetPrefix(root);
  let count = 0;
  let saved = false;
  try {
    count = readdirSync2(source).length;
    saved = saveWorkspace(prefix, source, `atomaton: workspace for issue #${root}${values.agent ? ` (${values.agent})` : ""} \u2014 ${count} entries`);
  } catch (error) {
    console.error(`[atomaton-workspace] could not save ${prefix}: ${error.message}`);
  }
  console.error(saved ? `[atomaton-workspace] saved ${count} entries to ${prefix}` : `[atomaton-workspace] WARN could not save ${prefix}; the next run on this issue starts without these files`);
}
if (import.meta.main)
  main();
export {
  ref
};
