#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/decide_turn_ending.ts
import { appendFileSync } from "fs";
import { parseArgs } from "util";

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
function shouldReleaseGuard(ending) {
  if (ending.continues === true)
    return false;
  return ending.ended !== "handed-off" && ending.ended !== "parked";
}
function nextToDispatch(ending) {
  return ending.ended === "handed-off" ? ending.next : undefined;
}
function refusedByChainLimit(ending) {
  return ending.ended === "chain-over" ? ending.next : undefined;
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

// src/entrypoints/machinery/decide_turn_ending.ts
var ref = defineScript(import.meta.url);
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      outcome: { type: "string" },
      "ended-because": { type: "string" },
      "loop-limit-reached": { type: "string" },
      "chain-continues": { type: "string" },
      parked: { type: "string" },
      directive: { type: "string" },
      reported: { type: "string" }
    }
  });
  if (!values.outcome) {
    console.error("decide_turn_ending: --outcome missing/empty -- treating as a failed turn (releases the guard)");
  }
  const ending = endingOf({
    succeeded: values.outcome === "success",
    endedBecause: values["ended-because"] ?? "",
    loopLimitReached: isTrue(values["loop-limit-reached"]),
    chainContinues: isTrue(values["chain-continues"]),
    parked: isTrue(values.parked),
    directive: values.directive ?? "",
    reported: isTrue(values.reported)
  });
  const published = {
    ended: ending.ended,
    should_release: String(shouldReleaseGuard(ending)),
    dispatch_to: nextToDispatch(ending)?.agent ?? "",
    chain_over_to: refusedByChainLimit(ending)?.agent ?? ""
  };
  const githubOutput = process.env.GITHUB_OUTPUT;
  if (githubOutput) {
    for (const [key, value] of Object.entries(published))
      appendFileSync(githubOutput, `${key}=${value}
`);
  }
  console.error(`decide_turn_ending: outcome=${values.outcome ?? "(missing)"} -> ` + Object.entries(published).map(([key, value]) => `${key}=${value || "(none)"}`).join(" "));
}
if (import.meta.main)
  main();
export {
  ref
};
