#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/record_run_metadata.ts
import { readFileSync, writeFileSync } from "fs";
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

// src/entrypoints/machinery/record_run_metadata.ts
var ref = defineScript(import.meta.url);
function updateRunMetadata(session, update) {
  if (update.commentId !== undefined) {
    const commentId = Number(update.commentId);
    const messages = session.messages ?? [];
    for (let index = messages.length - 1;index >= 0; index--) {
      const message = messages[index];
      if (message.role === "assistant" && message.content) {
        message.atoma_metadata = { ...message.atoma_metadata, github_comment_id: commentId, agent: update.agent };
        break;
      }
    }
  }
  if (update.snapshotHash !== undefined) {
    const metadata = typeof session.metadata === "object" && session.metadata !== null ? session.metadata : {};
    metadata.github_context = {
      ...metadata.github_context,
      snapshot_hash: update.snapshotHash,
      event_count: Number(update.eventCount ?? 0),
      agent: update.agent,
      type: update.type,
      session_number: update.sessionNumber
    };
    session.metadata = metadata;
  }
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      session: { type: "string" },
      "comment-id": { type: "string" },
      agent: { type: "string" },
      "snapshot-hash": { type: "string" },
      "event-count": { type: "string" },
      type: { type: "string" },
      "session-number": { type: "string" }
    }
  });
  if (!values.session) {
    console.error("usage: record_run_metadata.ts --session session.json [...]");
    process.exit(2);
  }
  const session = JSON.parse(readFileSync(values.session, "utf8"));
  updateRunMetadata(session, {
    commentId: values["comment-id"],
    agent: values.agent,
    snapshotHash: values["snapshot-hash"],
    eventCount: values["event-count"],
    type: values.type,
    sessionNumber: values["session-number"]
  });
  if (values["comment-id"] !== undefined)
    console.error(`Tagged last assistant message with github_comment_id=${values["comment-id"]}`);
  if (values["snapshot-hash"] !== undefined) {
    console.error(`Recorded shared context snapshot hash=${values["snapshot-hash"]} for agent=${values.agent}`);
  }
  writeFileSync(values.session, JSON.stringify(session, null, 2) + `
`);
}
if (import.meta.main)
  main();
export {
  ref,
  updateRunMetadata
};
