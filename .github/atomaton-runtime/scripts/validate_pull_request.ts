#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/validate_pull_request.ts
import { appendFileSync as appendFileSync2, existsSync as existsSync2, readFileSync as readFileSync2 } from "fs";
import { parseArgs } from "util";

// src/domain/work/pr-validation.ts
var PASSING = new Set(["success", "skipped", "neutral"]);
function contextsPassed(verdict) {
  return verdict === "passed";
}
function handTo(agent) {
  const named = agent.trim();
  return named === "" ? {} : { next: { agent: named } };
}
function askFor(engineerAgent) {
  const named = engineerAgent.trim();
  return named === "" ? "" : ` Comment /${named} on this pull request to ask for it.`;
}
function decideValidationOutcome(input) {
  const { conclusion, reviewerAgent, engineerAgent } = input;
  const hold = input.hold ?? { kind: "with-person" };
  const callWaiting = hold.kind === "free" || hold.kind === "parked";
  const next = callWaiting ? handTo(reviewerAgent) : {};
  const handedOn = next.next !== undefined;
  const deliverableProblems = input.deliverableProblems ?? [];
  if (deliverableProblems.length > 0) {
    const count = `${deliverableProblems.length} problem${deliverableProblems.length === 1 ? "" : "s"}`;
    const headline = `.github/atomaton/ is not internally consistent (${count}), so CI was not run.`;
    return {
      verdict: "deliverable-invalid",
      ...next,
      summary: handedOn ? headline : `${headline}${askFor(engineerAgent)}`
    };
  }
  const normalised = conclusion.trim().toLowerCase();
  const passed = PASSING.has(normalised);
  if (passed) {
    if (handedOn) {
      return { verdict: "passed", ...next, summary: `CI concluded ${normalised}.` };
    }
    const because = {
      free: "the thread names no agent to start",
      parked: "the thread names no agent to start",
      "in-flight": "a run already holds this pull request, and its own report is the thread's next word",
      chain: "the thread's newest word handed the work to another agent, and that call is the chain's to carry",
      "with-person": "the thread's newest word handed the turn back to a person",
      unreadable: "the thread could not be read to rule out a run in flight"
    };
    return {
      verdict: "passed",
      summary: `CI concluded ${normalised}, and nothing was dispatched: ${because[hold.kind]}.`
    };
  }
  if (!normalised) {
    return {
      verdict: "no-conclusion",
      summary: "CI never reported a conclusion. Nothing was dispatched; a human should look."
    };
  }
  return {
    verdict: "failed",
    ...next,
    summary: handedOn ? `CI concluded ${normalised}.` : `CI concluded ${normalised}. Nobody was dispatched to fix it.${askFor(engineerAgent)}`
  };
}

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
function openUnderOf(repo, number) {
  const links = issueLinks(repo, Number(number));
  if (links.unavailable !== undefined)
    return;
  const open = (nodes) => nodes.filter((node) => node.state === "open").length;
  return open(links.children) + open(links.pullRequests);
}
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

// src/domain/work/stale-lock.ts
var CLAIM_WITHOUT_RUN_MINUTES = 360;
var PARK_COLLECTION_MINUTES = 30;
function ageMinutes(at, nowMs) {
  const parsed = Date.parse(at);
  if (!Number.isFinite(parsed))
    return;
  return (nowMs - parsed) / 60000;
}
function claimFreshness(claim, run, nowMs) {
  if (claim.run > 0) {
    if (!run.read)
      return "unknown";
    return run.status === "completed" ? "stale" : "fresh";
  }
  const age = ageMinutes(claim.at, nowMs);
  if (age === undefined)
    return "unknown";
  return age >= CLAIM_WITHOUT_RUN_MINUTES ? "stale" : "fresh";
}
function parkOverdue(parkedSince, nowMs) {
  const age = ageMinutes(parkedSince, nowMs);
  return age !== undefined && age >= PARK_COLLECTION_MINUTES;
}
function lockVerdict(args) {
  if (args.parked) {
    if (args.openUnder === undefined)
      return "undetermined";
    if (args.openUnder > 0)
      return "hold";
    if (!parkOverdue(args.parkedSince, args.nowMs))
      return "hold";
    if (args.claim === undefined)
      return "release";
    const parkedClaim = claimFreshness(args.claim, args.run, args.nowMs);
    return parkedClaim === "fresh" ? "hold" : parkedClaim === "unknown" ? "undetermined" : "release";
  }
  if (args.claim === undefined)
    return "hold";
  const freshness = claimFreshness(args.claim, args.run, args.nowMs);
  if (freshness === "unknown")
    return "undetermined";
  return freshness === "fresh" ? "hold" : "release";
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
function latestRequestedAgent(body, comments, readers) {
  const { comments: kept } = shapedThread(comments, readers);
  for (let index = kept.length - 1;index >= 0; index -= 1) {
    const entry = kept[index];
    if (entry.quoted)
      continue;
    const agent = readers.requestedAgent(entry.body);
    if (agent !== "")
      return agent;
  }
  return readers.requestedAgent(body);
}
function threadHold(events, openClaim) {
  const newest = events[events.length - 1];
  if (openClaim !== undefined) {
    return newest === "waiting" ? { kind: "parked" } : { kind: "in-flight" };
  }
  if (newest === undefined || newest === "asked")
    return { kind: "free" };
  if (newest === "waiting")
    return { kind: "parked" };
  if (newest === "handed-off")
    return { kind: "chain" };
  return { kind: "with-person" };
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
function readThread(repo, number, excludeCommentId) {
  const issue = ghRead("api", `repos/${repo}/issues/${number}`, "--jq", ".body");
  if (issue.code !== 0)
    throw new Error(`could not read #${number}: ${issue.stderr || issue.stdout}`);
  return { body: issue.stdout ?? "", shaped: readComments(repo, number, excludeCommentId) };
}
function lockOf(entries, shaped) {
  const park = shaped.events[shaped.events.length - 1] === "waiting" ? newestWaiting(entries) : undefined;
  const open = shaped.openClaim;
  const lock = park === undefined ? {} : { park };
  if (open === undefined)
    return lock;
  const at = newestWith(entries, (body) => {
    const claim = readers.claim(body);
    return claim !== undefined && claim.ask === open.ask && claim.run === open.run;
  });
  return { ...lock, claim: { ...open, at } };
}
function newestWith(entries, which) {
  for (let index = entries.length - 1;index >= 0; index -= 1) {
    const entry = entries[index];
    if (entry.quoted)
      continue;
    if (which(entry.body))
      return entry.at ?? "";
  }
  return "";
}
function newestWaiting(entries) {
  for (let index = entries.length - 1;index >= 0; index -= 1) {
    const entry = entries[index];
    if (entry.quoted)
      continue;
    const ended = ENDED_TAG.readProse(entry.body);
    if (ended?.verb === "waiting")
      return { at: entry.at ?? "", answers: ended.answers };
  }
  return;
}
function latestRequestOn(repo, number, isKnownAgent) {
  const { body, shaped } = readThread(repo, number);
  const known = (name) => isKnownAgent(name) ? name : "";
  return {
    newestRequestedAgent: latestRequestedAgent(body, shaped.comments, {
      ...readers,
      requestedAgent: (text) => known(readers.requestedAgent(text))
    }),
    hold: holdHealed(repo, number, shaped)
  };
}
function holdHealed(repo, number, shaped) {
  const hold = threadHold(shaped.events, shaped.openClaim);
  if (hold.kind !== "in-flight" && hold.kind !== "parked")
    return hold;
  const lock = lockOf(shaped.comments, shaped);
  const verdict = lockVerdict({
    claim: lock.claim,
    parked: lock.park !== undefined,
    parkedSince: lock.park?.at ?? "",
    openUnder: lock.park !== undefined ? openUnderOf(repo, number) : 0,
    run: lock.claim !== undefined && lock.claim.run > 0 ? readRunStatus(repo, lock.claim.run) : { read: false, why: "the claim names no run" },
    nowMs: Date.now()
  });
  return verdict === "release" ? { kind: "free" } : hold;
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

// src/entrypoints/machinery/lib/flags.ts
function isTrue(value) {
  return value === "true";
}

// src/entrypoints/machinery/validate_pull_request.ts
var ref2 = defineScript(import.meta.url);
function log(message) {
  console.error(`[atomaton-validate-pr] ${message}`);
}
function pickDispatchedRun(runs, headSha, since) {
  const candidates = runs.filter((run) => run.event === "workflow_dispatch").filter((run) => run.head_sha === headSha).filter((run) => run.created_at >= since).sort((a, b) => a.created_at < b.created_at ? 1 : -1);
  const run = candidates[0];
  return run ? { id: run.id, status: run.status, conclusion: run.conclusion } : undefined;
}
var VERDICT_EVENTS = new Set(["workflow_dispatch", "pull_request"]);
function hasVerdict(run) {
  if (run.status !== "completed")
    return false;
  const conclusion = (run.conclusion ?? "").trim();
  return conclusion !== "" && conclusion !== "cancelled" && conclusion !== "action_required";
}
function waitable(run) {
  return run.status !== "completed" && run.event === "workflow_dispatch";
}
function findExistingCiRun(runs, workflow, headSha) {
  const onThisCommit = runs.filter((run) => VERDICT_EVENTS.has(run.event)).filter((run) => run.head_sha === headSha).filter((run) => run.path.endsWith(`/${workflow}`));
  const newest = (candidates) => [...candidates].sort((a, b) => a.created_at < b.created_at ? 1 : -1)[0];
  const run = newest(onThisCommit.filter(hasVerdict)) ?? newest(onThisCommit.filter(waitable));
  return run ? { id: run.id, status: run.status, conclusion: run.conclusion } : undefined;
}
function reportFailure(repo, number, runUrl, summary, details = []) {
  const body = [
    LLM_CONTEXT_TAG.write("include"),
    `Atomaton: ${summary}`,
    ...details.length > 0 ? ["", ...details.map((detail) => `- ${detail}`)] : [],
    "",
    runUrl ? `Failing run: ${runUrl}` : ""
  ].filter(Boolean).join(`
`);
  const posted = gh("issue", "comment", number, "--repo", repo, "--body", body);
  if (posted.code)
    log(`WARN could not post the failure comment: ${posted.stderr}`);
}
function writeCheckRun(repo, headSha, name, conclusion) {
  const post = () => gh("api", "--method", "POST", `repos/${repo}/check-runs`, "-f", `name=${name}`, "-f", `head_sha=${headSha}`, "-f", "status=completed", "-f", `conclusion=${conclusion}`);
  let written = post();
  if (written.code) {
    log(`WARN could not write check "${name}" as ${conclusion}: ${written.stderr || written.stdout}; retrying once`);
    written = post();
  }
  if (written.code) {
    console.error(`::error::could not write the required check "${name}" on ${headSha.slice(0, 7)}: ` + `${written.stderr || written.stdout}`);
    return;
  }
  log(`wrote check "${name}" as ${conclusion}`);
}
function notifyCiWaiting(repo, number, askedByPerson, runUrl) {
  if (!askedByPerson)
    return;
  const body = [
    LLM_CONTEXT_TAG.write("exclude"),
    `Atomaton: CI is running on this pull request (${runUrl}). Its result decides who works next.`
  ].join(`
`);
  const posted = gh("issue", "comment", number, "--repo", repo, "--body", body);
  if (posted.code)
    log(`WARN could not post the CI-waiting comment: ${posted.stderr}`);
}
function runCiAndWait(repo, number, workflow, branch, headSha, timeoutSeconds, askedByPerson) {
  const runUrlFor = (id) => `https://github.com/${repo}/actions/runs/${id}`;
  const listRuns = () => {
    const listed = ghRead("api", `repos/${repo}/actions/runs?head_sha=${headSha}&per_page=100`).stdout;
    const { workflow_runs = [] } = JSON.parse(listed || "{}");
    return workflow_runs;
  };
  const existing = findExistingCiRun(listRuns(), workflow, headSha);
  if (existing?.status === "completed") {
    log(`reusing CI run ${existing.id} for ${headSha.slice(0, 7)}: ${existing.conclusion}`);
    return { conclusion: existing.conclusion ?? "", runUrl: runUrlFor(existing.id) };
  }
  const alreadyRunning = existing !== undefined && existing.status !== "completed";
  const since = new Date().toISOString();
  let notified = alreadyRunning;
  if (alreadyRunning) {
    log(`CI run ${existing.id} is already ${existing.status} for ${headSha.slice(0, 7)}; waiting for it`);
    notifyCiWaiting(repo, number, askedByPerson, runUrlFor(existing.id));
  } else {
    if (!dispatchWorkflow(`validate_pull_request: CI for ${branch}`, workflow, ["--repo", repo, "--ref", branch], log)) {
      process.exit(1);
    }
  }
  const deadline = Date.now() + timeoutSeconds * 1000;
  while (Date.now() < deadline) {
    Bun.sleepSync(1e4);
    const run = alreadyRunning ? listRuns().find((candidate) => candidate.id === existing.id) : pickDispatchedRun(listRuns(), headSha, since);
    if (!run)
      continue;
    if (!notified && run.status !== "completed") {
      notifyCiWaiting(repo, number, askedByPerson, runUrlFor(run.id));
      notified = true;
    }
    if (run.status !== "completed")
      continue;
    const conclusion = run.conclusion ?? "";
    log(`CI run ${run.id} concluded ${conclusion}`);
    return { conclusion, runUrl: runUrlFor(run.id) };
  }
  log(`no conclusion within ${timeoutSeconds}s`);
  return { conclusion: "", runUrl: "" };
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      repo: { type: "string" },
      number: { type: "string" },
      branch: { type: "string" },
      workflow: { type: "string" },
      "def-dir": { type: "string" },
      "deliverable-report": { type: "string" },
      "asked-by-person": { type: "string" },
      "timeout-seconds": { type: "string" }
    }
  });
  const repo = values.repo ?? "";
  const branch = values.branch ?? "";
  const workflow = values.workflow ?? "";
  const defDir = values["def-dir"] ?? "";
  if (!repo || !branch || !workflow || !defDir) {
    console.error("usage: validate_pull_request.ts --repo owner/name --number N --branch B --workflow W --def-dir DIR");
    process.exit(1);
  }
  const reportPath = values["deliverable-report"] ?? "";
  if (!reportPath) {
    console.error("usage: validate_pull_request.ts ... --deliverable-report FILE");
    process.exit(1);
  }
  if (!existsSync2(reportPath)) {
    log(`cannot validate: no deliverable report at ${reportPath}`);
    process.exit(1);
  }
  const deliverableProblems = readFileSync2(reportPath, "utf8").split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  if (deliverableProblems.length > 0) {
    log(`the deliverable is inconsistent (${deliverableProblems.length} problem(s)); CI will not be dispatched`);
  }
  const githubOutput = process.env.GITHUB_OUTPUT;
  const write = (line) => {
    if (githubOutput)
      appendFileSync2(githubOutput, `${line}
`);
  };
  const prRead = readPullRequest(repo, Number(values.number), ["headRefOid", "baseRefName"]);
  if (!prRead.known)
    log(prRead.why);
  const headSha = prRead.known ? prRead.value.headRefOid ?? "" : "";
  const baseRef = prRead.known ? prRead.value.baseRefName ?? "" : "";
  if (!headSha) {
    log("could not read the pull request's head SHA");
    process.exit(1);
  }
  const required = readBranchRules(repo, baseRef);
  if (!required.known) {
    log(`cannot validate: ${required.why}`);
    process.exit(1);
  }
  const requiredContexts = required.contexts;
  log(`required contexts on ${baseRef}: ${requiredContexts.join(", ") || "(none)"}`);
  if (!required.enforceable) {
    log(`::notice::${required.why}. Atomaton enforces the CI result itself when merging.`);
  } else if (requiredContexts.length === 0) {
    log(`::warning::${baseRef} requires no status checks, so CI results gate nothing here. ` + "Import .github/atomaton/rulesets/main.json if that was not intended.");
  }
  const { conclusion, runUrl } = deliverableProblems.length > 0 ? { conclusion: "", runUrl: "" } : runCiAndWait(repo, values.number ?? "", workflow, branch, headSha, Number(values["timeout-seconds"] ?? "1800"), isTrue(values["asked-by-person"]));
  let hold;
  let threadRequest;
  try {
    threadRequest = latestRequestOn(repo, values.number ?? "", (name) => hasAgentDefinition(name, defDir));
    hold = threadRequest.hold;
  } catch (e) {
    log(`could not read the thread for the routing gate: ${e.message}`);
    hold = { kind: "unreadable" };
  }
  const bodyRead = readPullRequest(repo, Number(values.number), "body");
  const prBody = bodyRead.known ? bodyRead.value.body ?? "" : "";
  const engineerAgent = ORIGIN_AGENT_TAG.readProse(prBody) ?? "";
  const outcome = decideValidationOutcome({
    conclusion,
    reviewerAgent: threadRequest?.newestRequestedAgent ?? "",
    engineerAgent,
    hold,
    deliverableProblems
  });
  const conclusionForContexts = contextsPassed(outcome.verdict) ? "success" : "failure";
  for (const name of requiredContexts)
    writeCheckRun(repo, headSha, name, conclusionForContexts);
  if (outcome.verdict !== "passed") {
    reportFailure(repo, values.number ?? "", runUrl, outcome.summary, deliverableProblems);
  }
  write(`next_agent=${outcome.next?.agent ?? ""}`);
  write(`conclusion=${conclusion}`);
  write(`verdict_passed=${contextsPassed(outcome.verdict)}`);
  write(`summary=${outcome.summary}`);
}
if (import.meta.main)
  main();
export {
  findExistingCiRun,
  pickDispatchedRun,
  ref2 as ref
};
