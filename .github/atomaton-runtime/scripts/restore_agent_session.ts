#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/restore_agent_session.ts
import { writeFileSync as writeFileSync2 } from "fs";
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
var ARCHIVE_DIR = "archive";
function sessionDir(type, number) {
  return `${SESSIONS_TREE}${type}-${number}`;
}

// src/entrypoints/machinery/lib/atomaton-data.ts
var DATA_BRANCH = "atomaton-data";
function sessionTargetPath(type, number, agent) {
  return `${sessionDir(type, number)}/${agent}.json`;
}
function nextArchiveSessionPath(type, number, agent, existingNames) {
  const escapedAgent = agent.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const archivePattern = new RegExp(`^${escapedAgent}-(\\d+)\\.json$`);
  const nextNumber = existingNames.map((name) => archivePattern.exec(name)).filter((match) => match !== null).reduce((max, match) => Math.max(max, Number(match[1])), 0) + 1;
  return `${sessionDir(type, number)}/${ARCHIVE_DIR}/${agent}-${nextNumber}.json`;
}
function restoreSession(targetPath) {
  if (gitRun("fetch", "origin", DATA_BRANCH, "--depth=1").code !== 0) {
    return;
  }
  if (gitRun("cat-file", "-e", `origin/${DATA_BRANCH}:${targetPath}`).code !== 0) {
    return;
  }
  const shown = gitRun("show", `origin/${DATA_BRANCH}:${targetPath}`);
  return shown.code === 0 ? shown.stdout : undefined;
}
function gitIn(cwd, ...args) {
  const proc = Bun.spawnSync({ cmd: ["git", ...args], cwd, stdout: "pipe", stderr: "pipe" });
  return { code: proc.exitCode ?? 1, stdout: proc.stdout ? proc.stdout.toString("utf8").trim() : "" };
}
function archiveSession(type, number, agent, content) {
  if (gitRun("ls-remote", "--exit-code", "origin", DATA_BRANCH).code !== 0)
    return;
  gitRun("fetch", "origin", DATA_BRANCH);
  const worktreeDir = mkdtempSync(join(tmpdir(), "atomaton-data-archive-wt-"));
  gitRun("worktree", "add", worktreeDir, `origin/${DATA_BRANCH}`);
  let archivedPath;
  try {
    gitIn(worktreeDir, "config", "user.email", "action@github.com");
    gitIn(worktreeDir, "config", "user.name", "GitHub Actions");
    for (let attempt = 1;attempt <= 5; attempt++) {
      gitIn(worktreeDir, "fetch", "origin", DATA_BRANCH);
      gitIn(worktreeDir, "reset", "--hard", `origin/${DATA_BRANCH}`);
      const archiveDir = join(worktreeDir, sessionDir(type, number), ARCHIVE_DIR);
      mkdirSync(archiveDir, { recursive: true });
      const relativePath = nextArchiveSessionPath(type, number, agent, readdirSync(archiveDir));
      const fullPath = join(worktreeDir, relativePath);
      if (existsSync(fullPath))
        continue;
      writeFileSync(fullPath, content);
      gitIn(worktreeDir, "add", relativePath);
      gitIn(worktreeDir, "commit", "-m", `session: archive ${agent} on ${type} ${number}`);
      if (gitIn(worktreeDir, "push", "origin", `HEAD:${DATA_BRANCH}`).code === 0) {
        archivedPath = relativePath;
        break;
      }
      console.error(`Archive push attempt ${attempt} failed -- retrying with the latest ${DATA_BRANCH} branch.`);
      Bun.sleepSync(attempt * 2000);
    }
  } finally {
    gitRun("worktree", "remove", "--force", worktreeDir);
    rmSync(worktreeDir, { recursive: true, force: true });
  }
  return archivedPath;
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

// src/entrypoints/machinery/restore_agent_session.ts
var ref = defineScript(import.meta.url);
function findAgentSession(type, number, agent, load = restoreSession) {
  const target = sessionTargetPath(type, number, agent);
  return { target, content: load(target) };
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      type: { type: "string" },
      number: { type: "string" },
      agent: { type: "string" },
      out: { type: "string" },
      "session-mode": { type: "string" }
    }
  });
  if (!values.type || !values.number || !values.agent || !values.out) {
    console.error("usage: restore_agent_session.ts --type issue|pr --number N --agent NAME --out session.json");
    process.exit(2);
  }
  const { target, content } = findAgentSession(values.type, values.number, values.agent);
  if (values["session-mode"] === "recover") {
    if (content === undefined) {
      console.error(`No existing session at ${target}; recovery will start fresh without an archive.`);
      return;
    }
    const archivedPath = archiveSession(values.type, values.number, values.agent, content);
    if (!archivedPath)
      throw new Error(`Failed to archive existing session before recovery: ${target}`);
    console.error(`Archived session to atomaton-data:${archivedPath}; starting fresh.`);
    return;
  }
  if (content !== undefined) {
    writeFileSync2(values.out, content);
    console.error(`Restored session: ${target}`);
  } else {
    console.error(`No existing session at ${target}, starting fresh.`);
  }
}
if (import.meta.main)
  main();
export {
  findAgentSession,
  ref
};
