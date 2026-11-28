#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/request_stop.ts
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
function gitRun(...args) {
  return run(["git", ...args]);
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
function commandLabel(command) {
  return command.command.error !== "" ? command.line : command.command.control ? controlLabel(command.command.control) : `/${command.command.agent}`;
}
function commandsIn(body) {
  const found = [];
  if (!body)
    return found;
  for (const rawLine of body.split(`
`)) {
    const parsed = parseCommandLine(rawLine);
    if (parsed !== undefined)
      found.push({ line: rawLine.trim(), command: parsed });
  }
  return found;
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

// src/domain/work/work-tree.ts
var MAX_DEPTH = 10;
function subtree(nodes, root) {
  const byParent = new Map;
  const byNumber = new Map;
  for (const node of nodes) {
    byNumber.set(node.number, node);
    if (node.parent === undefined)
      continue;
    const siblings = byParent.get(node.parent) ?? [];
    siblings.push(node);
    byParent.set(node.parent, siblings);
  }
  const start = byNumber.get(root);
  if (!start)
    return [];
  const found = [start];
  const seen = new Set([root]);
  let frontier = [root];
  for (let depth = 0;depth < MAX_DEPTH && frontier.length > 0; depth += 1) {
    const next = [];
    for (const parent of frontier) {
      for (const child of byParent.get(parent) ?? []) {
        if (seen.has(child.number))
          continue;
        seen.add(child.number);
        found.push(child);
        next.push(child.number);
      }
    }
    frontier = next;
  }
  return found;
}
function nodesToStop(nodes) {
  return nodes.filter((node) => node.running);
}
function descendants(nodes, root) {
  return nodes.filter((node) => node.number !== root);
}
function stopReachedNotice(root) {
  return [
    `Atomaton: a stop on #${root} reached this work, so the run here has been asked to stop.`,
    "",
    "It stops after its current step, so it may take a minute, and it will report here when it has.",
    "",
    `To pick the work back up, comment \`/resume\` on #${root} \u2014 that restarts everything this stop held, rather than this piece alone.`
  ].join(`
`);
}

// src/adapters/github/outcome.ts
function issueOutcome(reason) {
  const said = (reason ?? "").toLowerCase();
  return said === "not_planned" || said === "duplicate" ? "abandoned" : "done";
}
function pullRequestOutcome(merged) {
  return merged ? "done" : "abandoned";
}
function saysOpen(state) {
  return (state ?? "").toLowerCase() === "open";
}

// src/domain/work/issue-links.ts
function dedupeByNumber(...lists) {
  const seen = new Map;
  for (const list of lists)
    for (const item of list)
      if (!seen.has(item.number))
        seen.set(item.number, item);
  return [...seen.values()].sort((a, b) => a.number - b.number);
}

// src/adapters/github/issue-links.ts
var LINK_LIMIT = 50;
var LABEL_LIMIT = 20;
var QUERY = `
query($owner:String!, $name:String!, $number:Int!, $limit:Int!, $labelLimit:Int!) {
  repository(owner:$owner, name:$name) {
    issueOrPullRequest(number:$number) {
      __typename
      ... on Issue {
        parent { number title state stateReason }
        subIssues(first:$limit) { nodes { number title state stateReason labels(first:$labelLimit) { nodes { name } } } }
        closedByPullRequestsReferences(first:$limit, includeClosedPrs:true) {
          nodes { number title state merged body }
        }
        timelineItems(last:$limit, itemTypes:[CROSS_REFERENCED_EVENT]) {
          nodes { ... on CrossReferencedEvent { source { ... on PullRequest { number title state merged body } } } }
        }
      }
      ... on PullRequest {
        closingIssuesReferences(first:$limit) { nodes { number title state stateReason } }
      }
    }
  }
}`;
function normalise(node) {
  return {
    number: node.number,
    title: node.title,
    state: saysOpen(node.state) ? "open" : issueOutcome(node.stateReason)
  };
}
function asChild(node) {
  return { ...normalise(node), labels: (node.labels?.nodes ?? []).map((label) => label.name) };
}
function asPr(node) {
  return {
    number: node.number,
    title: node.title,
    state: saysOpen(node.state) ? "open" : pullRequestOutcome(Boolean(node.merged))
  };
}
function issueLinks(repo, number) {
  const [owner, name] = repo.split("/");
  if (!owner || !name) {
    return { children: [], pullRequests: [], unavailable: `"${repo}" is not an owner/name repository` };
  }
  let issue = null;
  try {
    issue = ghGraphqlRead(QUERY, { owner, name, number, limit: LINK_LIMIT, labelLimit: LABEL_LIMIT }).repository?.issueOrPullRequest ?? null;
  } catch (error) {
    const why = error.message;
    console.error(`[atomaton-github] WARN could not read links for #${number}: ${why}`);
    return { children: [], pullRequests: [], unavailable: `GitHub could not be reached: ${why}` };
  }
  if (!issue)
    return { children: [], pullRequests: [], unavailable: `#${number} was not found` };
  if (issue.__typename === "PullRequest") {
    const closes = issue.closingIssuesReferences?.nodes ?? [];
    return {
      kind: "pull_request",
      parent: closes[0] ? normalise(closes[0]) : undefined,
      children: [],
      pullRequests: []
    };
  }
  const declared = (issue.closedByPullRequestsReferences?.nodes ?? []).map(asPr);
  const referenced = (issue.timelineItems?.nodes ?? []).map((node) => node.source).filter((source) => Boolean(source?.number) && PARENT_ISSUE_TAG.readProse(source?.body ?? "") === number).map(asPr);
  return {
    kind: "issue",
    parent: issue.parent ? normalise(issue.parent) : undefined,
    children: (issue.subIssues?.nodes ?? []).map(asChild),
    pullRequests: dedupeByNumber(declared, referenced)
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
function whoseTurn(events) {
  const last = events[events.length - 1];
  return last === "asked" || last === "handed-off" || last === "waiting" ? "agent" : "person";
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
function readComments(repo, number, excludeCommentId) {
  const excluded = String(excludeCommentId ?? "").trim();
  const comments = listComments(repo, number).filter((comment) => String(comment.id) !== excluded).map(entryOf);
  return shapedThread(comments, readers);
}
function openClaimOn(repo, number) {
  const shaped = readComments(repo, number);
  if (shaped.openClaim === undefined)
    return;
  const last = shaped.comments[shaped.comments.length - 1];
  return {
    ...shaped.openClaim,
    parked: shaped.events[shaped.events.length - 1] === "waiting",
    claimed: last !== undefined && readers.claim(last.body) !== undefined
  };
}
function runInFlight(repo, number, options = {}) {
  const shaped = readComments(repo, number);
  if (options.merged === true)
    return shaped.holder === "agent";
  return shaped.holder === "agent" || whoseTurn(shaped.events) === "agent";
}

// src/adapters/github/work-tree.ts
function parseListed(stdout) {
  try {
    return JSON.parse(stdout || "[]");
  } catch {
    return null;
  }
}
function runningOn(repo, number, merged) {
  try {
    return { running: runInFlight(repo, number, { merged }) };
  } catch (e) {
    return { running: false, problem: `could not read the thread on #${number}: ${e.message}` };
  }
}
function readNode(repo, number) {
  const { code, stdout, stderr } = ghRead("api", `repos/${repo}/issues/${number}`);
  if (code !== 0) {
    return { problem: `could not read #${number}: ${(stderr || stdout).trim().split(`
`)[0] ?? ""}` };
  }
  let raw;
  try {
    raw = JSON.parse(stdout);
  } catch {
    return { problem: `the response for #${number} was not JSON` };
  }
  const isPr = raw.pull_request !== undefined;
  if (raw.state !== "open" && raw.state !== "closed") {
    return { problem: `#${number} reported an unrecognised state ${JSON.stringify(raw.state ?? null)}` };
  }
  const state = raw.state === "open" ? "open" : isPr ? pullRequestOutcome(Boolean(raw.pull_request?.merged_at)) : issueOutcome(raw.state_reason);
  const merged = isPr && raw.state !== "open" && Boolean(raw.pull_request?.merged_at);
  const { running, problem } = runningOn(repo, number, merged);
  if (problem)
    return { problem };
  return {
    node: {
      number,
      kind: isPr ? "pull-request" : "issue",
      state,
      parent: isPr ? PARENT_ISSUE_TAG.readProse(raw.body ?? "") : undefined,
      running
    }
  };
}
function readChildren(repo, parent) {
  const nodes = [];
  const problems = [];
  const prs = ghRead("pr", "list", "--repo", repo, "--state", "all", "--limit", "200", "--search", `${PARENT_ISSUE_TAG.search(parent)} in:body`, "--json", "number,body,state,labels");
  if (prs.code !== 0)
    problems.push(`could not list the pull requests for #${parent}`);
  const listedPrs = parseListed(prs.stdout);
  if (listedPrs === null)
    problems.push(`the pull request listing for #${parent} was not readable`);
  for (const found of listedPrs ?? []) {
    if (PARENT_ISSUE_TAG.readProse(found.body ?? "") !== parent)
      continue;
    const merged = !saysOpen(found.state) && found.state === "MERGED";
    const { running, problem } = runningOn(repo, found.number, merged);
    if (problem) {
      problems.push(problem);
      continue;
    }
    nodes.push({
      number: found.number,
      kind: "pull-request",
      state: saysOpen(found.state) ? "open" : pullRequestOutcome(found.state === "MERGED"),
      parent,
      running
    });
  }
  const links = issueLinks(repo, parent);
  if (links.unavailable) {
    problems.push(`could not read GitHub's own links for #${parent}: ${links.unavailable}`);
  }
  const already = new Set(nodes.map((node) => node.number));
  for (const child of links.children) {
    if (already.has(child.number))
      continue;
    const { running, problem } = runningOn(repo, child.number, false);
    if (problem) {
      problems.push(problem);
      continue;
    }
    already.add(child.number);
    nodes.push({
      number: child.number,
      kind: "issue",
      state: child.state,
      parent,
      running
    });
  }
  for (const linked of links.pullRequests) {
    if (already.has(linked.number))
      continue;
    const { node, problem } = readNode(repo, linked.number);
    if (!node) {
      problems.push(problem ?? `could not read #${linked.number}`);
      continue;
    }
    already.add(linked.number);
    nodes.push({ ...node, parent });
  }
  return { nodes, problems };
}
function readWorkTree(repo, root) {
  const { node, problem } = readNode(repo, root);
  if (!node)
    return { nodes: [], problems: [problem ?? `could not read #${root}`] };
  const nodes = [node];
  const problems = [];
  const seen = new Set([root]);
  let frontier = [root];
  for (let depth = 0;depth < MAX_DEPTH && frontier.length > 0; depth += 1) {
    const next = [];
    for (const parent of frontier) {
      const found = readChildren(repo, parent);
      problems.push(...found.problems);
      for (const child of found.nodes) {
        if (seen.has(child.number))
          continue;
        seen.add(child.number);
        nodes.push(child);
        if (child.kind === "issue")
          next.push(child.number);
      }
    }
    frontier = next;
  }
  return { nodes, problems };
}
function commentOn(repo, number, body) {
  return gh("issue", "comment", String(number), "--repo", repo, "--body", body).code === 0;
}
function requestStopAcross(repo, root, rootBody) {
  const { nodes, problems } = readWorkTree(repo, root);
  const all = subtree(nodes, root);
  const stopped = [];
  const rootNotified = commentOn(repo, root, rootBody);
  if (!rootNotified)
    problems.push(`could not post the stop request on #${root}`);
  else if (all.find((node) => node.number === root)?.running)
    stopped.push(root);
  for (const node of nodesToStop(descendants(all, root))) {
    const body = [LLM_CONTEXT_TAG.write("exclude"), STOP_TAG.write("requested"), stopReachedNotice(root)].join(`
`);
    if (commentOn(repo, node.number, body))
      stopped.push(node.number);
    else
      problems.push(`could not post the stop request on #${node.number}`);
  }
  return { stopped, rootNotified, problems };
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
var MACHINERY_ROOT_VAR = "ATOMATON_MACHINERY_ROOT";

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

// src/adapters/runner/machinery.ts
function machineryRoot() {
  return process.env[MACHINERY_ROOT_VAR]?.trim() || undefined;
}
function machineryPath(relative) {
  const root = machineryRoot();
  return root ? `${root}/${relative}` : relative;
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
var DEFAULT_LABELS = {
  sub_issue: "atomaton/sub-issue",
  launched: "atomaton/launched",
  in_progress: "atomaton/in-progress",
  waiting_for_person: "atomaton/waiting-for-person"
};
function getLabel(key) {
  return loadConfig().chain?.labels?.[key] ?? DEFAULT_LABELS[key];
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

// src/adapters/runner/ops-log.ts
var OPS_LOG_PATH = process.env.ATOMATON_OPS_LOG ?? "/tmp/atomaton_ops.log";
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
function dispatchPrValidation(repo, prNumber, branch, options = {}) {
  return dispatchWorkflow(`dispatchPrValidation: validating PR #${prNumber}`, "atomaton-validate-pr.yml", [
    "--repo",
    repo,
    "-f",
    `number=${prNumber}`,
    "-f",
    `branch=${branch}`,
    "-f",
    `asked_by_person=${options.askedByPerson === true ? "true" : "false"}`,
    "-f",
    `ask=${options.ask ?? ""}`
  ], log);
}

// src/adapters/github/branch-rules.ts
var FEATURE_UNAVAILABLE = /upgrade to github|make this repository public/i;
function readBranchRules(repo, baseRef) {
  if (!baseRef)
    return { known: false, why: "no base branch was given" };
  const { code, stdout, stderr } = gh("api", `repos/${repo}/rules/branches/${baseRef}`);
  if (code) {
    if (FEATURE_UNAVAILABLE.test(`${stderr} ${stdout}`)) {
      return {
        known: true,
        enforceable: false,
        contexts: [],
        pullRequestRequired: false,
        why: "branch rules are not available on this repository (they are a paid feature on a " + "private one), so GitHub cannot require a status check or refuse a merge here"
      };
    }
    return { known: false, why: `the branch rules for ${baseRef} could not be read` };
  }
  try {
    const rules = JSON.parse(stdout || "[]");
    return {
      known: true,
      enforceable: true,
      contexts: rules.filter((rule) => rule.type === "required_status_checks").flatMap((rule) => rule.parameters?.required_status_checks ?? []).map((check) => check.context),
      pullRequestRequired: rules.some((rule) => rule.type === "pull_request")
    };
  } catch {
    return { known: false, why: `the branch rules for ${baseRef} were not valid JSON` };
  }
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

// src/entrypoints/machinery/lib/pr-head-branch.ts
function prHead(pr) {
  return {
    branch: typeof pr?.headRefName === "string" ? pr.headRefName.trim() : "",
    crossRepository: pr?.isCrossRepository === true
  };
}
function isBranchName(name) {
  if (!name)
    return false;
  return gitRun("check-ref-format", "--branch", name).code === 0;
}

// src/entrypoints/machinery/lib/handoff-validation.ts
var PASSING2 = new Set(["success", "neutral", "skipped"]);
var COMMIT_IS_FINAL = new Set(["CLEAN", "UNSTABLE", "BLOCKED"]);
function handoffValidationNeeded(signals) {
  if (signals.kind !== "pull-request") {
    return { dispatch: false, why: "the node is an issue, which nothing merges" };
  }
  const state = (signals.state ?? "").toUpperCase();
  if (state !== "OPEN") {
    return { dispatch: false, why: `the pull request is ${state ? state.toLowerCase() : "in an unknown state"}, not open` };
  }
  if (signals.isDraft)
    return { dispatch: false, why: "the pull request is a draft" };
  const mergeState = (signals.mergeStateStatus ?? "").toUpperCase();
  if (!COMMIT_IS_FINAL.has(mergeState)) {
    return {
      dispatch: false,
      why: `GitHub reports ${mergeState || "no merge state"}, so the head commit will change before it is merged`
    };
  }
  const { required } = signals;
  if (!required.known) {
    return { dispatch: false, why: `the required contexts on the base branch could not be read (${required.why})` };
  }
  if (!required.enforceable)
    return { dispatch: false, why: "this repository cannot require status checks" };
  if (required.contexts.length === 0) {
    return { dispatch: false, why: "the base branch requires no status checks" };
  }
  const byName = new Map(signals.checks.map((check) => [check.name, check]));
  const unsatisfied = required.contexts.filter((context) => {
    const run = byName.get(context);
    return run === undefined || run.status !== "completed" || !PASSING2.has((run.conclusion ?? "").toLowerCase());
  });
  if (unsatisfied.length === 0) {
    return { dispatch: false, why: `every required context has passed on the head commit (${required.contexts.join(", ")})` };
  }
  return {
    dispatch: true,
    why: `required context${unsatisfied.length === 1 ? "" : "s"} not satisfied on the head commit: ` + unsatisfied.join(", ")
  };
}
function log2(message) {
  console.error(`[atomaton-handoff] ${message}`);
}
function ensureMergeableAtHandoff(kind, repoArg, number) {
  try {
    ensure(kind, (repoArg ?? "").trim(), number);
  } catch (e) {
    log2(`WARN could not check the head commit before the handoff: ${e.message}`);
  }
}
function ensure(kind, repoArg, number) {
  if (kind !== "pull-request")
    return;
  const repo = repoArg || (process.env.GITHUB_REPOSITORY ?? "").trim();
  if (!repo) {
    log2("WARN cannot check the head commit before the handoff: no repository is named");
    return;
  }
  const read = readPullRequest(repo, Number(number), [
    "headRefOid",
    "headRefName",
    "isCrossRepository",
    "baseRefName",
    "isDraft",
    "state",
    "mergeStateStatus"
  ]);
  if (!read.known) {
    log2(`WARN could not read the pull request before the handoff: ${read.why}`);
    return;
  }
  const view = read.value;
  const headSha = (view.headRefOid ?? "").trim();
  if (!headSha) {
    log2("WARN could not check the head commit before the handoff: the pull request reports no head commit");
    return;
  }
  const decision = handoffValidationNeeded({
    kind,
    state: view.state ?? "",
    isDraft: view.isDraft === true,
    mergeStateStatus: view.mergeStateStatus ?? "",
    required: readBranchRules(repo, (view.baseRefName ?? "").trim()),
    checks: readChecks(repo, headSha)
  });
  if (!decision.dispatch) {
    log2(decision.why);
    return;
  }
  log2(`${decision.why}; dispatching a validation so the mirror lands on ${headSha.slice(0, 7)}`);
  const head = prHead({ headRefName: view.headRefName, isCrossRepository: view.isCrossRepository });
  if (head.crossRepository) {
    log2("WARN the head is a fork's branch, not a branch of this repository; nothing was dispatched");
    return;
  }
  if (!isBranchName(head.branch)) {
    log2(`WARN '${head.branch}' is not a branch name this repository can use; nothing was dispatched`);
    return;
  }
  if (dispatchPrValidation(repo, Number(number), head.branch)) {
    log2(`dispatched validation for #${number} at the handoff, so the required check reaches ${headSha.slice(0, 7)}`);
  }
}
function readChecks(repo, sha) {
  const { code, stdout, stderr } = ghRead("api", `repos/${repo}/commits/${sha}/check-runs?per_page=100`);
  if (code) {
    log2(`WARN could not read the check runs on ${sha.slice(0, 7)}: ${stderr || stdout}`);
    return [];
  }
  try {
    const { check_runs = [] } = JSON.parse(stdout || "{}");
    return check_runs.map((run) => ({
      name: run.name ?? "",
      status: run.status ?? "",
      conclusion: run.conclusion ?? null
    }));
  } catch {
    log2(`WARN the check runs on ${sha.slice(0, 7)} were not JSON`);
    return [];
  }
}

// src/entrypoints/machinery/lib/turn-labels.ts
function applyTurnLabels(phase, repo, number) {
  const inProgress = getLabel("in_progress");
  const waitingForPerson = getLabel("waiting_for_person");
  const edit = (action, label) => gh("issue", "edit", number, action, label, ...repo ? ["--repo", repo] : []);
  const create = (label, color, description) => gh("label", "create", label, "--force", "-c", color, "-d", description, ...repo ? ["--repo", repo] : []);
  const state = readTargetState(number, repo);
  const present = state.known ? new Set(state.labels ?? []) : undefined;
  const remove = (label) => {
    if (present !== undefined && !present.has(label))
      return;
    const removed = edit("--remove-label", label);
    if (removed.code !== 0)
      console.error(`::warning::could not remove '${label}' from #${number}`);
  };
  let effective = phase;
  if (phase === "closed") {
    if (state.known && state.state === "open")
      return;
  } else if (!state.known) {
    console.error(`::warning::could not read #${number}'s state (${state.why}); applying '${phase}' labels without the closed guard`);
  } else if (state.state !== "open") {
    console.error(`#${number} is closed; the turn labels come off and neither goes on (#352)`);
    effective = "closed";
  }
  if (effective === "closed") {
    remove(waitingForPerson);
    remove(inProgress);
    return;
  }
  if (effective === "running") {
    create(inProgress, "0366d6", "Issue is being worked on by an Atomaton agent");
    const added = edit("--add-label", inProgress);
    if (added.code !== 0)
      console.error(`::warning::could not add '${inProgress}' to #${number}`);
    remove(waitingForPerson);
    return;
  }
  create(waitingForPerson, "fbca04", "The turn is with a person on this issue or pull request");
  remove(inProgress);
  const added = edit("--add-label", waitingForPerson);
  if (added.code !== 0)
    console.error(`::warning::could not add '${waitingForPerson}' to #${number}`);
  if (state.known)
    ensureMergeableAtHandoff(state.kind, repo, number);
}

// src/entrypoints/machinery/lib/removed-comment.ts
var MAX_PRESERVED_CHARS = 30000;
function fenceFor(text) {
  let longest = 0;
  for (const run of text.match(/`+/g) ?? [])
    longest = Math.max(longest, run.length);
  return "`".repeat(Math.max(3, longest + 1));
}
function preservedCommentBlock(original) {
  const truncated = original.length > MAX_PRESERVED_CHARS;
  const kept = truncated ? original.slice(0, MAX_PRESERVED_CHARS) : original;
  const fence = fenceFor(kept);
  return [
    "<details><summary>Your comment, as you wrote it</summary>",
    "",
    fence,
    kept,
    fence,
    ...truncated ? ["", `The comment was longer than ${MAX_PRESERVED_CHARS.toLocaleString("en-US")} characters; the rest is not preserved here.`] : [],
    "",
    "</details>"
  ];
}

// src/entrypoints/machinery/lib/script-ref.ts
import { basename } from "path";
import { fileURLToPath } from "url";
function defineScript(importMetaUrl) {
  return { runtimePath: `${SCRIPTS_DIR}/${basename(fileURLToPath(importMetaUrl))}` };
}

// src/entrypoints/machinery/request_stop.ts
var ref = defineScript(import.meta.url);
function commandsAfterStop(original) {
  return commandsIn(proseOnly(original)).slice(1).map(commandLabel);
}
function parkExit(openClaim) {
  if (openClaim === undefined || !openClaim.parked || openClaim.claimed)
    return;
  return { ask: openClaim.ask, run: 0 };
}
function parkEndedNotice(park) {
  return [
    LLM_CONTEXT_TAG.write("exclude"),
    ENDED_TAG.write({ verb: "stopped", answers: park.ask, run: 0 }),
    "Atomaton: the turn this node's agent parked on its subtree came back to you \u2014 your stop ended the park. " + "Nothing is running on this node. Comment again to start new work."
  ].join(`
`);
}
function stopRequestedNotice(commenter, deleted, alsoReached, extra) {
  const whose = commenter ? `${commenter}'s` : "The";
  const quoted = extra !== undefined && deleted && extra.original !== "";
  const lines = [
    LLM_CONTEXT_TAG.write("exclude"),
    ...quoted ? [QUOTES_TAG.write(Number(extra.commentId))] : [],
    STOP_TAG.write("requested"),
    "Atomaton: stop requested.",
    "",
    deleted ? `${whose} \`!stop\` comment was removed so it does not become part of the agent's context.` : `${whose} \`!stop\` comment could not be removed, so it may end up in the agent's context.`
  ];
  if (extra && extra.commands.length > 0) {
    const named = extra.commands.map((line) => `\`${line}\``).join(", ");
    lines.push("", `It also carried ${named}, which was not acted on. Atomaton acts on one command per comment, and a stop ` + "is the one command that has to reach a running agent, so the stop is the one that ran. Comment " + `${extra.commands.length > 1 ? "each of them" : "it"} on its own once the run has reported.`);
  }
  lines.push("", "The run will stop after its current step, so it may take a minute, and it will report here when it has.");
  if (extra && quoted) {
    lines.push("", ...preservedCommentBlock(extra.original));
  }
  if (alsoReached.length > 0) {
    lines.push("", `It also reached the work running on ${alsoReached.map((n) => `#${n}`).join(", ")}, ` + "which is under this issue. `/resume` here brings all of it back.");
  }
  return lines.join(`
`);
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      number: { type: "string" },
      "comment-id": { type: "string" },
      commenter: { type: "string" }
    }
  });
  if (!values.number || !values["comment-id"]) {
    console.error("usage: request_stop.ts --number N --comment-id ID --commenter LOGIN");
    process.exit(2);
  }
  const repo = process.env.GITHUB_REPOSITORY ?? "";
  const number = String(values.number);
  const original = process.env.ATOMATON_COMMENT_BODY ?? "";
  const others = commandsAfterStop(original);
  const extra = others.length > 0 ? { commands: others, original, commentId: values["comment-id"] } : undefined;
  const { code: delCode, stdout: delOut, stderr: delErr } = gh("api", "--method", "DELETE", `repos/${repo}/issues/comments/${values["comment-id"]}`);
  const deleted = delCode === 0;
  if (!deleted) {
    console.error(`Warning: failed to delete comment #${values["comment-id"]} on #${number}: ${delErr || delOut}`);
  }
  const root = Number(number);
  const { nodes } = readWorkTree(repo, root);
  const under = nodesToStop(descendants(subtree(nodes, root), root)).map((node) => node.number);
  let park;
  try {
    park = parkExit(openClaimOn(repo, number));
  } catch (e) {
    console.error(`::warning::could not read the thread on #${number}, so a parked turn could not be ended by this stop: ${e}`);
    park = undefined;
  }
  const result = requestStopAcross(repo, root, stopRequestedNotice(values.commenter ?? "", deleted, under, extra));
  for (const problem of result.problems)
    console.error(`::warning::${problem}`);
  if (!result.rootNotified)
    process.exit(1);
  if (park !== undefined) {
    gh("issue", "comment", String(number), "--body", parkEndedNotice(park));
    applyTurnLabels("with-person", repo, String(number));
    console.error(`#${number} was parked; the stop ended the park and returned the turn.`);
  }
  console.error(`Stop requested across #${root}: ${result.stopped.length ? result.stopped.map((n) => `#${n}`).join(", ") : "no run was holding anything"}`);
}
if (import.meta.main)
  main();
export {
  commandsAfterStop,
  parkEndedNotice,
  parkExit,
  ref,
  stopRequestedNotice
};
