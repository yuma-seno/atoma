#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/plan_deploy.ts
import { appendFileSync as appendFileSync2 } from "fs";

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
function runsOnOutput(labels) {
  return JSON.stringify(labels);
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
function refMatches(pattern, ref) {
  return pattern.endsWith("*") ? ref.startsWith(pattern.slice(0, -1)) : ref === pattern;
}
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
function branchOf(ref) {
  return ref.startsWith("refs/heads/") ? ref.slice("refs/heads/".length) : "";
}
function tagOf(ref) {
  return ref.startsWith("refs/tags/") ? ref.slice("refs/tags/".length) : "";
}
function mergeJobsFor(jobs, branch, defaultBranch) {
  if (!branch)
    return [];
  return jobs.filter((job) => job.trigger === "merge" && coversBranch(job, branch, defaultBranch));
}
function tagJobsFor(jobs, tag) {
  if (!tag)
    return [];
  return jobs.filter((job) => job.trigger === "tag" && job.tags.some((pattern) => refMatches(pattern, tag)));
}
function mayDispatchNewTags(request) {
  return !request.ref.startsWith("refs/tags/") && request.trigger !== "tag";
}
function needsReachableTags(jobs, request) {
  if (request.target || request.event !== "push")
    return false;
  const branch = branchOf(request.ref);
  if (!branch)
    return false;
  return jobs.some((job) => job.trigger === "tag" && coversBranch(job, branch, request.defaultBranch));
}
function coversBranch(job, branch, defaultBranch) {
  return job.branches.length === 0 ? branch === defaultBranch : job.branches.some((pattern) => refMatches(pattern, branch));
}
function branchesOf(job, defaultBranch) {
  return job.branches.length === 0 ? [defaultBranch] : job.branches;
}
function planned(jobs, ref) {
  return jobs.map((job) => ({ job, ref }));
}
function candidatesFor(jobs, tags, defaultBranch) {
  return tags.flatMap((tag) => tagJobsFor(jobs, tag).map((job) => ({ job, tag, branches: branchesOf(job, defaultBranch) })));
}
function selectDeployJobs(jobs, request) {
  const nothing = { ready: [], tagCandidates: [] };
  if (request.target) {
    const named = jobs.find((job) => job.name === request.target);
    if (!named)
      return null;
    return named.trigger === "tag" && tagOf(request.ref) ? { ready: [], tagCandidates: candidatesFor([named], [tagOf(request.ref)], request.defaultBranch) } : { ready: planned([named], request.ref), tagCandidates: [] };
  }
  const pushedTag = request.event === "push" ? tagOf(request.ref) : "";
  const dispatchedTag = request.trigger === "tag" ? tagOf(request.ref) : "";
  const tag = pushedTag || dispatchedTag;
  if (tag) {
    return { ready: [], tagCandidates: candidatesFor(jobs, [tag], request.defaultBranch) };
  }
  const branch = branchOf(request.ref);
  if (request.event === "push" || request.trigger === "merge") {
    if (!branch)
      return nothing;
    return {
      ready: planned(mergeJobsFor(jobs, branch, request.defaultBranch), request.ref),
      tagCandidates: candidatesFor(jobs, request.reachableTags, request.defaultBranch)
    };
  }
  return nothing;
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
function deploymentRefusal(branch, rules) {
  if (!rules.known) {
    return `${rules.why}, so whether a pull request is required on '${branch}' could not be established. ` + "A deployment runs with credentials, so this is refused rather than assumed: fix the read, " + "or move the deployment to a branch whose rules can be seen.";
  }
  if (!rules.enforceable) {
    return `${rules.why}. A deployment runs with credentials and GitHub will not refuse a direct push ` + `to '${branch}' here, so nothing would stand between an unreviewed commit and those ` + "credentials. Deploy from a repository where a ruleset can require a pull request.";
  }
  if (!rules.pullRequestRequired) {
    return `'${branch}' is not covered by a ruleset requiring a pull request, so anyone who can push ` + "to it can run this deployment's commands with its credentials. Add a ruleset that requires " + `a pull request on '${branch}', or deploy from a branch that has one.`;
  }
  return "";
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
function getDeploySection() {
  return loadConfig().deploy;
}

// src/adapters/github/git-tags.ts
function readTags(repo) {
  const { code, stdout } = gh("api", "--paginate", `repos/${repo}/git/matching-refs/tags`, "--jq", '.[] | "\\(.ref) \\(.object.sha)"');
  if (code)
    return null;
  return stdout.split(`
`).map((line) => line.trim().split(" ")).filter(([ref, sha]) => ref?.startsWith("refs/tags/") && sha).map(([ref, sha]) => ({ name: ref.slice("refs/tags/".length), sha }));
}
function commitsAdded(repo, before, after) {
  if (!before || !after || /^0+$/.test(before))
    return [];
  const { code, stdout } = gh("api", "--paginate", `repos/${repo}/compare/${before}...${after}`, "--jq", ".commits[].sha");
  if (code)
    return null;
  return stdout.split(`
`).map((line) => line.trim()).filter(Boolean);
}
function isContained(repo, branch, commit) {
  const { code, stdout } = gh("api", `repos/${repo}/compare/${branch}...${commit}`, "--jq", ".status");
  if (code)
    return null;
  const status = stdout.trim();
  return status === "behind" || status === "identical";
}

// src/entrypoints/machinery/lib/cli.ts
function parseAcrossReleases(names, argv) {
  const known = new Set(names);
  const values = Object.fromEntries(names.map((name) => [name, ""]));
  const ignored = [];
  for (let index = 0;index < argv.length; index += 1) {
    const token = argv[index] ?? "";
    if (!token.startsWith("--"))
      continue;
    const [flag, inline] = splitFlag(token.slice(2));
    const value = inline ?? argv[index + 1] ?? "";
    if (inline === undefined)
      index += 1;
    if (known.has(flag))
      values[flag] = value;
    else
      ignored.push(flag);
  }
  if (ignored.length > 0) {
    console.error(`::warning::Ignored ${ignored.map((flag) => `\`--${flag}\``).join(", ")}: this script is from an ` + "older release than the workflow that ran it. It will understand them once the upgrade reaches " + "the default branch.");
  }
  return values;
}
function splitFlag(token) {
  const at = token.indexOf("=");
  return at === -1 ? [token, undefined] : [token.slice(0, at), token.slice(at + 1)];
}

// src/entrypoints/machinery/lib/publish-matrix.ts
import { appendFileSync } from "fs";
function partsOf(entry) {
  return "job" in entry ? entry : { job: entry, ref: "" };
}
function publishMatrix(jobs, options) {
  const include = jobs.map(partsOf).map(({ job, ref }) => ({
    name: job.name,
    runs_on: runsOnOutput(job.runsOn),
    commands: job.commands,
    secrets: job.secrets,
    ref
  }));
  const output = process.env.GITHUB_OUTPUT;
  const line = `jobs=${JSON.stringify(include)}
`;
  if (output)
    appendFileSync(output, line);
  else
    process.stdout.write(line);
  if (include.length === 0) {
    console.error(options.warnWhenEmpty ? `::warning::${options.warnWhenEmpty}` : `No ${options.what}s to run.`);
    return;
  }
  console.error(`${include.length} ${options.what}(s): ${include.map((job) => job.name).join(", ")}`);
}

// src/entrypoints/machinery/lib/script-ref.ts
import { basename } from "path";
import { fileURLToPath } from "url";
function defineScript(importMetaUrl) {
  return { runtimePath: `${SCRIPTS_DIR}/${basename(fileURLToPath(importMetaUrl))}` };
}

// src/entrypoints/machinery/plan_deploy.ts
var ref = defineScript(import.meta.url);
function branchesToVerify(selected, request) {
  if (request.target)
    return [];
  const branches = new Set;
  for (const { job } of selected) {
    if (job.trigger === "demand")
      continue;
    for (const branch of job.branches.length === 0 ? [request.defaultBranch] : job.branches) {
      if (branch)
        branches.add(branch);
    }
  }
  return [...branches];
}
function reachableTags(repo, before, ref, tags) {
  if (tags.length === 0)
    return [];
  const added = commitsAdded(repo, before.trim(), ref);
  if (added === null) {
    console.error("::error::Could not read which commits arrived with this push, so a tag it made reachable " + "would not be deployed. Refused rather than skipped.");
    process.exit(1);
  }
  const arrived = new Set(added);
  return tags.filter((tag) => arrived.has(tag.sha)).map((tag) => tag.name);
}
function branchRefusal(repo, branch) {
  if (!branch)
    return "";
  if (!repo) {
    return `no repository was given, so the rules on '${branch}' could not be read.`;
  }
  return deploymentRefusal(branch, readBranchRules(repo, branch));
}
function publishTagsBefore(jobs, selected, request, tagsNow) {
  const watching = selected.length > 0 && jobs.some((job) => job.trigger === "tag") && mayDispatchNewTags(request);
  if (!watching)
    return;
  const names = tagsNow().map((tag) => tag.name);
  const output = process.env.GITHUB_OUTPUT;
  const line = `tags_before=${JSON.stringify(names)}
`;
  if (output)
    appendFileSync2(output, line);
  else
    process.stdout.write(line);
  console.error(`Watching for tags these deployments add; ${names.length} exist now.`);
}
function shaLookup(tags) {
  const byName = new Map(tags.map((tag) => [tag.name, tag.sha]));
  return (tag) => byName.get(tag) ?? tag;
}
function keepTagsInsideTheirBranches(repo, candidates, tagSha) {
  const answers = new Map;
  const contained = (branch, tag) => {
    const key = `${branch}\x00${tag}`;
    const known = answers.get(key);
    if (known !== undefined)
      return known;
    const answer = isContained(repo, branch, tagSha(tag));
    if (answer === null) {
      console.error(`::error::Could not determine whether '${tag}' is on '${branch}', so this cannot tell ` + "whether the commit it points at was ever reviewed. Refused rather than assumed.");
      process.exit(1);
    }
    answers.set(key, answer);
    return answer;
  };
  const kept = [];
  for (const candidate of candidates) {
    const branch = candidate.branches.find((name) => contained(name, candidate.tag));
    if (!branch) {
      console.error(`'${candidate.tag}' matches \`${candidate.job.name}\`, but is not on ` + `${candidate.branches.map((name) => `'${name}'`).join(" or ")}; not deploying it.`);
      continue;
    }
    kept.push({ job: candidate.job, ref: `refs/tags/${candidate.tag}` });
  }
  return kept;
}
function main() {
  const values = parseAcrossReleases(["ref", "default-branch", "event", "trigger", "target", "repo", "before"], Bun.argv.slice(2));
  const repo = (values.repo ?? "").trim();
  const { jobs, problems } = resolveDeployJobs(getDeploySection());
  if (problems.length > 0) {
    for (const problem of problems)
      console.error(`::error::.github/atomaton/config.yaml: ${problem}`);
    console.error("::error::`deploy` could not be read, so nothing was deployed.");
    process.exit(1);
  }
  const base = {
    ref: values.ref ?? "",
    defaultBranch: (values["default-branch"] ?? "").trim(),
    event: (values.event ?? "").trim(),
    trigger: (values.trigger ?? "").trim(),
    target: (values.target ?? "").trim()
  };
  let cached;
  const tagsNow = () => {
    if (cached === undefined)
      cached = repo ? readTags(repo) : null;
    if (cached === null) {
      console.error("::error::The repository's tags could not be read, so a tag that should deploy would not. " + "A tag deployment is declared, so this is refused rather than skipped.");
      process.exit(1);
    }
    return cached;
  };
  const tags = needsReachableTags(jobs, { ...base, reachableTags: [] }) ? tagsNow() : [];
  const request = { ...base, reachableTags: reachableTags(repo, values.before ?? "", base.ref, tags) };
  const selection = selectDeployJobs(jobs, request);
  if (selection === null) {
    const known = jobs.map((job) => job.name).join(", ") || "none are configured";
    console.error(`::error::No deployment named '${request.target}'. Configured: ${known}.`);
    process.exit(1);
  }
  const selected = selection.tagCandidates.length === 0 ? selection.ready : [
    ...selection.ready,
    ...keepTagsInsideTheirBranches(repo, selection.tagCandidates, shaLookup(tagsNow()))
  ];
  if (selected.length > 0) {
    for (const branch of branchesToVerify(selected, request)) {
      const refusal = branchRefusal(repo, branch);
      if (refusal) {
        console.error(`::error::${refusal}`);
        console.error(`::error::Refused to deploy: ${selected.map((plan) => plan.job.name).join(", ")}.`);
        process.exit(1);
      }
    }
  }
  publishMatrix(selected, { what: "deployment" });
  publishTagsBefore(jobs, selected, request, tagsNow);
}
if (import.meta.main)
  main();
export {
  main,
  ref
};
