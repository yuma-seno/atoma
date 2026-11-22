#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/launch_atoma.ts
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

// src/entrypoints/machinery/launch_atoma.ts
var ref = defineScript(import.meta.url);
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
      out: { type: "string" },
      logs: { type: "string" }
    }
  });
  for (const required of ["run-dir", "tool-cache", "tools-file", "credentials-file", "stop-file", "deadline-at", "min-budget-secs", "out", "logs"]) {
    if (!values[required]) {
      console.error(`launch_atoma: --${required} is required`);
      process.exit(2);
    }
  }
  const agent = process.env.AGENT ?? "";
  if (!agent) {
    console.error("launch_atoma: AGENT is not set; no definition to run");
    process.exit(2);
  }
  const deadlineAt = Number(values["deadline-at"]);
  if (!Number.isFinite(deadlineAt)) {
    console.error(`launch_atoma: --deadline-at '${values["deadline-at"]}' is not a number`);
    process.exit(2);
  }
  const minBudgetSecs = Number(values["min-budget-secs"]);
  const { budget, floored } = agentBudget(deadlineAt, Date.now() / 1000, minBudgetSecs);
  if (floored) {
    console.error(`::warning::only ${Math.floor(deadlineAt - Date.now() / 1000)} seconds of the job are left; giving the agent the ${minBudgetSecs}-second floor`);
  }
  console.error(`launch_atoma: agent time budget: ${budget} seconds`);
  const root = machineryRoot() ?? ".";
  const exitCode = launchAtoma({
    agent,
    machineryRoot: root,
    toolsFile: values["tools-file"],
    credentialsFile: values["credentials-file"],
    stopFile: values["stop-file"],
    session: `${values["run-dir"]}/session.json`,
    outFile: values.out,
    logsFile: values.logs,
    appendLogs: false,
    budgetSecs: budget,
    envEntries: buildAgentEnv(process.env, {
      toolCache: values["tool-cache"],
      runDir: values["run-dir"]
    })
  });
  process.exit(exitCode);
}
if (import.meta.main)
  main();
export {
  ref
};
