#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/reconcile_github_session.ts
import { createHash } from "crypto";
import { appendFileSync, existsSync, readFileSync, writeFileSync } from "fs";
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

// src/domain/work/actor.ts
var BOT_TYPE = "Bot";
function isHumanActor(type) {
  return (type ?? "").trim().toLowerCase() !== BOT_TYPE.toLowerCase();
}

// src/adapters/github/gh.ts
function ghCommand() {
  const fake = (process.env.ATOMATON_FAKE_GH ?? "").trim();
  return fake ? [process.execPath, fake] : ["gh"];
}
function ghBytes(...args) {
  const proc = Bun.spawnSync({ cmd: [...ghCommand(), ...args], stdout: "pipe", stderr: "pipe" });
  return { code: proc.exitCode ?? 1, bytes: proc.stdout ?? new Uint8Array };
}

// src/adapters/github/issue-images.ts
var MAX_IMAGE_BYTES = 4000000;
var MAX_IMAGES = 4;
var IMAGE_MARKDOWN = /!\[[^\]]*\]\((https?:\/\/[^\s)]+)\)/g;
var IMAGE_HTML = /<img[^>]+src=["'](https?:\/\/[^"']+)["']/gi;
function sniffMimeType(bytes) {
  const starts = (...sig) => sig.every((b, i) => bytes[i] === b);
  if (starts(137, 80, 78, 71))
    return "image/png";
  if (starts(255, 216, 255))
    return "image/jpeg";
  if (starts(71, 73, 70, 56))
    return "image/gif";
  if (starts(82, 73, 70, 70) && [87, 69, 66, 80].every((b, i) => bytes[8 + i] === b)) {
    return "image/webp";
  }
  return "";
}
function extractImageUrls(body) {
  const urls = [];
  for (const pattern of [IMAGE_MARKDOWN, IMAGE_HTML]) {
    pattern.lastIndex = 0;
    for (const match of body.matchAll(pattern)) {
      const url = match[1];
      if (url && !urls.includes(url))
        urls.push(url);
    }
  }
  return urls.slice(0, MAX_IMAGES);
}
function fetchImageBlock(url) {
  const { code, bytes } = ghBytes("api", url, "--method", "GET", "--header", "Accept: application/vnd.github.raw");
  if (code !== 0 || bytes.length === 0)
    return;
  const mimeType = sniffMimeType(bytes);
  if (!mimeType)
    return;
  const data = Buffer.from(bytes).toString("base64");
  if (!data || data.length > MAX_IMAGE_BYTES)
    return;
  return { type: "image", data, mimeType };
}
function contentWithImages(text) {
  const urls = extractImageUrls(text);
  if (urls.length === 0)
    return text;
  const images = urls.map(fetchImageBlock).filter((block) => block !== undefined);
  if (images.length === 0)
    return text;
  return [{ type: "text", text }, ...images];
}

// src/entrypoints/machinery/reconcile_github_session.ts
var ref = defineScript(import.meta.url);
var GITHUB_CONTEXT_LAYER = "github-context";
function githubEventKey(message) {
  const metadata = message.atoma_metadata;
  if (metadata?.source !== "github" || metadata.layer !== GITHUB_CONTEXT_LAYER)
    return;
  if (metadata.event_type === undefined || metadata.id === undefined)
    return;
  return `${String(metadata.event_type)}:${String(metadata.id)}`;
}
function githubEventKeyFromEvent(event) {
  return `${event.event_type}:${String(event.id)}`;
}
function deletedGithubMessage(message) {
  if (message.atoma_metadata?.deleted === true)
    return message;
  return {
    role: "user",
    content: `[Deleted GitHub ${String(message.atoma_metadata?.event_type ?? "event")}]`,
    atoma_metadata: { ...message.atoma_metadata, deleted: true }
  };
}
function reconcilePersistedGithubContext(session, messages, fetchedEventKeys) {
  const incomingByKey = new Map(messages.map((message) => [githubEventKey(message), message]));
  const seen = new Set;
  const reconciled = [];
  for (const message of session.messages ?? []) {
    const key = githubEventKey(message);
    if (key === undefined) {
      reconciled.push(message);
      continue;
    }
    const replacement = incomingByKey.get(key);
    if (replacement !== undefined && !seen.has(key)) {
      reconciled.push(replacement);
      seen.add(key);
    } else if (!fetchedEventKeys.has(key) && !seen.has(key)) {
      reconciled.push(deletedGithubMessage(message));
      seen.add(key);
    }
  }
  for (const message of messages) {
    const key = githubEventKey(message);
    if (!seen.has(key)) {
      reconciled.push(message);
      seen.add(key);
    }
  }
  return { ...session, messages: reconciled };
}
function mergeGithubContext(session, messages, fetchedEventKeys = new Set(messages.map((message) => githubEventKey(message)))) {
  const existingMessages = session.messages ?? [];
  if (session.metadata?.github_context?.version === 1) {
    return reconcilePersistedGithubContext(session, messages, fetchedEventKeys);
  }
  const firstHistoryIndex = existingMessages.findIndex((message) => message.role !== "system");
  const insertionIndex = firstHistoryIndex === -1 ? existingMessages.length : firstHistoryIndex;
  return {
    ...session,
    messages: [
      ...existingMessages.slice(0, insertionIndex),
      ...messages,
      ...existingMessages.slice(insertionIndex)
    ]
  };
}
function normalizeId(val) {
  return val === undefined ? undefined : String(val);
}
function buildOwnCommentIds(session, agentName) {
  const ownIds = new Set;
  for (const msg of session.messages ?? []) {
    const meta = msg.atoma_metadata;
    if (msg.role !== "assistant" || !meta || meta.github_comment_id === undefined)
      continue;
    if (meta.agent !== undefined && meta.agent !== agentName)
      continue;
    const id = normalizeId(meta.github_comment_id);
    if (id !== undefined)
      ownIds.add(id);
  }
  return ownIds;
}
function extractResultCommentAgent(event) {
  if (isHumanActor(event.author_type))
    return;
  return event.agent;
}
function isSelfEvent(event, agentName, ownCommentIds) {
  const eventId = normalizeId(event.id);
  if (eventId !== undefined && ownCommentIds.has(eventId))
    return true;
  return extractResultCommentAgent(event) === agentName;
}
function filterEventsForAgent(events, agentName, ownCommentIds) {
  const filtered = [];
  for (const event of events) {
    if (!isHumanActor(event.author_type) && event.llm_context === "exclude") {
      console.error(`  Skipping operational notification from LLM context: id=${event.id}`);
      continue;
    }
    if (isSelfEvent(event, agentName, ownCommentIds)) {
      console.error(`  Skipping current agent comment from shared context: id=${event.id}`);
      continue;
    }
    filtered.push(event);
  }
  return filtered;
}
function agentReadsImages(agentDefPath) {
  if (!agentDefPath || !existsSync(agentDefPath))
    return false;
  return /^vision:\s*true\s*$/m.test(readFileSync(agentDefPath, "utf8"));
}
function eventToUserMessage(event, vision) {
  return {
    role: "user",
    content: vision ? contentWithImages(event.content) : event.content,
    atoma_metadata: {
      source: "github",
      layer: GITHUB_CONTEXT_LAYER,
      event_type: event.event_type,
      id: event.id,
      author: event.author ?? "unknown",
      created_at: event.created_at ?? "",
      ...event.sha !== undefined ? { sha: event.sha } : {}
    }
  };
}
function canonicalStringify(value) {
  if (value === null || typeof value !== "object")
    return JSON.stringify(value);
  if (Array.isArray(value))
    return `[${value.map(canonicalStringify).join(",")}]`;
  const obj = value;
  const keys = Object.keys(obj).sort();
  return `{${keys.map((k) => `${JSON.stringify(k)}:${canonicalStringify(obj[k])}`).join(",")}}`;
}
function snapshotHashForEvents(events) {
  return createHash("sha256").update(canonicalStringify(events)).digest("hex");
}
function previousSnapshotHash(session) {
  return session.metadata?.github_context?.snapshot_hash;
}
function reconcileGithubSession(session, events, agentName, vision = false) {
  const ownCommentIds = buildOwnCommentIds(session, agentName);
  const filteredEvents = filterEventsForAgent(events, agentName, ownCommentIds);
  const currentHash = snapshotHashForEvents(filteredEvents);
  const previousHash = previousSnapshotHash(session);
  const contextMessages = filteredEvents.map((event) => eventToUserMessage(event, vision));
  const fetchedEventKeys = new Set(events.map(githubEventKeyFromEvent));
  let changedCount;
  if (previousHash === currentHash)
    changedCount = 0;
  else if (previousHash === undefined)
    changedCount = filteredEvents.length;
  else
    changedCount = 1;
  const mergedSession = mergeGithubContext(session, contextMessages, fetchedEventKeys);
  mergedSession.metadata = {
    ...mergedSession.metadata,
    github_context: {
      ...mergedSession.metadata?.github_context,
      version: 1,
      snapshot_hash: currentHash,
      event_count: filteredEvents.length,
      agent: agentName
    }
  };
  return {
    mergedSession,
    changedCount,
    snapshotHash: currentHash,
    eventCount: filteredEvents.length
  };
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      events: { type: "string" },
      "agent-name": { type: "string" },
      "agent-def": { type: "string" },
      session: { type: "string" },
      out: { type: "string" }
    }
  });
  if (!values.events || !values["agent-name"] || !values.session || !values.out) {
    console.error("usage: reconcile_github_session.ts --events events.json --agent-name AGENT --session session.json --out session.json");
    process.exit(2);
  }
  const session = existsSync(values.session) ? JSON.parse(readFileSync(values.session, "utf8")) : { messages: [] };
  const events = JSON.parse(readFileSync(values.events, "utf8"));
  const { mergedSession, changedCount, snapshotHash, eventCount } = reconcileGithubSession(session, events, values["agent-name"], agentReadsImages(values["agent-def"]));
  writeFileSync(values.out, JSON.stringify(mergedSession, null, 2) + `
`);
  const githubOutput = process.env.GITHUB_OUTPUT;
  if (githubOutput) {
    appendFileSync(githubOutput, `new_event_count=${changedCount}
context_snapshot_hash=${snapshotHash}
context_event_count=${eventCount}
` + `messages_before=${mergedSession.messages?.length ?? 0}
`);
  }
  console.error(`Context build complete: ${events.length} events fetched, ${eventCount} shared messages, changed=${changedCount}`);
}
if (import.meta.main)
  main();
export {
  agentReadsImages,
  mergeGithubContext,
  reconcileGithubSession,
  ref
};
