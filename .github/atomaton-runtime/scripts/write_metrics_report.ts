#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/write_metrics_report.ts
import { parseArgs } from "util";
import { readFileSync, readdirSync as readdirSync2 } from "fs";

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
function gitRun(...args) {
  return run(["git", ...args]);
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
function ghPaginated(...args) {
  const { code, stdout, stderr } = gh(...args, "--paginate");
  if (code !== 0) {
    throw new Error(`gh ${args.join(" ")} --paginate: ${stderr || stdout}`);
  }
  if (!stdout.trim())
    return [];
  const flat = [];
  for (const page of splitConcatenatedJson(stdout)) {
    if (Array.isArray(page))
      flat.push(...page);
  }
  return flat;
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
var MACHINERY_ROOT_VAR = "ATOMATON_MACHINERY_ROOT";

// src/entrypoints/machinery/lib/script-ref.ts
function defineScript(importMetaUrl) {
  return { runtimePath: `${SCRIPTS_DIR}/${basename(fileURLToPath(importMetaUrl))}` };
}

// src/entrypoints/machinery/lib/atomaton-data.ts
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readlinkSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync
} from "fs";
import { tmpdir } from "os";
import { dirname, join, relative } from "path";
var DATA_BRANCH = "atomaton-data";
function gitIn(cwd, ...args) {
  const proc = Bun.spawnSync({ cmd: ["git", ...args], cwd, stdout: "pipe", stderr: "pipe" });
  return { code: proc.exitCode ?? 1, stdout: proc.stdout ? proc.stdout.toString("utf8").trim() : "" };
}
function saveSession(targetPath, content, commitMessage) {
  if (gitRun("ls-remote", "--exit-code", "origin", DATA_BRANCH).code !== 0) {
    gitRun("config", "user.email", "action@github.com");
    gitRun("config", "user.name", "GitHub Actions");
    const commit = gitRun("commit-tree", "4b825dc642cb6eb9a060e54bf8d69288fbee4904", "-m", `init: ${DATA_BRANCH} session store`).stdout;
    gitRun("push", "origin", `${commit}:refs/heads/${DATA_BRANCH}`);
  }
  gitRun("fetch", "origin", DATA_BRANCH);
  const worktreeDir = mkdtempSync(join(tmpdir(), "atomaton-data-wt-"));
  gitRun("worktree", "add", worktreeDir, `origin/${DATA_BRANCH}`);
  let saved = false;
  try {
    gitIn(worktreeDir, "config", "user.email", "action@github.com");
    gitIn(worktreeDir, "config", "user.name", "GitHub Actions");
    for (let attempt = 1;attempt <= 5; attempt++) {
      gitIn(worktreeDir, "fetch", "origin", DATA_BRANCH);
      gitIn(worktreeDir, "reset", "--hard", `origin/${DATA_BRANCH}`);
      const fullTarget = join(worktreeDir, targetPath);
      mkdirSync(dirname(fullTarget), { recursive: true });
      writeFileSync(fullTarget, content);
      gitIn(worktreeDir, "add", targetPath);
      if (gitIn(worktreeDir, "diff", "--cached", "--quiet").code === 0) {
        saved = true;
        break;
      }
      gitIn(worktreeDir, "commit", "-m", commitMessage);
      if (gitIn(worktreeDir, "push", "origin", `HEAD:${DATA_BRANCH}`).code === 0) {
        saved = true;
        break;
      }
      console.error(`Push attempt ${attempt} failed (concurrent push) -- resetting and retrying with a fresh pull...`);
      Bun.sleepSync(attempt * 2000);
    }
  } finally {
    gitRun("worktree", "remove", "--force", worktreeDir);
    rmSync(worktreeDir, { recursive: true, force: true });
  }
  return saved;
}

// src/domain/work/search-streak.ts
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

// src/adapters/runner/machinery.ts
function machineryRoot() {
  return process.env[MACHINERY_ROOT_VAR]?.trim() || undefined;
}
function machineryPath(relative) {
  const root = machineryRoot();
  return root ? `${root}/${relative}` : relative;
}

// src/domain/record/metrics.ts
function distributionOf(values) {
  if (values.length === 0)
    return { p50: 0, p90: 0, p99: 0, max: 0, total: 0 };
  const sorted = [...values].sort((a, b) => a - b);
  const at = (p) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))] ?? 0;
  return {
    p50: at(0.5),
    p90: at(0.9),
    p99: at(0.99),
    max: sorted[sorted.length - 1] ?? 0,
    total: values.reduce((sum, v) => sum + v, 0)
  };
}
function tally(names) {
  const counts = new Map;
  for (const name of names)
    counts.set(name, (counts.get(name) ?? 0) + 1);
  return [...counts].map(([name, count]) => ({ name, count })).sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
}
function metricsOf(sessions, declaredServers, declaredSkills, tokens) {
  const calls = sessions.flatMap((s) => s.calls);
  const byTool = new Map;
  for (const call of calls) {
    const row = byTool.get(call.tool) ?? { name: call.tool, count: 0, failed: 0, refused: 0 };
    row.count += 1;
    if (call.failed)
      row.failed += 1;
    if (call.refused)
      row.refused += 1;
    byTool.set(call.tool, row);
  }
  const degraded = new Map;
  for (const session of sessions) {
    for (const call of session.calls) {
      if (call.failed || call.refused)
        continue;
      for (const { server, problem } of call.problems ?? []) {
        const key = `${server}\x00${problem}`;
        const row = degraded.get(key) ?? { problem, server, count: 0, sessions: 0, seen: new Set };
        row.count += 1;
        row.seen.add(session.path);
        if (session.at && (row.lastSeen === undefined || session.at > row.lastSeen)) {
          row.lastSeen = session.at;
        }
        degraded.set(key, row);
      }
    }
  }
  const usedServers = new Set;
  let sawServerCalls = false;
  for (const session of sessions) {
    for (const run of session.runs) {
      if (!run.server_calls)
        continue;
      sawServerCalls = true;
      for (const server of Object.keys(run.server_calls))
        usedServers.add(server);
    }
  }
  const serverUsageKnown = sawServerCalls;
  const loaded = new Set(calls.flatMap((c) => c.skill ? [c.skill] : []));
  return {
    sessions: sessions.length,
    messages: distributionOf(sessions.map((s) => s.messages)),
    byAgent: tally(sessions.map((s) => s.agent)),
    byTool: [...byTool.values()].sort((a, b) => b.count - a.count || a.name.localeCompare(b.name)),
    bySkill: tally(calls.flatMap((c) => c.skill ? [c.skill] : [])),
    byAct: tally(calls.flatMap((c) => c.act ? [c.act] : [])),
    neverUsedServers: declaredServers && serverUsageKnown ? declaredServers.filter((s) => !usedServers.has(s.name)).map((s) => s.name).sort() : undefined,
    neverLoaded: declaredSkills?.filter((s) => !loaded.has(s)).sort(),
    refusals: calls.filter((c) => c.refused).length,
    completions: completionsOf(sessions),
    degraded: [...degraded.values()].map(({ seen, ...row }) => ({ ...row, sessions: seen.size })).sort((a, b) => (b.lastSeen ?? "").localeCompare(a.lastSeen ?? "") || b.count - a.count),
    runs: sessions.flatMap((s) => s.runs),
    tokens: tokens.length === 0 ? undefined : tokenSummary(tokens)
  };
}
function completionsOf(sessions) {
  let completed = 0;
  let silent = 0;
  let unknown = 0;
  for (const session of sessions) {
    const last = session.runs[session.runs.length - 1];
    if (last === undefined) {
      unknown += 1;
      continue;
    }
    if (last.ended_because !== "completed")
      continue;
    completed += 1;
    if (!session.reported)
      silent += 1;
  }
  return { completed, silent, unknown };
}
function tokenSummary(tokens) {
  const total = tokens.reduce((sum, t) => sum + t.total, 0);
  const prompt = tokens.reduce((sum, t) => sum + t.prompt, 0);
  const reported = tokens.filter((t) => t.cached !== undefined);
  const cachedPrompt = reported.reduce((sum, t) => sum + t.prompt, 0);
  const cachedTokens = reported.reduce((sum, t) => sum + (t.cached ?? 0), 0);
  return {
    runs: tokens.length,
    total,
    perRun: distributionOf(tokens.map((t) => t.total)),
    promptShare: total === 0 ? 0 : prompt / total,
    cached: reported.length === 0 ? undefined : {
      runs: reported.length,
      tokens: cachedTokens,
      ofPrompt: cachedPrompt === 0 ? 0 : cachedTokens / cachedPrompt
    }
  };
}

// src/domain/record/tool-trouble.ts
function looksRefused(content) {
  return /blocked by hook|shell_guard:|Tool blocked/.test(content) || /is blocked by denylist pattern/.test(content) || /is not permitted by the allowlist/.test(content) || /Refusing to close issue #[0-9]+: opened by a human/.test(content);
}
function looksFailed(content) {
  if (looksRefused(content))
    return false;
  return /^\s*(Error|error):/.test(content) || /"status"\s*:\s*"(failed|error)"/.test(content);
}
function problemsIn(content) {
  const marker = /^--- \d+ problems? reported by the '([^']+)' server/m.exec(content);
  if (!marker)
    return [];
  const server = marker[1];
  const out = [];
  for (const line of content.slice(marker.index).split(`
`)) {
    const reported = /^(error|warning):\s*(.+)$/.exec(line.trim());
    if (reported)
      out.push({ server, problem: normaliseProblem(reported[2]) });
  }
  return out;
}
function normaliseProblem(text) {
  return text.replace(/\s+/g, " ").replace(/#[0-9]+/g, "#N").replace(/[0-9]{3,}/g, "N").trim().slice(0, 120);
}

// src/domain/record/closing-report.ts
function textOf(content) {
  if (typeof content === "string")
    return content;
  if (!Array.isArray(content))
    return "";
  return content.map((block) => block.type === "text" ? block.text : "").join("");
}
function leftClosingReport(session) {
  const last = lastAssistantMessage(session);
  return last === undefined ? false : textOf(last.content).trim() !== "";
}
function lastAssistantMessage(session) {
  const messages = session?.messages ?? [];
  for (let i = messages.length - 1;i >= 0; i -= 1) {
    const message = messages[i];
    if (message?.role === "assistant")
      return message;
  }
  return;
}

// src/domain/record/metrics-windows.ts
var WINDOWS = [
  { label: "Last 7 days", days: 7 },
  { label: "Last 30 days", days: 30 },
  { label: "Last year", days: 365 },
  { label: "All time" }
];
function within(ended, window, now) {
  if (window.days === undefined)
    return true;
  if (ended === undefined)
    return false;
  const at = Date.parse(ended);
  if (Number.isNaN(at))
    return false;
  return now.getTime() - at <= window.days * 86400000;
}
function sessionEndedAt(runs) {
  return runs.length === 0 ? undefined : runs[runs.length - 1]?.ended;
}
function endings(runs) {
  const counts = new Map;
  for (const run of runs)
    counts.set(run.ended_because, (counts.get(run.ended_because) ?? 0) + 1);
  return [...counts].map(([name, count]) => ({ name, count })).sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
}
function gaveUpShare(runs) {
  if (runs.length === 0)
    return 0;
  return runs.filter((r) => r.ended_because !== "completed").length / runs.length;
}

// src/domain/record/metrics-report.ts
function n(value) {
  return value.toLocaleString("en-US");
}
function plural(value, singular, pluralWord) {
  return `${n(value)} ${value === 1 ? singular : pluralWord}`;
}
function distributionRow(label, d) {
  return `| ${label} | ${n(d.p50)} | ${n(d.p90)} | ${n(d.p99)} | ${n(d.max)} | ${n(d.total)} |`;
}
function tallyTable(rows, of, what, unit) {
  if (rows.length === 0)
    return [`No ${what} recorded.`];
  const out = [`| ${what} | ${unit} | share |`, "| --- | ---: | ---: |"];
  for (const row of rows) {
    const share = of === 0 ? 0 : Math.round(row.count / of * 1000) / 10;
    out.push(`| \`${row.name}\` | ${n(row.count)} | ${share}% |`);
  }
  return out;
}
function runSection(runs, now) {
  const out = ["## Runs", ""];
  if (runs.length === 0) {
    out.push("No run has recorded itself yet. Atomaton writes `atoma_runs` into a session from " + "v0.1.28; sessions older than that carry no times, and there is no way to backfill " + "one that would not be a guess.", "");
    return out;
  }
  out.push("| window | runs | gave up | median seconds | longest | median round trips | median seconds each |");
  out.push("| --- | ---: | ---: | ---: | ---: | ---: | ---: |");
  for (const window of WINDOWS) {
    const inside = runs.filter((run) => within(run.ended, window, now));
    if (inside.length === 0) {
      out.push(`| ${window.label} | 0 | \u2014 | \u2014 | \u2014 | \u2014 | \u2014 |`);
      continue;
    }
    const seconds = inside.map((r) => r.seconds).sort((a, b) => a - b);
    const median = seconds[Math.floor(seconds.length / 2)] ?? 0;
    const share = Math.round(gaveUpShare(inside) * 1000) / 10;
    const longest = seconds[seconds.length - 1] ?? 0;
    const counted = inside.filter((r) => (r.iterations ?? 0) > 0);
    const trips = counted.map((r) => r.iterations ?? 0).sort((a, b) => a - b);
    const medianTrips = trips[Math.floor(trips.length / 2)];
    const each = counted.map((r) => r.seconds / (r.iterations ?? 1)).sort((a, b) => a - b);
    const medianEach = each[Math.floor(each.length / 2)];
    out.push(`| ${window.label} | ${n(inside.length)} | ${share}% | ${n(median)} | ${n(longest)} | ` + `${medianTrips === undefined ? "\u2014" : n(medianTrips)} | ` + `${medianEach === undefined ? "\u2014" : medianEach.toFixed(1)} |`);
  }
  out.push("");
  out.push("**Gave up** is every ending that is not `completed` \u2014 a ceiling reached, a person " + "asking, a provider hanging up, a loop cut short. Each one is a mechanism deciding " + "the run should not continue, which is worth watching whether or not it was right.");
  out.push("");
  out.push("What is left over is not the same as work delivered. `completed` is the core " + "saying its own loop ended rather than being stopped \u2014 it says nothing about " + "whether the run produced a report. **Ran to an end without a report**, in each " + "window below, is that question asked separately.");
  out.push("");
  out.push("| ended because | runs |");
  out.push("| --- | ---: |");
  for (const row of endings(runs))
    out.push(`| \`${row.name}\` | ${n(row.count)} |`);
  out.push("");
  return out;
}
function degradedSection(metrics) {
  const out = ["## Degraded answers", ""];
  out.push("A tool can answer and report that it answered badly \u2014 a search that came back " + "unranked, a log that went nowhere. The call succeeded, so it is neither a failure " + "nor a refusal, and it is easy for one of these to run for months with nobody " + "reading it. **Read `last seen` before `reports`**: an old count is a fixed fault. " + "That date is when the session file was last written, which is the run that touched " + "it last and not necessarily the run that reported the problem \u2014 so it errs recent.");
  out.push("");
  if (metrics.degraded.length === 0) {
    out.push("Nothing reported a problem alongside an answer.", "");
    return out;
  }
  out.push("| last seen | server | reports | sessions | problem |");
  out.push("| --- | --- | ---: | ---: | --- |");
  for (const row of metrics.degraded) {
    out.push(`| ${row.lastSeen?.slice(0, 10) ?? "\u2014"} | \`${row.server}\` | ${n(row.count)} | ` + `${n(row.sessions)} | ${row.problem.replace(/\|/g, String.fromCharCode(92) + "|")} |`);
  }
  out.push("");
  return out;
}
function completionSection(tally) {
  const out = [];
  const share = tally.completed === 0 ? 0 : Math.round(tally.silent / tally.completed * 1000) / 10;
  out.push(`**Ran to an end without a report:** ${n(tally.silent)} of ` + `${plural(tally.completed, "session", "sessions")} whose last run the core recorded as ` + `\`completed\` \u2014 ${share}%. These are not runs that gave up: nothing stopped them, ` + "they simply ended without writing a closing line, so the work is in a saved " + "session and nowhere a person or the next agent reads.");
  if (tally.unknown > 0) {
    out.push("", `Neither figure covers ${plural(tally.unknown, "session", "sessions")} with no run record ` + "at all \u2014 without one, neither question can be asked. Everything from before atoma " + "recorded its runs is in there.");
  }
  out.push("");
  return out;
}
function windowSection(label, metrics) {
  const out = [`## ${label}`, ""];
  if (metrics.sessions === 0) {
    out.push("No session ran in this window.", "");
    return out;
  }
  out.push(`${plural(metrics.sessions, "session", "sessions")}.`);
  out.push("");
  out.push(...completionSection(metrics.completions));
  if (metrics.tokens) {
    const t = metrics.tokens;
    out.push(`**${n(t.total)} tokens** over ${plural(t.runs, "run", "runs")} that reported them, ` + `**${Math.round(t.promptShare * 1000) / 10}% of it prompt** \u2014 what the agents were ` + "made to read, not what they wrote. Anything spent on making runs cheaper belongs " + "on that side. No money here, deliberately: of the four providers only one reports " + "a cost, and a price table goes quietly stale and then prints confident wrong " + "numbers.");
    out.push("");
    out.push(t.cached ? `**${Math.round(t.cached.ofPrompt * 1000) / 10}% of that prompt was served from cache**, ` + `over the ${t.cached.runs === t.runs ? "runs" : `${n(t.cached.runs)} of ${n(t.runs)} runs`} whose ` + "provider reported it. A cached prompt token costs a fraction of a fresh one, so this " + "is most of what separates the counts above from the bill \u2014 and it is the figure that " + "moves when what gets resent changes." : "No run in this window reported how much of its prompt was cached, so the share is " + "unknown rather than zero \u2014 either the provider does not report it or the run predates " + "the field.");
    out.push("");
  }
  out.push("| | p50 | p90 | p99 | max | total |");
  out.push("| --- | ---: | ---: | ---: | ---: | ---: |");
  if (metrics.tokens)
    out.push(distributionRow("tokens per run", metrics.tokens.perRun));
  out.push(distributionRow("messages per session", metrics.messages));
  out.push("");
  out.push(...tallyTable(metrics.byAgent, metrics.sessions, "agent", "sessions"));
  out.push("");
  if (metrics.byTool.length > 0) {
    out.push("**Refused** is the machinery saying no \u2014 a denylist, an allowlist, a hook. A guard " + "working is not a tool breaking, and a reader cannot act on the two the same way, so " + "they are counted apart. **Failed** is everything else that came back as an error, " + "by string match, so it is an estimate.");
    out.push("");
    out.push("| tool | calls | failed | refused | failure rate |");
    out.push("| --- | ---: | ---: | ---: | ---: |");
    for (const row of metrics.byTool) {
      const rate = row.count === 0 ? 0 : Math.round(row.failed / row.count * 1000) / 10;
      out.push(`| \`${row.name}\` | ${n(row.count)} | ${n(row.failed)} | ${n(row.refused)} | ${rate}% |`);
    }
    out.push("");
  }
  out.push("What the agents do when they reach for a shell. `search` without a matching `open` is " + "the shape that produced this project's most expensive runs; `edit` against `verify` " + "is the shape that turned out not to occur at all.");
  out.push("");
  out.push(...tallyTable(metrics.byAct, metrics.byAct.reduce((s, a) => s + a.count, 0), "act", "calls"));
  out.push("");
  out.push(...tallyTable(metrics.bySkill, metrics.bySkill.reduce((s, k) => s + k.count, 0), "skill", "loads"));
  out.push("");
  return out;
}
function renderReport(all, forWindow, now) {
  const out = [];
  out.push("# Agent metrics");
  out.push("");
  out.push("Read from the sessions stored on this branch. Nothing here is recorded specially: " + "every number is something the agents already wrote down while working.");
  out.push("");
  out.push(`Generated ${now.toISOString().slice(0, 10)}.`);
  out.push("");
  out.push("A session appears in a dated window only if it recorded when its runs ended. " + "Sessions from before run recording existed are counted under All time alone, " + "so the dated windows are thinner than the project was \u2014 that gap closes as new " + "sessions arrive, not by anything changing here.");
  out.push("");
  out.push(...runSection(all.runs, now));
  for (const window of WINDOWS)
    out.push(...windowSection(window.label, forWindow(window)));
  out.push(...degradedSection(all));
  out.push("## Never used");
  out.push("");
  out.push("Over all time, because something used once a year is still used. Each of these sits " + "in the prompt of every run and returns nothing.");
  out.push("");
  out.push(all.neverUsedServers === undefined ? "The declared servers could not be read, so this cannot say which are unused." : all.neverUsedServers.length === 0 ? "Every declared server has been called at least once." : `Servers never called:

` + all.neverUsedServers.map((t) => `- \`${t}\``).join(`
`));
  out.push("");
  out.push(all.neverLoaded === undefined ? "The declared skills could not be read, so this cannot say which are unloaded." : all.neverLoaded.length === 0 ? "Every skill has been loaded at least once." : `Skills never loaded:

` + all.neverLoaded.map((s) => `- \`${s}\``).join(`
`));
  out.push("");
  return out.join(`
`);
}

// src/domain/record/token-line.ts
var PATTERN = /_Tokens:\s*([\d,]+)\s*total\s*\(([\d,]+)\s*prompt\s*\+\s*([\d,]+)\s*completion(?:,\s*([\d,]+)\s*of the prompt cached)?\)_/;
function parseTokenLine(body) {
  const match = PATTERN.exec(body);
  if (!match)
    return;
  const toNumber = (s) => Number(s.replace(/,/g, ""));
  return {
    total: toNumber(match[1]),
    prompt: toNumber(match[2]),
    completion: toNumber(match[3]),
    cached: match[4] === undefined ? undefined : toNumber(match[4])
  };
}

// src/entrypoints/machinery/write_metrics_report.ts
var ref = defineScript(import.meta.url);
var BRANCH = "atomaton-data";
var REPORT_PATH = "metrics/report.md";
var ROWS_PATH = "metrics/rows.json";
function rowsOf(sessions) {
  return sessions.map((s) => {
    const tools = {};
    const acts = {};
    for (const call of s.calls) {
      tools[call.tool] = (tools[call.tool] ?? 0) + 1;
      if (call.act)
        acts[call.act] = (acts[call.act] ?? 0) + 1;
    }
    return {
      path: s.path,
      agent: s.agent,
      messages: s.messages,
      calls: s.calls.length,
      failed: s.calls.filter((c) => c.failed).length,
      refused: s.calls.filter((c) => c.refused).length,
      skills: s.calls.flatMap((c) => c.skill ? [c.skill] : []),
      reported: s.reported,
      tools,
      acts,
      runs: s.runs
    };
  });
}
function log(message) {
  console.error(`[metrics] ${message}`);
}
function agentOf(session) {
  return session.metadata?.github_context?.agent?.trim() || "unknown";
}
function sessionFrom(path, raw) {
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch {
    log(`${path} is not readable as JSON; skipping it`);
    return;
  }
  const messages = parsed.messages ?? [];
  const results = new Map;
  for (const message of messages) {
    const id = message.tool_call_id;
    if (message.role === "tool" && typeof id === "string") {
      results.set(id, typeof message.content === "string" ? message.content : "");
    }
  }
  const calls = [];
  const agent = agentOf(parsed);
  for (const message of messages) {
    for (const call of message.tool_calls ?? []) {
      const tool = call.function?.name ?? "";
      if (!tool)
        continue;
      const result = results.get(call.id ?? "") ?? "";
      const { skill, act } = classifyCall(tool, call.function?.arguments ?? "{}");
      calls.push({
        tool,
        agent,
        failed: looksFailed(result),
        refused: looksRefused(result),
        skill,
        act,
        problems: problemsIn(result)
      });
    }
  }
  const runs = Array.isArray(parsed.atoma_runs) ? parsed.atoma_runs : [];
  return { path, agent, messages: messages.length, calls, runs, reported: leftClosingReport(parsed) };
}
function shellAct(command) {
  if (/>\s*[^\s|&>]+/.test(command) && !/>\s*\/dev\/null/.test(command))
    return "edit";
  const head = command.trim().split(/\s*(?:\|\||&&|[;|])\s*/)[0] ?? "";
  const word = (head.trim().split(/\s+/).find((t) => t && !t.includes("=") && t !== "sudo" && t !== "time") ?? "").split("/").pop();
  if (word === "sed" && /\s-i\b/.test(command))
    return "edit";
  if (/^(bun|npm|npx|pnpm|yarn|cargo|go|pytest|python3?|make|tsc|jest|vitest|mvn|gradle|dotnet)$/.test(word ?? "")) {
    return "verify";
  }
  const classified = classifyShellAct(command);
  return classified === "other" ? "other" : classified;
}
function classifyCall(tool, args) {
  if (tool.endsWith("load_skill")) {
    try {
      return { skill: JSON.parse(args).skill_name };
    } catch {
      return {};
    }
  }
  if (tool === "bash") {
    try {
      return { act: shellAct(JSON.parse(args).command ?? "") };
    } catch {
      return { act: "other" };
    }
  }
  return {};
}
function tokensReported(repo) {
  const comments = ghPaginated("api", `repos/${repo}/issues/comments?per_page=100`);
  const out = [];
  for (const comment of comments) {
    const figures = parseTokenLine(comment.body ?? "");
    if (!figures)
      continue;
    const number = Number(/(\d+)$/.exec(comment.issue_url ?? "")?.[1] ?? 0);
    out.push({
      issue: number,
      ...figures,
      at: comment.created_at
    });
  }
  return out;
}
function skillsUnder(dir) {
  let entries;
  try {
    entries = readdirSync2(dir, { recursive: true }).map(String);
  } catch {
    return;
  }
  const skills = entries.filter((entry) => entry.endsWith(".md")).map((entry) => entry.replaceAll("\\", "/").slice(0, -".md".length)).sort();
  return skills.length === 0 ? undefined : skills;
}
function declared() {
  let tools;
  try {
    const config = Bun.YAML.parse(readFileSync(machineryPath(CONFIG_FILE), "utf8"));
    tools = Object.keys(config.tools?.servers ?? {}).map((name) => ({ name }));
  } catch {
    log("could not read the tool servers from config.yaml; the report will not name unused tools");
  }
  const skillsDir = machineryPath(SKILLS_DIR);
  const skills = skillsUnder(skillsDir);
  if (skills === undefined) {
    log(`no skills found under ${skillsDir}; the report will say it could not check`);
  }
  return { tools, skills };
}
function sessionDates(paths) {
  const wanted = new Set(paths);
  const out = new Map;
  const log = gitRun("log", `origin/${BRANCH}`, "--name-only", "--format=%x00%aI", "--", "sessions");
  if (log.code !== 0)
    return out;
  let date = "";
  for (const line of log.stdout.split(`
`)) {
    const trimmed = line.trim();
    if (trimmed.startsWith("\x00")) {
      date = trimmed.slice(1);
      continue;
    }
    if (trimmed && wanted.has(trimmed) && !out.has(trimmed))
      out.set(trimmed, date);
  }
  return out;
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: { repo: { type: "string" }, stdout: { type: "boolean" } }
  });
  const repo = values.repo ?? process.env.GITHUB_REPOSITORY ?? "";
  if (gitRun("fetch", "origin", BRANCH).code !== 0) {
    log(`${BRANCH} does not exist yet; nothing to report on`);
    return;
  }
  const listed = gitRun("ls-tree", "-r", "--name-only", `origin/${BRANCH}`, "--", "sessions");
  const paths = listed.stdout.split(`
`).map((s) => s.trim()).filter((s) => s.endsWith(".json"));
  const writtenAt = sessionDates(paths);
  const sessions = [];
  for (const path of paths) {
    const shown = gitRun("show", `origin/${BRANCH}:${path}`);
    if (shown.code !== 0)
      continue;
    const record = sessionFrom(path, shown.stdout);
    if (record)
      sessions.push({ ...record, at: writtenAt.get(path) });
  }
  let tokens = [];
  if (repo) {
    try {
      tokens = tokensReported(repo);
    } catch (error) {
      log(`could not read the reported tokens: ${error.message}`);
    }
  }
  const { tools, skills } = declared();
  const now = new Date;
  const forWindow = (window) => metricsOf(sessions.filter((s) => within(sessionEndedAt(s.runs), window, now)), tools, skills, tokens.filter((t) => within(t.at, window, now)));
  const report = renderReport(metricsOf(sessions, tools, skills, tokens), forWindow, now);
  const runs = sessions.flatMap((s) => s.runs);
  log(`${sessions.length} sessions, ${runs.length} recorded runs, ${tokens.length} reporting tokens`);
  if (values.stdout) {
    console.log(report);
    return;
  }
  if (!saveSession(ROWS_PATH, `${JSON.stringify(rowsOf(sessions), null, 2)}
`, `atomaton: metric rows from ${sessions.length} sessions`)) {
    log("could not write the rows; the report is unaffected");
  }
  if (!saveSession(REPORT_PATH, report, `atomaton: metrics from ${sessions.length} sessions`)) {
    log("could not write the report; the run is unaffected");
  }
}
if (import.meta.main)
  main();
export {
  REPORT_PATH,
  ROWS_PATH,
  agentOf,
  classifyCall,
  ref,
  skillsUnder
};
