#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/aggregate_sub_issues.ts
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
var MACHINERY_ROOT_VAR = "ATOMATON_MACHINERY_ROOT";

// src/entrypoints/machinery/lib/script-ref.ts
function defineScript(importMetaUrl) {
  return { runtimePath: `${SCRIPTS_DIR}/${basename(fileURLToPath(importMetaUrl))}` };
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
function dispatchWorkflow(context, workflow, args = [], log = (m) => console.error(m), token = "") {
  const argv = ["workflow", "run", workflow, ...args];
  const { code, stdout, stderr } = token.trim() ? ghAs(token.trim(), ...argv) : gh(...argv);
  if (code) {
    log(`${context}: WARN failed to dispatch ${workflow}: ${stderr || stdout}`);
    return false;
  }
  log(`${context}: dispatched ${workflow}`);
  return true;
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
function wasLaunched(comments, readers) {
  return comments.some((entry) => readers.isDispatchMarker(entry.body));
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
function commentBodies(repo, number) {
  return listComments(repo, number).map((comment) => comment.body ?? "");
}
function readComments(repo, number, excludeCommentId) {
  const excluded = String(excludeCommentId ?? "").trim();
  const comments = listComments(repo, number).filter((comment) => String(comment.id) !== excluded).map(entryOf);
  return shapedThread(comments, readers);
}
function wasLaunchedOn(repo, number) {
  return wasLaunched(readComments(repo, number).comments, readers);
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

// src/adapters/github/sibling-check.ts
function countOpenSiblings(opts) {
  const label = opts.label || getLabel("sub_issue");
  const links = issueLinks(opts.repo, opts.parent);
  if (links.unavailable) {
    throw new Error(`countOpenSiblings: could not read the sub-issues of #${opts.parent}: ${links.unavailable}`);
  }
  return links.children.filter((child) => child.state === "open" && child.labels.includes(label) && child.number !== opts.exclude && wasLaunchedOn(opts.repo, child.number)).length;
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
  if (!dispatchWorkflow(d.context, runnerWorkflow(), args, d.log, d.token)) {
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
var MAX_PARENT_HOPS = 6;
function* parentChain(start, read, maxHops = MAX_PARENT_HOPS) {
  const visited = new Set;
  let current = start;
  for (let hop = 0;hop < maxHops; hop++) {
    if (visited.has(current))
      return;
    visited.add(current);
    const { data, parent } = read(current);
    yield { number: current, data, parent };
    if (!parent.known || parent.parent === 0)
      return;
    current = parent.parent;
  }
}

// src/adapters/github/notify.ts
function log2(message) {
  console.error(`[atomaton-notify] ${message}`);
}
function repositoryOwner(repo) {
  const owner = repo.split("/")[0]?.trim() ?? "";
  if (!owner)
    log2(`WARN could not read an owner out of ${JSON.stringify(repo)}; nobody will be mentioned`);
  return owner;
}
function fetchIssueLookup(repo, number) {
  const { code, stderr, stdout } = gh("api", `repos/${repo}/issues/${number}`, "--jq", "{body: .body, login: .user.login, type: .user.type, is_pr: (.pull_request != null)}");
  if (code !== 0 || !stdout.trim()) {
    log2(`WARN could not read issue #${number} to resolve a mention: ${stderr.trim() || `gh exited ${code}`}`);
    return {};
  }
  try {
    return JSON.parse(stdout);
  } catch {
    log2(`WARN issue #${number} lookup was not valid JSON; no mention will be resolved from it`);
    return {};
  }
}
function nativeParentOf(repo, issue) {
  const found = parentIssueOf(repo, issue);
  return found.known && found.parent ? found.parent : undefined;
}
function resolveNotify(repo, number) {
  const read = (current) => {
    const d = fetchIssueLookup(repo, current);
    const body = d.body ?? "";
    const parent = d.is_pr ? PARENT_ISSUE_TAG.readProse(body) : nativeParentOf(repo, current);
    return { data: d, parent: parent === undefined ? { known: false, why: "no parent" } : { known: true, parent } };
  };
  for (const hop of parentChain(number, read)) {
    const body = hop.data.body ?? "";
    const tagged = NOTIFY_TAG.readProse(body);
    if (tagged)
      return tagged;
    if (isHumanActor(hop.data.type) && hop.data.login)
      return hop.data.login;
  }
  const owner = repositoryOwner(repo);
  if (owner)
    log2(`no requester found for #${number}; falling back to the repository owner @${owner}`);
  return owner;
}

// src/adapters/github/agent-on-issue.ts
function mostRecentAgent(bodies) {
  for (let i = bodies.length - 1;i >= 0; i--) {
    const body = bodies[i] ?? "";
    if (isQuotation(body))
      continue;
    const result = AGENT_TAG.readProse(body);
    if (result)
      return result;
    const asked = parseCommentCommand(proseOnly(body)).agent;
    if (asked)
      return asked;
  }
  return "";
}
function mostRecentAgentOn(repo, number) {
  try {
    return mostRecentAgent(commentBodies(repo, number));
  } catch {
    return "";
  }
}

// src/adapters/github/work-tree.ts
function commentOnWithId(repo, number, body) {
  const { code, stdout } = gh("api", `repos/${repo}/issues/${number}/comments`, "--method", "POST", "-f", `body=${body}`, "--jq", ".id");
  if (code !== 0)
    return null;
  return stdout.trim() || null;
}

// src/adapters/github/sub-issue-report.ts
var REPORT_FOLD_OPEN = "<details><summary>Report</summary>";
function subIssueReportHeaderLines(child) {
  return [LLM_CONTEXT_TAG.write("include"), REPORT_FOLD_OPEN, "", `**Sub-issue #${child}**`];
}
function subIssueProgressLine(child, remaining) {
  return `Atomaton: Sub-task #${child} completed. ${remaining} sub-task(s) still in progress.`;
}
var PROGRESS_OPENING = "Atomaton: Sub-task #";
function withProgressLine(body, line) {
  const kept = body.split(`
`).filter((text) => !text.trimStart().startsWith(PROGRESS_OPENING));
  return `${kept.join(`
`).replace(/\s+$/, "")}

${line}`;
}
function findSubIssueReport(repo, parent, child) {
  const header = subIssueReportHeaderLines(child).join(`
`);
  for (const comment of listComments(repo, parent)) {
    const body = comment.body ?? "";
    if (isHumanActor(comment.user?.type))
      continue;
    if (!body.trimStart().startsWith(header))
      continue;
    return { id: String(comment.id), body };
  }
  return;
}
function mergeProgressIntoReport(repo, parent, child, line) {
  let found;
  try {
    found = findSubIssueReport(repo, parent, child);
  } catch (error) {
    console.error(`[atomaton-report] could not look for #${child}'s report on #${parent}: ${error.message}`);
    return false;
  }
  if (found === undefined)
    return false;
  const body = withProgressLine(found.body, line);
  if (body === found.body)
    return true;
  const { code, stdout, stderr } = gh("api", `repos/${repo}/issues/comments/${found.id}`, "--method", "PATCH", "-f", `body=${body}`);
  if (code !== 0) {
    console.error(`[atomaton-report] WARN could not add the progress line to #${child}'s report on #${parent}: ${stderr || stdout}`);
    return false;
  }
  return true;
}

// src/app/aggregation.ts
function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
function parentAgent(repo, parent) {
  return mostRecentAgentOn(repo, parent);
}
function needsAttention(result) {
  return result.kind === "dispatch-failed" || result.kind === "undetermined" || result.kind === "parent-closed" || result.kind === "parent-busy";
}
function describeGateResult(result, closedNum, parent) {
  const which = parent === undefined ? "the parent issue" : `#${parent}`;
  switch (result.kind) {
    case "not-tracked":
      return `#${closedNum} is not a tracked sub-issue; nothing to aggregate.`;
    case "waiting":
      return `${result.remaining} sibling(s) of ${which} still open. No action needed.`;
    case "already-aggregated":
      return `Another caller already aggregated #${closedNum}. Nothing to do -- this is the normal race.`;
    case "dispatched":
      return `All sub-tasks of ${which} complete. The parent's agent was re-invoked.`;
    case "dispatch-failed":
      return `All sub-tasks of ${which} complete, but the dispatch FAILED. ` + `The aggregation marker was removed, so closing a sub-issue again retries it -- ` + `or re-run the parent's agent by hand.`;
    case "parent-closed":
      return `All sub-tasks of ${which} complete, but ${which} is closed, so no agent was started. ` + `The aggregation marker is already written, so no other caller will retry: ` + `reopen it and run the parent's agent by hand. Whoever asked for the run has been told on the issue.`;
    case "parent-busy":
      return `All sub-tasks of ${which} complete, but ${which} already had an agent asked for on it, ` + `so no second one was started. The aggregation marker was removed, so closing a sub-issue ` + `again retries it once that run is done: the run that was asked for first is the one to ` + `wait for.`;
    case "undetermined":
      return `Did not aggregate #${closedNum}: ${result.why}. Nothing was dispatched, and nothing will retry.`;
  }
}
async function dispatchOrchestratorIfReady(opts) {
  const excludeNum = opts.exclude ? opts.closedNum : undefined;
  const count = () => countOpenSiblings({ repo: opts.repo, parent: opts.parent, exclude: excludeNum });
  let remaining;
  try {
    remaining = count();
    if (opts.retry) {
      for (let attempt = 1;remaining > 0 && attempt < 4; attempt++) {
        await sleep(2000 * attempt);
        remaining = count();
      }
    }
  } catch (error) {
    const why = `could not count #${opts.parent}'s open sub-issues: ${error.message}`;
    console.error(why);
    return withNotice(opts, { kind: "undetermined", why });
  }
  if (remaining > 0) {
    if (opts.progress) {
      const line = subIssueProgressLine(opts.closedNum, remaining);
      if (!mergeProgressIntoReport(opts.repo, opts.parent, opts.closedNum, line)) {
        gh("issue", "comment", String(opts.parent), "--repo", opts.repo, "--body", `${LLM_CONTEXT_TAG.write("exclude")}
${SUB_RESULT_TAG.write(opts.closedNum)}
${line}`);
      }
    }
    return { kind: "waiting", remaining };
  }
  let markerAlreadyWritten;
  try {
    markerAlreadyWritten = commentBodies(opts.repo, opts.parent).some((body) => AGGREGATED_TAG.readProse(body) === opts.closedNum);
  } catch {
    const why = `could not read #${opts.parent}'s comments, so this cannot tell whether the aggregation already ran`;
    console.error(`${why}; not dispatching`);
    return withNotice(opts, { kind: "undetermined", why });
  }
  if (markerAlreadyWritten) {
    return { kind: "already-aggregated" };
  }
  const agent = parentAgent(opts.repo, opts.parent);
  const markerLines = [
    AGGREGATED_TAG.write(opts.closedNum),
    `Atomaton: All sub-tasks completed (last: #${opts.closedNum}). Re-invoking the parent's agent for aggregation.`
  ];
  const markerId = commentOnWithId(opts.repo, opts.parent, [...agent === "" ? [] : [DISPATCH_TAG.write(agent)], ...markerLines].join(`
`));
  if (markerId === null) {
    const why = `could not write the aggregation marker on #${opts.parent} (the comment POST failed)`;
    console.error(`${why}; not dispatching, because without the marker a second caller would dispatch too`);
    return withNotice(opts, { kind: "undetermined", why });
  }
  const outcome = dispatchRunner({
    context: `all sub-issues of #${opts.parent} are complete, so the agent that was on it was to be re-invoked`,
    agent,
    type: "issue",
    number: opts.parent,
    notify: resolveNotify(opts.repo, opts.parent),
    repo: opts.repo,
    continuesChain: opts.continuesChain,
    markerCommentId: markerId
  });
  const takeBackMarker = (why) => {
    const removed = gh("api", "--method", "DELETE", `repos/${opts.repo}/issues/comments/${markerId}`);
    if (removed.code === 0) {
      console.error(`${why}; the aggregation marker was removed so this can be retried`);
    } else {
      console.error(`${why}; AND the aggregation marker could not be removed (${removed.stderr.trim() || removed.stdout.trim()}), ` + `so nothing will retry: aggregate #${opts.parent} by hand`);
    }
  };
  const takeBackDispatchTag = () => {
    const { code, stdout, stderr } = gh("api", `repos/${opts.repo}/issues/comments/${markerId}`, "--method", "PATCH", "-f", `body=${markerLines.join(`
`)}`);
    if (code !== 0) {
      console.error(`#${opts.parent} is closed, and the dispatch tag could not be taken off its marker ` + `(${stderr.trim() || stdout.trim()}): the thread will read as though a run were coming.`);
    }
  };
  if (outcome === "dispatched")
    return { kind: "dispatched" };
  if (outcome === "refused-closed") {
    takeBackDispatchTag();
    return { kind: "parent-closed" };
  }
  if (outcome === "refused-outstanding")
    return { kind: "parent-busy" };
  if (outcome === "unconfirmed")
    return { kind: "dispatch-failed" };
  takeBackMarker(`the dispatch of ${agent || "(no agent)"} onto #${opts.parent} failed`);
  return { kind: "dispatch-failed" };
}
function withNotice(opts, result) {
  if (result.kind !== "undetermined")
    return result;
  const body = `${LLM_CONTEXT_TAG.write("exclude")}
` + `Atomaton: this sub-issue's completion could not be aggregated into this issue \u2014 ` + `${result.why}.

` + `Nothing will retry on its own, and the work under this issue is done. ` + `Fix the cause and comment \`/atomaton\` here to continue, or close one of the ` + `sub-issues again to retry the aggregation.`;
  try {
    gh("issue", "comment", String(opts.parent), "--repo", opts.repo, "--body", body);
  } catch {}
  return result;
}

// src/entrypoints/machinery/aggregate_sub_issues.ts
var ref = defineScript(import.meta.url);
async function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      repo: { type: "string" },
      parent: { type: "string" },
      "closed-num": { type: "string" }
    }
  });
  const repo = values.repo;
  const parent = values.parent;
  const closedNum = values["closed-num"];
  if (!repo || !parent || !closedNum) {
    console.error("usage: aggregate_sub_issues.ts --repo OWNER/REPO --parent N --closed-num N");
    process.exit(2);
  }
  console.error(`PR merged (sub-issue #${closedNum}, parent #${parent}). Checking siblings...`);
  const result = await dispatchOrchestratorIfReady({
    repo,
    parent: Number(parent),
    closedNum: Number(closedNum),
    exclude: true,
    progress: true
  });
  console.error(describeGateResult(result, Number(closedNum), Number(parent)));
  if (needsAttention(result))
    process.exit(1);
}
if (import.meta.main)
  main();
export {
  ref
};
