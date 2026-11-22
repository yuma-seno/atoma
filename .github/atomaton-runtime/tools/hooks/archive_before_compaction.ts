#!/usr/bin/env bun
// @bun

// src/entrypoints/tools/hooks/archive_before_compaction.ts
import { existsSync as existsSync2, readFileSync } from "fs";

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
function nextArchiveSessionPath(type, number, agent, existingNames) {
  const escapedAgent = agent.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const archivePattern = new RegExp(`^${escapedAgent}-(\\d+)\\.json$`);
  const nextNumber = existingNames.map((name) => archivePattern.exec(name)).filter((match) => match !== null).reduce((max, match) => Math.max(max, Number(match[1])), 0) + 1;
  return `${sessionDir(type, number)}/${ARCHIVE_DIR}/${agent}-${nextNumber}.json`;
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

// src/entrypoints/tools/hooks/archive_before_compaction.ts
async function readStdin() {
  const chunks = [];
  for await (const chunk of Bun.stdin.stream())
    chunks.push(chunk);
  return Buffer.concat(chunks).toString("utf8");
}
async function main() {
  let payload;
  try {
    payload = JSON.parse(await readStdin());
  } catch {
    console.error("archive_before_compaction: stdin was not JSON; nothing archived");
    console.log(JSON.stringify({ archived: false }));
    return;
  }
  const path = payload.session_path ?? "";
  if (!path || !existsSync2(path)) {
    console.error(`archive_before_compaction: no session at ${path || "(no path given)"}; nothing archived`);
    console.log(JSON.stringify({ archived: false }));
    return;
  }
  const type = process.env.TYPE === "pr" ? "pr" : "issue";
  const number = process.env.NUMBER ?? "";
  const agent = payload.agent ?? "unknown";
  if (!number) {
    console.error("archive_before_compaction: NUMBER is not set, so the archive target is unknown; nothing archived");
    console.log(JSON.stringify({ archived: false }));
    return;
  }
  const content = readFileSync(path, "utf8");
  const archivedPath = archiveSession(type, number, agent, content);
  if (!archivedPath) {
    console.error("archive_before_compaction: could not archive to atomaton-data");
    console.log(JSON.stringify({ archived: false }));
    return;
  }
  console.error(`archive_before_compaction: session archived to atomaton-data:${archivedPath}`);
  console.log(JSON.stringify({ archived: true }));
}
await main();
