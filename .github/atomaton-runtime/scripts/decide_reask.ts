#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/decide_reask.ts
import { spawnSync as spawnSync2 } from "child_process";
import { copyFileSync, existsSync, readFileSync as readFileSync2, rmSync, writeFileSync as writeFileSync2 } from "fs";
import { join as join2 } from "path";
import { parseArgs } from "util";

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
var TOOL_USER = "atomaton-tools";

// src/adapters/runner/machinery.ts
function machineryRoot() {
  return process.env[MACHINERY_ROOT_VAR]?.trim() || undefined;
}

// src/entrypoints/machinery/lib/agent-env.ts
import { readFileSync } from "fs";
function buildAgentEnv(source, paths) {
  const entries = [
    ["HOME", source.HOME ?? ""],
    ["PATH", source.PATH ?? ""],
    ["AGENT", source.AGENT ?? ""],
    [MACHINERY_ROOT_VAR, source[MACHINERY_ROOT_VAR] ?? ""],
    ["ATOMATON_UNTRUSTED_USER", source.ATOMATON_UNTRUSTED_USER ?? ""],
    ["GITHUB_REPOSITORY", source.GITHUB_REPOSITORY ?? ""],
    ["BRANCH", source.BRANCH ?? ""],
    ["NUMBER", source.NUMBER ?? ""],
    ["NOTIFY", source.NOTIFY ?? ""],
    ["TYPE", source.TYPE ?? ""],
    ["ATOMATON_RELOAD_COUNT", source.ATOMATON_RELOAD_COUNT ?? ""],
    ["ATOMATON_DISPATCHED_BY", source.ATOMATON_DISPATCHED_BY ?? ""],
    ["ATOMATON_ASK_ID", source.ATOMATON_ASK_ID ?? ""],
    ["GITHUB_RUN_ID", source.GITHUB_RUN_ID ?? ""],
    ["ATOMATON_OPS_LOG", source.ATOMATON_OPS_LOG ?? ""],
    ["ATOMATON_RUN_DIR", paths.runDir],
    ["ATOMATON_RUN_URL", `${source.GITHUB_SERVER_URL ?? ""}/${source.GITHUB_REPOSITORY ?? ""}/actions/runs/${source.GITHUB_RUN_ID ?? ""}`],
    ["XDG_CACHE_HOME", paths.toolCache],
    ["XDG_CONFIG_HOME", `${paths.toolCache}/config`],
    ["XDG_DATA_HOME", `${paths.toolCache}/data`],
    ["BUN_INSTALL_CACHE_DIR", `${paths.toolCache}/bun`],
    ["npm_config_cache", `${paths.toolCache}/npm`],
    ["PIP_CACHE_DIR", `${paths.toolCache}/pip`],
    ["CARGO_HOME", `${paths.toolCache}/cargo`]
  ];
  for (const name of ["OPENAI_BASE_URL", "ATOMA_PROVIDER"]) {
    const value = source[`${name}_IN`];
    if (value !== undefined && value !== "")
      entries.push([name, value]);
  }
  return entries.map(([name, value]) => `${name}=${value}`);
}
function agentBudget(deadlineAt, now, minBudgetSecs) {
  const left = Math.floor(deadlineAt - now);
  if (left < minBudgetSecs)
    return { budget: minBudgetSecs, floored: true };
  return { budget: left, floored: false };
}
function changedSoFar(opsLogPath) {
  if (!opsLogPath)
    return false;
  let content;
  try {
    content = readFileSync(opsLogPath, "utf8");
  } catch {
    return false;
  }
  return content.split(`
`).some((line) => /"op":"(commit_and_push|create_pr|merge_pr)"/.test(line) || /"op":"merge_base_into_branch".*"status":"merged"/.test(line));
}

// src/entrypoints/machinery/lib/atoma-launch.ts
import { spawnSync } from "child_process";
import { appendFileSync, writeFileSync } from "fs";
import { join } from "path";
function launchAtoma(opts) {
  const argv = [
    "sudo",
    "-n",
    "-u",
    TOOL_USER,
    "env",
    ...opts.envEntries,
    "atoma",
    "run",
    "--agent-def",
    join(opts.machineryRoot, AGENT_DEFINITIONS_DIR, `${opts.agent}.md`),
    "--in-session",
    opts.session,
    "--out-session",
    opts.session,
    "--template",
    join(opts.machineryRoot, PROMPT_TEMPLATE),
    "--skills-dir",
    join(opts.machineryRoot, SKILLS_DIR),
    "--max-runtime-secs",
    String(opts.budgetSecs),
    "--stop-file",
    opts.stopFile,
    "--loop-retries",
    "--credentials-file",
    opts.credentialsFile,
    "--tools-file",
    opts.toolsFile
  ];
  const run = spawnSync(argv[0], argv.slice(1), {
    encoding: "buffer"
  });
  let status = run.status;
  if (run.error) {
    const code = run.error.code === "ENOENT" ? 127 : run.error.code === "EACCES" ? 126 : 1;
    const line = `${argv[0]}: ${run.error.message}`;
    if (opts.appendLogs)
      appendFileSync(opts.logsFile, `${line}
`);
    else
      writeFileSync(opts.logsFile, `${line}
`);
    status = code;
  } else if (status === null && run.signal === "SIGTERM") {
    status = 143;
  }
  writeFileSync(opts.outFile, run.stdout?.toString() ?? "");
  if (opts.appendLogs)
    appendFileSync(opts.logsFile, run.stderr?.toString() ?? "");
  else
    writeFileSync(opts.logsFile, run.stderr?.toString() ?? "");
  return status ?? 1;
}

// src/entrypoints/machinery/lib/script-ref.ts
import { basename } from "path";
import { fileURLToPath } from "url";
function defineScript(importMetaUrl) {
  return { runtimePath: `${SCRIPTS_DIR}/${basename(fileURLToPath(importMetaUrl))}` };
}

// src/entrypoints/machinery/decide_reask.ts
var ref = defineScript(import.meta.url);
function sibling(script, args) {
  const run = spawnSync2("bun", ["run", join2(import.meta.dir, script), ...args], { stdio: "inherit" });
  if (run.status !== 0) {
    throw new Error(`decide_reask: ${script} exited ${run.status}`);
  }
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      "run-dir": { type: "string" },
      "tool-cache": { type: "string" },
      "tools-file": { type: "string" },
      "credentials-file": { type: "string" },
      "stop-file": { type: "string" },
      "deadline-at": { type: "string" },
      "min-budget-secs": { type: "string" },
      "exit-code": { type: "string" }
    }
  });
  for (const required of ["run-dir", "tool-cache", "tools-file", "credentials-file", "stop-file", "deadline-at", "min-budget-secs", "exit-code"]) {
    if (values[required] === undefined) {
      console.error(`decide_reask: --${required} is required`);
      process.exit(2);
    }
  }
  const runDir = values["run-dir"];
  const session = join2(runDir, "session.json");
  const sessionBefore = join2(runDir, "session_before_reask.json");
  const output = join2(runDir, "atomaton_output.txt");
  const outputAgain = join2(runDir, "atomaton_output_again.txt");
  const logs = join2(runDir, "atomaton_logs.txt");
  const problemFile = join2(runDir, "ending-problem.txt");
  const peopleFile = join2(runDir, "callees.json");
  const stopFile = values["stop-file"];
  const root = machineryRoot() ?? ".";
  const checkArgs = ["--output-file", output, "--def-dir", join2(root, AGENT_DEFINITIONS_DIR), "--problem-file", problemFile, "--people-file", peopleFile];
  const envEntries = buildAgentEnv(process.env, {
    toolCache: values["tool-cache"],
    runDir
  });
  const launchArgs = {
    agent: process.env.AGENT ?? "",
    machineryRoot: root,
    toolsFile: values["tools-file"],
    credentialsFile: values["credentials-file"],
    stopFile,
    session,
    logsFile: logs,
    appendLogs: true,
    envEntries
  };
  let exitCode = values["exit-code"];
  let endingInvalid = false;
  let reask = "";
  if (exitCode === "0" && !existsSync(stopFile)) {
    sibling("check_ending.ts", checkArgs);
    if (readFileSync2(problemFile, "utf8").length > 0) {
      reask = "ending";
    } else {
      const stalled = spawnSync2("bun", ["run", join2(import.meta.dir, "should_continue_stalled.ts"), "--session", session, "--output-file", output, "--def-dir", join2(root, AGENT_DEFINITIONS_DIR), "--people-file", peopleFile, "--changed", String(changedSoFar(join2(runDir, "atomaton_ops.log")))], { encoding: "utf8" });
      if (stalled.status === 0 && stalled.stdout.trim() === "true")
        reask = "stalled";
    }
  }
  if (reask !== "") {
    const { budget: reaskBudget, floored } = agentBudget(Number(values["deadline-at"]), Date.now() / 1000, Number(values["min-budget-secs"]));
    if (floored) {
      if (reask === "ending") {
        console.error("::warning::the closing message calls more than one thing and there is no time left to ask again; nothing it calls will be started");
        endingInvalid = true;
      } else {
        console.error("::warning::the run stopped on a sentence having changed nothing, and there is no time left to ask again");
      }
    } else {
      copyFileSync(session, sessionBefore);
      if (reask === "ending") {
        console.error("::warning::the closing message calls more than one thing; asking for it again");
        sibling("inject_ending_rejection.ts", ["--session", session, "--problem-file", problemFile]);
      } else {
        console.error("::warning::the run stopped on a sentence having changed nothing; asking for it again");
        sibling("inject_stalled_continuation.ts", ["--session", session]);
      }
      const secondExit = launchAtoma({
        ...launchArgs,
        outFile: outputAgain,
        budgetSecs: reaskBudget
      });
      if (secondExit === 0 && existsSync(outputAgain) && readFileSync2(outputAgain, "utf8").length > 0) {
        rmSync(output, { force: true });
        copyFileSync(outputAgain, output);
        rmSync(outputAgain, { force: true });
        sibling("check_ending.ts", checkArgs);
        if (readFileSync2(problemFile, "utf8").length > 0) {
          console.error("::warning::the closing message still calls more than one thing; nothing it calls will be started");
          endingInvalid = true;
        }
      } else if (existsSync(stopFile)) {
        exitCode = "2";
        if (existsSync(outputAgain) && readFileSync2(outputAgain, "utf8").length > 0) {
          rmSync(output, { force: true });
          copyFileSync(outputAgain, output);
        } else {
          writeFileSync2(output, "");
        }
        rmSync(outputAgain, { force: true });
      } else {
        copyFileSync(sessionBefore, session);
        if (reask === "ending")
          endingInvalid = true;
        rmSync(outputAgain, { force: true });
      }
    }
  }
  console.error(`decide_reask: exit_code=${exitCode} ending_invalid=${endingInvalid}`);
  process.stdout.write(`exit_code=${exitCode}
ending_invalid=${endingInvalid}
`);
}
if (import.meta.main)
  main();
export {
  ref
};
