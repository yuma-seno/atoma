#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/inject_dispatcher_notice.ts
import { existsSync, readFileSync, writeFileSync } from "fs";
import { parseArgs } from "util";

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

// src/entrypoints/machinery/inject_dispatcher_notice.ts
var ref = defineScript(import.meta.url);
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: { session: { type: "string" }, "dispatched-by": { type: "string" } }
  });
  const path = values.session;
  if (!path) {
    console.error("usage: inject_dispatcher_notice.ts --session <path> --dispatched-by <agent>");
    process.exit(2);
  }
  const dispatchedBy = (values["dispatched-by"] ?? "").trim();
  if (!dispatchedBy) {
    console.error("inject_dispatcher_notice: no dispatcher; nothing to do");
    return;
  }
  if (!existsSync(path)) {
    console.error(`inject_dispatcher_notice: ${path} does not exist; nothing to do`);
    return;
  }
  const session = JSON.parse(readFileSync(path, "utf8"));
  const messages = session.messages ?? [];
  messages.push({
    role: "user",
    content: `This run was started by the \`${dispatchedBy}\` agent, which is waiting on your result. ` + `When you finish, report back to it: end your response with a line holding nothing but ` + `\`/${dispatchedBy}\`, followed by what you concluded and what it should do next. ` + `Do not end with a plain comment and no such line -- the agent that started you would ` + `never be told the work is done.`
  });
  session.messages = messages;
  writeFileSync(path, JSON.stringify(session, null, 2));
  console.error(`inject_dispatcher_notice: appended notice to ${path} (dispatched by ${dispatchedBy})`);
}
if (import.meta.main)
  main();
export {
  ref
};
