#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/decide_reask.ts
import { spawnSync } from "child_process";
import { copyFileSync, existsSync, readFileSync as readFileSync2 } from "fs";
import { join } from "path";
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

// src/adapters/runner/machinery.ts
function machineryRoot() {
  return process.env[MACHINERY_ROOT_VAR]?.trim() || undefined;
}

// src/domain/work/loop-continuation.ts
function loopAbortFromSession(session) {
  const runs = session?.atoma_runs;
  if (!Array.isArray(runs) || runs.length === 0)
    return;
  const last = runs[runs.length - 1];
  const recorded = last?.loop_abort;
  if (typeof recorded !== "object" || recorded === null)
    return;
  const verdict = recorded;
  if (typeof verdict.rule !== "string" || verdict.rule === "")
    return;
  return {
    rule: verdict.rule,
    tool: typeof verdict.tool === "string" ? verdict.tool : "",
    arguments: typeof verdict.arguments === "string" ? verdict.arguments : "",
    count: typeof verdict.count === "number" ? verdict.count : 0,
    capped: verdict.capped === true,
    lastError: typeof verdict.last_error === "string" && verdict.last_error !== "" ? verdict.last_error : undefined
  };
}

// src/entrypoints/machinery/lib/agent-env.ts
import { readFileSync } from "fs";
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

// src/entrypoints/machinery/lib/script-ref.ts
import { basename } from "path";
import { fileURLToPath } from "url";
function defineScript(importMetaUrl) {
  return { runtimePath: `${SCRIPTS_DIR}/${basename(fileURLToPath(importMetaUrl))}` };
}

// src/entrypoints/machinery/read_run_ending.ts
var ref = defineScript(import.meta.url);
function parseSession(raw) {
  if (raw === undefined)
    return;
  try {
    return JSON.parse(raw);
  } catch {
    return;
  }
}
if (false)
  ;

// src/entrypoints/machinery/decide_reask.ts
var ref2 = defineScript(import.meta.url);
function sibling(script, args) {
  const run = spawnSync("bun", ["run", join(import.meta.dir, script), ...args], { stdio: "inherit" });
  if (run.status !== 0) {
    throw new Error(`decide_reask: ${script} exited ${run.status}`);
  }
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      "run-dir": { type: "string" },
      "stop-file": { type: "string" },
      "deadline-at": { type: "string" },
      "min-budget-secs": { type: "string" },
      "exit-code": { type: "string" }
    }
  });
  for (const required of ["run-dir", "stop-file", "deadline-at", "min-budget-secs", "exit-code"]) {
    if (values[required] === undefined) {
      console.error(`decide_reask: --${required} is required`);
      process.exit(2);
    }
  }
  const runDir = values["run-dir"];
  const session = join(runDir, "session.json");
  const sessionBefore = join(runDir, "session_before_reask.json");
  const output = join(runDir, "atomaton_output.txt");
  const problemFile = join(runDir, "ending-problem.txt");
  const peopleFile = join(runDir, "callees.json");
  const stopFile = values["stop-file"];
  const root = machineryRoot() ?? ".";
  const exitCode = values["exit-code"];
  const checkArgs = ["--output-file", output, "--def-dir", join(root, AGENT_DEFINITIONS_DIR), "--problem-file", problemFile, "--people-file", peopleFile];
  let reask = "";
  if (!existsSync(stopFile)) {
    if (exitCode === "0") {
      sibling("check_ending.ts", checkArgs);
      if (readFileSync2(problemFile, "utf8").length > 0) {
        reask = "ending";
      } else {
        const stalled = spawnSync("bun", ["run", join(import.meta.dir, "should_continue_stalled.ts"), "--session", session, "--output-file", output, "--def-dir", join(root, AGENT_DEFINITIONS_DIR), "--people-file", peopleFile, "--changed", String(changedSoFar(join(runDir, "atomaton_ops.log")))], { encoding: "utf8" });
        if (stalled.status === 0 && stalled.stdout.trim() === "true")
          reask = "stalled";
      }
    } else if (loopAbortFromSession(parseSession(existsSync(session) ? readFileSync2(session, "utf8") : undefined)) !== undefined) {
      reask = "loop";
    }
  }
  let budgetSecs = 0;
  let endingInvalid = false;
  let injected = false;
  if (reask !== "") {
    const { budget, floored } = agentBudget(Number(values["deadline-at"]), Date.now() / 1000, Number(values["min-budget-secs"]));
    if (floored) {
      if (reask === "ending") {
        console.error("::warning::the closing message calls more than one thing and there is no time left to ask again; nothing it calls will be started");
        endingInvalid = true;
      } else if (reask === "loop") {
        console.error("::warning::the loop guard stopped this run and there is no time left to ask again");
      } else {
        console.error("::warning::the run stopped on a sentence having changed nothing, and there is no time left to ask again");
      }
      reask = "";
    } else {
      budgetSecs = budget;
      copyFileSync(session, sessionBefore);
      if (reask === "ending") {
        console.error("::warning::the closing message calls more than one thing; asking for it again");
        sibling("inject_ending_rejection.ts", ["--session", session, "--problem-file", problemFile]);
      } else if (reask === "loop") {
        console.error("::warning::the loop guard stopped this run; asking for one more turn");
        sibling("inject_loop_continuation.ts", ["--session", session]);
      } else {
        console.error("::warning::the run stopped on a sentence having changed nothing; asking for it again");
        sibling("inject_stalled_continuation.ts", ["--session", session]);
      }
      injected = true;
    }
  }
  console.error(`decide_reask: reask=${reask || "none"} budget_secs=${budgetSecs} injected=${injected} ending_invalid=${endingInvalid}`);
  process.stdout.write(`reask=${reask}
budget_secs=${budgetSecs}
exit_code=${exitCode}
ending_invalid=${endingInvalid}
injected=${injected}
`);
}
if (import.meta.main)
  main();
export {
  ref2 as ref
};
