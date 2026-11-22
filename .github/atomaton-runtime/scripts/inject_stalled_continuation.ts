#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/inject_stalled_continuation.ts
import { existsSync, readFileSync, writeFileSync } from "fs";
import { parseArgs } from "util";

// src/domain/work/stalled-continuation.ts
function stalledContinuationMessage() {
  return `Your last message ended the run, and this run changed nothing: no commit was pushed, and no pull request was opened or merged.

A message that calls no tool ends the run, so a step your message announced never happened. If work remains, make that tool call now.

If the run is complete without a change -- you investigated and nothing needs editing -- say so and give the grounds: that is a finished report. Do not end on a sentence that introduces a next step.`;
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

// src/entrypoints/machinery/inject_stalled_continuation.ts
var ref = defineScript(import.meta.url);
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: { session: { type: "string" } }
  });
  const path = values.session;
  if (!path) {
    console.error("usage: inject_stalled_continuation.ts --session PATH");
    process.exit(2);
  }
  if (!existsSync(path)) {
    console.error(`inject_stalled_continuation: ${path} does not exist; nothing to do`);
    return;
  }
  const session = JSON.parse(readFileSync(path, "utf8"));
  const messages = session.messages ?? [];
  messages.push({ role: "user", content: stalledContinuationMessage() });
  session.messages = messages;
  writeFileSync(path, JSON.stringify(session, null, 2));
  console.error(`inject_stalled_continuation: appended the continuation to ${path}`);
}
if (import.meta.main)
  main();
export {
  ref
};
