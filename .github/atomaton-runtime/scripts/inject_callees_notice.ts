#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/inject_callees_notice.ts
import { existsSync as existsSync2, readFileSync as readFileSync2, writeFileSync } from "fs";
import { parseArgs } from "util";

// src/entrypoints/machinery/lib/callees.ts
import { existsSync, readFileSync } from "fs";
var NO_CALLEES = { humans: [], participants: [], notify: "" };
function readCalleesFile(path) {
  if (!path || !existsSync(path))
    return NO_CALLEES;
  try {
    const parsed = JSON.parse(readFileSync(path, "utf8"));
    const logins = (value) => Array.isArray(value) ? value.filter((login) => typeof login === "string" && login !== "") : [];
    return {
      humans: logins(parsed.humans),
      participants: logins(parsed.participants),
      notify: typeof parsed.notify === "string" ? parsed.notify : ""
    };
  } catch {
    return NO_CALLEES;
  }
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

// src/entrypoints/machinery/inject_callees_notice.ts
var ref = defineScript(import.meta.url);
function calleesNotice({ humans, notify }) {
  if (humans.length === 0) {
    return "No person can be called from this run: no owner is configured for this work and no " + "participant could be read. End by calling an agent, or by calling nobody; the machinery " + "has nobody it can name.";
  }
  const list = humans.map((login) => `@${login}`).join(", ");
  const owner = notify ? ` @${notify} is the one this work belongs to.` : "";
  return `The people you may call in your closing message: ${list}.${owner}

` + "A closing message calls at most one thing: one agent, as a line holding nothing but " + "`/name`; or one of those people, as a single `@login`; or nobody, and the machinery tells " + "the person this work belongs to. A mention of one of them is a call \u2014 write one only when " + "you are handing the work to them. Two calls, an agent and a person or two of either, are " + "refused: the message is posted with its mentions left as text, nothing in it is started, " + "and the machinery makes the call itself.";
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: { session: { type: "string" }, "people-file": { type: "string" } }
  });
  const path = values.session;
  const peopleFile = values["people-file"];
  if (!path || !peopleFile) {
    console.error("usage: inject_callees_notice.ts --session PATH --people-file FILE");
    process.exit(2);
  }
  if (!existsSync2(path)) {
    console.error(`inject_callees_notice: ${path} does not exist; nothing to do`);
    return;
  }
  const callees = readCalleesFile(peopleFile);
  const session = JSON.parse(readFileSync2(path, "utf8"));
  const messages = session.messages ?? [];
  messages.push({ role: "user", content: calleesNotice(callees) });
  session.messages = messages;
  writeFileSync(path, JSON.stringify(session, null, 2));
  console.error(`inject_callees_notice: named ${callees.humans.length} callable people in ${path}`);
}
if (import.meta.main)
  main();
export {
  calleesNotice,
  ref
};
