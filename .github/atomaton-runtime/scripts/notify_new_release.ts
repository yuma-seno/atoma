#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/notify_new_release.ts
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

// src/entrypoints/machinery/lib/notice-issue.ts
var NOTICE_MARKER = "<!-- atomaton:new-release -->";
function findOpenNotice(repo) {
  const result = ghRead("issue", "list", "--repo", repo, "--state", "open", "--search", `"Atomaton release" in:title`, "--json", "number,body,state");
  if (result.code !== 0) {
    log(`WARN could not look for an existing notice in ${repo}: ${result.stderr.trim() || `gh exited ${result.code}`}`);
    return null;
  }
  let parsed;
  try {
    parsed = JSON.parse(result.stdout || "[]");
  } catch {
    log(`WARN the issue search in ${repo} did not answer with JSON`);
    return null;
  }
  if (!Array.isArray(parsed)) {
    log(`WARN the issue search in ${repo} did not answer with a list`);
    return null;
  }
  const found = parsed.find((entry) => {
    const issue = entry;
    return typeof issue.body === "string" && issue.body.includes(NOTICE_MARKER) && (issue.state === undefined || issue.state === "OPEN");
  });
  const number = Number(found?.number ?? 0);
  return Number.isInteger(number) && number > 0 ? number : 0;
}
function log(message) {
  console.error(`[notice-issue] ${message}`);
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

// src/entrypoints/machinery/lib/update-branch.ts
function updateBranchFor(tag) {
  return `atomaton/update-${tag}`;
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
var NEW_RELEASE_SCOPES = ["all", "marked"];
function isNewReleaseScope(value) {
  return typeof value === "string" && NEW_RELEASE_SCOPES.includes(value);
}

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
function getNewReleaseMode(fallback = "issue") {
  const mode = loadConfig().update?.on_new_release;
  return isNewReleaseMode(mode) ? mode : fallback;
}
function getNewReleaseScope(fallback = "all") {
  const scope = loadConfig().update?.detection_scope;
  return isNewReleaseScope(scope) ? scope : fallback;
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
function log2(message) {
  console.error(`[atomaton-github] ${message}`);
}
function dispatchUpdate(repo, source) {
  return dispatchWorkflow(`dispatchUpdate: taking a newer release onto ${repo}`, "atomaton-update.yml", ["--repo", repo, "-f", `source=${source > 0 ? String(source) : ""}`], log2);
}

// src/entrypoints/machinery/lib/release-files.ts
import { existsSync, mkdtempSync, readFileSync as readFileSync3, readdirSync, rmSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";

// src/entrypoints/machinery/lib/zip-read.ts
import { readFileSync as readFileSync2 } from "fs";
import { inflateRawSync } from "zlib";
var EOCD_SIG = 101010256;
var CENTRAL_SIG = 33639248;
var LOCAL_SIG = 67324752;
var EOCD_MAX_SEARCH = 22 + 65535;
function zipEntries(path) {
  let buf;
  try {
    buf = readFileSync2(path);
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
  const dir = mkdtempSync(join(tmpdir(), "atomaton-release-"));
  try {
    const downloaded = ghRead("release", "download", tag, "-R", UPSTREAM_REPOSITORY, "-p", ASSET_NAME, "-D", dir);
    if (downloaded.code !== 0) {
      return { why: `could not download ${tag}'s ${ASSET_NAME}: ${downloaded.stderr.trim() || `gh exited ${downloaded.code}`}` };
    }
    const asset = join(dir, ASSET_NAME);
    if (!existsSync(asset)) {
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
      rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 });
    } catch {}
  }
}
function readTreeFiles(paths) {
  const files = new Map;
  for (const path of paths) {
    const onDisk = machineryPath(path);
    if (!existsSync(onDisk))
      continue;
    try {
      files.set(path, readFileSync3(onDisk, "utf8"));
    } catch {}
  }
  return files;
}
function readLocalManifest() {
  const path = machineryPath(RELEASE_MANIFEST);
  if (!existsSync(path)) {
    return { why: `${path} is missing, so this tree cannot say which release it came from` };
  }
  try {
    const parsed = JSON.parse(readFileSync3(path, "utf8"));
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
// src/entrypoints/machinery/notify_new_release.ts
var ref = defineScript(import.meta.url);
var UPGRADE_DOC = `https://github.com/${UPSTREAM_REPOSITORY}/blob/main/docs/runtime/tasks/move-to-a-newer-release.md`;
var MAX_RELEASES = 20;
var MAX_NOTES_CHARS = 1200;
function releaseWindow(notes, localVersion, max = MAX_RELEASES) {
  const at = notes.findIndex((note) => note.tag === localVersion);
  const newer = at === -1 ? [...notes] : notes.slice(0, at);
  return { newer: newer.slice(0, max), omitted: Math.max(0, newer.length - max), found: at !== -1 };
}
var ADOPT_MARKER = "atomaton:adopt";
function isMarked(note) {
  return note.body.includes(ADOPT_MARKER);
}
function markedReleases(notes, localVersion) {
  return releaseWindow(notes, localVersion, notes.length).newer.filter(isMarked).map((note) => note.tag);
}
function notesOf(body) {
  const demoted = body.trim().split(`
`).map((line) => /^#{1,4}\s/.test(line) ? `##${line}` : line).join(`
`);
  if (demoted.length <= MAX_NOTES_CHARS)
    return demoted;
  return `${demoted.slice(0, MAX_NOTES_CHARS).trimEnd()}

\u2026the rest is on the release page.`;
}
function autoUpdateWarningLines(failure) {
  return [
    "> [!WARNING]",
    `> \`update.on_new_release\` is \`${failure.mode}\` here, but nothing was started: ${failure.why}.`,
    `> Take the release by hand with the steps below, or see [move to a newer release](${UPGRADE_DOC}).`,
    ""
  ];
}
function noticeTitle(input) {
  return `Atomaton release ${input.latestVersion} is available; .github/ is on ${input.localVersion}`;
}
function noticeBody(input) {
  const lines = [NOTICE_MARKER];
  if (input.autoUpdateFailure !== null)
    lines.push(...autoUpdateWarningLines(input.autoUpdateFailure));
  lines.push(`Upstream has published **${input.latestVersion}**. This repository's \`.github/\` came from **${input.localVersion}**.`, "", "| | |", "| --- | --- |", `| This repository's \`.github/\` is from | \`${input.localVersion}\` |`, `| The latest release is | \`${input.latestVersion}\` |`, "");
  if (input.markedReleases !== null) {
    const tags = input.markedReleases.map((tag) => `\`${tag}\``).join(", ");
    lines.push(`\`update.detection_scope\` is \`marked\`: of the releases newer than ${input.localVersion}, ` + `${tags} ${input.markedReleases.length === 1 ? "carries" : "carry"} \`${ADOPT_MARKER}\`. ` + "That mark is the reason this notice exists, and taking the latest release takes it too.", "");
  }
  lines.push("## Taking it", "");
  if (input.plan === null) {
    lines.push(`What this update would touch here: not computed \u2014 ${input.planWhy}.`, "", "Nothing has been changed for you: this is a notice, and the upgrade is a decision.", "", "Comment **`!upgrade`** on this issue, or run the **Atomaton Update** workflow from the Actions tab.", "");
  } else {
    lines.push(verdictSentence(input.localVersion, input.latestVersion, input.plan.both.length, input.plan.kept.length), "", "Nothing has been changed for you: this is a notice, and the upgrade is a decision.", "", "Comment **`!upgrade`** on this issue, or run the **Atomaton Update** workflow from the Actions tab.", "The update applies the release to `.github/` on a branch and merges it itself when nothing in this", "tree collides; a file this tree changed AND upstream changed is what stops it at a pull request,", "where the resolution is yours to make \u2014 comment on that pull request and ask an agent to help.", "");
    const plan = input.plan;
    lines.push("What it would do on this tree:", "", "| | |", "| --- | --- |", `| Files updated (this tree never changed them) | ${plan.take.length} |`, `| Files added | ${plan.add.length} |`, `| Files removed (unmodified here) | ${plan.deleted.length} |`, `| Files changed on **both** sides | ${plan.both.length} |`, `| Paths the plan will not touch without a person | ${plan.kept.length} |`, "");
    if (plan.both.length > 0) {
      lines.push("The both-sides files are where your edits and upstream's meet:", "", ...plan.both.slice(0, 10).map((path) => `- \`${path}\``), ...plan.both.length > 10 ? [`- \u2026and ${plan.both.length - 10} more`] : [], "");
    }
    if (plan.kept.length > 0) {
      lines.push("The plan leaves these alone until a person says otherwise:", "", ...plan.kept.slice(0, 10).map((entry) => `- \`${entry.path}\``), ...plan.kept.length > 10 ? [`- \u2026and ${plan.kept.length - 10} more`] : [], "");
    }
  }
  lines.push("Or by hand \u2014 the documented procedure, which is what the workflow automates:", "", "```bash", `gh release download ${input.latestVersion} -R ${UPSTREAM_REPOSITORY} -p ${ASSET_NAME}`, `unzip -o ${ASSET_NAME}   # the archive holds .github/, so run this at the repository root`, `rm ${ASSET_NAME}`, "git diff .github/                # every difference is now a decision", "```", "", `What to keep, and which files an upgrade replaces: [move to a newer release](${UPGRADE_DOC}).`, "", "## Files upstream no longer ships", "");
  if (input.removed === null) {
    lines.push(`Not computed: ${input.removedWhy}`, "", `Compare by hand \u2014 [the page](${UPGRADE_DOC}) has the command.`);
  } else if (input.removed.length === 0) {
    lines.push("None: everything the previous release shipped is still shipped by this one.");
  } else {
    lines.push("Extracting never deletes, so these stay in the tree unless somebody removes them.", "The list is the difference between your release's manifest and the one inside the", "release above, so nothing you wrote is on it \u2014 but a path here can still be one you", "edited on purpose, and that is the judgement to make before deleting.", "", ...input.removed.map((path) => `- \`${path}\``));
  }
  lines.push("", "## Release notes", "");
  if (input.window === null) {
    lines.push(`Not read: ${input.windowWhy}`, "", `What changed is on [the releases page](https://github.com/${UPSTREAM_REPOSITORY}/releases).`);
  } else if (input.window.newer.length === 0) {
    lines.push(`No notes: ${input.localVersion} is not in the list of releases the API returned, so`, `everything between there and ${input.latestVersion} is on [the releases page](https://github.com/${UPSTREAM_REPOSITORY}/releases).`);
  } else {
    for (const note of input.window.newer) {
      lines.push(`### ${note.tag}`, "", notesOf(note.body) || "_No notes on the release._", "");
    }
    if (input.window.omitted > 0) {
      lines.push(`\u2026and ${input.window.omitted} older release(s), which [the releases page](https://github.com/${UPSTREAM_REPOSITORY}/releases) lists.`, "");
    }
    if (!input.window.found) {
      lines.push(`${input.localVersion} is older than the releases above, so the notes here are the newest ones`, "rather than everything since yours.", "");
    }
  }
  return `${lines.join(`
`).trimEnd()}
`;
}
function log4(message) {
  console.error(`[new-release] ${message}`);
}
function releaseNotes() {
  const result = ghRead("api", `repos/${UPSTREAM_REPOSITORY}/releases?per_page=100`, "--jq", '[.[] | {tag: .tag_name, body: (.body // ""), url: .html_url}]');
  if (result.code !== 0) {
    log4(`WARN could not read ${UPSTREAM_REPOSITORY}'s releases: ${result.stderr.trim() || `gh exited ${result.code}`}`);
    return null;
  }
  try {
    const parsed = JSON.parse(result.stdout);
    if (!Array.isArray(parsed)) {
      log4("WARN the release list was not an array");
      return null;
    }
    return parsed.filter((entry) => {
      const note = entry;
      return typeof note.tag === "string" && note.tag !== "";
    }).map((note) => ({ tag: note.tag, body: String(note.body ?? ""), url: String(note.url ?? "") }));
  } catch {
    log4("WARN the release list was not valid JSON");
    return null;
  }
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: { repo: { type: "string" } }
  });
  const repo = (values.repo ?? process.env.GITHUB_REPOSITORY ?? "").trim();
  if (!repo) {
    console.error("usage: notify_new_release.ts --repo OWNER/REPO  (or set GITHUB_REPOSITORY)");
    process.exit(2);
  }
  const local = readLocalManifest();
  if (!("manifest" in local)) {
    console.error(`::error::${local.why}. An upgrade cannot be announced for a tree that cannot be compared.`);
    process.exit(1);
  }
  const latestTag = latestReleaseTag();
  if (latestTag === null) {
    console.error(`::error::could not read ${UPSTREAM_REPOSITORY}'s latest release, so a newer one cannot be ` + "announced. A failed read is not an answer of 'up to date'.");
    process.exit(1);
  }
  const latest = { tag: latestTag };
  if (latest.tag === local.manifest.version) {
    const existing = findOpenNotice(repo);
    if (existing === null) {
      console.error(`::error::could not search ${repo} for an existing notice, so this run wrote nothing.`);
      process.exit(1);
    }
    if (existing === 0) {
      log4(`${local.manifest.version} is the latest release; nothing to announce`);
      return;
    }
    const closed = gh("issue", "close", String(existing), "--repo", repo, "--comment", `${NOTICE_MARKER}
${LLM_CONTEXT_TAG.write("exclude")}
\`.github/\` is on ${local.manifest.version}, which is the latest release, so this notice is settled and closes itself. A newer one opens a new notice.`);
    if (closed.code !== 0) {
      console.error(`::error::this tree caught up to ${local.manifest.version}, but the open notice #${existing} could not ` + `be closed: ${closed.stderr.trim() || `gh exited ${closed.code}`}`);
      process.exit(1);
    }
    log4(`caught up to ${local.manifest.version}; closed the settled notice #${existing}`);
    return;
  }
  const scope = getNewReleaseScope();
  const scoped = scope === "marked" ? releaseNotes() : undefined;
  let marked = null;
  if (scope === "marked") {
    if (scoped === null || scoped === undefined) {
      console.error(`::error::\`update.detection_scope\` is \`marked\` and ${UPSTREAM_REPOSITORY}'s release list could not be ` + "read, so there is nothing to compare against. A failed read is not the answer 'nothing is marked'.");
      process.exit(1);
    }
    marked = markedReleases(scoped, local.manifest.version);
    if (marked.length === 0) {
      log4(`nothing to announce: \`update.detection_scope\` is \`marked\`, and no release newer than ` + `${local.manifest.version} carries ${ADOPT_MARKER}`);
      return;
    }
  }
  const mode = getNewReleaseMode();
  const updateToken = (process.env.ATOMATON_UPDATE_TOKEN ?? "").trim();
  let autoUpdateFailure = null;
  if (mode !== "issue") {
    if (updateToken === "") {
      log4(`WARN update.on_new_release is ${mode}, but ATOMATON_UPDATE_TOKEN is not set \u2014 nothing was started`);
      autoUpdateFailure = {
        mode,
        why: "no `ATOMATON_UPDATE_TOKEN` secret is set here, and the update cannot push, open " + "its pull request or merge without it"
      };
    } else {
      const open = openPrForHead(repo, updateBranchFor(latest.tag), updateToken);
      if (open > 0) {
        log4(`an update pull request for ${latest.tag} (#${open}) is already open; nothing was started`);
        return;
      }
      if (dispatchUpdate(repo, 0)) {
        log4(`started the update to ${latest.tag}; its report lands on the pull request it opens`);
        return;
      }
      log4(`WARN GitHub refused the dispatch of the update to ${latest.tag}`);
      autoUpdateFailure = {
        mode,
        why: "GitHub refused the dispatch, and this run's log is where the reason is"
      };
    }
  }
  const existing = findOpenNotice(repo);
  if (existing === null) {
    console.error(`::error::could not search ${repo} for an existing notice, so this run wrote nothing.`);
    process.exit(1);
  }
  const notes = scoped !== undefined ? scoped : releaseNotes();
  const targetRelease = releaseFiles(latest.tag);
  const removed = "why" in targetRelease ? null : noLongerShipped(targetRelease.files.manifest, local.manifest.files);
  const removedWhy = `${RELEASE_MANIFEST} could not be read out of ${latest.tag}'s release asset.`;
  const baseRelease = releaseFiles(local.manifest.version);
  let plan = null;
  let planWhy = "";
  if ("why" in baseRelease) {
    planWhy = `this tree's own release (${local.manifest.version}) could not be read: ${baseRelease.why}`;
  } else if ("why" in targetRelease) {
    planWhy = `the target release (${latest.tag}) could not be read: ${targetRelease.why}`;
  } else {
    const paths = new Set([...baseRelease.files.files.keys(), ...targetRelease.files.files.keys()]);
    const tree = readTreeFiles(paths);
    const computed = planUpdate(local.manifest.version, latest.tag, baseRelease.files.files, targetRelease.files.files, tree);
    plan = {
      take: computed.take,
      add: computed.add,
      deleted: computed.delete,
      both: computed.merge,
      kept: computed.kept
    };
  }
  const input = {
    localVersion: local.manifest.version,
    latestVersion: latest.tag,
    window: notes === null ? null : releaseWindow(notes, local.manifest.version),
    windowWhy: `the release list could not be read.`,
    markedReleases: marked,
    removed,
    removedWhy,
    plan,
    planWhy,
    autoUpdateFailure
  };
  const title = noticeTitle(input);
  const body = noticeBody(input);
  const written = existing === 0 ? gh("issue", "create", "--repo", repo, "--title", title, "--body", body) : gh("issue", "edit", String(existing), "--repo", repo, "--title", title, "--body", body);
  if (written.code !== 0) {
    console.error(`::error::could not ${existing === 0 ? "open" : `update #${existing} in`} ${repo} with the notice for ` + `${latest.tag}: ${written.stderr.trim() || `gh exited ${written.code}`}`);
    process.exit(1);
  }
  const where = existing === 0 ? written.stdout.trim() : `#${existing}`;
  log4(`announced ${latest.tag} (this tree is on ${local.manifest.version}) at ${where}`);
}
if (import.meta.main)
  main();
export {
  ADOPT_MARKER,
  ASSET_NAME,
  MAX_NOTES_CHARS,
  MAX_RELEASES,
  NOTICE_MARKER,
  UPSTREAM_REPOSITORY,
  isMarked,
  markedReleases,
  noticeBody,
  noticeTitle,
  ref,
  releaseWindow
};
