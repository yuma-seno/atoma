#!/usr/bin/env bun
// @bun

// src/entrypoints/tools/hooks/shell_guard.ts
import { resolve, sep } from "path";

// src/domain/work/search-streak.ts
var MAX_SEARCHES_WITHOUT_OPENING = 15;
var SEARCHES = /^(grep|egrep|fgrep|rg|ag|ack|ugrep|find)$/;
var OPENS = /^(cat|bat|head|tail|sed|less|more|nl|od|xxd)$/;
function classifyShellAct(command) {
  const first = command.trim().split(/\s*(?:\|\||&&|[;|])\s*/)[0]?.trim().split(/\s+/).find((token) => token.length > 0 && !token.includes("=") && token !== "sudo" && token !== "time");
  if (first === undefined)
    return "other";
  const name = first.split("/").pop() ?? first;
  if (SEARCHES.test(name))
    return "search";
  if (OPENS.test(name))
    return "open";
  return "other";
}
function nextStreak(streak, act) {
  if (act === "search")
    return streak + 1;
  if (act === "open")
    return 0;
  return streak;
}
function refusalReason(streak, limit = MAX_SEARCHES_WITHOUT_OPENING) {
  if (streak < limit)
    return;
  return `${streak} searches in a row without opening any of the files they found. A search returns ` + "where something is, not what it is, so nothing found so far has been read. Do one of two " + "things before searching again: open the most promising result \u2014 with `read`, or `sed -n` " + "for a range \u2014 or, if you are guessing at what the thing is called, ask " + "search_code the same question in a sentence. Measured, that finds the right file " + "in the top five 70% of the time, against 41.5% for the regex patterns agents search with.";
}

// src/entrypoints/tools/lib/search-streak-file.ts
import { readFileSync, writeFileSync } from "fs";
function streakFile() {
  const opsLog = process.env.ATOMATON_OPS_LOG;
  if (!opsLog)
    return;
  const dir = opsLog.replace(/[/\\][^/\\]*$/, "");
  return dir === opsLog ? undefined : `${dir}/search-streak`;
}
function readStreak(file) {
  if (!file)
    return 0;
  try {
    const n = Number(readFileSync(file, "utf8").trim());
    return Number.isFinite(n) && n >= 0 ? n : 0;
  } catch {
    return 0;
  }
}
function writeStreak(file, streak) {
  if (!file)
    return;
  try {
    writeFileSync(file, String(streak));
  } catch {}
}

// src/entrypoints/tools/hooks/shell_guard.ts
var ROUTING_RULES = {
  gh: "gh CLI is disabled. Use the github tools -- create_pr, create_sub_issue, merge_pr and the rest -- for GitHub operations.",
  curl: "curl is disabled. Use fetch, which returns the page as text.",
  wget: "wget is disabled. Use fetch.",
  ssh: "ssh is disabled: this run works on the checked-out repository, not on other hosts.",
  scp: "scp is disabled: this run works on the checked-out repository, not on other hosts.",
  rsync: "rsync is disabled: this run works on the checked-out repository, not on other hosts."
};
var COMMAND_WRAPPERS = new Set(["sudo", "env", "time", "nohup", "nice", "xargs", "command", "exec"]);
var RESERVED_WORDS = new Set([
  "if",
  "then",
  "else",
  "elif",
  "fi",
  "for",
  "while",
  "until",
  "do",
  "done",
  "case",
  "esac",
  "select",
  "function",
  "in",
  "!",
  "{",
  "}",
  "(",
  ")",
  "[[",
  "]]"
]);
function segmentsOf(text) {
  return text.split(/\s*(?:&&|\|\||[;|&\n])\s*/);
}
var HEREDOC = /<<-?\s*(['"]?)([A-Za-z_][A-Za-z0-9_]*)\1/g;
function withoutHeredocBodies(command) {
  const kept = [];
  const terminators = [];
  for (const line of command.split(`
`)) {
    const ending = terminators[0];
    if (ending !== undefined) {
      if (line.trim() === ending)
        terminators.shift();
      continue;
    }
    kept.push(line);
    for (const match of line.matchAll(HEREDOC)) {
      const before = match.index === 0 ? "" : line[match.index - 1] ?? "";
      if (before !== "" && !/[\s\d]/.test(before))
        continue;
      terminators.push(match[2]);
    }
  }
  return kept.join(`
`);
}
function blankSingleQuoted(text) {
  const out = [...text];
  let index = 0;
  let inDouble = false;
  while (index < text.length) {
    const char = text[index];
    if (char === "\\") {
      index += 2;
      continue;
    }
    if (char === '"') {
      inDouble = !inDouble;
      index += 1;
      continue;
    }
    if (char === "'" && !inDouble) {
      let end = index + 1;
      while (end < text.length && text[end] !== "'")
        end += 1;
      for (let at = index;at < Math.min(end + 1, text.length); at += 1)
        out[at] = " ";
      index = end + 1;
      continue;
    }
    index += 1;
  }
  return out.join("");
}
function substitutions(command) {
  const text = blankSingleQuoted(command);
  const found = [];
  for (let index = 0;index < text.length; index += 1) {
    if (text.startsWith("$(", index)) {
      const end = closingParen(text, index + 1);
      if (end !== -1) {
        found.push(text.slice(index + 2, end));
        index = end;
      }
    } else if (text[index] === "`") {
      const end = text.indexOf("`", index + 1);
      if (end !== -1) {
        found.push(text.slice(index + 1, end));
        index = end;
      }
    }
  }
  return found;
}
function closingParen(text, open) {
  let depth = 0;
  for (let index = open;index < text.length; index += 1) {
    if (text[index] === "(")
      depth += 1;
    else if (text[index] === ")") {
      depth -= 1;
      if (depth === 0)
        return index;
    }
  }
  return -1;
}
var SHELLS = new Set(["sh", "bash", "zsh", "dash", "ksh"]);
function invokedPrograms(command, depth = 0) {
  const text = withoutHeredocBodies(command);
  const found = [];
  for (const segment of segmentsOf(text)) {
    const tokens = segment.trim().split(/\s+/).filter(Boolean);
    let index = 0;
    while (index < tokens.length) {
      const token = tokens[index];
      if (/^[A-Za-z_][A-Za-z0-9_]*=/.test(token)) {
        index++;
        continue;
      }
      const name = token.split("/").pop() ?? token;
      if (COMMAND_WRAPPERS.has(name) || RESERVED_WORDS.has(name)) {
        index++;
        continue;
      }
      break;
    }
    const program = tokens[index];
    if (!program)
      continue;
    const name = program.split("/").pop() ?? program;
    found.push(name);
    if (SHELLS.has(name) && depth < 2) {
      const flag = tokens.indexOf("-c", index + 1);
      const inner = flag === -1 ? "" : tokens.slice(flag + 1).join(" ").replace(/^['"]|['"]$/g, "");
      if (inner)
        found.push(...invokedPrograms(inner, depth + 1));
    }
  }
  if (depth < 2) {
    for (const inner of substitutions(text))
      found.push(...invokedPrograms(inner, depth + 1));
  }
  return found;
}
var PROCESS_ENVIRONMENT_READ = [
  /^(?=[\s\S]*\/proc)(?=[\s\S]*\benviron\b)/,
  "Reading a process's environment through /proc is disabled: credential-holding tool servers run as a different OS user and the shell has no sudo."
];
var ROUTED_GIT_COMMANDS = new Set([
  "add",
  "am",
  "apply",
  "cherry-pick",
  "commit",
  "fetch",
  "merge",
  "mv",
  "pull",
  "push",
  "rebase",
  "restore",
  "revert",
  "rm",
  "checkout",
  "switch",
  "branch",
  "tag"
]);
var ROUTED_GIT_ADVICE = {
  merge: " To bring the base branch into this branch, call merge_base_into_branch."
};
var READ_ONLY_BRANCH_FLAG = /^(-a|-r|-v|-vv|--all|--remotes|--verbose|--list|-l|--show-current|--merged|--no-merged|--contains|--points-at|--sort=.+|--format=.+|--color|--no-color)$/;
var READ_ONLY_TAG_FLAG = /^(-l|--list|-n\d*|--sort=.+|--contains|--points-at|--merged|--no-merged)$/;
var READ_ONLY_GIT_FORMS = {
  branch: (args) => args.every((arg) => READ_ONLY_BRANCH_FLAG.test(arg)),
  stash: (args) => /^(list|show)$/.test(args[0] ?? ""),
  config: (args) => /^(--get|--get-all|--get-regexp|--list|-l)$/.test(args[0] ?? ""),
  remote: (args) => args.every((arg) => /^(-v|--verbose)$/.test(arg)) || /^(show|get-url)$/.test(args[0] ?? ""),
  tag: (args) => args.every((arg) => READ_ONLY_TAG_FLAG.test(arg))
};
function isReadOnlyGitForm(subcommand, args) {
  return READ_ONLY_GIT_FORMS[subcommand]?.(args) ?? false;
}
var MUTATING_GIT_COMMANDS = new Set([
  "add",
  "am",
  "apply",
  "bisect",
  "branch",
  "checkout",
  "cherry-pick",
  "clean",
  "commit",
  "config",
  "fetch",
  "init",
  "merge",
  "mv",
  "pull",
  "push",
  "rebase",
  "remote",
  "reset",
  "restore",
  "revert",
  "rm",
  "stash",
  "switch",
  "tag",
  "worktree"
]);
function findMutatingGitCommand(command, depth = 0) {
  const text = withoutHeredocBodies(command);
  for (const segment of segmentsOf(text)) {
    const tokens = segment.trim().split(/\s+/);
    const gitIndex = tokens.findIndex((token) => token === "git" || token.endsWith("/git"));
    if (gitIndex === -1)
      continue;
    let index = gitIndex + 1;
    while (index < tokens.length && tokens[index].startsWith("-")) {
      const option = tokens[index++];
      if (["-C", "-c", "--git-dir", "--work-tree", "--namespace", "--super-prefix", "--config-env"].includes(option)) {
        index++;
      }
    }
    const subcommand = tokens[index];
    if (!subcommand || !MUTATING_GIT_COMMANDS.has(subcommand))
      continue;
    if (isReadOnlyGitForm(subcommand, tokens.slice(index + 1)))
      continue;
    return subcommand;
  }
  if (depth < 2) {
    for (const inner of substitutions(text)) {
      const nested = findMutatingGitCommand(inner, depth + 1);
      if (nested)
        return nested;
    }
  }
  return;
}
var ALLOWED = { allow: true, reason: "" };
function outsideRepository(workingDirectory) {
  const resolved = resolve(workingDirectory);
  const root = resolve(process.cwd());
  if (resolved === root || resolved.startsWith(root + sep))
    return;
  return `working_directory must stay inside the repository (${root}); '${workingDirectory}' is outside it.`;
}
function checkInvocation(invocation) {
  const cwd = invocation.workingDirectory?.trim();
  if (cwd) {
    const reason = outsideRepository(cwd);
    if (reason)
      return { allow: false, reason };
  }
  const gitCommand = findMutatingGitCommand(invocation.command);
  if (gitCommand) {
    const routed = ROUTED_GIT_COMMANDS.has(gitCommand);
    return {
      allow: false,
      reason: routed ? `Raw 'git ${gitCommand}' is disabled. Use the github tools for Git mutations and branch synchronization.` + (ROUTED_GIT_ADVICE[gitCommand] ?? "") : `Raw 'git ${gitCommand}' is disabled, and there is no MCP tool for it: this run does not do that. ` + `Commit with commit_and_push; read-only inspection (status, diff, log) runs normally.`
    };
  }
  const [environPattern, environReason] = PROCESS_ENVIRONMENT_READ;
  if (environPattern.test(invocation.command))
    return { allow: false, reason: environReason };
  for (const program of invokedPrograms(invocation.command)) {
    const reason = ROUTING_RULES[program];
    if (reason)
      return { allow: false, reason };
  }
  return ALLOWED;
}
function streakRefusal(command) {
  const file = streakFile();
  const act = classifyShellAct(command);
  const streak = nextStreak(readStreak(file), act);
  writeStreak(file, streak);
  return act === "search" ? refusalReason(streak) : undefined;
}
async function main() {
  let data;
  try {
    const raw = await new Response(Bun.stdin.stream()).text();
    data = JSON.parse(raw);
  } catch {
    console.log(JSON.stringify({ allow: false, reason: "shell_guard: failed to parse input" }));
    return;
  }
  const args = data.arguments ?? {};
  const command = String(args.command ?? args.cmd ?? args.shell ?? "");
  const { allow, reason } = checkInvocation({
    command,
    workingDirectory: typeof args.working_directory === "string" ? args.working_directory : undefined
  });
  const refusal = allow ? streakRefusal(command) : undefined;
  if (refusal !== undefined) {
    console.log(JSON.stringify({ allow: false, reason: `shell_guard: ${refusal}` }));
    return;
  }
  if (allow) {
    console.log(JSON.stringify({ allow: true }));
  } else {
    console.log(JSON.stringify({
      allow: false,
      reason: `Command blocked by shell guard: ${reason} (attempted: ${command.slice(0, 120)})`
    }));
  }
}
if (import.meta.main)
  main();
