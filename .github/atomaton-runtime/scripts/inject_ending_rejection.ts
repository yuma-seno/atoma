#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/inject_ending_rejection.ts
import { existsSync, readFileSync, writeFileSync } from "fs";
import { parseArgs } from "util";

// src/domain/work/agent-name.ts
var AGENT_NAME_PATTERN = "[a-z][a-z0-9-]*";
var AGENT_NAME_RE = new RegExp(`^${AGENT_NAME_PATTERN}$`);

// src/domain/work/comment-command.ts
var COMMAND_RE = new RegExp(`^\\/(${AGENT_NAME_PATTERN})(?:\\s+(.*))?$`);
var DISPATCH_RE = new RegExp(`^<!--\\s*atomaton:dispatch\\s*=\\s*(${AGENT_NAME_PATTERN})\\s*-->`);

// src/domain/work/mention.ts
var LOGIN_PATTERN = "[A-Za-z0-9](?:[A-Za-z0-9]|-(?=[A-Za-z0-9])){0,38}";
var MENTION = new RegExp(`(^|[^\\w@/-])@(${LOGIN_PATTERN})\\b(?!\\/)`, "g");

// src/domain/work/ending-calls.ts
var DIRECTIVE = new RegExp(`^\\/(${AGENT_NAME_PATTERN})$`);
function rejectionMessage(problem) {
  return `Your closing message was not posted, and nothing it named was started or notified.

Your closing message ${problem}. A closing message calls at most one thing:

- one agent, as a line holding nothing but \`/name\`, taken from the colleagues named in your instructions;
- or one person, as a single \`@login\` mention of somebody who is a collaborator or has taken part in this thread;
- or nobody, in which case the machinery tells the person this work belongs to.

Write the closing message again, whole: it is the comment that will be posted. End it by calling one thing, or none. If it still calls more than one, the comment is posted with its calls left as text, none of them made, and the machinery notifies the person itself.`;
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

// src/entrypoints/machinery/inject_ending_rejection.ts
var ref = defineScript(import.meta.url);
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: { session: { type: "string" }, "problem-file": { type: "string" } }
  });
  const path = values.session;
  const problemFile = values["problem-file"];
  if (!path || !problemFile) {
    console.error("usage: inject_ending_rejection.ts --session PATH --problem-file FILE");
    process.exit(2);
  }
  const problem = existsSync(problemFile) ? readFileSync(problemFile, "utf8").trim() : "";
  if (!problem) {
    console.error("inject_ending_rejection: no problem to report; nothing to do");
    return;
  }
  if (!existsSync(path)) {
    console.error(`inject_ending_rejection: ${path} does not exist; nothing to do`);
    return;
  }
  const session = JSON.parse(readFileSync(path, "utf8"));
  const messages = session.messages ?? [];
  messages.push({ role: "user", content: rejectionMessage(problem) });
  session.messages = messages;
  writeFileSync(path, JSON.stringify(session, null, 2));
  console.error(`inject_ending_rejection: appended the rejection to ${path}`);
}
if (import.meta.main)
  main();
export {
  ref
};
