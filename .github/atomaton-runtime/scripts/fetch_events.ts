#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/fetch_events.ts
import { appendFileSync, writeFileSync } from "fs";
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
function ghJson(...args) {
  const { code, stdout, stderr } = gh(...args);
  if (code !== 0) {
    throw new Error(`gh ${args.join(" ")}: ${stderr || stdout}`);
  }
  return stdout ? JSON.parse(stdout) : null;
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
function ghPaginated(...args) {
  const { code, stdout, stderr } = gh(...args, "--paginate");
  if (code !== 0) {
    throw new Error(`gh ${args.join(" ")} --paginate: ${stderr || stdout}`);
  }
  if (!stdout.trim())
    return [];
  const flat = [];
  for (const page of splitConcatenatedJson(stdout)) {
    if (Array.isArray(page))
      flat.push(...page);
  }
  return flat;
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
function readPullRequestDiff(repo, number) {
  const what = `gh api repos/${repo}/pulls/${number} (diff)`;
  const result = ghRead("api", `repos/${repo}/pulls/${number}`, "-H", "Accept: application/vnd.github.v3.diff");
  if (result.code !== 0)
    return { known: false, why: whyNot(what, result) };
  return { known: true, diff: result.stdout };
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
function withoutTags(text) {
  return fenceSegments(text).map((segment) => segment.code ? segment.text : stripTags(segment.text)).join("");
}
function stripTags(text) {
  const tags = EVERY_TAG_PATTERN.join("|");
  const lineEnd = String.raw`(?:\r?\n|(?:\\r)?\\n)?`;
  return text.replace(new RegExp(String.raw`(?:^[ \t]*)?(?:${tags})[ \t]*${lineEnd}`, "gm"), "");
}
function proseOnly(text) {
  return fenceSegments(text).map((segment) => segment.code ? `
` : segment.text).join("");
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
function keptCommentIds(comments) {
  const entries = comments.map((comment) => ({ id: comment.id, ...entryOf(comment) }));
  return new Set(shapedThread(entries, readers).comments.map((entry) => entry.id));
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

// src/domain/work/node-type.ts
function isNodeType(value) {
  return value === "issue" || value === "pr";
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

// src/entrypoints/machinery/fetch_events.ts
var ref = defineScript(import.meta.url);
function repoParts() {
  const repo = process.env.GITHUB_REPOSITORY ?? "";
  const [owner, name] = repo.split("/", 2);
  if (!owner || !name)
    throw new Error(`GITHUB_REPOSITORY is not set or malformed: "${repo}"`);
  return [owner, name];
}
function contextList(what, ...args) {
  try {
    return ghPaginated(...args);
  } catch (error) {
    console.error(`::warning::Could not fetch ${what}: ${error.message}. Continuing without it \u2014 the run has less context than usual.`);
    return [];
  }
}
function threadComments(what, repo, number) {
  try {
    return listComments(repo, number);
  } catch (error) {
    console.error(`::warning::Could not fetch ${what}: ${error.message}. Continuing without it \u2014 the run has less context than usual.`);
    return [];
  }
}
function requiredJson(what, ...args) {
  const { code, stdout, stderr } = ghRead(...args);
  if (code !== 0) {
    throw new Error(`Could not fetch ${what}, which a run cannot proceed without: ${stderr || stdout}`);
  }
  return JSON.parse(stdout);
}
function fetchIssueEvents(owner, repo, issueNum, openedType, commentType, idPrefix) {
  const issue = requiredJson(`issue #${issueNum}`, "api", `repos/${owner}/${repo}/issues/${issueNum}`);
  const found = parentIssueOf(`${owner}/${repo}`, issueNum);
  const parentLine = found.known && found.parent > 0 ? `**Sub-issue of:** #${found.parent}
` : "";
  const labelsLine = issue.labels.length > 0 ? `**Labels:** ${issue.labels.map((l) => l.name).join(", ")}
` : "";
  const openedEvent = {
    id: `${idPrefix}-${issue.number}`,
    event_type: openedType,
    content: `Issue #${issue.number}: ${issue.title}
${labelsLine}${parentLine}
${issue.body ?? ""}`,
    author: issue.user.login,
    author_type: issue.user.type,
    created_at: issue.created_at
  };
  const comments = threadComments(`comments on #${issueNum}`, `${owner}/${repo}`, issueNum);
  const kept = keptCommentIds(comments);
  const commentEvents = comments.filter((c) => kept.has(c.id)).map((c) => ({
    id: c.id,
    event_type: commentType,
    content: c.body ?? "",
    author: c.user?.login ?? "",
    author_type: c.user?.type,
    created_at: c.created_at ?? ""
  }));
  return [openedEvent, ...commentEvents];
}
function fetchPrEvents(owner, repo, number, maxDiffChars) {
  const read = readPullRequestObject(`${owner}/${repo}`, number);
  if (!read.known) {
    throw new Error(`Could not fetch pull request #${number}, which a run cannot proceed without: ${read.why}`);
  }
  const pr = read.value;
  const prBody = pr.body ?? "";
  const parentIssue = PARENT_ISSUE_TAG.readProse(prBody);
  const headSha = pr.head.sha.slice(0, 8);
  const events = [];
  const labelsLine = pr.labels.length > 0 ? `**Labels:** ${pr.labels.map((label) => label.name).join(", ")}` : "";
  const linkedLine = parentIssue ? `**Linked Issue:** #${parentIssue}` : "";
  const prContentLines = [`PR #${number}: ${pr.title}`, labelsLine, linkedLine].filter(Boolean);
  prContentLines.push("", prBody);
  events.push({
    id: `pr-${number}`,
    event_type: "pr_opened",
    content: prContentLines.join(`
`),
    author: pr.user.login,
    author_type: pr.user.type,
    created_at: pr.created_at
  });
  const diffRead = readPullRequestDiff(`${owner}/${repo}`, number);
  const diff = diffRead.known ? diffRead.diff : "";
  if (diff) {
    const truncated = diff.slice(0, maxDiffChars);
    let diffContent = "```diff\n" + truncated + "\n```";
    if (diff.length > maxDiffChars)
      diffContent += `

*[Diff truncated at ${maxDiffChars} characters due to size]*`;
    events.push({
      id: `pr-${number}-diff`,
      event_type: "pr_diff",
      content: diffContent,
      sha: headSha,
      author: "github",
      created_at: pr.updated_at
    });
  }
  const prComments = threadComments(`comments on #${number}`, `${owner}/${repo}`, number);
  const keptPrComments = keptCommentIds(prComments);
  events.push(...prComments.filter((comment) => keptPrComments.has(comment.id)).map((comment) => ({
    id: comment.id,
    event_type: "pr_comment",
    content: comment.body ?? "",
    author: comment.user?.login ?? "",
    author_type: comment.user?.type,
    created_at: comment.created_at ?? ""
  })));
  const reviews = contextList(`reviews on #${number}`, "api", `repos/${owner}/${repo}/pulls/${number}/reviews`);
  events.push(...reviews.filter((review) => review.submitted_at != null).map((review) => ({
    id: `pr-review-${review.id}`,
    event_type: "pr_review",
    content: `Review state: ${review.state}

${review.body ?? ""}`,
    author: review.user.login,
    author_type: review.user.type,
    created_at: review.submitted_at
  })));
  const inlineComments = contextList(`inline review comments on #${number}`, "api", `repos/${owner}/${repo}/pulls/${number}/comments`);
  events.push(...inlineComments.map((comment) => ({
    id: comment.id,
    event_type: "pr_review_comment",
    content: `On \`${comment.path}\` line ${comment.line ?? comment.original_line ?? "?"}:

${comment.body}`,
    author: comment.user.login,
    author_type: comment.user.type,
    created_at: comment.created_at
  })));
  return { events, parentIssue };
}
function linkedPrNumbers(owner, repo, issueNumber) {
  const prs = ghJson("pr", "list", "--repo", `${owner}/${repo}`, "--state", "all", "--search", `"${PARENT_ISSUE_TAG.search(issueNumber)}" in:body`, "--limit", "1000", "--json", "number,body") ?? [];
  if (prs.length === 1000) {
    console.error(`::warning::Linked PR search for Issue #${issueNumber} reached GitHub's 1000-result search limit.`);
  }
  return prs.filter((pr) => PARENT_ISSUE_TAG.readProse(pr.body ?? "") === issueNumber).map((pr) => pr.number);
}
function tryLinkedPrNumbers(owner, repo, issueNumber) {
  try {
    return linkedPrNumbers(owner, repo, issueNumber);
  } catch {
    console.error(`::warning::Could not search linked PRs for Issue #${issueNumber}; using the available context.`);
    return [];
  }
}
function appendPrEvents(events, owner, repo, prNumber, maxDiffChars) {
  try {
    events.push(...fetchPrEvents(owner, repo, prNumber, maxDiffChars).events);
  } catch {
    console.error(`::warning::Could not fetch linked PR #${prNumber}; skipping it.`);
  }
}
function fetchEvents(type, number, maxDiffChars) {
  const [owner, repo] = repoParts();
  if (type === "issue") {
    const events = fetchIssueEvents(owner, repo, number, "issue_opened", "issue_comment", "issue");
    for (const prNumber of tryLinkedPrNumbers(owner, repo, number)) {
      appendPrEvents(events, owner, repo, prNumber, maxDiffChars);
    }
    events.sort((a, b) => a.created_at.localeCompare(b.created_at));
    return { events, sessionType: "issue", sessionNumber: number };
  }
  const prResult = fetchPrEvents(owner, repo, number, maxDiffChars);
  let events = [...prResult.events];
  if (prResult.parentIssue) {
    try {
      events = fetchIssueEvents(owner, repo, prResult.parentIssue, "issue_opened", "issue_comment", "issue");
      const relatedPrNumbers = new Set([number, ...tryLinkedPrNumbers(owner, repo, prResult.parentIssue)]);
      for (const relatedPrNumber of relatedPrNumbers) {
        if (relatedPrNumber === number)
          events.push(...prResult.events);
        else
          appendPrEvents(events, owner, repo, relatedPrNumber, maxDiffChars);
      }
    } catch {
      console.error(`::warning::Linked Issue #${prResult.parentIssue} not found or inaccessible \u2014 using PR-local context.`);
      events.sort((a, b) => a.created_at.localeCompare(b.created_at));
      return { events, sessionType: "pr", sessionNumber: number };
    }
  }
  events.sort((a, b) => a.created_at.localeCompare(b.created_at));
  return {
    events,
    sessionType: prResult.parentIssue ? "issue" : "pr",
    sessionNumber: prResult.parentIssue ?? number
  };
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      type: { type: "string" },
      number: { type: "string" },
      "max-diff-chars": { type: "string" },
      out: { type: "string" }
    }
  });
  if (!isNodeType(values.type) || !values.number || !values.out) {
    console.error("usage: fetch_events.ts --type issue|pr --number N [--max-diff-chars N] --out events.json");
    process.exit(2);
  }
  const maxDiffChars = Number(values["max-diff-chars"] ?? 30000);
  const { events, sessionType, sessionNumber } = fetchEvents(values.type, Number(values.number), maxDiffChars);
  const withoutBookkeeping = events.map((event) => {
    if (event.event_type === "pr_diff")
      return event;
    return {
      ...event,
      llm_context: LLM_CONTEXT_TAG.readProse(event.content),
      agent: AGENT_TAG.readProse(event.content),
      content: withoutTags(event.content)
    };
  });
  writeFileSync(values.out, JSON.stringify(withoutBookkeeping, null, 2));
  const githubOutput = process.env.GITHUB_OUTPUT;
  if (githubOutput)
    appendFileSync(githubOutput, `session_type=${sessionType}
session_number=${sessionNumber}
`);
  console.error(`Fetched ${events.length} events \u2192 ${values.out}`);
}
if (import.meta.main)
  main();
export {
  fetchEvents,
  ref
};
