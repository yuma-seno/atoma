#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/summarize_run.ts
import { spawnSync } from "child_process";
import { appendFileSync, existsSync, readFileSync as readFileSync2 } from "fs";
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
function machineryPath(relative) {
  const root = machineryRoot();
  return root ? `${root}/${relative}` : relative;
}

// src/entrypoints/machinery/lib/agent-env.ts
import { readFileSync } from "fs";
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

// src/entrypoints/machinery/summarize_run.ts
var ref = defineScript(import.meta.url);
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      "run-dir": { type: "string" },
      "ending-invalid": { type: "string" }
    }
  });
  const runDir = values["run-dir"];
  if (!runDir) {
    console.error("summarize_run: --run-dir is required");
    process.exit(2);
  }
  const githubOutput = process.env.GITHUB_OUTPUT;
  if (!githubOutput) {
    console.error("summarize_run: GITHUB_OUTPUT is not set; there is nowhere to write the run's outputs");
    process.exit(2);
  }
  const endingInvalid = values["ending-invalid"] === "true";
  const resultDelimiter = Array.from(crypto.getRandomValues(new Uint8Array(15))).map((b) => b.toString(16).padStart(2, "0")).join("");
  const outputFile = join(runDir, "atomaton_output.txt");
  const result = existsSync(outputFile) ? readFileSync2(outputFile, "utf8") : "";
  appendFileSync(githubOutput, `result<<${resultDelimiter}
${result}${result.endsWith(`
`) || result === "" ? "" : `
`}${resultDelimiter}
`);
  if (endingInvalid) {
    appendFileSync(githubOutput, `directive=
`);
  } else {
    const directive = spawnSync("bun", [
      "run",
      join(import.meta.dir, "extract_directive.ts"),
      "--output-file",
      outputFile,
      "--def-dir",
      machineryPath(AGENT_DEFINITIONS_DIR)
    ], { encoding: "utf8", stdio: ["ignore", "inherit", "inherit"] });
    if (directive.status !== 0) {
      console.error(`summarize_run: extract_directive exited ${directive.status}`);
      process.exit(1);
    }
  }
  appendFileSync(githubOutput, `ending_invalid=${endingInvalid}
`);
  const opsLog = join(runDir, "atomaton_ops.log");
  const opsContent = existsSync(opsLog) ? readFileSync2(opsLog, "utf8") : "";
  const chainContinues = opsContent.includes('"op":"dispatch"');
  appendFileSync(githubOutput, `chain_continues=${chainContinues}
`);
  const parked = opsContent.includes('"op":"dispatch-elsewhere"');
  appendFileSync(githubOutput, `parked=${parked}
`);
  appendFileSync(githubOutput, `changed=${changedSoFar(opsLog)}
`);
}
if (import.meta.main)
  main();
export {
  ref
};
