#!/usr/bin/env bun
// @bun
var __require = import.meta.require;

// src/entrypoints/machinery/claim_turn.ts
import { parseArgs } from "util";

// src/adapters/github/gh.ts
function run(cmd) {
  const proc = Bun.spawnSync({
    cmd,
    stdout: "pipe",
    stderr: "pipe"
  });
  return {
    code: proc.exitCode ?? 1,
    stdout: proc.stdout ? proc.stdout.toString("utf8").trim() : "",
    stderr: proc.stderr ? proc.stderr.toString("utf8").trim() : ""
  };
}
function ghCommand() {
  const fake = (process.env.ATOMATON_FAKE_GH ?? "").trim();
  return fake ? [process.execPath, fake] : ["gh"];
}
function gh(...args) {
  return run([...ghCommand(), ...args]);
}
function ghRead(...args) {
  let result = gh(...args);
  for (const delay of [2000, 6000]) {
    if (result.code === 0 || !looksTransient(result))
      return result;
    console.error(`::warning::gh ${args.slice(0, 2).join(" ")} failed transiently, retrying: ${result.stderr || result.stdout}`);
    Bun.sleepSync(delay);
    result = gh(...args);
  }
  return result;
}
function looksTransient(result) {
  const text = `${result.stderr} ${result.stdout}`;
  if (/HTTP (429|5[0-9][0-9])(?![0-9])/.test(text))
    return true;
  return /(timeout|timed out|connection reset|unexpected EOF|TLS handshake|temporary failure)/i.test(text);
}
function splitConcatenatedJson(text) {
  const results = [];
  let depth = 0;
  let start = -1;
  let inString = false;
  let escaped = false;
  for (let i = 0;i < text.length; i++) {
    const c = text[i];
    if (inString) {
      if (escaped)
        escaped = false;
      else if (c === "\\")
        escaped = true;
      else if (c === '"')
        inString = false;
      continue;
    }
    if (c === '"') {
      inString = true;
      continue;
    }
    if (c === "{" || c === "[") {
      if (depth === 0)
        start = i;
      depth++;
    } else if (c === "}" || c === "]") {
      depth--;
      if (depth === 0 && start !== -1) {
        results.push(JSON.parse(text.slice(start, i + 1)));
        start = -1;
      }
    }
  }
  return results;
}

// src/domain/work/actor.ts
var BOT_TYPE = "Bot";
function isHumanActor(type) {
  return (type ?? "").trim().toLowerCase() !== BOT_TYPE.toLowerCase();
}

// src/domain/work/agent-name.ts
var AGENT_NAME_PATTERN = "[a-z][a-z0-9-]*";
var AGENT_NAME_RE = new RegExp(`^${AGENT_NAME_PATTERN}$`);

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

// src/domain/work/mention.ts
var LOGIN_PATTERN = "[A-Za-z0-9](?:[A-Za-z0-9]|-(?=[A-Za-z0-9])){0,38}";
var MENTION = new RegExp(`(^|[^\\w@/-])@(${LOGIN_PATTERN})\\b(?!\\/)`, "g");

// src/adapters/github/tags.ts
var TAG_PREFIX = `atomaton:`;
var EVERY_TAG_PATTERN = [];
function makeTag(key, valuePattern, parse, render) {
  const pattern = `<!--\\s*${TAG_PREFIX}${key}=(?:${valuePattern})\\s*-->`;
  EVERY_TAG_PATTERN.push(pattern);
  const re = new RegExp(`<!--\\s*${TAG_PREFIX}${key}=(${valuePattern})\\s*-->`);
  const read = (text) => {
    const m = re.exec(text);
    return m ? parse(m[1]) : undefined;
  };
  return {
    marker: `${TAG_PREFIX}${key}`,
    write: (value) => `<!-- ${TAG_PREFIX}${key}=${render(value)} -->`,
    read,
    readProse: (text) => read(proseOnly(text)),
    has: (text) => re.test(text),
    hasProse: (text) => re.test(proseOnly(text)),
    search: (value) => `${TAG_PREFIX}${key}=${render(value)}`
  };
}
function numericTag(key) {
  return makeTag(key, "\\d+", Number, String);
}
function stringTag(key, valuePattern) {
  return makeTag(key, valuePattern, (raw) => raw, (value) => value);
}
function makeCompoundTag(key, valuePattern, parse, render, searchValue) {
  const pattern = `<!--\\s*${TAG_PREFIX}${key}=(?:${valuePattern})\\s*-->`;
  EVERY_TAG_PATTERN.push(pattern);
  const re = new RegExp(`<!--\\s*${TAG_PREFIX}${key}=(?:${valuePattern})\\s*-->`);
  const read = (text) => {
    const m = re.exec(text);
    return m ? parse(m) : undefined;
  };
  return {
    marker: `${TAG_PREFIX}${key}`,
    write: (value) => `<!-- ${TAG_PREFIX}${key}=${render(value)} -->`,
    read,
    readProse: (text) => read(proseOnly(text)),
    has: (text) => re.test(text),
    hasProse: (text) => re.test(proseOnly(text)),
    search: searchValue
  };
}
var STOP_TAG = stringTag("stop", "requested");
var ENDED_TAG = makeCompoundTag("ended", String.raw`(stopped|limit|done|handoff|waiting|refused) answers=(\d+) run=(\d+)`, (match) => {
  const verb = match[1];
  const answers = match[2];
  const run = match[3];
  if (verb === undefined || answers === undefined || run === undefined)
    return;
  return { verb, answers: Number(answers), run: Number(run) };
}, ({ verb, answers, run }) => `${verb} answers=${answers} run=${run}`, ({ verb }) => `atomaton:ended=${verb}`);
var TOOK_TAG = makeCompoundTag("took", String.raw`(\d+) run=(\d+)`, (match) => {
  const ask = match[1];
  const run = match[2];
  if (ask === undefined || run === undefined)
    return;
  return { ask: Number(ask), run: Number(run) };
}, ({ ask, run }) => `${ask} run=${run}`, ({ ask }) => `atomaton:took=${ask}`);
var PARENT_ISSUE_TAG = numericTag("parent-issue");
var NOTIFY_TAG = stringTag("notify", LOGIN_PATTERN);
var ORIGIN_AGENT_TAG = stringTag("origin-agent", AGENT_NAME_PATTERN);
var DISPATCH_TAG = stringTag("dispatch", AGENT_NAME_PATTERN);
var AGENT_TAG = stringTag("agent", AGENT_NAME_PATTERN);
var CHANGED_TAG = stringTag("changed", "yes|no");
var LLM_CONTEXT_TAG = stringTag("llm-context", "include|exclude");
var AGGREGATED_TAG = numericTag("aggregated");
var SUB_RESULT_TAG = numericTag("sub-result");
var QUOTES_TAG = numericTag("quotes");
function isQuotation(body) {
  return QUOTES_TAG.has(body);
}
function proseOnly(text) {
  return fenceSegments(text).map((segment) => segment.code ? `
` : segment.text).join("");
}

// src/adapters/github/run-status.ts
function readRunStatus(repo, runId) {
  const read = ghRead("api", `repos/${repo}/actions/runs/${runId}`, "--jq", ".status");
  if (read.code !== 0) {
    return { read: false, why: (read.stderr || read.stdout || `gh exited ${read.code}`).trim().split(`
`)[0] ?? "" };
  }
  const status = (read.stdout ?? "").trim();
  if (status === "")
    return { read: false, why: "the run read returned no status" };
  return { read: true, status };
}

// src/domain/work/control-commands.ts
var CONTROL_COMMAND_NAMES = ["stop", "resume", "upgrade"];
function isControlCommand(name) {
  return CONTROL_COMMAND_NAMES.includes(name);
}
function controlLabel(name) {
  return name ? `!${name}` : "";
}

// src/domain/work/comment-command.ts
var COMMAND_RE = new RegExp(`^\\/(${AGENT_NAME_PATTERN})(?:\\s+(.*))?$`);
var CONTROL_RE = /^!([a-z][a-z0-9-]*)(?:\s+(.*))?$/;
var DISPATCH_RE = new RegExp(`^<!--\\s*atomaton:dispatch\\s*=\\s*(${AGENT_NAME_PATTERN})\\s*-->`);
var NOTHING = { agent: "", control: "", sessionMode: "continue", error: "" };
function parseCommentCommand(body) {
  if (!body)
    return NOTHING;
  for (const rawLine of body.split(`
`)) {
    const parsed = parseCommandLine(rawLine);
    if (parsed !== undefined)
      return parsed;
  }
  return NOTHING;
}
function parseCommandLine(rawLine) {
  const line = rawLine.trim();
  const bangMatch = CONTROL_RE.exec(line);
  if (bangMatch && isControlCommand(bangMatch[1])) {
    return readControl(bangMatch[1], bangMatch[2]);
  }
  const commandMatch = COMMAND_RE.exec(line);
  if (commandMatch) {
    const name = commandMatch[1];
    const modifier = commandMatch[2]?.trim() ?? "";
    if (isControlCommand(name))
      return readControl(name, modifier === "" ? undefined : modifier);
    if (!modifier)
      return { ...NOTHING, agent: name };
    if (modifier === "recover")
      return { ...NOTHING, agent: name, sessionMode: "recover" };
    return {
      ...NOTHING,
      error: `Unknown command syntax: '/${name} ${modifier}'. Put instructions on the lines after '/${name}', or use '/${name} recover'.`
    };
  }
  const dispatchMatch = DISPATCH_RE.exec(line);
  if (dispatchMatch)
    return { ...NOTHING, agent: dispatchMatch[1] };
  return;
}
function readControl(name, modifier) {
  if (!modifier)
    return { ...NOTHING, control: name };
  return {
    ...NOTHING,
    error: `'${controlLabel(name)}' takes nothing after it. To resume with an instruction, use '/<agent>' and put the instruction on the following lines.`
  };
}

// src/adapters/github/thread.ts
var readers = {
  ending: (body) => {
    const ended = ENDED_TAG.readProse(body)?.verb;
    if (ended === "handoff")
      return "handed-off";
    if (ended === "waiting")
      return "waiting";
    if (ended === "stopped" || ended === "limit" || ended === "done" || ended === "refused")
      return "returned";
    return "";
  },
  requestedAgent: (body) => parseCommentCommand(proseOnly(body)).agent,
  isDispatchMarker: (body) => DISPATCH_TAG.hasProse(body),
  claim: (body) => TOOK_TAG.readProse(body),
  answeredAsk: (body) => {
    const answers = ENDED_TAG.readProse(body)?.answers;
    return answers !== undefined && answers > 0 ? answers : undefined;
  },
  endingAgent: (body) => AGENT_TAG.readProse(body) ?? ""
};
var endingWalkReaders = {
  ending: readers.ending,
  quoted: isQuotation,
  claim: (body) => TOOK_TAG.readProse(body),
  ended: (body) => ENDED_TAG.readProse(body)
};
function listComments(repo, number) {
  const listed = ghRead("api", `repos/${repo}/issues/${number}/comments`, "--paginate");
  if (listed.code !== 0)
    throw new Error(`could not read comments on #${number}: ${listed.stderr || listed.stdout}`);
  const pages = splitConcatenatedJson(listed.stdout);
  const flat = [];
  for (const page of pages) {
    if (Array.isArray(page))
      flat.push(...page);
  }
  return flat;
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

// src/entrypoints/machinery/claim_turn.ts
var ref = defineScript(import.meta.url);
var READ_DELAYS_MS = [1000, 2000, 4000];
function runFinishedOn(repo, run) {
  const status = readRunStatus(repo, run);
  return status.read && status.status === "completed";
}
function readComments(repo, number) {
  return listComments(repo, number).map((comment) => ({
    id: comment.id,
    body: comment.body ?? "",
    human: isHumanActor(comment.user?.type)
  }));
}
var CLAIM_NOTICE = "Atomaton: claimed this turn.";
function claimCommentBody(args) {
  return [LLM_CONTEXT_TAG.write("exclude"), TOOK_TAG.write(args), CLAIM_NOTICE].join(`
`);
}
function claimOnMarkerBody(original, args) {
  return `${original.replace(/\s+$/, "")}
${TOOK_TAG.write(args)}`;
}
function patchCommentBody(repo, commentId, body) {
  return gh("api", `repos/${repo}/issues/comments/${commentId}`, "--method", "PATCH", "-f", `body=${body}`);
}
function markerBody(agent, type) {
  const noun = type === "pr" ? "pull request" : "issue";
  return `${LLM_CONTEXT_TAG.write("exclude")}
${DISPATCH_TAG.write(agent)}
` + `Atomaton: \`${agent}\` starting on this ${noun}.`;
}
async function mintMarker(repo, number, agent, type) {
  if (!agent) {
    throw new Error("--agent is required when --ask is absent: an ask-less run mints its ask from a marker that names the agent");
  }
  const posted = gh("api", `repos/${repo}/issues/${number}/comments`, "--method", "POST", "-f", `body=${markerBody(agent, type)}`, "--jq", ".id");
  if (posted.code !== 0)
    throw new Error(`could not mint the dispatch marker on #${number}: ${posted.stderr || posted.stdout}`);
  const markerId = Number(posted.stdout.trim());
  if (!Number.isInteger(markerId) || markerId <= 0)
    throw new Error(`the minted marker's id came back unreadable: ${posted.stdout.trim()}`);
  return markerId;
}
function raceOutcome(comments, mine, runFinished) {
  for (const comment of comments) {
    if (comment.id >= mine.id)
      break;
    const claim = TOOK_TAG.readProse(comment.body);
    if (claim === undefined || claim.ask !== mine.ask)
      continue;
    if (runFinished !== undefined && claim.run > 0 && runFinished(claim.run))
      continue;
    return "backed-off";
  }
  return "held";
}
async function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      number: { type: "string" },
      ask: { type: "string" },
      "run-id": { type: "string" },
      repo: { type: "string" },
      agent: { type: "string" },
      type: { type: "string" }
    }
  });
  const repo = values.repo ?? process.env.GITHUB_REPOSITORY ?? "";
  const number = Number(values.number);
  const runId = Number(values["run-id"] ?? process.env.GITHUB_RUN_ID ?? 0);
  let ask = Number(values.ask ?? 0) || 0;
  const record = (claimed, askId) => {
    const out = process.env.GITHUB_OUTPUT;
    if (out) {
      const { appendFileSync } = __require("fs");
      appendFileSync(out, `claimed=${claimed ? "true" : "false"}
`);
      appendFileSync(out, `ask=${askId}
`);
    }
    console.error(`claim_turn: claimed=${claimed ? "true" : "false"} (ask=${askId}, run=${runId})`);
  };
  if (!Number.isInteger(number) || number <= 0)
    throw new Error("--number must be a positive number");
  let minted = false;
  if (ask <= 0) {
    ask = await mintMarker(repo, number, (values.agent ?? "").trim(), values.type ?? "");
    minted = true;
  }
  if (!Number.isInteger(runId) || runId <= 0)
    throw new Error("--run-id must be a positive number");
  if (minted) {
    for (let attempt = 0;attempt <= READ_DELAYS_MS.length; attempt += 1) {
      if (readComments(repo, number).some((comment) => comment.id === ask))
        break;
      const delay = READ_DELAYS_MS[attempt];
      if (delay === undefined) {
        throw new Error(`the minted marker on #${number} never became visible, so the race check cannot be trusted`);
      }
      await Bun.sleep(delay);
    }
  }
  const comments = readComments(repo, number);
  const askComment = comments.find((comment) => comment.id === ask);
  const isMarker = askComment !== undefined && !askComment.human && DISPATCH_TAG.hasProse(askComment.body);
  if (askComment !== undefined && isMarker) {
    const original = askComment.body;
    const existing = TOOK_TAG.readProse(original);
    if (existing !== undefined) {
      if (!runFinishedOn(repo, existing.run)) {
        record(false, ask);
        return;
      }
      const stripped = original.split(`
`).filter((line) => !TOOK_TAG.hasProse(line)).join(`
`);
      const takenOver = patchCommentBody(repo, ask, claimOnMarkerBody(stripped, { ask, run: runId }));
      if (takenOver.code !== 0) {
        throw new Error(`could not claim the dispatch marker on #${number}: ${takenOver.stderr || takenOver.stdout}`);
      }
      const readBack = readComments(repo, number).find((comment) => comment.id === ask);
      const nowOurs = readBack === undefined ? undefined : TOOK_TAG.readProse(readBack.body);
      if (nowOurs === undefined || nowOurs.run !== runId) {
        throw new Error(`the takeover of the claim on #${number} did not read back as this run's`);
      }
      const outcome = raceOutcome(readComments(repo, number), { id: ask, ask }, (run) => runFinishedOn(repo, run));
      if (outcome === "held") {
        record(true, ask);
        return;
      }
      const restored = patchCommentBody(repo, ask, original);
      if (restored.code !== 0) {
        throw new Error(`could not take back the claim on #${number}: ${restored.stderr || restored.stdout}`);
      }
      record(false, ask);
      return;
    }
    const edited = patchCommentBody(repo, ask, claimOnMarkerBody(original, { ask, run: runId }));
    if (edited.code !== 0) {
      throw new Error(`could not claim the dispatch marker on #${number}: ${edited.stderr || edited.stdout}`);
    }
    let held;
    for (let attempt = 0;attempt <= READ_DELAYS_MS.length; attempt += 1) {
      const current = readComments(repo, number).find((comment) => comment.id === ask);
      const claim = current === undefined ? undefined : TOOK_TAG.readProse(current.body);
      if (claim !== undefined) {
        held = claim.run === runId;
        break;
      }
      const delay = READ_DELAYS_MS[attempt];
      if (delay === undefined)
        break;
      await Bun.sleep(delay);
    }
    if (held === undefined)
      throw new Error(`the claim on #${number} never became visible, so the race check cannot be trusted`);
    if (!held) {
      record(false, ask);
      return;
    }
    const outcome = raceOutcome(readComments(repo, number), { id: ask, ask }, (run) => runFinishedOn(repo, run));
    if (outcome === "held") {
      record(true, ask);
      return;
    }
    const restored = patchCommentBody(repo, ask, original);
    if (restored.code !== 0) {
      throw new Error(`could not take back the claim on #${number}: ${restored.stderr || restored.stdout}`);
    }
    record(false, ask);
    return;
  }
  const posted = gh("api", `repos/${repo}/issues/${number}/comments`, "--method", "POST", "-f", `body=${claimCommentBody({ ask, run: runId })}`, "--jq", ".id");
  if (posted.code !== 0)
    throw new Error(`could not post the claim on #${number}: ${posted.stderr || posted.stdout}`);
  const claimId = Number(posted.stdout.trim());
  if (!Number.isInteger(claimId) || claimId <= 0)
    throw new Error(`the claim's id came back unreadable: ${posted.stdout.trim()}`);
  let visible = false;
  for (let attempt = 0;attempt <= READ_DELAYS_MS.length; attempt += 1) {
    if (readComments(repo, number).some((comment) => comment.id === claimId)) {
      visible = true;
      break;
    }
    const delay = READ_DELAYS_MS[attempt];
    if (delay === undefined)
      break;
    await Bun.sleep(delay);
  }
  if (!visible)
    throw new Error(`the claim on #${number} never became visible, so the race check cannot be trusted`);
  const outcome = raceOutcome(readComments(repo, number), { id: claimId, ask }, (run) => runFinishedOn(repo, run));
  if (outcome === "held") {
    record(true, ask);
    return;
  }
  const removed = gh("api", `repos/${repo}/issues/comments/${claimId}`, "--method", "DELETE");
  if (removed.code !== 0) {
    throw new Error(`could not take back the claim on #${number}: ${removed.stderr || removed.stdout}`);
  }
  record(false, ask);
}
if (import.meta.main)
  main();
export {
  claimCommentBody,
  claimOnMarkerBody,
  markerBody,
  raceOutcome,
  ref
};
