#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/post_result_comment.ts
import { appendFileSync, existsSync as existsSync4, readFileSync as readFileSync4 } from "fs";
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
function graphqlArgs(query, variables) {
  const args = ["api", "graphql", "-f", `query=${query}`];
  for (const [key, value] of Object.entries(variables)) {
    args.push("-F", `${key}=${value}`);
  }
  return args;
}
function graphqlResult({ code, stdout, stderr }) {
  if (code !== 0) {
    throw new Error(`GraphQL query failed: ${stderr || stdout.slice(0, 200)}`);
  }
  const result = JSON.parse(stdout);
  if (result.errors) {
    throw new Error(`GraphQL errors: ${JSON.stringify(result.errors)}`);
  }
  return result.data;
}
function ghGraphqlRead(query, variables = {}) {
  return graphqlResult(ghRead(...graphqlArgs(query, variables)));
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
function mentionPrefix(login) {
  const name = (login ?? "").trim();
  return name ? `@${name} ` : "";
}
function escapeUnknownMentions(text, known) {
  const allowed = new Set([...known].map((login) => login.trim().toLowerCase()).filter(Boolean));
  const escaped = [];
  const transform = (segment) => segment.replace(MENTION, (whole, before, login) => {
    if (allowed.has(login.toLowerCase()))
      return whole;
    if (!escaped.includes(login))
      escaped.push(login);
    return `${before}\`@${login}\``;
  });
  return {
    text: codeSegments(text).map((segment) => segment.code ? segment.text : transform(segment.text)).join(""),
    escaped
  };
}
function escapedMentionNotice(escaped, reason = "unknown") {
  if (escaped.length === 0)
    return;
  const names = escaped.map((login) => `\`@${login}\``).join(", ");
  const one = escaped.length === 1;
  if (reason === "not-a-call") {
    return `> [!NOTE]
` + `> ${names} ${one ? "was" : "were"} written as ${one ? "a mention" : "mentions"} ` + `and had the notification removed: this run's text was not a call, so nothing in it ` + `was made. Nobody was notified. If ${one ? "the mention was" : "a mention was"} meant, mention them yourself.`;
  }
  return `> [!NOTE]
` + `> ${names} ${one ? "was" : "were"} written as ${one ? "a mention" : "mentions"} ` + `and had the notification removed: this run could not confirm ${one ? "that account" : "those accounts"} ` + `as a participant in this repository or this thread. Nobody was notified. If the mention was meant, mention them yourself.`;
}

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
function proseOnly(text) {
  return fenceSegments(text).map((segment) => segment.code ? `
` : segment.text).join("");
}

// src/adapters/github/parent-issue.ts
function log(message) {
  console.error(`[atomaton-parent] ${message}`);
}
function parentIssueOf(repo, issue) {
  const [owner, name] = repo.split("/", 2);
  if (!owner || !name) {
    const why = `'${repo}' is not an owner/name repository, so #${issue}'s parent could not be asked for`;
    log(`WARN ${why}`);
    return { known: false, why };
  }
  try {
    const data = ghGraphqlRead("query($owner:String!,$repo:String!,$num:Int!){repository(owner:$owner,name:$repo){issue(number:$num){parent{number}}}}", { owner, repo: name, num: issue });
    return { known: true, parent: data.repository.issue.parent?.number ?? 0 };
  } catch (error) {
    const why = `could not read the parent of #${issue}: ${error.message}`;
    log(`WARN ${why}`);
    return { known: false, why };
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

// src/adapters/github/pull-request.ts
function whyNot(what, result) {
  return `${what}: ${result.stderr || result.stdout || `gh exited ${result.code}`}`;
}
function readPullRequestObject(repo, number) {
  const what = `gh api repos/${repo}/pulls/${number}`;
  const result = ghRead("api", `repos/${repo}/pulls/${number}`);
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

// src/domain/work/closed-target.ts
function isMerged(target) {
  return target.known && target.kind === "pull-request" && target.state === "done";
}

// src/domain/work/completion-mention.ts
function shouldMentionOnCompletion(signals) {
  if (!signals.notify)
    return false;
  if (signals.chainContinues || signals.ending.continues === true)
    return false;
  if (signals.ending.ended === "no-report")
    return true;
  if (signals.ending.next)
    return false;
  if (signals.agentCalledHuman && signals.ending.ended === "finished")
    return false;
  if (signals.isSubIssue && signals.issueClosed)
    return false;
  return true;
}

// src/domain/work/turn.ts
function endingOf(signals) {
  const named = signals.directive.trim();
  const next = named === "" ? undefined : { agent: named };
  if (!signals.succeeded || signals.endedBecause === "failed")
    return { ended: "failed" };
  if (signals.endedBecause === "loop")
    return { ended: "looped" };
  if (signals.endedBecause === "stopped")
    return { ended: "stopped" };
  if (signals.endedBecause === "iterations" || signals.endedBecause === "runtime" || signals.endedBecause === "context_limit")
    return { ended: "spent" };
  if (signals.parked)
    return { ended: "parked" };
  if (signals.loopLimitReached)
    return { ended: "chain-over", ...next ? { next } : {} };
  if (next)
    return { ended: "handed-off", next, ...signals.chainContinues ? { continues: true } : {} };
  const continues = signals.chainContinues ? { continues: true } : {};
  if (!signals.reported)
    return { ended: "no-report", ...continues };
  return { ended: "finished", ...continues };
}

// src/shared/redaction.ts
var PATTERNS = [
  /\bsk-[A-Za-z0-9_-]{16,}/g,
  /\bsk-ant-[A-Za-z0-9_-]{16,}/g,
  /\bgh[pousr]_[A-Za-z0-9]{16,}/g,
  /\bgithub_pat_[A-Za-z0-9_]{20,}/g,
  /\bAKIA[0-9A-Z]{16}\b/g,
  /\bASIA[0-9A-Z]{16}\b/g,
  /\bxox[abposr]-[A-Za-z0-9-]{10,}/g,
  /\bAIza[0-9A-Za-z_-]{35}\b/g,
  /-----BEGIN [A-Z ]*PRIVATE KEY-----/g
];
var REDACTED = "[redacted]";
function redact(text, literals = []) {
  let out = text;
  for (const literal of literals)
    out = out.split(literal).join(REDACTED);
  for (const pattern of PATTERNS)
    out = out.replace(pattern, REDACTED);
  return out;
}

// src/domain/record/token-line.ts
function renderTokenLine(u) {
  const split = `${u.prompt ?? "?"} prompt + ${u.completion ?? "?"} completion`;
  const share = u.cached === undefined ? "" : `, ${u.cached} of the prompt cached`;
  return `_Tokens: ${u.total ?? "?"} total (${split}${share})_`;
}

// src/domain/record/tool-trouble.ts
var NAMED = 3;
function looksRefused(content) {
  return /blocked by hook|shell_guard:|Tool blocked/.test(content) || /is blocked by denylist pattern/.test(content) || /is not permitted by the allowlist/.test(content) || /Refusing to close issue #[0-9]+: opened by a human/.test(content);
}
function looksFailed(content) {
  if (looksRefused(content))
    return false;
  return /^\s*(Error|error):/.test(content) || /"status"\s*:\s*"(failed|error)"/.test(content);
}
function problemsIn(content) {
  const marker = /^--- \d+ problems? reported by the '([^']+)' server/m.exec(content);
  if (!marker)
    return [];
  const server = marker[1];
  const out = [];
  for (const line of content.slice(marker.index).split(`
`)) {
    const reported = /^(error|warning):\s*(.+)$/.exec(line.trim());
    if (reported)
      out.push({ server, problem: normaliseProblem(reported[2]) });
  }
  return out;
}
function normaliseProblem(text) {
  return text.replace(/\s+/g, " ").replace(/#[0-9]+/g, "#N").replace(/[0-9]{3,}/g, "N").trim().slice(0, 120);
}
function toolTroubleLine(session, from) {
  if (from === undefined || !Number.isFinite(from))
    return;
  const messages = session?.messages ?? [];
  const servers = new Map;
  let problems = 0;
  let stopped = 0;
  for (let i = Math.max(0, from);i < messages.length; i += 1) {
    const message = messages[i];
    if (message?.role !== "tool")
      continue;
    const content = typeof message.content === "string" ? message.content : "";
    for (const { server } of problemsIn(content)) {
      problems += 1;
      servers.set(server, (servers.get(server) ?? 0) + 1);
    }
    if (looksRefused(content) || looksFailed(content))
      stopped += 1;
  }
  if (problems === 0 && stopped === 0)
    return;
  const parts = [];
  if (problems > 0) {
    const ranked = [...servers.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
    const shown = ranked.slice(0, NAMED).map(([server]) => `\`${server}\``);
    const rest = ranked.length - shown.length;
    if (rest > 0)
      shown.push(`and ${rest} other server${rest === 1 ? "" : "s"}`);
    const said = servers.size === 1 ? "a server reported about itself" : "servers reported about themselves";
    parts.push(`${problems} problem${problems === 1 ? "" : "s"} ${said} (${shown.join(", ")})`);
  }
  if (stopped > 0)
    parts.push(`${stopped} refused or errored tool call${stopped === 1 ? "" : "s"}`);
  return `Counted from the session: ${parts.join(", ")}.`;
}

// src/domain/record/tool-tally.ts
var NAMED2 = 4;
function toolCallTally(session) {
  const names = [];
  for (const message of session?.messages ?? []) {
    for (const call of message.tool_calls ?? []) {
      const name = call.function?.name;
      if (typeof name === "string" && name !== "")
        names.push(name);
    }
  }
  if (names.length === 0)
    return;
  const counts = new Map;
  for (const name of names)
    counts.set(name, (counts.get(name) ?? 0) + 1);
  const ranked = [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  const shown = ranked.slice(0, NAMED2).map(([name, n]) => `\`${name}\` ${n}`);
  const rest = ranked.slice(NAMED2);
  if (rest.length > 0) {
    const restCalls = rest.reduce((sum, [, n]) => sum + n, 0);
    shown.push(`and ${rest.length} other tool${rest.length === 1 ? "" : "s"} ${restCalls}`);
  }
  return `Spent on: ${names.length} tool calls \u2014 ${shown.join(", ")}.`;
}

// src/domain/work/comment-command.ts
var COMMAND_RE = new RegExp(`^\\/(${AGENT_NAME_PATTERN})(?:\\s+(.*))?$`);
var DISPATCH_RE = new RegExp(`^<!--\\s*atomaton:dispatch\\s*=\\s*(${AGENT_NAME_PATTERN})\\s*-->`);
function recoveryLines(agent) {
  return [
    `- Continue: comment \`/${agent}\` and put what to do next on the following lines.`,
    `- Start clean: comment \`/${agent} recover\`, which archives this session and rebuilds`,
    "  it from the issue."
  ];
}

// src/domain/work/ending-calls.ts
var DIRECTIVE = new RegExp(`^\\/(${AGENT_NAME_PATTERN})$`);
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

// src/entrypoints/machinery/lib/failure-notice.ts
import { existsSync as existsSync2, readFileSync as readFileSync2 } from "fs";
var INTERESTING = /error|fail|panic|exception|unauthorized|warn/i;
var EXCERPT_LINES = 5;
function logExcerpt(text) {
  return text.split(`
`).filter((line) => INTERESTING.test(line)).slice(-EXCERPT_LINES).map((line) => redact(line)).join(`
`).trim();
}
function logExcerptFrom(path) {
  if (!path || !existsSync2(path))
    return "";
  try {
    return logExcerpt(readFileSync2(path, "utf8"));
  } catch {
    return "";
  }
}
function failureLines(args) {
  const lines = [
    `${mentionPrefix(args.notify)}Atomaton: \`${args.agent}\` did not finish.`,
    "",
    "**The session was saved.** What this run worked out is still there.",
    "",
    ...recoveryLines(args.agent)
  ];
  if (args.runUrl)
    lines.push("", `Workflow logs: ${args.runUrl}`);
  if (args.excerpt) {
    lines.push("", "From the log, which is often about the infrastructure rather than the work:", "```", args.excerpt, "```");
  }
  return lines;
}

// src/adapters/github/ending-comment.ts
import { existsSync as existsSync3, readFileSync as readFileSync3, writeFileSync } from "fs";
function readEndingComment(path) {
  if (!path || !existsSync3(path))
    return;
  try {
    const parsed = JSON.parse(readFileSync3(path, "utf8"));
    if (typeof parsed.commentId !== "string" || !parsed.commentId)
      return;
    return { commentId: parsed.commentId, node: parsed.node ?? "" };
  } catch {
    return;
  }
}

// src/entrypoints/machinery/lib/flags.ts
function isTrue(value) {
  return value === "true";
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

// src/entrypoints/machinery/post_result_comment.ts
var ref = defineScript(import.meta.url);
function tokenUsageLine(logsFile) {
  if (!existsSync4(logsFile))
    return;
  const usageLine = readFileSync4(logsFile, "utf8").split(`
`).find((l) => l.includes("ATOMA_TOKEN_USAGE:"));
  if (!usageLine)
    return;
  const prompt = /prompt=(\d+)/.exec(usageLine)?.[1];
  const completion = /completion=(\d+)/.exec(usageLine)?.[1];
  const total = /total=(\d+)/.exec(usageLine)?.[1];
  const cached = /cached=(\d+)/.exec(usageLine)?.[1];
  return renderTokenLine({ total, prompt, completion, cached });
}
function nodeFacts(number, type, interrupted) {
  const repo = process.env.GITHUB_REPOSITORY ?? "";
  if (type !== "issue") {
    if (!interrupted)
      return { isSubIssue: false, issueClosed: false };
    const read = readPullRequestObject(repo, Number(number));
    if (!read.known) {
      return { isSubIssue: false, issueClosed: false, target: { known: false, why: read.why } };
    }
    const { state, merged_at: mergedAt, body } = read.value;
    if (state !== "open" && state !== "closed") {
      const why = `unrecognised pull request state ${JSON.stringify(state ?? null)}`;
      return { isSubIssue: false, issueClosed: false, target: { known: false, why } };
    }
    return {
      isSubIssue: false,
      issueClosed: false,
      target: { known: true, kind: "pull-request", state: state === "open" ? "open" : mergedAt ? "done" : "abandoned" },
      parentIssue: PARENT_ISSUE_TAG.readProse(body ?? "")
    };
  }
  const target = readTargetState(number, repo);
  const found = parentIssueOf(repo, Number(number));
  const parent = found.known && found.parent > 0 ? found.parent : undefined;
  return {
    isSubIssue: parent !== undefined,
    issueClosed: target.known && target.state !== "open",
    target,
    parentIssue: parent
  };
}
function readSession(path) {
  if (!path || !existsSync4(path))
    return;
  try {
    return JSON.parse(readFileSync4(path, "utf8"));
  } catch {
    return;
  }
}
function lastAgentText(sessionPath, from) {
  if (from === undefined || !Number.isFinite(from))
    return;
  const messages = readSession(sessionPath)?.messages ?? [];
  for (let i = messages.length - 1;i >= from; i -= 1) {
    const message = messages[i];
    if (message?.role !== "assistant")
      continue;
    const content = message.content;
    if (typeof content === "string" && content.trim() !== "")
      return content;
  }
  return;
}
function endedTag(ending) {
  switch (ending.ended) {
    case "stopped":
      return "stopped";
    case "spent":
    case "looped":
      return "limit";
    case "handed-off":
      return "handoff";
    case "parked":
      return "waiting";
    case "failed":
    case "chain-over":
    case "no-report":
    case "finished":
      return "done";
  }
}
function cutShort(endedBecause) {
  return endedBecause === "stopped" || endedBecause === "runtime" || endedBecause === "iterations" || endedBecause === "loop" || endedBecause === "context_limit" || endedBecause === "failed";
}
function reachedLimit(endedBecause) {
  return endedBecause === "runtime" || endedBecause === "iterations" || endedBecause === "loop" || endedBecause === "context_limit";
}
function howItWasCutShort(endedBecause) {
  if (endedBecause === "stopped")
    return "was stopped";
  if (endedBecause === "loop")
    return "got stuck repeating itself";
  if (endedBecause === "failed")
    return "did not finish";
  if (endedBecause === "context_limit")
    return "ran out of context";
  return endedBecause === "iterations" ? "ran out of iterations" : "ran out of time";
}
function mentionLine(ending, args) {
  if (args.endedBecause === "failed") {
    return `@${args.notify} \u2014 **${args.agent}** stopped mid-run, and no agent will run next. ` + continuation(args, "Resume it, or say what to do instead.");
  }
  if (cutShort(args.endedBecause)) {
    return `@${args.notify} \u2014 **${args.agent}** ${howItWasCutShort(args.endedBecause)} ` + `before it finished, and no agent will run next. ` + continuation(args, "Resume it, or say what to do instead.");
  }
  if (ending.ended === "no-report") {
    return `@${args.notify} \u2014 **${args.agent}** ended without writing a report. Nothing interrupted it ` + `and nothing runs next; it simply left no closing text, so there is nothing here to review. ` + `What it did is in its saved session.`;
  }
  return `@${args.notify} \u2014 **${args.agent}** task completed. No agent will be automatically executed next. Please review the results or provide instructions for the next step.`;
}
function continuation(args, resume) {
  if (args.target === undefined)
    return resume;
  if (isMerged(args.target)) {
    const elsewhere = args.parentIssue !== undefined && args.parentIssue > 0 ? `Continue on #${args.parentIssue}, the issue this pull request delivered.` : "Continue on the issue this pull request delivered.";
    return `The work this pull request carried is merged already, so there is nothing here to resume. ${elsewhere}`;
  }
  if (args.target.known && args.target.state !== "open") {
    return "That thread is closed, so there is nothing left to resume here \u2014 reopen it to continue, or say what to do instead.";
  }
  return resume;
}
function endingHere(args) {
  return endingOf({
    succeeded: true,
    endedBecause: args.endedBecause ?? "",
    loopLimitReached: false,
    chainContinues: isTrue(args.chainContinues),
    directive: args.directive ?? "",
    reported: args.reported === true
  });
}
function makesCompletionMention(signals) {
  return shouldMentionOnCompletion({
    ending: endingHere(signals),
    chainContinues: isTrue(signals.chainContinues),
    notify: signals.notify,
    agentCalledHuman: signals.agentCalledHuman === true,
    isSubIssue: signals.isSubIssue ?? false,
    issueClosed: signals.issueClosed ?? false
  });
}
function buildCommentBody(args) {
  const ending = endingHere(args);
  const failed = args.endedBecause === "failed";
  const lines = [
    AGENT_TAG.write(args.agent),
    CHANGED_TAG.write(args.changed === true ? "yes" : "no"),
    ENDED_TAG.write({
      verb: endedTag(ending),
      answers: Number(args.ask ?? 0) || 0,
      run: Number(args["run-id"] ?? 0) || 0
    })
  ];
  if (failed) {
    lines.push(...failureLines({ agent: args.agent, excerpt: args.excerpt ?? "" }), "");
  }
  if (args.endingInvalid === true) {
    lines.push("> [!WARNING]", "> This run's closing message called more than one thing, so none of its calls", "> was made: nothing it named is started or notified. It was asked to write the", "> message again. Below is what it wrote.", "");
  } else if (args.salvaged === true) {
    lines.push("> [!WARNING]", "> This run ended before it wrote a report. Below is the last thing it said,", "> from the middle of the work \u2014 not a conclusion, and not a summary of what it found.", "");
  } else if (args.wroteNothing === true) {
    lines.push("> [!WARNING]", "> This run ended before it said anything at all, so there is no report below \u2014", "> not even a partial one. What it had done is in its saved session.", "");
  }
  if (args.output.trim() !== "")
    lines.push(args.output, "");
  const escapedNotice = escapedMentionNotice(args.escapedMentions ?? [], args.escapedMentionsReason ?? "unknown");
  if (escapedNotice !== undefined)
    lines.push("", escapedNotice, "");
  if (makesCompletionMention(args)) {
    lines.push(mentionLine(ending, args), "");
  }
  lines.push(buildFooter({
    agent: args.agent,
    runUrl: args.runUrl,
    repo: args.repo,
    tokenLine: args.tokenLine,
    toolTrouble: args.toolTrouble,
    endedBecause: args.endedBecause,
    target: args.target,
    spentOn: args.spentOn
  }));
  return lines.join(`
`);
}
function buildFooter(args) {
  const lines = ["---"];
  if (args.tokenLine !== undefined)
    lines.push(args.tokenLine);
  const metrics = args.repo ? ` \xB7 [metrics](https://github.com/${args.repo}/blob/atomaton-data/metrics/report.md)` : "";
  lines.push(`_run by [${args.agent}](${args.runUrl})${metrics}_`);
  if (args.toolTrouble !== undefined)
    lines.push(`_${args.toolTrouble}_`);
  if (args.endedBecause === "stopped") {
    const saved = "\u23F8\uFE0F _Stopped on request. **The session is saved.**";
    if (args.target !== undefined && isMerged(args.target)) {
      lines.push(`${saved} The work this pull request carried is merged already, so there is ` + `nothing here to resume._`);
    } else if (args.target !== undefined && args.target.known && args.target.state !== "open") {
      lines.push(`${saved} That thread is closed, so there is nothing left to resume here \u2014 ` + `reopen it to continue._`);
    } else {
      lines.push(`${saved} Comment \`!resume\` to continue from here, or \`/${args.agent}\` ` + `with an instruction on the following lines._`);
    }
  } else if (reachedLimit(args.endedBecause)) {
    lines.push(`\u26A0\uFE0F _The run ${howItWasCutShort(args.endedBecause)}. Comment \`/${args.agent}\` to continue._`);
  }
  if (args.spentOn !== undefined)
    lines.push(`_${args.spentOn}_`);
  return lines.join(`
`);
}
function appendFooter(args) {
  const current = gh("api", `repos/${args.repo}/issues/comments/${args.commentId}`, "--jq", ".body");
  if (current.code !== 0 || !current.stdout.trim()) {
    console.error(`could not read the ending comment ${args.commentId}: ` + `${current.stderr || current.stdout || "(empty body)"} -- the footer is not appended`);
    return;
  }
  const body = `${current.stdout.trim()}

${buildFooter({
    agent: args.agent,
    runUrl: args.runUrl,
    repo: args.repo,
    tokenLine: args.tokenLine,
    toolTrouble: args.toolTrouble
  })}`;
  const updated = gh("api", `repos/${args.repo}/issues/comments/${args.commentId}`, "--method", "PATCH", "-f", `body=${body}`);
  if (updated.code !== 0) {
    console.error(`could not append the footer: ${updated.stderr || updated.stdout}`);
    return;
  }
  console.error(`footer appended to the ending comment ${args.commentId}`);
}
function colourForStepFailure(stepFailed, atomaFailed) {
  if (!stepFailed || atomaFailed)
    return;
  console.error("The machinery failed, not the agent; the notice healed the thread, but the run stays red.");
  process.exitCode = 1;
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      number: { type: "string" },
      agent: { type: "string" },
      type: { type: "string" },
      notify: { type: "string" },
      directive: { type: "string" },
      "chain-continues": { type: "string" },
      "ended-because": { type: "string" },
      reported: { type: "string" },
      "messages-before": { type: "string" },
      "run-url": { type: "string" },
      changed: { type: "string" },
      session: { type: "string" },
      output: { type: "string" },
      "logs-file": { type: "string" },
      "ending-comment": { type: "string" },
      "ending-invalid": { type: "string" },
      "people-file": { type: "string" },
      ask: { type: "string" },
      "run-id": { type: "string" },
      outcome: { type: "string" },
      "atoma-failed": { type: "string" }
    }
  });
  if (!values.number || !values.agent || !values["run-url"]) {
    console.error("usage: post_result_comment.ts --number N --agent NAME --run-url URL [...]");
    process.exit(2);
  }
  const outputFile = values.output;
  if (!outputFile) {
    console.error("post_result_comment.ts: --output is required (the agent's stdout file)");
    process.exit(2);
  }
  const redacted = redact(existsSync4(outputFile) ? readFileSync4(outputFile, "utf8") : "");
  let output = redacted;
  let salvaged = false;
  let wroteNothing = false;
  const stepFailed = values.outcome === "failure";
  const endedBecause = values["ended-because"] || (stepFailed ? "failed" : "");
  const endedEarly = cutShort(endedBecause);
  const failed = endedBecause === "failed";
  if (!output.trim() && endedEarly) {
    const last = lastAgentText(values.session, Number(values["messages-before"]));
    if (last !== undefined) {
      output = redact(last);
      salvaged = true;
      console.error("salvaged the agent's last message from the session (cut short)");
    }
  }
  if (!output.trim()) {
    const ending = stepFailed ? undefined : readEndingComment(values["ending-comment"]);
    if (ending) {
      console.error(`appending the result footer onto the tool's ending comment ${ending.commentId} (on ${ending.node})`);
      appendFooter({
        repo: process.env.GITHUB_REPOSITORY ?? "",
        commentId: ending.commentId,
        agent: values.agent,
        runUrl: values["run-url"],
        tokenLine: tokenUsageLine(values["logs-file"] ?? ""),
        toolTrouble: toolTroubleLine(readSession(values.session), Number(values["messages-before"]))
      });
      return;
    }
    if (failed) {
      console.error("the run failed with nothing to report -- posting the failure notice rather than nothing.");
    } else if (endedEarly) {
      wroteNothing = true;
      output = endedBecause === "stopped" ? "_This run was stopped before it said anything._" : `_This run ${howItWasCutShort(endedBecause)} before it said anything._`;
      console.error("the run was cut short with nothing to report -- posting the notice rather than nothing.");
    } else {
      wroteNothing = true;
      output = "_This run ended without saying anything._";
      console.error("atomaton_output.txt is empty and no session-ending tool recorded a comment " + "-- posting the no-report notice rather than skipping.");
    }
  }
  if (failed && !salvaged && output.trim() !== "")
    salvaged = true;
  const endingInvalid = isTrue(values["ending-invalid"]);
  const notACall = endingInvalid || salvaged || endedEarly;
  const callees = readCalleesFile(values["people-file"]);
  const known = notACall ? [] : callees.participants;
  const callable = notACall ? [] : callees.humans;
  const checked = escapeUnknownMentions(output, known);
  if (checked.escaped.length > 0) {
    console.error(`escaped ${checked.escaped.length} mention(s): ${checked.escaped.join(", ")}`);
  }
  const called = calledHumans(output, callable);
  const facts = nodeFacts(values.number, values.type, cutShort(values["ended-because"]));
  const mentionSignals = {
    notify: values.notify,
    directive: values.directive,
    chainContinues: values["chain-continues"],
    endedBecause,
    reported: isTrue(values.reported),
    agentCalledHuman: called.length === 1,
    isSubIssue: facts.isSubIssue,
    issueClosed: facts.issueClosed
  };
  const advice = { target: facts.target, parentIssue: facts.parentIssue };
  const body = buildCommentBody({
    ...mentionSignals,
    ...advice,
    agent: values.agent,
    salvaged,
    wroteNothing,
    endingInvalid,
    runUrl: values["run-url"],
    ask: values.ask,
    "run-id": values["run-id"],
    repo: process.env.GITHUB_REPOSITORY ?? "",
    output: checked.text,
    escapedMentions: checked.escaped,
    escapedMentionsReason: notACall ? "not-a-call" : "unknown",
    changed: isTrue(values.changed),
    toolTrouble: toolTroubleLine(readSession(values.session), Number(values["messages-before"])),
    tokenLine: tokenUsageLine(values["logs-file"] ?? ""),
    spentOn: reachedLimit(endedBecause) ? toolCallTally(readSession(values.session)) : undefined,
    excerpt: failed ? logExcerptFrom(values["logs-file"]) : undefined
  });
  const { code, stdout, stderr } = gh("api", `repos/${process.env.GITHUB_REPOSITORY}/issues/${values.number}/comments`, "--method", "POST", "-f", `body=${body}`, "--jq", ".id");
  if (code !== 0) {
    throw new Error(`Failed to post result comment: ${stderr || stdout}`);
  }
  const commentId = stdout.trim();
  const githubOutput = process.env.GITHUB_OUTPUT;
  if (githubOutput) {
    appendFileSync(githubOutput, `comment_id=${commentId}
`);
    appendFileSync(githubOutput, `notified=${makesCompletionMention(mentionSignals) ? "true" : "false"}
`);
  }
  console.error(`Posted comment ID: ${commentId}`);
  colourForStepFailure(stepFailed, isTrue(values["atoma-failed"]));
}
if (import.meta.main)
  main();
export {
  buildCommentBody,
  buildFooter,
  lastAgentText,
  makesCompletionMention,
  ref
};
