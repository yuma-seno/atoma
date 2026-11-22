#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/merge_tool_packages.ts
import { existsSync, readFileSync, writeFileSync } from "fs";
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

// src/entrypoints/machinery/lib/script-ref.ts
function defineScript(importMetaUrl) {
  return { runtimePath: `${SCRIPTS_DIR}/${basename(fileURLToPath(importMetaUrl))}` };
}

// src/entrypoints/machinery/merge_tool_packages.ts
var ref = defineScript(import.meta.url);
var ECOSYSTEMS = ["npm", "bun", "pip"];
function mergePackages(shipped, project) {
  const out = {};
  for (const ecosystem of ECOSYSTEMS) {
    const names = [...shipped[ecosystem] ?? [], ...project[ecosystem] ?? []].filter((name) => typeof name === "string" && name.trim().length > 0).map((name) => name.trim());
    out[ecosystem] = [...new Set(names)];
  }
  return out;
}
function projectPackages(configPath) {
  if (!existsSync(configPath))
    return {};
  try {
    const config = Bun.YAML.parse(readFileSync(configPath, "utf8"));
    return config.tools?.packages ?? {};
  } catch (error) {
    console.error(`::warning::${configPath} could not be read for \`tools.packages\`: ${error.message}`);
    return {};
  }
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: { shipped: { type: "string" }, config: { type: "string" }, out: { type: "string" } }
  });
  if (!values.shipped || !values.config || !values.out) {
    console.error("usage: merge_tool_packages.ts --shipped packages.json --config config.yaml --out merged.json");
    process.exit(2);
  }
  let shipped;
  try {
    shipped = JSON.parse(readFileSync(values.shipped, "utf8"));
  } catch (error) {
    console.error(`::error::${values.shipped}: ${error.message}`);
    process.exit(1);
  }
  const merged = mergePackages(shipped, projectPackages(values.config));
  writeFileSync(values.out, JSON.stringify(merged, null, 2) + `
`);
  const total = Object.values(merged).reduce((n, list) => n + list.length, 0);
  console.error(`merge_tool_packages: ${total} package(s) -> ${values.out}`);
}
if (import.meta.main)
  main();
export {
  mergePackages,
  ref
};
