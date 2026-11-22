#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/should_continue_stalled.ts
import { existsSync as existsSync3, readFileSync as readFileSync3 } from "fs";
import { parseArgs } from "util";

// src/domain/record/closing-report.ts
function lastTurnCalledTools(session) {
  const last = lastAssistantMessage(session);
  if (last === undefined)
    return false;
  const calls = last.tool_calls;
  return Array.isArray(calls) && calls.length > 0;
}
function lastAssistantMessage(session) {
  const messages = session?.messages ?? [];
  for (let i = messages.length - 1;i >= 0; i -= 1) {
    const message = messages[i];
    if (message?.role === "assistant")
      return message;
  }
  return;
}

// src/domain/work/agent-name.ts
var AGENT_NAME_PATTERN = "[a-z][a-z0-9-]*";
var AGENT_NAME_RE = new RegExp(`^${AGENT_NAME_PATTERN}$`);

// src/domain/work/comment-command.ts
var COMMAND_RE = new RegExp(`^\\/(${AGENT_NAME_PATTERN})(?:\\s+(.*))?$`);
var DISPATCH_RE = new RegExp(`^<!--\\s*atomaton:dispatch\\s*=\\s*(${AGENT_NAME_PATTERN})\\s*-->`);
function lineForms(rawLine) {
  let line = rawLine.trim();
  if (!line)
    return [];
  line = line.replace(/^(?:[-*+]\s+|>\s*)+/, "");
  const variants = [line];
  if (line.startsWith("`") && line.endsWith("`") && line.length > 2) {
    variants.push(line.slice(1, -1).trim());
  }
  if (line.startsWith("/`") && line.endsWith("`") && line.length > 3) {
    variants.push("/" + line.slice(2, -1).trim());
  }
  return variants;
}

// src/shared/fence.ts
function fenceSegments(text) {
  const lines = text.match(/[^\n]*\n|[^\n]+$/g) ?? [];
  const segments = [];
  let open = null;
  let current = { text: "", code: false };
  let startsBlock = true;
  let previousDepth = 0;
  const flush = () => {
    if (current.text)
      segments.push(current);
    current = { text: "", code: false };
  };
  for (let index = 0;index < lines.length; index += 1) {
    const line = lines[index];
    const bare = line.replace(/\r?\n$/, "");
    const { depth, rest } = stripQuotes(bare, Number.POSITIVE_INFINITY);
    if (depth > previousDepth)
      startsBlock = true;
    previousDepth = depth;
    if (open !== null && depth < open.depth) {
      flush();
      open = null;
      startsBlock = true;
    }
    if (open === null) {
      const fence = FENCE.exec(rest);
      if (fence !== null && (fence[1][0] !== "`" || !fence[2].includes("`"))) {
        flush();
        open = { char: fence[1][0], length: fence[1].length, depth, indent: null };
        current = { text: line, code: true };
        startsBlock = false;
        continue;
      }
      const indented = INDENTED_FENCE.exec(rest);
      const opensIndented = indented !== null && startsBlock && (indented[2][0] !== "`" || !indented[3].includes("`")) && hasIndentedCloser(lines, index, depth, indented[1].length, indented[2]);
      if (opensIndented) {
        flush();
        open = {
          char: indented[2][0],
          length: indented[2].length,
          depth,
          indent: indented[1].length
        };
        current = { text: line, code: true };
        startsBlock = false;
        continue;
      }
      current.text += line;
      startsBlock = indented !== null && !startsBlock ? false : endsBlock(rest);
      continue;
    }
    current.text += line;
    if (closesFence(stripQuotes(bare, open.depth).rest, open)) {
      flush();
      open = null;
      startsBlock = true;
    }
  }
  flush();
  return segments;
}
var FENCE = /^ {0,3}(`{3,}|~{3,})(.*)$/;
var INDENTED_FENCE = /^( {4,})(`{3,}|~{3,})(.*)$/;
var ATX_HEADING = /^ {0,3}#{1,6}(?:[ \t]|$)/;
var THEMATIC_BREAK = /^ {0,3}(?:(?:\*[ \t]*){3,}|(?:-[ \t]*){3,}|(?:_[ \t]*){3,})$/;
var LIST_ITEM = /^ {0,3}(?:[-+*]|\d{1,9}[.)])(?:[ \t]+|$)/;
function endsBlock(rest) {
  if (rest.trim() === "")
    return true;
  return ATX_HEADING.test(rest) || THEMATIC_BREAK.test(rest) || LIST_ITEM.test(rest) || FENCE.test(rest) || INDENTED_FENCE.test(rest);
}
function closesFence(rest, open) {
  if (open.indent === null) {
    const normal = FENCE.exec(rest);
    return normal !== null && normal[1][0] === open.char && normal[1].length >= open.length && normal[2].trim() === "";
  }
  const indented = INDENTED_FENCE.exec(rest);
  if (indented === null)
    return false;
  const indent = indented[1].length;
  return indent >= 4 && indent <= open.indent + 3 && indented[2][0] === open.char && indented[2].length >= open.length && indented[3].trim() === "";
}
function hasIndentedCloser(lines, opener, depth, indent, marker) {
  for (let index = opener + 1;index < lines.length; index += 1) {
    const bare = lines[index].replace(/\r?\n$/, "");
    const { depth: lineDepth, rest } = stripQuotes(bare, depth);
    if (lineDepth < depth)
      return false;
    if (rest.trim() !== "" && rest.length - rest.trimStart().length < indent)
      return false;
    if (closesFence(rest, { char: marker[0], length: marker.length, depth, indent }))
      return true;
  }
  return false;
}
function stripQuotes(line, limit) {
  let rest = line;
  let depth = 0;
  while (depth < limit) {
    const marker = /^ {0,3}> ?/.exec(rest);
    if (marker === null)
      break;
    rest = rest.slice(marker[0].length);
    depth += 1;
  }
  return { depth, rest };
}
function codeSegments(text) {
  return fenceSegments(text).flatMap((segment) => segment.code ? [segment] : spans(segment.text));
}
function spans(text) {
  const segments = [];
  let prose = "";
  let at = 0;
  while (at < text.length) {
    if (text[at] !== "`") {
      prose += text[at];
      at += 1;
      continue;
    }
    const opening = runLength(text, at);
    const lineEnd = text.indexOf(`
`, at);
    const closing = closingRun(text, at + opening, opening, lineEnd === -1 ? text.length : lineEnd);
    if (closing === -1) {
      prose += text.slice(at, at + opening);
      at += opening;
      continue;
    }
    if (prose)
      segments.push({ text: prose, code: false });
    prose = "";
    segments.push({ text: text.slice(at, closing + opening), code: true });
    at = closing + opening;
  }
  if (prose)
    segments.push({ text: prose, code: false });
  return segments;
}
function runLength(text, at) {
  let length = 0;
  while (text[at + length] === "`")
    length += 1;
  return length;
}
function closingRun(text, from, length, end) {
  let at = from;
  for (;; ) {
    const found = text.indexOf("`", at);
    if (found === -1 || found + length > end)
      return -1;
    const run = runLength(text, found);
    if (run === length)
      return found;
    at = found + run;
  }
}

// src/domain/work/mention.ts
var LOGIN_PATTERN = "[A-Za-z0-9](?:[A-Za-z0-9]|-(?=[A-Za-z0-9])){0,38}";
var MENTION = new RegExp(`(^|[^\\w@/-])@(${LOGIN_PATTERN})\\b(?!\\/)`, "g");
function mentionsIn(text) {
  const found = [];
  const seen = new Set;
  const prose = codeSegments(text).map((segment) => segment.code ? " " : segment.text).join("");
  for (const match of prose.matchAll(new RegExp(MENTION.source, "g"))) {
    const login = match[2];
    const key = login.toLowerCase();
    if (seen.has(key))
      continue;
    seen.add(key);
    found.push(login);
  }
  return found;
}

// src/domain/work/ending-calls.ts
var DIRECTIVE = new RegExp(`^\\/(${AGENT_NAME_PATTERN})$`);
function endingCalls(output, callable) {
  const agents = [];
  for (const rawLine of output.split(`
`)) {
    for (const candidate of lineForms(rawLine)) {
      const match = DIRECTIVE.exec(candidate);
      if (!match)
        continue;
      const agent = match[1];
      if (!callable.knowsAgent(agent))
        continue;
      if (!agents.includes(agent))
        agents.push(agent);
    }
  }
  return { agents, humans: calledHumans(output, callable.humans) };
}
function calledHumans(text, humans) {
  const allowed = new Set([...humans].map((login) => login.trim().toLowerCase()).filter(Boolean));
  const called = [];
  for (const login of mentionsIn(text)) {
    if (!allowed.has(login.toLowerCase()))
      continue;
    if (called.some((seen) => seen.toLowerCase() === login.toLowerCase()))
      continue;
    called.push(login);
  }
  return called;
}

// src/domain/work/stalled-continuation.ts
function shouldAskToContinue(facts) {
  return facts.endedBecause === "completed" && !facts.lastTurnCalledTools && !facts.changed && !facts.calledSomeone;
}

// src/entrypoints/machinery/extract_directive.ts
import { existsSync, readFileSync, appendFileSync } from "fs";
import { join } from "path";

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

// src/entrypoints/machinery/extract_directive.ts
var ref = defineScript(import.meta.url);
var COMMAND_RE2 = new RegExp(`^\\/(${AGENT_NAME_PATTERN})$`);
function hasAgentDefinition(name, defDir) {
  return name !== "" && existsSync(join(defDir, `${name}.md`));
}
if (false)
  ;

// src/entrypoints/machinery/lib/callees.ts
import { existsSync as existsSync2, readFileSync as readFileSync2 } from "fs";
var NO_CALLEES = { humans: [], participants: [], notify: "" };
function readCalleesFile(path) {
  if (!path || !existsSync2(path))
    return NO_CALLEES;
  try {
    const parsed = JSON.parse(readFileSync2(path, "utf8"));
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

// src/entrypoints/machinery/read_run_ending.ts
var ref2 = defineScript(import.meta.url);
function parseSession(raw) {
  if (raw === undefined)
    return;
  try {
    return JSON.parse(raw);
  } catch {
    return;
  }
}
function endingFromSession(session) {
  const runs = session?.atoma_runs;
  if (!Array.isArray(runs) || runs.length === 0)
    return;
  const last = runs[runs.length - 1];
  return typeof last?.ended_because === "string" && last.ended_because !== "" ? last.ended_because : undefined;
}
if (false)
  ;

// src/entrypoints/machinery/should_continue_stalled.ts
var ref3 = defineScript(import.meta.url);
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      session: { type: "string" },
      "output-file": { type: "string" },
      "def-dir": { type: "string" },
      "people-file": { type: "string" },
      changed: { type: "string" }
    }
  });
  const sessionPath = values.session;
  const outputFile = values["output-file"];
  const defDir = values["def-dir"];
  if (!sessionPath || !outputFile || !defDir) {
    console.error("usage: should_continue_stalled.ts --session FILE --output-file FILE --def-dir DIR --people-file FILE --changed true|false");
    process.exit(2);
  }
  const session = parseSession(existsSync3(sessionPath) ? readFileSync3(sessionPath, "utf8") : undefined);
  const output = existsSync3(outputFile) ? readFileSync3(outputFile, "utf8") : "";
  const callees = readCalleesFile(values["people-file"]);
  const calls = endingCalls(output, {
    knowsAgent: (name) => hasAgentDefinition(name, defDir),
    humans: callees.humans
  });
  const answer = shouldAskToContinue({
    endedBecause: endingFromSession(session),
    lastTurnCalledTools: lastTurnCalledTools(session),
    changed: values.changed === "true",
    calledSomeone: calls.agents.length + calls.humans.length > 0
  });
  console.log(answer ? "true" : "false");
  console.error(`should_continue_stalled: ${answer}`);
}
if (import.meta.main)
  main();
export {
  ref3 as ref
};
