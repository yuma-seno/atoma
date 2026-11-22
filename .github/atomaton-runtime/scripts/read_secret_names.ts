#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/read_secret_names.ts
import { appendFileSync, readFileSync } from "fs";
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

// src/entrypoints/machinery/lib/script-ref.ts
import { basename } from "path";
import { fileURLToPath } from "url";
function defineScript(importMetaUrl) {
  return { runtimePath: `${SCRIPTS_DIR}/${basename(fileURLToPath(importMetaUrl))}` };
}

// src/entrypoints/machinery/read_secret_names.ts
var ref = defineScript(import.meta.url);
function declarationIn(configText) {
  const config = Bun.YAML.parse(configText);
  return config.tools?.secrets;
}
function main() {
  const { values } = parseArgs({ args: Bun.argv.slice(2), options: { config: { type: "string" } } });
  let declared;
  if (!values.config) {
    console.error("::warning::read_secret_names: no --config given, so no credentials are declared for this run. The workflow should pass the default branch's config.yaml.");
  } else {
    try {
      declared = declarationIn(readFileSync(values.config, "utf8"));
    } catch (error) {
      console.error(`No credential declaration could be read (${error.message}); declaring none.`);
    }
  }
  const { names, problems } = resolveDeclaredSecrets(declared, TOOL_SECRETS);
  if (problems.length > 0) {
    for (const problem of problems) {
      console.error(`::error::.github/atomaton/config.yaml: ${problem}`);
    }
    process.exit(1);
  }
  const githubOutput = process.env.GITHUB_OUTPUT;
  if (githubOutput) {
    appendFileSync(githubOutput, `names=${JSON.stringify(names)}
`);
  }
  console.error(names.length > 0 ? `Secrets declared in \`tools.secrets\`: ${names.join(", ")}` : "No secrets are declared in `tools.secrets`.");
}
if (import.meta.main)
  main();
export {
  declarationIn,
  ref
};
