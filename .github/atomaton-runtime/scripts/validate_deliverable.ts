#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/validate_deliverable.ts
import { existsSync, mkdtempSync, readdirSync, readFileSync as readFileSync2, writeFileSync } from "fs";
import { tmpdir } from "os";
import { join as join4 } from "path";
import { parseArgs } from "util";

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
var BUILT_FROM = [
  [USER_ROOT, "content"],
  [SCRIPTS_DIR, "entrypoints/machinery"],
  [TOOLS_DIR, "entrypoints/tools"]
];
function sourceOf(deployed) {
  const found = BUILT_FROM.find(([target]) => target === deployed);
  if (!found)
    throw new Error(`machinery-layout: nothing builds ${deployed}`);
  return found[1];
}

// src/domain/delivery/declared-secrets.ts
var SECRET_SLOTS = 10;
var SECRET_SLOT_PREFIX = "ATOMATON_SECRET_";
var NAME_PATTERN = /^[A-Z][A-Z0-9_]*$/;
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
function resolveDeclaredSecrets(raw, destination) {
  const { field, reserved } = destination;
  if (raw === undefined || raw === null)
    return { names: [], problems: [] };
  if (!Array.isArray(raw)) {
    return { names: [], problems: [`\`${field}\` must be an array of secret names.`] };
  }
  const problems = [];
  const names = [];
  const seen = new Set;
  for (const entry of raw) {
    if (typeof entry !== "string") {
      problems.push(`\`${field}\` entries must be strings; found ${JSON.stringify(entry)}.`);
      continue;
    }
    const name = entry.trim();
    if (!NAME_PATTERN.test(name)) {
      problems.push(`\`${field}\`: '${name}' is not a usable secret name. Expected uppercase letters, digits and underscores, starting with a letter \u2014 e.g. 'SLACK_TOKEN'.`);
      continue;
    }
    if (reserved.has(name)) {
      problems.push(`\`${field}\`: '${name}' is already part of the environment this workflow provides, so declaring it would replace that value rather than add a credential. Give the secret another name.`);
      continue;
    }
    if (name.startsWith(SECRET_SLOT_PREFIX)) {
      problems.push(`\`${field}\`: '${name}' collides with the slots this mechanism uses internally. Give the secret another name.`);
      continue;
    }
    if (seen.has(name)) {
      problems.push(`\`${field}\`: '${name}' is declared more than once.`);
      continue;
    }
    seen.add(name);
    names.push(name);
  }
  if (names.length > SECRET_SLOTS) {
    problems.push(`\`${field}\` declares ${names.length} secrets but a run carries at most ${SECRET_SLOTS}. Raising the cap needs a new release, since each slot is a line of generated workflow YAML.`);
  }
  return problems.length > 0 ? { names: [], problems } : { names, problems };
}

// src/domain/delivery/check-jobs.ts
var CHECKS_FROM_PULL_REQUEST = {
  where: "checks.from_pull_request",
  secrets: {
    refused: "These commands come from the pull request, which may rewrite them, so a credential " + "named beside them is one the change being judged can read. Move the check to " + "`checks.from_default_branch`, where the commands come from a branch a person approved."
  }
};
var CHECKS_FROM_DEFAULT_BRANCH = {
  where: "checks.from_default_branch",
  secrets: { reserved: CHECK_JOB_RESERVED }
};
var NO_PULL_REQUEST_CHECKS = "This check verified nothing: `checks.from_pull_request` in .github/atomaton/config.yaml is empty, " + "so a pull request satisfying it has not been tested. Add the commands that check this project, " + "or point `checks.your_workflow` at a workflow of your own.";

// src/domain/work/control-commands.ts
var CONTROL_COMMAND_NAMES = ["stop", "resume", "upgrade"];
function isControlCommand(name) {
  return CONTROL_COMMAND_NAMES.includes(name);
}

// src/domain/delivery/runner-label.ts
var DEFAULT_RUNNER = "ubuntu-latest";
function resolveRunsOn(configured) {
  if (configured === undefined || configured === null)
    return { labels: [DEFAULT_RUNNER], problems: [] };
  if (typeof configured === "string") {
    const label = configured.trim();
    if (!label)
      return { labels: [DEFAULT_RUNNER], problems: ["runs_on is empty; using " + DEFAULT_RUNNER] };
    return { labels: [label], problems: [] };
  }
  if (Array.isArray(configured)) {
    const labels = configured.filter((entry) => typeof entry === "string").map((entry) => entry.trim()).filter(Boolean);
    const problems = [];
    if (labels.length !== configured.length) {
      problems.push("runs_on has entries that are not non-empty strings; those are ignored");
    }
    if (labels.length === 0) {
      return { labels: [DEFAULT_RUNNER], problems: [...problems, `runs_on names no usable label; using ${DEFAULT_RUNNER}`] };
    }
    return { labels, problems };
  }
  return { labels: [DEFAULT_RUNNER], problems: [`runs_on must be a string or a list of strings; using ${DEFAULT_RUNNER}`] };
}

// src/domain/delivery/declared-jobs.ts
var NAME_PATTERN2 = /^[a-z0-9]+(-[a-z0-9]+)*$/;
var SHARED_KEYS = ["name", "runs_on", "commands", "secrets"];
function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function resolveDeclaredJobs(raw, rules) {
  if (raw === undefined || raw === null)
    return { jobs: [], problems: [] };
  if (!Array.isArray(raw))
    return { jobs: [], problems: [`\`${rules.where}\` must be an array.`] };
  const problems = [];
  const jobs = [];
  const seen = new Set;
  const allowed = new Set([...SHARED_KEYS, ...rules.extra?.keys ?? []]);
  raw.forEach((entry, index) => {
    const where = `\`${rules.where}[${index}]\``;
    if (!isRecord(entry)) {
      problems.push(`${where} must be an object.`);
      return;
    }
    const unknown = Object.keys(entry).filter((key) => !allowed.has(key));
    if (unknown.length > 0) {
      problems.push(`${where}: unknown key(s) ${unknown.map((k) => `\`${k}\``).join(", ")}.`);
      return;
    }
    const name = typeof entry.name === "string" ? entry.name.trim() : "";
    if (!NAME_PATTERN2.test(name)) {
      problems.push(`${where}: \`name\` must be lowercase letters, digits and hyphens \u2014 e.g. 'cloud-names'.`);
      return;
    }
    if (seen.has(name)) {
      problems.push(`${where}: '${name}' is declared more than once.`);
      return;
    }
    const commandsRaw = entry.commands ?? [];
    if (!Array.isArray(commandsRaw) || commandsRaw.some((c) => typeof c !== "string" || c.trim() === "")) {
      problems.push(`${where}: \`commands\` must be an array of non-empty shell commands.`);
      return;
    }
    const commands = commandsRaw.map((c) => c.trim());
    if (commands.length === 0) {
      problems.push(`${where}: \`commands\` is empty, so this job would do nothing and report success.`);
      return;
    }
    const secrets = readSecrets(entry.secrets, rules.secrets, `${rules.where}[${index}]`, where, problems);
    if (secrets === null)
      return;
    const extra = rules.extra ? rules.extra.read(entry, where, problems) : {};
    if (extra === null)
      return;
    const runner = resolveRunsOn(entry.runs_on);
    for (const problem of runner.problems)
      problems.push(`${where}: ${problem}`);
    seen.add(name);
    jobs.push({ name, runsOn: runner.labels, commands, secrets, ...extra });
  });
  return { jobs, problems };
}
function readSecrets(raw, rule, path, where, problems) {
  if (raw === undefined || raw === null)
    return [];
  if ("refused" in rule) {
    if (Array.isArray(raw) && raw.length === 0)
      return [];
    problems.push(`${where}: \`secrets\` cannot be named here. ${rule.refused}`);
    return null;
  }
  const { names, problems: found } = resolveDeclaredSecrets(raw, {
    field: `${path}.secrets`,
    reserved: rule.reserved
  });
  problems.push(...found);
  return found.length > 0 ? null : names;
}

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
function isRecord2(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function resolveDeployJobs(deploy) {
  const section = isRecord2(deploy) ? deploy : {};
  const problems = [];
  const jobs = [];
  const seen = new Set;
  for (const trigger of TRIGGERS) {
    const arm = DEPLOY_ARMS[trigger];
    const resolved = resolveDeclaredJobs(section[arm.key], arm.rules);
    problems.push(...resolved.problems);
    for (const job of resolved.jobs) {
      if (seen.has(job.name)) {
        problems.push(`\`${arm.key}\`: '${job.name}' is already declared in another \`deploy\` list.`);
        continue;
      }
      seen.add(job.name);
      jobs.push({ ...job, trigger });
    }
  }
  return problems.length > 0 ? { jobs: [], problems } : { jobs, problems };
}

// src/domain/delivery/path-patterns.ts
var NEGATION = "!";
function pathPatternNegates(pattern) {
  return pattern.startsWith(NEGATION);
}
function pathPatternTarget(pattern) {
  return pathPatternNegates(pattern) ? pattern.slice(NEGATION.length) : pattern;
}
function pathPatternListProblem(patterns) {
  if (patterns.length === 0 || !patterns.every(pathPatternNegates))
    return "";
  return "every pattern is an exclusion, so the list matches nothing. An exclusion takes " + "a file back out of a pattern before it in the same list, so name what it covers first.";
}
var GLOB_CHARACTERS = /[*?[\]{}]/;
function pathPatternProblem(pattern) {
  if (typeof pattern !== "string" || pattern.trim() === "") {
    return "a path pattern must be a non-empty string";
  }
  if (pattern !== pattern.trim()) {
    return `"${pattern}" has surrounding whitespace`;
  }
  if (pattern === NEGATION) {
    return `"${pattern}" is a bare "!", which negates no pattern. Write "!<pattern>" naming what to exclude.`;
  }
  if (pattern.startsWith(NEGATION + NEGATION)) {
    return `"${pattern}" starts with "!!". One "!" negates the pattern after it; ` + "a second one is not a form this matcher has, so this would match nothing.";
  }
  const target = pathPatternTarget(pattern);
  if (target !== target.trim()) {
    return `"${pattern}" has whitespace between the "!" and the pattern it negates`;
  }
  if (target.endsWith("/")) {
    return `"${pattern}" ends in a slash, so it would match nothing. ` + `Write "${pattern}**" for everything under it, or drop the slash to match that one path.`;
  }
  const body = target.endsWith("/**") ? target.slice(0, -3) : target;
  if (body === "") {
    return `"${pattern}" names no directory. Write the directory before the "/**".`;
  }
  const glob = GLOB_CHARACTERS.exec(body);
  if (glob) {
    return `"${pattern}" uses the glob character '${glob[0]}', which this matcher cannot honour, ` + 'so it would match nothing. Write a literal path, or a directory followed by "/**".';
  }
  return "";
}

// src/domain/delivery/merge-gates.ts
var CONDITION_KEYS = [
  "files_added",
  "files_removed",
  "files_modified",
  "files_changed",
  "labels",
  "title_matches"
];
var GATE_KEYS = ["reason", "when"];
function isRecord3(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function readPatterns2(raw, where, problems) {
  if (raw === undefined)
    return [];
  if (!Array.isArray(raw)) {
    problems.push(`${where} must be an array of path patterns.`);
    return [];
  }
  if (raw.length === 0) {
    problems.push(`${where} is empty, so it constrains nothing; remove the key instead.`);
    return [];
  }
  const patterns = [];
  for (const entry of raw) {
    if (typeof entry !== "string") {
      problems.push(`${where}: every pattern must be a string; found ${JSON.stringify(entry)}.`);
      continue;
    }
    const problem = pathPatternProblem(entry);
    if (problem) {
      problems.push(`${where}: ${problem}`);
      continue;
    }
    patterns.push(entry.trim());
  }
  const listProblem = pathPatternListProblem(patterns);
  if (listProblem) {
    problems.push(`${where}: ${listProblem}`);
    return [];
  }
  return patterns;
}
function readLabels(raw, where, problems) {
  if (raw === undefined)
    return [];
  if (!Array.isArray(raw) || raw.some((label) => typeof label !== "string" || label.trim() === "")) {
    problems.push(`${where} must be an array of non-empty label names.`);
    return [];
  }
  if (raw.length === 0) {
    problems.push(`${where} is empty, so it constrains nothing; remove the key instead.`);
    return [];
  }
  return raw.map((label) => label.trim());
}
function readTitleMatches(raw, where, problems) {
  if (raw === undefined)
    return "";
  if (typeof raw !== "string" || raw.trim() === "") {
    problems.push(`${where} must be a non-empty regular expression.`);
    return "";
  }
  try {
    new RegExp(raw, "i");
  } catch (error) {
    problems.push(`${where} is not a valid regular expression: ${error.message}`);
    return "";
  }
  return raw;
}
function constrainsAnything(when) {
  return when.filesAdded.length > 0 || when.filesRemoved.length > 0 || when.filesModified.length > 0 || when.filesChanged.length > 0 || when.labels.length > 0 || when.titleMatches !== "";
}
function resolveMergeGates(raw) {
  if (raw === undefined || raw === null)
    return { gates: [], problems: [] };
  if (!Array.isArray(raw)) {
    return { gates: [], problems: ["`merge.gates` must be an array of gate objects."] };
  }
  const problems = [];
  const gates = [];
  raw.forEach((entry, index) => {
    const where = `\`merge.gates[${index}]\``;
    if (!isRecord3(entry)) {
      problems.push(`${where} must be an object with \`reason\` and \`when\`.`);
      return;
    }
    for (const key of Object.keys(entry)) {
      if (!GATE_KEYS.includes(key)) {
        problems.push(`${where}: unknown key \`${key}\`; a gate has \`reason\` and \`when\`.`);
      }
    }
    const reason = typeof entry.reason === "string" ? entry.reason.trim() : "";
    if (reason === "") {
      problems.push(`${where}: \`reason\` must say why a person should merge this, in their words.`);
    }
    if (!isRecord3(entry.when)) {
      problems.push(`${where}: \`when\` must be an object naming at least one condition ` + `(${CONDITION_KEYS.join(", ")}).`);
      return;
    }
    const declared = entry.when;
    for (const key of Object.keys(declared)) {
      if (!CONDITION_KEYS.includes(key)) {
        problems.push(`${where}: unknown condition \`${key}\`. A misspelled condition matches nothing, which ` + `looks exactly like a gate nobody needed -- so it is an error rather than a no-op. ` + `Known conditions: ${CONDITION_KEYS.join(", ")}.`);
      }
    }
    const when = {
      filesAdded: readPatterns2(declared.files_added, `${where}.when.files_added`, problems),
      filesRemoved: readPatterns2(declared.files_removed, `${where}.when.files_removed`, problems),
      filesModified: readPatterns2(declared.files_modified, `${where}.when.files_modified`, problems),
      filesChanged: readPatterns2(declared.files_changed, `${where}.when.files_changed`, problems),
      labels: readLabels(declared.labels, `${where}.when.labels`, problems),
      titleMatches: readTitleMatches(declared.title_matches, `${where}.when.title_matches`, problems)
    };
    if (!constrainsAnything(when)) {
      problems.push(`${where}: \`when\` names no usable condition, so this gate would stop every merge. ` + `Set \`merge.policy\` to "manual" if that is the intent.`);
      return;
    }
    gates.push({ reason, when });
  });
  return problems.length > 0 ? { gates: [], problems } : { gates, problems };
}

// src/domain/delivery/shipped-workflows.ts
var DEFAULT_CI_WORKFLOW = "atomaton-check.yml";
var DEFAULT_CD_WORKFLOW = "atomaton-deploy.yml";

// src/domain/delivery/deliverable-integrity.ts
var REQUIRED_AGENT_KEYS = ["on_config_finding"];
var CONFIG_SCHEMA = {
  children: {
    base_branch: null,
    environment: { children: { setup_commands: null, max_reloads: null } },
    checks: {
      children: {
        from_pull_request: null,
        from_default_branch: null,
        your_workflow: null
      }
    },
    deploy: {
      children: {
        on_merge: null,
        on_tag: null,
        on_demand: null,
        your_workflow: null
      }
    },
    merge: { children: { policy: null, governed_paths: null, gates: null } },
    chain: {
      children: {
        after_handoffs: null,
        after_runs_without_change: null,
        labels: { children: { in_progress: null, sub_issue: null, launched: null }, anyName: null }
      }
    },
    agents: { children: { on_config_finding: null } },
    tools: {
      children: {
        secrets: null,
        watch: { anyName: null },
        servers: { anyName: { anyName: null } },
        packages: { children: { npm: null, bun: null, pip: null } }
      }
    }
  }
};
function leafOf(path) {
  return path.slice(path.lastIndexOf(".") + 1);
}
function isRecord4(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function unknownKeys(value, section, prefix) {
  if (!isRecord4(value))
    return [];
  const unknown = [];
  for (const [name, child] of Object.entries(value)) {
    const path = `${prefix}${name}`;
    const declared = section.children?.[name];
    if (declared !== undefined) {
      if (declared)
        unknown.push(...unknownKeys(child, declared, `${path}.`));
      continue;
    }
    if (section.anyName !== undefined) {
      if (section.anyName)
        unknown.push(...unknownKeys(child, section.anyName, `${path}.`));
      continue;
    }
    unknown.push(path);
  }
  return unknown;
}
function configProblems(facts) {
  const problems = [];
  const { config, agentNames, workflowFiles } = facts;
  if (!isRecord4(config)) {
    return ["`config.yaml` must be a YAML mapping."];
  }
  for (const key of unknownKeys(config, CONFIG_SCHEMA, "").sort()) {
    problems.push(`\`${key}\` in config.yaml is not a setting Atomaton reads. Check the spelling.`);
  }
  for (const [section, atomatonLists] of [
    ["checks", [CHECKS_FROM_PULL_REQUEST, CHECKS_FROM_DEFAULT_BRANCH].map((rules) => leafOf(rules.where))],
    ["deploy", Object.values(DEPLOY_ARMS).map((arm) => arm.key)]
  ]) {
    const value = config[section];
    if (!isRecord4(value) || value.your_workflow === undefined)
      continue;
    const declared = atomatonLists.filter((list) => value[list] !== undefined);
    if (declared.length === 0)
      continue;
    const named = declared.map((list) => `\`${list}\``).join(" and ");
    problems.push(`\`${section}\` sets ${named} and \`your_workflow\`. They are alternatives: \`your_workflow\` ` + `dispatches a workflow of your own and nothing reads ${named}. Remove whichever you did not mean.`);
  }
  const merge = isRecord4(config.merge) ? config.merge : {};
  problems.push(...resolveMergeGates(merge.gates).problems);
  problems.push(...resolveDeployJobs(config.deploy).problems);
  const checks = isRecord4(config.checks) ? config.checks : {};
  problems.push(...resolveDeclaredJobs(checks.from_pull_request, CHECKS_FROM_PULL_REQUEST).problems);
  problems.push(...resolveDeclaredJobs(checks.from_default_branch, CHECKS_FROM_DEFAULT_BRANCH).problems);
  const tools = isRecord4(config.tools) ? config.tools : {};
  problems.push(...resolveDeclaredSecrets(tools.secrets, TOOL_SECRETS).problems);
  for (const name of agentNames.filter(isControlCommand).sort()) {
    problems.push(`agent-definitions/${name}.md is named after the '/${name}' control command, ` + `so '/${name}' will never dispatch it. Rename the agent.`);
  }
  if (agentNames.length === 0) {
    problems.push("No agent definitions were found. `.github/atomaton/agent-definitions/*.md` is empty or missing.");
  }
  const agents = isRecord4(config.agents) ? config.agents : {};
  for (const key of REQUIRED_AGENT_KEYS) {
    const value = agents[key];
    if (typeof value !== "string" || value.trim() === "") {
      problems.push(`\`agents.${key}\` is required in config.yaml. It names the agent a workflow starts when no ` + `issue or pull request can name one, so there is nothing to fall back to.`);
      continue;
    }
    if (agentNames.length > 0 && !agentNames.includes(value.trim())) {
      problems.push(`\`agents.${key}\` names '${value.trim()}', which has no agent-definitions/${value.trim()}.md.`);
    }
  }
  if (workflowFiles.length > 0) {
    const present = new Set(workflowFiles);
    for (const [section, fallback] of [
      ["checks", DEFAULT_CI_WORKFLOW],
      ["deploy", DEFAULT_CD_WORKFLOW]
    ]) {
      const named = isRecord4(config[section]) ? config[section].your_workflow : undefined;
      const configured = typeof named === "string" ? named.trim() : "";
      const effective = configured || fallback;
      if (!present.has(effective)) {
        problems.push(`\`${section}.your_workflow\` resolves to '${effective}', which is not a file in .github/workflows/. ` + (configured ? "Check the name." : "The shipped default is missing from this repository."));
      }
    }
  }
  const chain = isRecord4(config.chain) ? config.chain : {};
  if (isRecord4(chain.labels)) {
    for (const [key, value] of Object.entries(chain.labels)) {
      if (typeof value !== "string" || value.trim() === "") {
        problems.push(`\`chain.labels.${key}\` must be a non-empty label name.`);
      }
    }
  }
  return problems;
}

// src/domain/machinery/generated-file-hint.ts
var EDITABLE_SOURCE = "`tools.servers` in .github/atomaton/config.yaml";
function withEditableSource(problem) {
  if (problem.includes("tools.yaml")) {
    return `${problem} \u2014 that file is generated from ${EDITABLE_SOURCE} and is rewritten on ` + "every build, so an edit to it is lost. Change the config.";
  }
  if (problem.startsWith("Hook script not found")) {
    return `${problem} \u2014 hook paths are resolved against .github/atomaton/tools/, and are ` + `declared in ${EDITABLE_SOURCE} (per server) or \`tools.watch\` (file-wide).`;
  }
  return problem;
}

// src/domain/machinery/tools-file.ts
import { isAbsolute as isAbsolute2, join as join3, resolve as resolve2 } from "path";

// src/shared/tool-output.ts
var TOOL_OUTPUT_BUDGET = 8000;
var TOOL_OUTPUT_BACKSTOP = TOOL_OUTPUT_BUDGET * 2;

// src/domain/machinery/shipped-servers.ts
import { readFileSync } from "fs";
import { basename, dirname, join } from "path";
import { fileURLToPath } from "url";
function defaultPath() {
  const belowSrc = sourceOf(TOOLS_DIR).split("/");
  return join(dirname(fileURLToPath(import.meta.url)), "..", "..", ...belowSrc, basename(TOOL_DEFAULTS_FILE));
}
var cached;
function toolDefaults(path = defaultPath()) {
  if (cached)
    return cached;
  const parsed = Bun.YAML.parse(readFileSync(path, "utf8"));
  cached = { watch: parsed.watch ?? {}, servers: parsed.servers ?? {} };
  return cached;
}

// src/domain/machinery/untrusted-servers.ts
import { isAbsolute, join as join2, resolve } from "path";
var RUN_AS_UNTRUSTED = "run_as_untrusted.sh";
var SHIPPED_UNTRUSTED = new Set(["shell"]);
function isUntrustedServer(name, shippedNames) {
  return SHIPPED_UNTRUSTED.has(name) || !shippedNames.has(name);
}
function asUntrustedServer(server, toolsDir) {
  if (typeof server.command !== "string")
    return server;
  const args = Array.isArray(server.args) ? server.args : [];
  return {
    ...server,
    command: "bash",
    args: [wrapperPath(toolsDir), server.command, ...args]
  };
}
function wrapperPath(toolsDir) {
  const from = isAbsolute(toolsDir) ? toolsDir : resolve(toolsDir);
  return join2(from, RUN_AS_UNTRUSTED).split("\\").join("/");
}

// src/domain/machinery/tools-file.ts
function toolsFileFrom(tools, hookBase, defaultsPath) {
  const out = {};
  const defaults = toolDefaults(defaultsPath);
  const watch = mergedWatch(tools?.watch, defaults);
  if (Object.keys(watch).length > 0)
    out.hooks = absoluteHooks(watch, hookBase);
  const shippedNames = new Set(Object.keys(defaults.servers));
  for (const [name, server] of Object.entries(mergedServers(tools?.servers, defaults))) {
    const { settings: _delivery, ...forTheCore } = server;
    if (isRecord5(forTheCore.hooks))
      forTheCore.hooks = absoluteHooks(forTheCore.hooks, hookBase);
    if (Object.hasOwn(defaults.servers, name) && forTheCore.max_output_chars === undefined) {
      forTheCore.max_output_chars = TOOL_OUTPUT_BACKSTOP;
    }
    out[name] = isUntrustedServer(name, shippedNames) ? asUntrustedServer(forTheCore, hookBase) : forTheCore;
  }
  return out;
}
function mergedServers(configured, defaults) {
  const out = {};
  for (const [name, server] of Object.entries(defaults.servers)) {
    const { description: _ours, ...rest } = server;
    out[name] = { ...rest };
  }
  for (const [name, server] of Object.entries(configured ?? {})) {
    out[name] = { ...out[name] ?? {}, ...server };
  }
  return out;
}
function mergedWatch(configured, defaults) {
  const out = {};
  for (const [key, scripts] of Object.entries(defaults.watch))
    out[key] = [...scripts];
  for (const [key, added] of Object.entries(configured ?? {})) {
    const theirs = Array.isArray(added) ? added : [added];
    out[key] = [...out[key] ?? [], ...theirs];
  }
  return out;
}
var HOOK_SCRIPT_KEYS = ["before_tool", "after_tool", "before_compaction"];
function isRecord5(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function absoluteHooks(hooks, base) {
  const out = { ...hooks };
  for (const key of HOOK_SCRIPT_KEYS) {
    const declared = out[key];
    if (Array.isArray(declared)) {
      out[key] = declared.map((script) => absolutePath(script, base));
    } else if (typeof declared === "string") {
      out[key] = absolutePath(declared, base);
    }
  }
  return out;
}
function absolutePath(script, base) {
  if (typeof script !== "string" || script.length === 0)
    return script;
  if (isAbsolute2(script))
    return script;
  const from = isAbsolute2(base) ? base : resolve2(base);
  return join3(from, script).split("\\").join("/");
}
function reservedServerNames(tools) {
  return Object.keys(tools?.servers ?? {}).filter((name) => name === "hooks");
}

// src/entrypoints/machinery/lib/script-ref.ts
import { basename as basename2 } from "path";
import { fileURLToPath as fileURLToPath2 } from "url";
function defineScript(importMetaUrl) {
  return { runtimePath: `${SCRIPTS_DIR}/${basename2(fileURLToPath2(importMetaUrl))}` };
}

// src/entrypoints/machinery/validate_deliverable.ts
var ref = defineScript(import.meta.url);

class CannotCheck extends Error {
}
function validatorProblems(output) {
  return output.split(/\r?\n/).map((line) => line.trim()).filter((line) => line.startsWith("\u2717")).map((line) => line.replace(/^\u2717\s*/, ""));
}
function validateAgentDefinition(atoma, agentDef, toolsFile, label) {
  const proc = Bun.spawnSync({
    cmd: [atoma, "validate", "--agent-def", agentDef, "--tools-file", toolsFile],
    stdout: "pipe",
    stderr: "pipe"
  });
  const stdout = proc.stdout ? proc.stdout.toString("utf8") : "";
  const stderr = proc.stderr ? proc.stderr.toString("utf8") : "";
  if (proc.exitCode === null) {
    throw new CannotCheck(`could not run \`${atoma} validate\`: ${stderr.trim() || "no output"}`);
  }
  if (proc.exitCode === 0)
    return [];
  const found = validatorProblems(`${stdout}
${stderr}`);
  if (found.length > 0)
    return found.map((problem) => `${label}: ${withEditableSource(problem)}`);
  return [`${label}: \`atoma validate\` failed without saying why: ${stderr.trim() || stdout.trim() || "no output"}`];
}
function writeToolsFileFor(root) {
  const atomaDir = join4(root, ".github", "atomaton");
  const runtimeTools = join4(root, ".github", "atomaton-runtime", "tools");
  const config = Bun.YAML.parse(readFileSync2(join4(atomaDir, "config.yaml"), "utf8"));
  const collisions = reservedServerNames(config.tools);
  if (collisions.length > 0) {
    throw new Error(`\`tools.servers\` may not be named ${collisions.join(", ")} \u2014 reserved by the core`);
  }
  const out = join4(mkdtempSync(join4(tmpdir(), "atomaton-validate-")), "tools.yaml");
  writeFileSync(out, Bun.YAML.stringify(toolsFileFrom(config.tools, runtimeTools, join4(runtimeTools, "defaults.yaml")), null, 2));
  return out;
}
function agentNames(agentDir) {
  if (!existsSync(agentDir))
    return [];
  return readdirSync(agentDir).filter((file) => file.endsWith(".md")).map((file) => file.slice(0, -".md".length)).sort();
}
function workflowFiles(workflowDir) {
  if (!existsSync(workflowDir))
    return [];
  return readdirSync(workflowDir).filter((file) => file.endsWith(".yml") || file.endsWith(".yaml"));
}
function collect(root, atoma) {
  if (!existsSync(root))
    throw new CannotCheck(`--root ${root} does not exist`);
  const atomaDir = join4(root, ".github", "atomaton");
  const agentDir = join4(atomaDir, "agent-definitions");
  const configFile = join4(atomaDir, "config.yaml");
  const names = agentNames(agentDir);
  const problems = [];
  if (!existsSync(configFile)) {
    problems.push(`${configFile} is missing. Every workflow reads it.`);
  } else {
    let config;
    try {
      config = Bun.YAML.parse(readFileSync2(configFile, "utf8"));
    } catch (error) {
      problems.push(`${configFile} is not valid YAML: ${error.message}`);
    }
    if (config !== undefined) {
      problems.push(...configProblems({
        config,
        agentNames: names,
        workflowFiles: workflowFiles(join4(root, ".github", "workflows"))
      }));
    }
  }
  if (names.length > 0) {
    let toolsFile;
    try {
      toolsFile = writeToolsFileFor(root);
    } catch (error) {
      problems.push(`could not write the tools file from config.yaml: ${error.message}`);
      return problems;
    }
    for (const name of names) {
      problems.push(...validateAgentDefinition(atoma, join4(agentDir, `${name}.md`), toolsFile, `${name}.md`));
    }
  }
  return problems;
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      root: { type: "string" },
      atoma: { type: "string" },
      report: { type: "string" }
    }
  });
  const root = values.root?.trim() || ".";
  const atoma = values.atoma?.trim() || "atoma";
  let problems;
  try {
    problems = collect(root, atoma);
  } catch (error) {
    if (!(error instanceof CannotCheck))
      throw error;
    console.error(`[atomaton-validate-deliverable] cannot check: ${error.message}`);
    process.exit(2);
  }
  if (values.report)
    writeFileSync(values.report, problems.map((problem) => `${problem}
`).join(""));
  if (problems.length === 0) {
    console.log(`The deliverable in ${root} is internally consistent.`);
    return;
  }
  console.error(`${problems.length} problem(s) in ${root}:`);
  for (const problem of problems)
    console.error(`  - ${problem}`);
  process.exit(1);
}
if (import.meta.main)
  main();
export {
  ref,
  validatorProblems
};
