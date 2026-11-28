#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/update_atomaton.ts
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
function ghAs(token, ...args) {
  const proc = Bun.spawnSync({
    cmd: [...ghCommand(), ...args],
    stdout: "pipe",
    stderr: "pipe",
    env: { ...process.env, GH_TOKEN: token }
  });
  return {
    code: proc.exitCode ?? 1,
    stdout: proc.stdout ? proc.stdout.toString("utf8").trim() : "",
    stderr: proc.stderr ? proc.stderr.toString("utf8").trim() : ""
  };
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
function dispatchWorkflow(context, workflow, args = [], log = (m) => console.error(m)) {
  const argv = ["workflow", "run", workflow, ...args];
  const { code, stdout, stderr } = gh(...argv);
  if (code) {
    log(`${context}: WARN failed to dispatch ${workflow}: ${stderr || stdout}`);
    return false;
  }
  log(`${context}: dispatched ${workflow}`);
  return true;
}

// src/adapters/github/pull-request.ts
function viewArgs(repo, number, fields) {
  const json = typeof fields === "string" ? fields : fields.join(",");
  return ["pr", "view", String(number), "--repo", repo, "--json", json];
}
function whyNot(what, result) {
  return `${what}: ${result.stderr || result.stdout || `gh exited ${result.code}`}`;
}
function readPullRequest(repo, number, fields) {
  const what = `gh pr view ${number} --repo ${repo}`;
  const result = ghRead(...viewArgs(repo, number, fields));
  if (result.code !== 0)
    return { known: false, why: whyNot(what, result) };
  if (!result.stdout.trim())
    return { known: false, why: `${what}: gh printed nothing` };
  try {
    return { known: true, value: JSON.parse(result.stdout) };
  } catch {
    return { known: false, why: `${what}: the answer was not JSON` };
  }
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
var RELEASE_MANIFEST = ".github/atomaton-release.json";
var MACHINERY_ROOT_VAR = "ATOMATON_MACHINERY_ROOT";

// src/adapters/runner/machinery.ts
function machineryRoot() {
  return process.env[MACHINERY_ROOT_VAR]?.trim() || undefined;
}
function machineryPath(relative) {
  const root = machineryRoot();
  return root ? `${root}/${relative}` : relative;
}

// src/domain/machinery/release-manifest.ts
function noLongerShipped(manifest, tree) {
  const shipped = new Set(manifest.files);
  return [...tree].map((path) => path.replace(/\\/g, "/")).filter((path) => !shipped.has(path)).sort();
}

// src/domain/machinery/update-plan.ts
function planUpdate(base, target, baseFiles, targetFiles, ours) {
  const paths = new Set([...baseFiles.keys(), ...targetFiles.keys()]);
  const entries = [];
  for (const path of [...paths].sort()) {
    const baseContent = baseFiles.get(path);
    const targetContent = targetFiles.get(path);
    const oursContent = ours.get(path);
    let upstream;
    if (baseContent === undefined && targetContent !== undefined)
      upstream = "added";
    else if (baseContent !== undefined && targetContent === undefined)
      upstream = "deleted";
    else if (baseContent === targetContent)
      upstream = "unchanged";
    else
      upstream = "modified";
    const inTheWay = oursContent !== undefined && oursContent !== baseContent;
    let outcome;
    if (upstream === "added") {
      outcome = inTheWay ? "keep" : "add";
    } else if (upstream === "deleted") {
      outcome = oursContent === undefined ? "skip" : inTheWay ? "keep" : "delete";
    } else if (upstream === "unchanged") {
      outcome = "skip";
    } else {
      if (oursContent === undefined)
        outcome = "keep";
      else if (!inTheWay)
        outcome = "take";
      else if (oursContent === targetContent)
        outcome = "skip";
      else
        outcome = "merge";
    }
    entries.push({ path, upstream, oursModified: inTheWay, outcome });
  }
  const by = (outcome) => entries.filter((e) => e.outcome === outcome).map((e) => e.path);
  return {
    base,
    target,
    entries,
    take: by("take"),
    add: by("add"),
    delete: by("delete"),
    merge: by("merge"),
    kept: entries.filter((e) => e.outcome === "keep")
  };
}
function leftoversOf(targetManifest, treePaths) {
  return noLongerShipped(targetManifest, treePaths);
}
function verdictSentence(base, target, mergeCount, keptCount) {
  if (mergeCount === 0 && keptCount === 0) {
    return `Nothing in this tree collides with ${target}: the update applies itself, and no decision is needed on the way from ${base}.`;
  }
  return `Taking ${target} onto ${base} needs a decision here: ${mergeCount} file(s) changed on both sides, ${keptCount} more the plan will not touch without a person.`;
}

// src/entrypoints/machinery/lib/script-ref.ts
import { basename } from "path";
import { fileURLToPath } from "url";
function defineScript(importMetaUrl) {
  return { runtimePath: `${SCRIPTS_DIR}/${basename(fileURLToPath(importMetaUrl))}` };
}

// src/entrypoints/machinery/lib/open-pull-request.ts
function listOpenForHead(token, repo, branch) {
  const argv = ["pr", "list", "--repo", repo, "--head", branch, "--state", "open", "--json", "number"];
  return token.trim() === "" ? ghRead(...argv) : ghAs(token.trim(), ...argv);
}
function openPrForHead(repo, branch, token = "") {
  const result = listOpenForHead(token, repo, branch);
  if (result.code !== 0) {
    const why = (result.stderr || result.stdout).split(`
`)[0] ?? "";
    console.error(`WARN could not list open pull requests on ${branch}: ${why || `gh exited ${result.code}`}`);
    return 0;
  }
  try {
    const parsed = JSON.parse(result.stdout || "[]");
    const number = Number(parsed[0]?.number ?? 0);
    return Number.isInteger(number) && number > 0 ? number : 0;
  } catch {
    return 0;
  }
}

// src/adapters/github/outcome.ts
function issueOutcome(reason) {
  const said = (reason ?? "").toLowerCase();
  return said === "not_planned" || said === "duplicate" ? "abandoned" : "done";
}
function pullRequestOutcome(merged) {
  return merged ? "done" : "abandoned";
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

// src/domain/work/actor.ts
var BOT_TYPE = "Bot";
function isHumanActor(type) {
  return (type ?? "").trim().toLowerCase() !== BOT_TYPE.toLowerCase();
}

// src/domain/work/thread.ts
function eventOf(entry, readers) {
  if (entry.quoted)
    return;
  const ending = readers.ending(entry.body);
  if (ending !== "")
    return ending;
  if (readers.requestedAgent(entry.body) !== "")
    return "asked";
  return;
}
function requestOutstanding(events) {
  return events[events.length - 1] === "asked";
}
function askSuperseded(comments, readers, askId, agent, verdictPassed) {
  let answeredHere;
  let answeredBy = "";
  const answeredAsks = new Set;
  const claimedAsks = new Set;
  const asks = [];
  for (const entry of comments) {
    if (entry.quoted)
      continue;
    if (readers.requestedAgent(entry.body) !== "" || readers.isDispatchMarker(entry.body)) {
      asks.push({ id: entry.id, marker: readers.isDispatchMarker(entry.body) });
    }
    const claim = readers.claim(entry.body);
    if (claim !== undefined)
      claimedAsks.add(claim.ask);
    const answer = readers.answeredAsk(entry.body);
    if (answer !== undefined && answer > 0) {
      answeredAsks.add(answer);
      if (answer === askId) {
        answeredHere = readers.ending(entry.body) || undefined;
        answeredBy = readers.endingAgent(entry.body);
      }
    }
  }
  const quoted = asks.find((ask) => ask.id === askId);
  if (quoted?.marker === true && !claimedAsks.has(askId))
    return "run-coming";
  if (answeredHere === "handed-off")
    return "handed-off";
  if (answeredHere === "waiting")
    return "waiting";
  if (answeredHere === "returned") {
    const own = answeredBy === "" || answeredBy === agent;
    if (own && verdictPassed !== false)
      return "own-verdict";
  }
  for (let index = asks.length - 1;index >= 0; index -= 1) {
    const ask = asks[index];
    if (ask.id <= askId)
      break;
    if (!answeredAsks.has(ask.id))
      return "newer-open-ask";
  }
  return;
}
function shapedThread(comments, readers) {
  const kept = [];
  const events = [];
  let openClaim;
  let openClaimRun;
  for (const entry of comments) {
    if (entry.isHuman && openClaim !== undefined)
      continue;
    kept.push(entry);
    if (!entry.quoted) {
      const event = eventOf(entry, readers);
      const claim = readers.claim(entry.body);
      if (claim !== undefined) {
        openClaim = claim.ask;
        openClaimRun = claim.run;
      } else if (openClaim !== undefined) {
        const answered = readers.answeredAsk(entry.body);
        if (answered !== undefined && answered === openClaim && event !== "waiting")
          openClaim = undefined;
      }
      if (event !== undefined)
        events.push(event);
    }
  }
  return {
    comments: kept,
    events,
    holder: openClaim !== undefined ? "agent" : "person",
    ...openClaim !== undefined ? { openClaim: { ask: openClaim, run: openClaimRun ?? 0 } } : {}
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
function isHumanComment(comment) {
  return isHumanActor(comment.user?.type);
}
function entryOf(comment) {
  const isHuman = isHumanComment(comment);
  const body = comment.body ?? "";
  return {
    body,
    isHuman,
    quoted: !isHuman && QUOTES_TAG.has(body),
    at: comment.created_at
  };
}
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
function checkDispatchMarker(repo, number, markerId) {
  const comments = listComments(repo, number);
  const marker = String(markerId);
  const markerVisible = comments.some((comment) => String(comment.id) === marker);
  const entries = comments.filter((comment) => String(comment.id) !== marker).map(entryOf);
  return { markerVisible, outstanding: requestOutstanding(shapedThread(entries, readers).events) };
}
function readAskState(repo, number, probeId) {
  const comments = listComments(repo, number);
  const probe = probeId === undefined ? undefined : String(probeId);
  const entries = comments.map((comment) => ({
    id: comment.id,
    ...entryOf(comment)
  }));
  return {
    probeVisible: probe === undefined ? undefined : comments.some((comment) => String(comment.id) === probe),
    comments: shapedThread(entries, readers).comments
  };
}
function isAskSuperseded(repo, number, args) {
  const read = readAskState(repo, number, args.probeId);
  return {
    superseded: askSuperseded(read.comments, readers, args.askId, args.agent, args.verdictPassed),
    probeVisible: read.probeVisible
  };
}

// src/adapters/github/work-tree.ts
function commentOnWithId(repo, number, body) {
  const { code, stdout } = gh("api", `repos/${repo}/issues/${number}/comments`, "--method", "POST", "-f", `body=${body}`, "--jq", ".id");
  if (code !== 0)
    return null;
  return stdout.trim() || null;
}

// src/adapters/runner/config.ts
import { readFileSync } from "fs";

// src/domain/delivery/merge-readiness.ts
var CI_WOULD_BE_WASTED = new Set([
  "not-open",
  "draft",
  "conflicting",
  "behind",
  "mergeability-unknown",
  "checks-pending",
  "checks-failing"
]);
var PASSING = new Set(["success", "neutral", "skipped"]);

// src/domain/delivery/declared-secrets.ts
var RUN_CREDENTIALS = [
  "OPENAI_API_KEY",
  "OPENROUTER_API_KEY",
  "ORCAROUTER_API_KEY",
  "ANTHROPIC_API_KEY",
  "ATOMA_COPILOT_TOKEN",
  "GH_TOKEN"
];
var AGENT_ENV_NAMES = [
  "HOME",
  "PATH",
  "AGENT",
  "ATOMATON_UNTRUSTED_USER",
  MACHINERY_ROOT_VAR,
  "GITHUB_REPOSITORY",
  "BRANCH",
  "NUMBER",
  "NOTIFY",
  "TYPE",
  "ATOMATON_RELOAD_COUNT",
  "ATOMATON_OPS_LOG",
  "ATOMATON_RUN_DIR",
  "ATOMATON_RUN_URL",
  "ATOMATON_DISPATCHED_BY",
  "ATOMATON_ASK_ID",
  "GITHUB_RUN_ID",
  "XDG_CACHE_HOME",
  "XDG_CONFIG_HOME",
  "XDG_DATA_HOME",
  "BUN_INSTALL_CACHE_DIR",
  "npm_config_cache",
  "PIP_CACHE_DIR",
  "CARGO_HOME",
  "OPENAI_BASE_URL",
  "ATOMA_PROVIDER"
];
var RUN_STEP_NAMES = [
  "OPENROUTER_BASE_URL",
  "ORCAROUTER_BASE_URL",
  "ANTHROPIC_BASE_URL",
  "COPILOT_BASE_URL",
  "ATOMA_PROVIDER_IN",
  "OPENAI_BASE_URL_IN"
];
var TOOL_SECRETS = {
  field: "tools.secrets",
  reserved: new Set([...RUN_CREDENTIALS, ...AGENT_ENV_NAMES, ...RUN_STEP_NAMES])
};
var JOB_ENV = ["ATOMATON_COMMANDS", "GH_TOKEN"];
var CHECK_JOB_RESERVED = new Set([...JOB_ENV, "ATOMATON_PR_TREE"]);
var DEPLOY_JOB_RESERVED = new Set([...JOB_ENV, "ATOMATON_DEPLOY_TARGET"]);

// src/domain/delivery/check-jobs.ts
var CHECKS_FROM_PULL_REQUEST = {
  where: "checks.from_pull_request",
  secrets: {
    refused: "These commands come from the pull request, which may rewrite them, so a credential " + "named beside them is one the change being judged can read. Move the check to " + "`checks.from_default_branch`, where the commands come from a branch a person approved."
  }
};
var NO_PULL_REQUEST_CHECKS = "This check verified nothing: `checks.from_pull_request` in .github/atomaton/config.yaml is empty, " + "so a pull request satisfying it has not been tested. Add the commands that check this project, " + "or point `checks.your_workflow` at a workflow of your own.";

// src/domain/delivery/declared-config.ts
var NEW_RELEASE_MODES = ["issue", "pr", "pr-agent"];
function isNewReleaseMode(value) {
  return typeof value === "string" && NEW_RELEASE_MODES.includes(value);
}

// src/adapters/runner/config.ts
function configPath() {
  return machineryPath(CONFIG_FILE);
}
var cached;
function loadConfig() {
  if (!cached) {
    cached = Bun.YAML.parse(readFileSync(configPath(), "utf8"));
  }
  return cached;
}
function getNewReleaseMode(fallback = "issue") {
  const mode = loadConfig().update?.on_new_release;
  return isNewReleaseMode(mode) ? mode : fallback;
}
function getUpdateConflictAgent() {
  return (loadConfig().agents?.on_update_conflict ?? "").trim();
}
function tryGetUpdateConflictPolicy() {
  try {
    return {
      agent: getNewReleaseMode() === "pr-agent" ? getUpdateConflictAgent() : "",
      readable: true
    };
  } catch {
    return { agent: "", readable: false };
  }
}

// src/adapters/runner/ops-log.ts
import { appendFileSync } from "fs";
var OPS_LOG_PATH = process.env.ATOMATON_OPS_LOG ?? "/tmp/atomaton_ops.log";
function logOp(op, payload = {}) {
  const entry = { ts: new Date().toISOString(), op, ...payload };
  try {
    appendFileSync(OPS_LOG_PATH, JSON.stringify(entry) + `
`);
  } catch (e) {
    console.error(`[ops-log] WARN: failed to write op log: ${e}`);
  }
}
function logDispatch(target, agent, extra = {}) {
  const { elsewhere, ...rest } = extra;
  logOp(elsewhere === true ? "dispatch-elsewhere" : "dispatch", { target, agent, ...rest });
}

// src/adapters/runner/run-node.ts
function nodeNumber() {
  const parsed = Number((process.env.NUMBER ?? "").trim());
  return Number.isInteger(parsed) && parsed > 0 ? parsed : undefined;
}

// src/adapters/github/target-state.ts
function readTargetState(number, repo) {
  const path = repo ? `repos/${repo}/issues/${number}` : `repos/{owner}/{repo}/issues/${number}`;
  const { code, stdout, stderr } = ghRead("api", path);
  if (code !== 0) {
    return { known: false, why: (stderr || stdout || `gh exited ${code}`).trim().split(`
`)[0] ?? "" };
  }
  let parsed;
  try {
    parsed = JSON.parse(stdout);
  } catch {
    return { known: false, why: "the response was not JSON" };
  }
  const isPr = parsed.pull_request !== undefined;
  const kind = isPr ? "pull-request" : "issue";
  const labels = (parsed.labels ?? []).map((label) => label.name ?? "").filter((name) => name !== "");
  if (parsed.state === "open")
    return { known: true, kind, state: "open", labels };
  if (parsed.state === "closed") {
    return {
      known: true,
      kind,
      state: isPr ? pullRequestOutcome(Boolean(parsed.pull_request?.merged_at)) : issueOutcome(parsed.state_reason),
      labels
    };
  }
  return { known: false, why: `unrecognised state ${JSON.stringify(parsed.state ?? null)}` };
}

// src/domain/work/closed-target.ts
function mayStartWorkOn(target) {
  return target.known && target.state === "open";
}
function isMerged(target) {
  return target.known && target.kind === "pull-request" && target.state === "done";
}
function canBeReopened(target) {
  return target.known && !isMerged(target);
}
function recoveryAdvice(state, number, command) {
  if (state.known && !canBeReopened(state)) {
    return `#${number} is merged, and GitHub cannot reopen a merged pull request. ` + `Open an issue for the follow-up instead.`;
  }
  return `Reopen #${number} and comment \`${command}\` to run it.`;
}
function mentionList(logins) {
  return logins.length > 0 ? `${logins.map((l) => `@${l}`).join(" ")} ` : "";
}
function dispatchRefusedNotice(refused) {
  const { agent, number, context, state, notify } = refused;
  const why = !state.known ? `the state of #${number} could not be read (${state.why})` : `#${number} is closed`;
  return [
    `${mentionList(notify ? [notify] : [])}Atomaton: \`${agent}\` was not started on #${number}, because ${why}.`,
    "",
    `What was about to happen: ${context}.`,
    "",
    "Nothing will retry this.",
    "",
    !state.known ? `Start it by hand once #${number} can be read: comment \`/${agent}\` on it.` : recoveryAdvice(state, number, `/${agent}`)
  ].join(`
`);
}
function dispatchFailedNotice(failed) {
  const { agent, number, context, notify } = failed;
  return [
    `${mentionList(notify ? [notify] : [])}Atomaton: \`${agent}\` was not started on #${number}, because GitHub rejected the dispatch call.`,
    "",
    `What was about to happen: ${context}.`,
    "",
    "Nothing will retry this. Start it by hand: comment `/" + agent + "` on this issue."
  ].join(`
`);
}
function dispatchUnconfirmedNotice(unconfirmed) {
  const { agent, number, context, notify } = unconfirmed;
  return [
    `${mentionList(notify ? [notify] : [])}Atomaton: \`${agent}\` was not started on #${number}, because the dispatch could not be confirmed.`,
    "",
    `What was about to happen: ${context}.`,
    "",
    "The comment that records a dispatch could not be posted or was not visible in the thread, so the machinery could not tell whether another run had already been asked for. Starting one anyway would leave a run nothing on this issue accounts for.",
    "",
    "This is usually transient. Retry shortly by commenting `/`" + agent + "` on this issue."
  ].join(`
`);
}

// src/adapters/actions/dispatch.ts
function runnerWorkflow() {
  return process.env.ATOMATON_DISPATCH_WORKFLOW || "atomaton-runner.yml";
}
function refusalEndingLines(d) {
  if (d.refusalEnding === undefined)
    return [];
  return [
    LLM_CONTEXT_TAG.write("exclude"),
    ENDED_TAG.write({ verb: "refused", answers: d.refusalEnding.answers, run: d.refusalEnding.run })
  ];
}
function refuseClosedTarget(d, state) {
  const log = d.log ?? ((message) => console.error(message));
  const body = [
    ...refusalEndingLines(d),
    dispatchRefusedNotice({
      agent: d.agent,
      number: Number(d.number),
      context: d.context,
      state,
      notify: d.notify ?? ""
    })
  ].join(`
`);
  const { code, stdout, stderr } = gh("issue", "comment", String(d.number), ...d.repo ? ["--repo", d.repo] : [], "--body", body);
  if (code !== 0) {
    log(`${d.context}: refused to dispatch onto #${d.number} (not open), and could not post the notice: ${stderr || stdout}`);
  } else {
    log(`${d.context}: refused to dispatch onto #${d.number} (not open); notice posted`);
  }
  return "refused-closed";
}
function postDispatchMarker(d) {
  const log = d.log ?? ((message) => console.error(message));
  const body = `${LLM_CONTEXT_TAG.write("exclude")}
${DISPATCH_TAG.write(d.agent)}
` + `Atomaton: \`${d.agent}\` starting on this ${d.type === "pr" ? "pull request" : "issue"}.`;
  const { code, stdout, stderr } = gh("api", `repos/${d.repo ?? "{owner}/{repo}"}/issues/${d.number}/comments`, "--method", "POST", "-f", `body=${body}`, "--jq", ".id");
  if (code !== 0) {
    log(`${d.context}: could not post the dispatch marker on #${d.number}: ${stderr || stdout}`);
    return;
  }
  return stdout.trim();
}
function refuseOutstandingRequest(d, markerId, ownMarker) {
  const log = d.log ?? ((message) => console.error(message));
  const repoPath = d.repo ?? "{owner}/{repo}";
  const removeMarker = () => {
    if (markerId === undefined)
      return;
    gh("api", "--method", "DELETE", `repos/${repoPath}/issues/comments/${markerId}`);
  };
  if (markerId === undefined) {
    const ownAsk = d.ask === undefined ? "" : String(d.ask).trim();
    if (ownAsk === "") {
      return refuseUnconfirmed(d, `the dispatch marker on #${d.number} could not be posted, so nothing on the thread would record this dispatch`);
    }
    return;
  }
  const delays = [1000, 2000, 4000];
  let check;
  for (let attempt = 0;attempt <= delays.length; attempt++) {
    try {
      check = checkDispatchMarker(repoPath, d.number, markerId);
    } catch (e) {
      removeMarker();
      log(`${d.context}: could not read the thread on #${d.number}, so ${d.agent} was not started: ${e}`);
      return "refused-outstanding";
    }
    if (check.markerVisible)
      break;
    const delay = delays[attempt];
    if (delay === undefined)
      break;
    log(`${d.context}: the dispatch marker on #${d.number} is not visible yet; waiting ${delay}ms and reading again`);
    Bun.sleepSync(delay);
  }
  if (check === undefined || !check.markerVisible) {
    if (ownMarker) {
      const reposted = postDispatchMarker(d);
      if (reposted !== undefined) {
        try {
          if (checkDispatchMarker(repoPath, d.number, reposted).markerVisible) {
            removeMarker();
            return;
          }
        } catch (e) {
          log(`${d.context}: could not read the thread on #${d.number} after reposting the marker: ${e}`);
        }
        gh("api", "--method", "DELETE", `repos/${repoPath}/issues/comments/${reposted}`);
      }
    }
    removeMarker();
    return refuseUnconfirmed(d, `the dispatch marker on #${d.number} never became visible, so the ordering check could not be trusted`);
  }
  if (!check.outstanding)
    return;
  removeMarker();
  log(`${d.context}: #${d.number} already has an agent asked for on it, so ${d.agent} was not started; ` + "the dispatch marker was removed");
  return "refused-outstanding";
}
function refuseUnconfirmed(d, why) {
  const log = d.log ?? ((message) => console.error(message));
  log(`${d.context}: ${why}; ${d.agent} was not started`);
  const body = [
    ...refusalEndingLines(d),
    dispatchUnconfirmedNotice({
      agent: d.agent,
      number: Number(d.number),
      context: d.context,
      notify: d.notify ?? ""
    })
  ].join(`
`);
  const { code, stdout, stderr } = gh("issue", "comment", String(d.number), ...d.repo ? ["--repo", d.repo] : [], "--body", body);
  if (code !== 0) {
    log(`${d.context}: could not post the unconfirmed notice on #${d.number}: ${stderr || stdout}`);
  }
  return "unconfirmed";
}
function refuseSupersededAsk(d) {
  const log = d.log ?? ((message) => console.error(message));
  const repoPath = d.repo ?? "{owner}/{repo}";
  const askId = Number(d.ask);
  if (!Number.isInteger(askId) || askId <= 0) {
    log(`${d.context}: the dispatch answers a request on #${d.number} but names no ask comment, ` + `so ${d.agent} was not started`);
    return "superseded";
  }
  const delays = [1000, 2000, 4000];
  let read;
  for (let attempt = 0;attempt <= delays.length; attempt++) {
    try {
      read = isAskSuperseded(repoPath, d.number, {
        askId,
        agent: d.agent,
        verdictPassed: d.verdictPassed,
        probeId: d.freshnessProbeId
      });
    } catch (e) {
      log(`${d.context}: could not read the thread on #${d.number}, so ${d.agent} was not started: ${e}`);
      return "superseded";
    }
    if (d.freshnessProbeId === undefined || read.probeVisible)
      break;
    const delay = delays[attempt];
    if (delay === undefined)
      break;
    log(`${d.context}: the comment this dispatch answers on #${d.number} is not visible yet; ` + `waiting ${delay}ms and reading again`);
    Bun.sleepSync(delay);
  }
  if (read === undefined || d.freshnessProbeId !== undefined && !read.probeVisible) {
    log(`${d.context}: whether #${d.number} still waits for ${d.agent} could not be established ` + "(the thread would not confirm it had caught up), so nothing was started");
    return "superseded";
  }
  const why = read.superseded;
  if (why === undefined)
    return;
  log(`${d.context}: the ask this dispatch answers on #${d.number} is no longer waiting ` + `(${why}), so ${d.agent} was not started`);
  return "superseded";
}
function dispatchRunner(d) {
  if (!d.agent.trim()) {
    const log = d.log ?? ((message) => console.error(message));
    log(`${d.context}: no agent was named, so nothing was dispatched (an empty agent is not a run).`);
    return "failed";
  }
  const state = readTargetState(d.number, d.repo);
  if (!mayStartWorkOn(state))
    return refuseClosedTarget(d, state);
  let ask;
  let markerId;
  if (d.answersRequest) {
    const refusal = refuseSupersededAsk(d);
    if (refusal !== undefined)
      return refusal;
    ask = d.ask;
  } else {
    const provided = d.markerCommentId === undefined ? "" : String(d.markerCommentId).trim();
    markerId = provided === "" ? postDispatchMarker(d) : provided;
    const refusal = refuseOutstandingRequest(d, markerId, provided === "");
    if (refusal !== undefined)
      return refusal;
    ask = d.ask ?? markerId;
  }
  const args = [
    ...d.repo ? ["--repo", d.repo] : [],
    "--field",
    `agent=${d.agent}`,
    "--field",
    `number=${d.number}`,
    "--field",
    `type=${d.type}`,
    "--field",
    `notify=${d.notify ?? ""}`,
    "--field",
    `ask=${ask ?? ""}`,
    "--field",
    `reload_count=${d.reloadCount ?? 0}`,
    "--field",
    `dispatched_by=${d.dispatchedBy ?? ""}`
  ];
  if (!dispatchWorkflow(d.context, runnerWorkflow(), args, d.log)) {
    if (d.refusalEnding !== undefined) {
      if (markerId !== undefined) {
        gh("api", "--method", "DELETE", `repos/${d.repo ?? "{owner}/{repo}"}/issues/comments/${markerId}`);
      }
      const body = [
        ...refusalEndingLines(d),
        dispatchFailedNotice({
          agent: d.agent,
          number: Number(d.number),
          context: d.context,
          notify: d.notify ?? ""
        })
      ].join(`
`);
      gh("issue", "comment", String(d.number), ...d.repo ? ["--repo", d.repo] : [], "--body", body);
    }
    return "failed";
  }
  const here = nodeNumber();
  const elsewhere = d.continuesChain !== true && here !== undefined && String(d.number) !== String(here);
  logDispatch(d.type, d.agent, { number: Number(d.number), ...elsewhere ? { elsewhere: true } : {} });
  return "dispatched";
}
// src/adapters/github/branch-placement.ts
var NO_BRANCH_MESSAGE = "This run is on a detached checkout with no local branch, so there is no branch to push: " + "commit_and_push and create_pr cannot publish this run's work. Report the work on the issue instead.";

// src/domain/delivery/deploy-jobs.ts
function refPatternProblem(pattern) {
  const body = pattern.endsWith("*") ? pattern.slice(0, -1) : pattern;
  if (body.includes("*")) {
    return `"${pattern}" uses a '*' somewhere other than the end, which this matcher cannot honour, ` + 'so it would match nothing. Write a literal ref, or a prefix followed by "*" \u2014 e.g. "v*".';
  }
  if (/[?[\]{}]/.test(body)) {
    return `"${pattern}" uses a glob character this matcher cannot honour, so it would match nothing. ` + 'Write a literal ref, or a prefix followed by "*".';
  }
  return "";
}
function readPatterns(raw, key, required, where, problems) {
  const list = raw ?? [];
  if (!Array.isArray(list) || list.some((p) => typeof p !== "string" || p.trim() === "")) {
    problems.push(`${where}: \`${key}\` must be an array of non-empty patterns.`);
    return null;
  }
  const patterns = list.map((p) => p.trim());
  const bad = patterns.map(refPatternProblem).find((problem) => problem !== "");
  if (bad) {
    problems.push(`${where}: ${bad}`);
    return null;
  }
  if (required && patterns.length === 0) {
    problems.push(`${where}: \`${key}\` needs at least one pattern \u2014 e.g. ["v*"].`);
    return null;
  }
  return patterns;
}
function refsFrom(keys) {
  const owned = [...keys.tags ? ["tags"] : [], ...keys.branches ? ["branches"] : []];
  return {
    keys: owned,
    read: (entry, where, problems) => {
      const tags = keys.tags ? readPatterns(entry.tags, "tags", true, where, problems) : [];
      const branches = keys.branches ? readPatterns(entry.branches, "branches", false, where, problems) : [];
      return tags === null || branches === null ? null : { tags, branches };
    }
  };
}
var DEPLOY_ARMS = {
  merge: {
    key: "on_merge",
    rules: {
      where: "deploy.on_merge",
      secrets: { reserved: DEPLOY_JOB_RESERVED },
      extra: refsFrom({ branches: true })
    }
  },
  tag: {
    key: "on_tag",
    rules: {
      where: "deploy.on_tag",
      secrets: { reserved: DEPLOY_JOB_RESERVED },
      extra: refsFrom({ branches: true, tags: true })
    }
  },
  demand: {
    key: "on_demand",
    rules: {
      where: "deploy.on_demand",
      secrets: { reserved: DEPLOY_JOB_RESERVED },
      extra: { keys: [], read: () => ({ branches: [], tags: [] }) }
    }
  }
};
var TRIGGERS = Object.keys(DEPLOY_ARMS);

// src/adapters/actions/dispatch-targets.ts
function log(message) {
  console.error(`[atomaton-github] ${message}`);
}
function dispatchUpdateConflictAgent(repo, prNumber, agent) {
  return dispatchDirective(repo, prNumber, agent, {
    context: `the update pull request #${prNumber} stopped on a conflict, so ${agent} was to resolve it`,
    type: "pr",
    directive: "Atomaton: this pull request takes a newer release onto this tree and stopped where the two " + "sides disagree \u2014 the files listed above carry conflict markers naming both. Resolve them, " + "keeping what this repository wrote where it was deliberate and taking what the release " + "changed everywhere else, and say in your report which side you took for each file and why. " + "One limit on where you can write: a conflicted path under `.github/workflows/` is a " + "person's, because GitHub refuses a workflow file written with your token and the push " + "carrying it would be rejected. Leave those files' markers exactly as they are, resolve " + "the others, and name the workflow paths in your report so a person can finish them. A " + "conflict that mixes the two kinds is the same: the workflow files stay for the person, " + "and you resolve the rest. " + "Nothing merges from here: the repository's checks run on the resolved tree, and the merge is " + "a person's."
  }) === "dispatched";
}
function dispatchDirective(repo, number, agent, request) {
  const body = `${LLM_CONTEXT_TAG.write("include")}
${DISPATCH_TAG.write(agent)}

${request.directive}`;
  const markerCommentId = commentOnWithId(repo, number, body);
  if (markerCommentId === null) {
    log(`${request.context}: could not post the directive on #${number}, so ${agent} was not started`);
    return "failed";
  }
  return dispatchRunner({
    context: request.context,
    agent,
    type: request.type ?? "issue",
    number,
    notify: request.notify ?? "",
    repo,
    markerCommentId,
    ...request.continuesChain === true ? { continuesChain: true } : {},
    log
  });
}

// src/entrypoints/machinery/lib/update-branch.ts
function updateBranchFor(tag) {
  return `atomaton/update-${tag}`;
}

// src/entrypoints/machinery/lib/notice-issue.ts
var NOTICE_MARKER = "<!-- atomaton:new-release -->";
function findOpenNotice(repo) {
  const result = ghRead("issue", "list", "--repo", repo, "--state", "open", "--search", `"Atomaton release" in:title`, "--json", "number,body,state");
  if (result.code !== 0) {
    log2(`WARN could not look for an existing notice in ${repo}: ${result.stderr.trim() || `gh exited ${result.code}`}`);
    return null;
  }
  let parsed;
  try {
    parsed = JSON.parse(result.stdout || "[]");
  } catch {
    log2(`WARN the issue search in ${repo} did not answer with JSON`);
    return null;
  }
  if (!Array.isArray(parsed)) {
    log2(`WARN the issue search in ${repo} did not answer with a list`);
    return null;
  }
  const found = parsed.find((entry) => {
    const issue = entry;
    return typeof issue.body === "string" && issue.body.includes(NOTICE_MARKER) && (issue.state === undefined || issue.state === "OPEN");
  });
  const number = Number(found?.number ?? 0);
  return Number.isInteger(number) && number > 0 ? number : 0;
}
function log2(message) {
  console.error(`[notice-issue] ${message}`);
}

// src/entrypoints/machinery/lib/update-apply.ts
import { existsSync, mkdirSync, mkdtempSync, readFileSync as readFileSync2, rmSync, statSync, writeFileSync } from "fs";
import { tmpdir } from "os";
import { basename as basename2, dirname, join, resolve, sep } from "path";
function applyPlan(treeRoot, plan, base, target, ours) {
  const root = resolve(treeRoot);
  const inside = (path) => {
    const destination = resolve(join(root, path));
    if (destination !== root && !destination.startsWith(root + sep))
      return null;
    return destination;
  };
  for (const path of [...plan.take, ...plan.add, ...plan.delete, ...plan.merge]) {
    if (inside(path) === null)
      return { why: `\`${path}\` resolves outside the tree` };
  }
  const writes = new Map;
  const deletes = new Set;
  const taken = [];
  const added = [];
  const removed = [];
  const merged = [];
  const conflicts = [];
  for (const path of plan.take) {
    const content = target.get(path);
    if (content === undefined) {
      return { why: `the target release does not carry \`${path}\`, which the plan says to take` };
    }
    writes.set(path, content);
    taken.push(path);
  }
  for (const path of plan.add) {
    const content = target.get(path);
    if (content === undefined) {
      return { why: `the target release does not carry \`${path}\`, which the plan says to add` };
    }
    const destination = inside(path);
    if (destination === null)
      return { why: `\`${path}\` resolves outside the tree` };
    if (existsSync(destination)) {
      return { why: `\`${path}\` exists in the tree but the plan did not expect it` };
    }
    writes.set(path, content);
    added.push(path);
  }
  for (const path of plan.delete) {
    deletes.add(path);
    removed.push(path);
  }
  const scratch = (() => {
    try {
      return mkdtempSync(join(tmpdir(), "atomaton-merge-"));
    } catch (error) {
      return null;
    }
  })();
  if (scratch === null) {
    return { why: `could not make a scratch directory for the merges` };
  }
  try {
    for (const path of plan.merge) {
      const baseContent = base.get(path);
      const oursContent = ours.get(path);
      const targetContent = target.get(path);
      if (baseContent === undefined || oursContent === undefined || targetContent === undefined) {
        return { why: `\`${path}\` is marked for a merge this run cannot read all three sides of` };
      }
      const outcome = mergeFile(scratch, basename2(path) || path, baseContent, oursContent, targetContent);
      if ("why" in outcome)
        return { why: outcome.why };
      writes.set(path, outcome.content);
      (outcome.conflicts > 0 ? conflicts : merged).push(path);
    }
  } finally {
    try {
      rmSync(scratch, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 });
    } catch {}
  }
  for (const path of writes.keys()) {
    let parent = dirname(path);
    while (parent !== "." && parent !== "/") {
      const stats = statSync(resolve(join(root, parent)), { throwIfNoEntry: false });
      if (stats?.isFile()) {
        if (!deletes.has(parent)) {
          return { why: `\`${path}\` needs \`${parent}\` to be a directory, but the tree holds a file there that the plan does not delete` };
        }
      }
      const next = dirname(parent);
      parent = next;
    }
  }
  for (const [path, content] of writes) {
    const destination = inside(path);
    if (destination === null)
      throw new Error(`\`${path}\` resolves outside the tree`);
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, content);
  }
  for (const path of deletes) {
    try {
      rmSync(resolve(join(root, path)), { force: true });
    } catch (error) {
      return { why: `could not remove \`${path}\`: ${error.message}` };
    }
  }
  return { result: { taken, added, removed, merged, conflicts } };
}
function mergeFile(scratch, label, base, ours, theirs) {
  const oursPath = join(scratch, "ours");
  const basePath = join(scratch, "base");
  const theirsPath = join(scratch, "theirs");
  writeFileSync(oursPath, ours);
  writeFileSync(basePath, base);
  writeFileSync(theirsPath, theirs);
  const run = Bun.spawnSync({
    cmd: [
      "git",
      "merge-file",
      "-L",
      `${label} (ours)`,
      "-L",
      `${label} (base)`,
      "-L",
      `${label} (theirs)`,
      oursPath,
      basePath,
      theirsPath
    ],
    stdout: "pipe",
    stderr: "pipe"
  });
  const code = run.exitCode ?? 1;
  if (code === 255) {
    return { why: `git merge-file could not run: ${run.stderr?.toString("utf8").trim() || "no message"}` };
  }
  if (code < 0) {
    return { why: `the merge of ${label} was interrupted: ${run.stderr?.toString("utf8").trim() || "no message"}` };
  }
  return { content: readFileSync2(oursPath, "utf8"), conflicts: code };
}

// src/entrypoints/machinery/lib/release-files.ts
import { existsSync as existsSync2, mkdtempSync as mkdtempSync2, readFileSync as readFileSync4, readdirSync, rmSync as rmSync2 } from "fs";
import { tmpdir as tmpdir2 } from "os";
import { join as join2 } from "path";

// src/entrypoints/machinery/lib/zip-read.ts
import { readFileSync as readFileSync3 } from "fs";
import { inflateRawSync } from "zlib";
var EOCD_SIG = 101010256;
var CENTRAL_SIG = 33639248;
var LOCAL_SIG = 67324752;
var EOCD_MAX_SEARCH = 22 + 65535;
function zipEntries(path) {
  let buf;
  try {
    buf = readFileSync3(path);
  } catch (error) {
    return { why: `could not read the archive at ${path}: ${error.message}` };
  }
  const floor = Math.max(0, buf.length - EOCD_MAX_SEARCH);
  let eocd = -1;
  for (let at = buf.length - 22;at >= floor; at--) {
    if (buf.readUInt32LE(at) === EOCD_SIG) {
      eocd = at;
      break;
    }
  }
  if (eocd === -1) {
    return { why: `${path} does not end like a zip archive (no end-of-central-directory record)` };
  }
  const entryCount = buf.readUInt16LE(eocd + 10);
  const centralOffset = buf.readUInt32LE(eocd + 16);
  const raw = [];
  let at = centralOffset;
  for (let index = 0;index < entryCount; index++) {
    if (at + 46 > buf.length || buf.readUInt32LE(at) !== CENTRAL_SIG) {
      return { why: `${path}'s central directory ends after ${index} of ${entryCount} entries` };
    }
    const method = buf.readUInt16LE(at + 10);
    const compressedSize = buf.readUInt32LE(at + 20);
    const nameLength = buf.readUInt16LE(at + 28);
    const extraLength = buf.readUInt16LE(at + 30);
    const commentLength = buf.readUInt16LE(at + 32);
    const localOffset = buf.readUInt32LE(at + 42);
    const name = buf.subarray(at + 46, at + 46 + nameLength).toString("utf8");
    if (commentLength !== 0) {
      return { why: `${path}'s entry ${name} carries a central-directory comment, which this reader does not handle` };
    }
    if (localOffset + 30 > buf.length || buf.readUInt32LE(localOffset) !== LOCAL_SIG) {
      return { why: `${path}'s entry ${name} points at a local header that is not one` };
    }
    const localNameLength = buf.readUInt16LE(localOffset + 26);
    const localExtraLength = buf.readUInt16LE(localOffset + 28);
    const dataAt = localOffset + 30 + localNameLength + localExtraLength;
    if (dataAt + compressedSize > buf.length) {
      return { why: `${path}'s entry ${name} claims more bytes than the archive holds` };
    }
    raw.push({ name, method, compressed: buf.subarray(dataAt, dataAt + compressedSize) });
    at += 46 + nameLength + extraLength + commentLength;
  }
  const entries = new Map;
  for (const entry of raw) {
    if (entry.name.endsWith("/"))
      continue;
    let content;
    try {
      content = entry.method === 0 ? Buffer.from(entry.compressed).toString("utf8") : inflateRawSync(entry.compressed).toString("utf8");
    } catch (error) {
      return { why: `${path}'s entry ${entry.name} could not be expanded: ${error.message}` };
    }
    if (!entries.has(entry.name))
      entries.set(entry.name, content);
  }
  return { entries };
}

// src/entrypoints/machinery/lib/release-files.ts
function log3(message) {
  console.error(`[release-files] ${message}`);
}
var UPSTREAM_REPOSITORY = "yuma-seno/atomaton";
var ASSET_NAME = "atomaton-delivery.zip";
function releaseFiles(tag) {
  const dir = mkdtempSync2(join2(tmpdir2(), "atomaton-release-"));
  try {
    const downloaded = ghRead("release", "download", tag, "-R", UPSTREAM_REPOSITORY, "-p", ASSET_NAME, "-D", dir);
    if (downloaded.code !== 0) {
      return { why: `could not download ${tag}'s ${ASSET_NAME}: ${downloaded.stderr.trim() || `gh exited ${downloaded.code}`}` };
    }
    const asset = join2(dir, ASSET_NAME);
    if (!existsSync2(asset)) {
      return { why: `${tag}'s download reported success but left no ${ASSET_NAME}` };
    }
    const read = zipEntries(asset);
    if ("why" in read) {
      return { why: `could not read ${tag}'s ${ASSET_NAME}: ${read.why}` };
    }
    const manifestJson = read.entries.get(RELEASE_MANIFEST);
    if (manifestJson === undefined) {
      return { why: `${tag}'s ${ASSET_NAME} carries no ${RELEASE_MANIFEST}` };
    }
    let manifest;
    try {
      const parsed = JSON.parse(manifestJson);
      if (!Array.isArray(parsed.files)) {
        return { why: `${RELEASE_MANIFEST} in ${tag}'s ${ASSET_NAME} holds no file list` };
      }
      manifest = { version: String(parsed.version ?? tag), files: parsed.files.map(String) };
    } catch (error) {
      return { why: `could not parse the ${RELEASE_MANIFEST} inside ${tag}'s ${ASSET_NAME}: ${error.message}` };
    }
    const files = new Map;
    for (const path of manifest.files) {
      const content = read.entries.get(path);
      if (content !== undefined)
        files.set(path, content);
    }
    return { files: { manifest, files } };
  } finally {
    try {
      rmSync2(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 });
    } catch {}
  }
}
function readTreeFiles(paths) {
  const files = new Map;
  for (const path of paths) {
    const onDisk = machineryPath(path);
    if (!existsSync2(onDisk))
      continue;
    try {
      files.set(path, readFileSync4(onDisk, "utf8"));
    } catch {}
  }
  return files;
}
function treePaths() {
  const root = machineryPath(".github");
  if (!existsSync2(root))
    return [];
  const found = [];
  const walk = (dir, prefix) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const here = `${prefix}/${entry.name}`;
      if (entry.isDirectory())
        walk(join2(dir, entry.name), here);
      else
        found.push(here);
    }
  };
  walk(root, ".github");
  return found.sort();
}
function readLocalManifest() {
  const path = machineryPath(RELEASE_MANIFEST);
  if (!existsSync2(path)) {
    return { why: `${path} is missing, so this tree cannot say which release it came from` };
  }
  try {
    const parsed = JSON.parse(readFileSync4(path, "utf8"));
    if (typeof parsed.version !== "string" || !parsed.version || !Array.isArray(parsed.files)) {
      return { why: `${path} does not hold a version and a file list` };
    }
    return { manifest: { version: parsed.version, files: parsed.files.map(String) } };
  } catch (error) {
    return { why: `${path} could not be parsed: ${error.message}` };
  }
}
function latestReleaseTag() {
  const result = ghRead("api", `repos/${UPSTREAM_REPOSITORY}/releases/latest`, "--jq", ".tag_name");
  if (result.code !== 0) {
    log3(`WARN could not read ${UPSTREAM_REPOSITORY}'s latest release: ${result.stderr.trim() || `gh exited ${result.code}`}`);
    return null;
  }
  const tag = result.stdout.trim();
  return tag === "" ? null : tag;
}

// src/entrypoints/machinery/update_atomaton.ts
var ref = defineScript(import.meta.url);
var COMMIT_NAME = "atomaton[bot]";
var COMMIT_EMAIL = "atomaton[bot]@users.noreply.github.com";
function updateTitle(tag) {
  return `Take Atomaton ${tag} onto this tree`;
}
var MAX_LISTED = 15;
function updateBody(report, conflictAgent = "") {
  const { plan } = report;
  const lines = [
    "<!-- atomaton:update -->",
    verdictSentence(plan.base, plan.target, report.conflicts.length, plan.kept.length),
    "",
    "## The plan",
    "",
    "| | |",
    "| --- | --- |",
    `| Files updated (untouched here) | ${plan.take.length} |`,
    `| Files added | ${plan.add.length} |`,
    `| Files removed (unmodified here) | ${plan.delete.length} |`,
    `| Files changed on **both** sides | ${plan.merge.length} |`,
    `| Resolved by the merge itself | ${report.merged.length} |`,
    `| Left with conflict markers | ${report.conflicts.length} |`,
    `| Paths the plan will not touch | ${plan.kept.length} |`,
    ""
  ];
  const list = (title, paths) => {
    if (paths.length === 0)
      return;
    lines.push(title, "", ...paths.slice(0, MAX_LISTED).map((path) => `- \`${path}\``));
    if (paths.length > MAX_LISTED)
      lines.push(`- \u2026and ${paths.length - MAX_LISTED} more`);
    lines.push("");
  };
  list("Conflicted \u2014 resolve here, then the checks finish the job:", report.conflicts);
  list("Merged cleanly by the machinery:", report.merged);
  list("Left alone for a person to look at:", plan.kept.map((entry) => entry.path));
  if (report.leftovers.length > 0) {
    lines.push("## Files upstream no longer ships", "", "Reported only \u2014 the manifest cannot tell these from files the adopter wrote,", "so nothing here deletes them on a guess:", "", ...report.leftovers.slice(0, MAX_LISTED).map((path) => `- \`${path}\``), ...report.leftovers.length > MAX_LISTED ? [`- \u2026and ${report.leftovers.length - MAX_LISTED} more`] : [], "");
  }
  lines.push("## What happens next", "", report.conflicts.length > 0 ? conflictAgent !== "" ? `This pull request does not merge on its own: the conflicted files above carry markers naming both sides. \`${conflictAgent}\` is started on it to resolve them \u2014 the checks run on the resolved tree, and then it can merge. One kind is not the agent's: a path under \`.github/workflows/\` cannot be written with its token, so those markers are left for a person, and a pull request carrying both kinds is handled the same way.` : "This pull request does not merge on its own: the conflicted files above carry markers naming both sides. Comment on this pull request and ask an agent to resolve them \u2014 the checks will run on the resolved tree, and only then can this merge." : plan.kept.length > 0 ? "This pull request does not merge itself: the kept paths above are the plan declining to touch them without a person. Read them \u2014 each is a place this repository's words and the release disagree \u2014 and merge by hand when they read right, or ask an agent there to help." : "The repository's checks run on this pull request; when they pass, the update merges it and closes the notice.", "");
  return `${lines.join(`
`).trimEnd()}
`;
}
function stopComment(report, prNumber, resolution = null) {
  const next = resolution === null ? "Comment on that pull request and ask an agent to resolve them \u2014 nothing merges until someone does." : resolution.started ? `\`${resolution.agent}\` has been started on that pull request to resolve them. A conflicted ` + "path under `.github/workflows/` is left for a person, because an agent's token cannot write " + "a workflow file; a pull request carrying those alongside ordinary conflicts is handled the " + "same way. The checks run on " + "the resolved tree; the merge itself is a person's." : `\`${resolution.agent}\` was to be started on that pull request and could not be \u2014 the run's log says ` + "why. Comment there and ask an agent, or resolve them yourself; nothing merges until someone does.";
  return [
    LLM_CONTEXT_TAG.write("exclude"),
    `Taking ${report.plan.target} stopped at a pull request: ${report.conflicts.length} file(s) changed on both sides.`,
    "",
    ...report.conflicts.map((path) => `- \`${path}\``),
    "",
    `The plan is applied on #${prNumber}, where the conflicts carry markers naming both sides. ${next}`
  ].join(`
`);
}
function keptComment(report, prNumber) {
  return [
    LLM_CONTEXT_TAG.write("exclude"),
    `Taking ${report.plan.target} stopped for a look: ${report.plan.kept.length} path(s) the plan will not touch without a person.`,
    "",
    ...report.plan.kept.map((entry) => `- \`${entry.path}\` (${entry.upstream})`),
    "",
    `The plan is applied on #${prNumber}, which does not merge itself. Read the kept paths there \u2014 ` + "each is this repository's words or files against a release that moved elsewhere \u2014 and merge by hand when they read right, " + "or comment on that pull request and ask an agent to help."
  ].join(`
`);
}
function doneComment(plan, prNumber, merging) {
  return [
    LLM_CONTEXT_TAG.write("exclude"),
    `Taking ${plan.target} onto ${plan.base} is applied on #${prNumber}.`,
    merging ? "The repository's checks passed there, and the merge is made; the merge's push starts the deploy." : "The repository's checks run on it; once they pass, it can merge."
  ].join(`
`);
}
function log4(message) {
  console.error(`[update] ${message}`);
}
function git(args) {
  const run = Bun.spawnSync({ cmd: ["git", ...args], stdout: "pipe", stderr: "pipe" });
  return {
    code: run.exitCode ?? 1,
    stdout: run.stdout?.toString("utf8") ?? "",
    stderr: run.stderr?.toString("utf8") ?? ""
  };
}
function gitOrFail(args, what) {
  const result = git(args);
  if (result.code !== 0) {
    return { fail: `could not ${what}: ${result.stderr.trim() || `git exited ${result.code}`}` };
  }
  return { stdout: result.stdout };
}
function pushArgs(url, token, branch) {
  const github = /^https:\/\/github\.com\/(.+?)(?:\.git)?$/.exec(url);
  if (!github)
    return ["push", "--force", url, `HEAD:refs/heads/${branch}`];
  return [
    "-c",
    "http.extraheader=",
    "-c",
    "http.https://github.com/.extraheader=",
    "push",
    "--force",
    `https://x-access-token:${token}@github.com/${github[1]}`,
    `HEAD:refs/heads/${branch}`
  ];
}
function pushBranch(branch, token) {
  const origin = gitOrFail(["remote", "get-url", "origin"], "read the origin remote");
  if ("fail" in origin)
    return origin;
  const pushed = gitOrFail(pushArgs(origin.stdout.trim(), token, branch), `push ${branch}`);
  return "fail" in pushed ? pushed : { ok: true };
}
function defaultBranch(repo) {
  const result = ghRead("api", `repos/${repo}`, "--jq", ".default_branch");
  if (result.code !== 0) {
    log4(`WARN could not read ${repo}'s default branch: ${result.stderr.trim() || `gh exited ${result.code}`}`);
    return null;
  }
  const branch = result.stdout.trim();
  return branch === "" ? null : branch;
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      repo: { type: "string" },
      source: { type: "string" },
      tag: { type: "string" }
    }
  });
  const repo = (values.repo ?? process.env.GITHUB_REPOSITORY ?? "").trim();
  if (!repo) {
    console.error("usage: update_atomaton.ts --repo OWNER/REPO [--source N] [--tag vX.Y.Z]");
    process.exit(2);
  }
  const source = values.source !== undefined && values.source !== "" ? Number(values.source) : 0;
  const token = process.env.ATOMATON_UPDATE_TOKEN?.trim() ?? "";
  if (!token) {
    console.error("::error::ATOMATON_UPDATE_TOKEN is not set. The update pushes a branch and asks for a merge, " + "which this workflow's own token may not do: a push from it would run none of the checks the " + "merge is meant to gate. Set the repository secret ATOMATON_UPDATE_TOKEN to a classic PAT " + "with repo and workflow scope, or a fine-grained token with Contents and Pull requests read " + "and write and Workflows write \u2014 the release ships workflows, and a push that updates one is " + "refused without it. Then run this again. Nothing has been changed.");
    process.exit(1);
  }
  const local = readLocalManifest();
  if (!("manifest" in local)) {
    console.error(`::error::${local.why}. An update cannot be applied to a tree that cannot say what it is on.`);
    process.exit(1);
  }
  const targetTag = (values.tag ?? "").trim() || latestReleaseTag();
  if (targetTag === null) {
    console.error(`::error::could not read ${UPSTREAM_REPOSITORY}'s latest release, so there is nothing to take. ` + "A failed read is not an answer of 'up to date'.");
    process.exit(1);
  }
  if (targetTag === local.manifest.version) {
    log4(`${local.manifest.version} is the release this tree is on; nothing to take`);
    if (source > 0) {
      const said = gh("issue", "comment", String(source), "--repo", repo, "--body", `${LLM_CONTEXT_TAG.write("exclude")}
This tree is already on ${targetTag}, which is the release asked for. Nothing to do.`);
      if (said.code !== 0)
        log4(`WARN could not answer in #${source}: ${said.stderr.trim()}`);
    }
    return;
  }
  const status = gitOrFail(["status", "--porcelain"], "read the working tree's status");
  if ("fail" in status) {
    console.error(`::error::${status.fail}. The update applies to a checkout, and this run is not standing in one it can trust.`);
    process.exit(1);
  }
  if (status.stdout.trim() !== "") {
    console.error("::error::the working tree is not clean. The update writes whole files and pushes a branch; " + "uncommitted work would be mixed into its commit. Run it from a clean checkout.");
    process.exit(1);
  }
  const baseRelease = releaseFiles(local.manifest.version);
  if ("why" in baseRelease) {
    console.error(`::error::this tree's own release (${local.manifest.version}) could not be read: ${baseRelease.why}`);
    process.exit(1);
  }
  const targetRelease = releaseFiles(targetTag);
  if ("why" in targetRelease) {
    console.error(`::error::the target release (${targetTag}) could not be read: ${targetRelease.why}`);
    process.exit(1);
  }
  const paths = new Set([...baseRelease.files.files.keys(), ...targetRelease.files.files.keys()]);
  const tree = readTreeFiles(paths);
  const plan = planUpdate(local.manifest.version, targetTag, baseRelease.files.files, targetRelease.files.files, tree);
  if (plan.take.length + plan.add.length + plan.delete.length + plan.merge.length === 0) {
    log4(`${targetTag} changes nothing here; nothing to apply`);
    if (source > 0) {
      const said = gh("issue", "comment", String(source), "--repo", repo, "--body", `${LLM_CONTEXT_TAG.write("exclude")}
${targetTag} differs from ${local.manifest.version} by name only: applying it would change nothing on this tree.`);
      if (said.code !== 0)
        log4(`WARN could not answer in #${source}: ${said.stderr.trim()}`);
    }
    return;
  }
  const leftovers = leftoversOf(targetRelease.files.manifest, treePaths());
  const applied = applyPlan(machineryPath("."), plan, baseRelease.files.files, targetRelease.files.files, tree);
  if ("why" in applied) {
    console.error(`::error::the plan could not be applied: ${applied.why}. The tree is left as it was before this run.`);
    process.exit(1);
  }
  const { conflicts, merged } = applied.result;
  log4(`applied: ${applied.result.taken.length} taken, ${applied.result.added.length} added, ` + `${applied.result.removed.length} removed, ${merged.length} merged, ${conflicts.length} conflicted`);
  const branch = updateBranchFor(targetTag);
  const branched = gitOrFail(["switch", "-C", branch], `create the branch ${branch}`);
  if ("fail" in branched) {
    console.error(`::error::${branched.fail}. The tree holds the applied plan but nothing has been committed.`);
    process.exit(1);
  }
  const staged = gitOrFail(["add", "-A"], "stage the applied plan");
  if ("fail" in staged) {
    console.error(`::error::${staged.fail}. The tree holds the applied plan but nothing has been committed.`);
    process.exit(1);
  }
  const committed = gitOrFail([
    "-c",
    `user.name=${COMMIT_NAME}`,
    "-c",
    `user.email=${COMMIT_EMAIL}`,
    "commit",
    "-m",
    updateTitle(targetTag),
    "-m",
    conflicts.length === 0 ? `Applied by the update machinery from ${plan.base}. Nothing here collided.` : `Applied by the update machinery from ${plan.base}. ${conflicts.length} file(s) carry conflict markers for a person or an agent to resolve on the pull request.`
  ], "commit the applied plan");
  if ("fail" in committed) {
    console.error(`::error::${committed.fail}. The tree holds the applied plan but nothing has been committed.`);
    process.exit(1);
  }
  const pushed = pushBranch(branch, token);
  if ("fail" in pushed) {
    console.error(`::error::${pushed.fail}. The commit exists locally as ${branch}; nothing has been opened upstream.`);
    process.exit(1);
  }
  const base = defaultBranch(repo);
  if (base === null) {
    console.error(`::error::could not read ${repo}'s default branch, so no pull request can be opened against it.`);
    process.exit(1);
  }
  const policy = tryGetUpdateConflictPolicy();
  if (!policy.readable) {
    log4("WARN config.yaml could not be read after the plan was applied \u2014 it is one of the paths the merge left with conflict markers. " + "The collision policy is unknown, so no agent is started and the run stops for a person at the pull request.");
  }
  const conflictAgent = policy.agent;
  const report = { plan, conflicts, merged, leftovers };
  const existing = openPrForHead(repo, branch);
  let prNumber;
  if (existing > 0) {
    const edited = gh("pr", "edit", String(existing), "--repo", repo, "--title", updateTitle(targetTag), "--body", updateBody(report, conflictAgent));
    if (edited.code !== 0) {
      console.error(`::error::could not update the pull request #${existing}: ${edited.stderr.trim() || `gh exited ${edited.code}`}`);
      process.exit(1);
    }
    prNumber = existing;
    log4(`updated pull request #${existing}`);
  } else {
    const created = ghAs(token, "pr", "create", "--repo", repo, "--base", base, "--head", branch, "--title", updateTitle(targetTag), "--body", updateBody(report, conflictAgent));
    if (created.code !== 0) {
      console.error(`::error::could not open the pull request: ${created.stderr.trim() || `gh exited ${created.code}`}`);
      process.exit(1);
    }
    const number = /\/pull\/(\d+)/.exec(created.stdout.trim())?.[1];
    if (number === undefined) {
      console.error(`::error::the pull request was opened (${created.stdout.trim()}) but its number could not be read, ` + "so the merge request and the notices cannot reference it.");
      process.exit(1);
    }
    prNumber = Number(number);
    log4(`opened pull request #${prNumber}`);
  }
  const notice = findOpenNotice(repo);
  const selfMerging = conflicts.length === 0 && plan.kept.length === 0;
  let merging = false;
  if (selfMerging) {
    let seen = "";
    for (let attempt = 0;attempt < 90; attempt++) {
      const rollup = readPullRequest(repo, prNumber, "statusCheckRollup");
      seen = rollup.known ? String((rollup.value.statusCheckRollup ?? []).filter((check) => check.name === "atomaton-check").length) : "0";
      if (seen !== "0")
        break;
      Bun.sleepSync(1e4);
    }
    if (seen === "0" || seen === "") {
      log4(`WARN the required check never registered on #${prNumber} in fifteen minutes \u2014 the pull request stands, ` + "and merges when a person or a run asks again.");
    } else if (gh("pr", "checks", String(prNumber), "--repo", repo, "--watch", "--fail-fast", "--interval", "10").code !== 0) {
      log4(`WARN the checks on #${prNumber} did not pass \u2014 the pull request stands with them.`);
    } else {
      const requested = ghAs(token, "pr", "merge", String(prNumber), "--repo", repo, "--merge");
      merging = requested.code === 0;
      if (!merging) {
        log4(`WARN the merge could not be made on #${prNumber}: ${requested.stderr.trim() || `gh exited ${requested.code}`}`);
      }
    }
  }
  if (!selfMerging && plan.kept.length > 0) {
    const said = keptComment(report, prNumber);
    if (notice !== null && notice !== 0) {
      const told = gh("issue", "comment", String(notice), "--repo", repo, "--body", said);
      if (told.code !== 0)
        log4(`WARN could not report to the notice #${notice}: ${told.stderr.trim()}`);
    }
    if (source > 0 && source !== notice) {
      const told = gh("issue", "comment", String(source), "--repo", repo, "--body", said);
      if (told.code !== 0)
        log4(`WARN could not answer in #${source}: ${told.stderr.trim()}`);
    }
    log4(`update to ${targetTag} is applied on #${prNumber}, held for a person: ${plan.kept.length} kept path(s)`);
    return;
  }
  if (conflicts.length > 0) {
    const resolution = conflictAgent === "" ? null : {
      agent: conflictAgent,
      started: dispatchUpdateConflictAgent(repo, prNumber, conflictAgent)
    };
    if (resolution !== null && !resolution.started) {
      log4(`WARN could not start ${conflictAgent} on #${prNumber}; the pull request stands for a person`);
    }
    const said = stopComment(report, prNumber, resolution);
    if (notice !== null && notice !== 0) {
      const told = gh("issue", "comment", String(notice), "--repo", repo, "--body", said);
      if (told.code !== 0)
        log4(`WARN could not report to the notice #${notice}: ${told.stderr.trim()}`);
    }
    if (source > 0 && source !== notice) {
      const told = gh("issue", "comment", String(source), "--repo", repo, "--body", said);
      if (told.code !== 0)
        log4(`WARN could not answer in #${source}: ${told.stderr.trim()}`);
    }
    log4(`stopped at pull request #${prNumber}: ${conflicts.length} conflicted file(s)`);
    return;
  }
  if (merging && notice !== null && notice !== 0) {
    const closed = gh("issue", "close", String(notice), "--repo", repo, "--comment", `${NOTICE_MARKER}
${LLM_CONTEXT_TAG.write("exclude")}
${doneComment(plan, prNumber, merging)}`);
    if (closed.code !== 0)
      log4(`WARN could not close the notice #${notice}: ${closed.stderr.trim()}`);
  }
  if (source > 0 && source !== notice) {
    const said = gh("issue", "comment", String(source), "--repo", repo, "--body", doneComment(plan, prNumber, merging));
    if (said.code !== 0)
      log4(`WARN could not answer in #${source}: ${said.stderr.trim()}`);
  }
  log4(`update to ${targetTag} is on #${prNumber}${merging ? ", merged after the repository's checks passed" : ""}`);
}
if (import.meta.main)
  main();
export {
  doneComment,
  keptComment,
  pushArgs,
  ref,
  stopComment,
  updateBody,
  updateBranchFor,
  updateTitle
};
