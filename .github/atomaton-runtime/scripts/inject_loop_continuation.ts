#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/inject_loop_continuation.ts
import { existsSync, readFileSync, writeFileSync } from "fs";
import { parseArgs } from "util";

// src/domain/work/loop-continuation.ts
function loopAbortFromSession(session) {
  const runs = session?.atoma_runs;
  if (!Array.isArray(runs) || runs.length === 0)
    return;
  const last = runs[runs.length - 1];
  const recorded = last?.loop_abort;
  if (typeof recorded !== "object" || recorded === null)
    return;
  const verdict = recorded;
  if (typeof verdict.rule !== "string" || verdict.rule === "")
    return;
  return {
    rule: verdict.rule,
    tool: typeof verdict.tool === "string" ? verdict.tool : "",
    arguments: typeof verdict.arguments === "string" ? verdict.arguments : "",
    count: typeof verdict.count === "number" ? verdict.count : 0,
    capped: verdict.capped === true,
    lastError: typeof verdict.last_error === "string" && verdict.last_error !== "" ? verdict.last_error : undefined
  };
}
function times(count) {
  return count > 1 ? `${count} times in a row` : "over and over";
}
function repeatedCallSentence(abort) {
  const call = abort.tool === "" ? "a tool" : `\`${abort.tool}\``;
  const count = times(abort.count);
  switch (abort.rule) {
    case "identical_answers":
      return `it called ${call} ${count} and got the same answer back every time`;
    case "empty_results":
      return `it called ${call} ${count} and every call returned nothing`;
    case "identical_failures":
      return `it called ${call} ${count} and every call failed the same way`;
    case "cycle":
      return `the same calls, starting with ${call}, came round in a circle ${count}`;
    default:
      return `it called ${call} ${count} and repeating it got no further (\`${abort.rule}\`)`;
  }
}
function repeatedArguments(abort) {
  return abort.arguments === "" ? undefined : abort.arguments;
}
function loopContinuationMessage(abort) {
  const parts = [`Your run was stopped by the loop guard: ${repeatedCallSentence(abort)}.`];
  const argumentsItRepeated = repeatedArguments(abort);
  if (argumentsItRepeated !== undefined)
    parts.push(`The arguments it repeated were \`${argumentsItRepeated}\`.`);
  if (abort.capped) {
    parts.push("The answer was also cut off by the output cap, so calling it again cannot reach what was cut. Work from what is there, or change the approach.");
  }
  if (abort.lastError !== undefined)
    parts.push(`The failure it reported was:

${abort.lastError}`);
  parts.push(nextStep(abort));
  return parts.join(`

`);
}
function nextStep(abort) {
  switch (abort.rule) {
    case "identical_answers":
      return "That answer is already in this session above, so reading it again returns the same text. Use what is in hand: write the closing report, verdict or commit from what you have already read, or take the step that reading was for. Do not call it again.";
    case "empty_results":
      return "Calling it again returns nothing again. Either it is the wrong tool for what you are after, or what you are after is not there \u2014 answer from what the other calls already gave you, and write the closing report or the verdict now.";
    case "identical_failures":
      return "It will fail the same way again. Fix what the error names, reach for a different tool, or write the closing report and say what you could not do.";
    case "cycle":
      return "Each round is the same work over again, so repeating the calls cannot get past it. Take the next step the cycle was circling: write the closing report or the verdict from what is in hand.";
    default:
      return "Repeating it will not move the run on. Work from what is in hand, and write the closing report or the verdict next.";
  }
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

// src/entrypoints/machinery/inject_loop_continuation.ts
var ref = defineScript(import.meta.url);
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: { session: { type: "string" } }
  });
  const path = values.session;
  if (!path) {
    console.error("usage: inject_loop_continuation.ts --session PATH");
    process.exit(2);
  }
  if (!existsSync(path)) {
    console.error(`inject_loop_continuation: ${path} does not exist; nothing to do`);
    return;
  }
  const session = JSON.parse(readFileSync(path, "utf8"));
  const abort = loopAbortFromSession(session);
  if (abort === undefined) {
    console.error(`inject_loop_continuation: ${path} records no loop verdict; nothing to say`);
    return;
  }
  const messages = session.messages ?? [];
  messages.push({ role: "user", content: loopContinuationMessage(abort) });
  session.messages = messages;
  writeFileSync(path, JSON.stringify(session, null, 2));
  console.error(`inject_loop_continuation: appended the continuation for the ${abort.rule} verdict to ${path}`);
}
if (import.meta.main)
  main();
export {
  ref
};
