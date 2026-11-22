#!/usr/bin/env bun
// @bun

// src/entrypoints/machinery/update_atomaton.ts
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

// src/adapters/github/pull-request.ts
function viewArgs(repo, number, fields) {
  const json = typeof fields === "string" ? fields : fields.join(",");
  return ["pr", "view", String(number), "--repo", repo, "--json", json];
}
function whyNot(what, result) {
  return `${what}: ${result.stderr || result.stdout || `gh exited ${result.code}`}`;
}
function readPullRequest(repo, number, fields) {
  const what = `gh pr view ${number} --repo ${repo}`;
  const result = ghRead(...viewArgs(repo, number, fields));
  if (result.code !== 0)
    return { known: false, why: whyNot(what, result) };
  if (!result.stdout.trim())
    return { known: false, why: `${what}: gh printed nothing` };
  try {
    return { known: true, value: JSON.parse(result.stdout) };
  } catch {
    return { known: false, why: `${what}: the answer was not JSON` };
  }
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

// src/adapters/runner/machinery.ts
function machineryRoot() {
  return process.env[MACHINERY_ROOT_VAR]?.trim() || undefined;
}
function machineryPath(relative) {
  const root = machineryRoot();
  return root ? `${root}/${relative}` : relative;
}

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
function leftoversOf(targetManifest, treePaths) {
  return noLongerShipped(targetManifest, treePaths);
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

// src/entrypoints/machinery/lib/update-apply.ts
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "fs";
import { tmpdir } from "os";
import { basename as basename2, dirname, join, resolve, sep } from "path";
function applyPlan(treeRoot, plan, base, target, ours) {
  const root = resolve(treeRoot);
  const inside = (path) => {
    const destination = resolve(join(root, path));
    if (destination !== root && !destination.startsWith(root + sep))
      return null;
    return destination;
  };
  for (const path of [...plan.take, ...plan.add, ...plan.delete, ...plan.merge]) {
    if (inside(path) === null)
      return { why: `\`${path}\` resolves outside the tree` };
  }
  const writes = new Map;
  const deletes = new Set;
  const taken = [];
  const added = [];
  const removed = [];
  const merged = [];
  const conflicts = [];
  for (const path of plan.take) {
    const content = target.get(path);
    if (content === undefined) {
      return { why: `the target release does not carry \`${path}\`, which the plan says to take` };
    }
    writes.set(path, content);
    taken.push(path);
  }
  for (const path of plan.add) {
    const content = target.get(path);
    if (content === undefined) {
      return { why: `the target release does not carry \`${path}\`, which the plan says to add` };
    }
    const destination = inside(path);
    if (destination === null)
      return { why: `\`${path}\` resolves outside the tree` };
    if (existsSync(destination)) {
      return { why: `\`${path}\` exists in the tree but the plan did not expect it` };
    }
    writes.set(path, content);
    added.push(path);
  }
  for (const path of plan.delete) {
    deletes.add(path);
    removed.push(path);
  }
  const scratch = (() => {
    try {
      return mkdtempSync(join(tmpdir(), "atomaton-merge-"));
    } catch (error) {
      return null;
    }
  })();
  if (scratch === null) {
    return { why: `could not make a scratch directory for the merges` };
  }
  try {
    for (const path of plan.merge) {
      const baseContent = base.get(path);
      const oursContent = ours.get(path);
      const targetContent = target.get(path);
      if (baseContent === undefined || oursContent === undefined || targetContent === undefined) {
        return { why: `\`${path}\` is marked for a merge this run cannot read all three sides of` };
      }
      const outcome = mergeFile(scratch, basename2(path) || path, baseContent, oursContent, targetContent);
      if ("why" in outcome)
        return { why: outcome.why };
      writes.set(path, outcome.content);
      (outcome.conflicts > 0 ? conflicts : merged).push(path);
    }
  } finally {
    try {
      rmSync(scratch, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 });
    } catch {}
  }
  for (const path of writes.keys()) {
    let parent = dirname(path);
    while (parent !== "." && parent !== "/") {
      const stats = statSync(resolve(join(root, parent)), { throwIfNoEntry: false });
      if (stats?.isFile()) {
        if (!deletes.has(parent)) {
          return { why: `\`${path}\` needs \`${parent}\` to be a directory, but the tree holds a file there that the plan does not delete` };
        }
      }
      const next = dirname(parent);
      parent = next;
    }
  }
  for (const [path, content] of writes) {
    const destination = inside(path);
    if (destination === null)
      throw new Error(`\`${path}\` resolves outside the tree`);
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, content);
  }
  for (const path of deletes) {
    try {
      rmSync(resolve(join(root, path)), { force: true });
    } catch (error) {
      return { why: `could not remove \`${path}\`: ${error.message}` };
    }
  }
  return { result: { taken, added, removed, merged, conflicts } };
}
function mergeFile(scratch, label, base, ours, theirs) {
  const oursPath = join(scratch, "ours");
  const basePath = join(scratch, "base");
  const theirsPath = join(scratch, "theirs");
  writeFileSync(oursPath, ours);
  writeFileSync(basePath, base);
  writeFileSync(theirsPath, theirs);
  const run = Bun.spawnSync({
    cmd: [
      "git",
      "merge-file",
      "-L",
      `${label} (ours)`,
      "-L",
      `${label} (base)`,
      "-L",
      `${label} (theirs)`,
      oursPath,
      basePath,
      theirsPath
    ],
    stdout: "pipe",
    stderr: "pipe"
  });
  const code = run.exitCode ?? 1;
  if (code === 255) {
    return { why: `git merge-file could not run: ${run.stderr?.toString("utf8").trim() || "no message"}` };
  }
  if (code < 0) {
    return { why: `the merge of ${label} was interrupted: ${run.stderr?.toString("utf8").trim() || "no message"}` };
  }
  return { content: readFileSync(oursPath, "utf8"), conflicts: code };
}

// src/entrypoints/machinery/lib/release-files.ts
import { existsSync as existsSync2, mkdtempSync as mkdtempSync2, readFileSync as readFileSync3, readdirSync, rmSync as rmSync2 } from "fs";
import { tmpdir as tmpdir2 } from "os";
import { join as join2 } from "path";

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
function log2(message) {
  console.error(`[release-files] ${message}`);
}
var UPSTREAM_REPOSITORY = "yuma-seno/atomaton";
var ASSET_NAME = "atomaton-delivery.zip";
function releaseFiles(tag) {
  const dir = mkdtempSync2(join2(tmpdir2(), "atomaton-release-"));
  try {
    const downloaded = ghRead("release", "download", tag, "-R", UPSTREAM_REPOSITORY, "-p", ASSET_NAME, "-D", dir);
    if (downloaded.code !== 0) {
      return { why: `could not download ${tag}'s ${ASSET_NAME}: ${downloaded.stderr.trim() || `gh exited ${downloaded.code}`}` };
    }
    const asset = join2(dir, ASSET_NAME);
    if (!existsSync2(asset)) {
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
      rmSync2(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 });
    } catch {}
  }
}
function readTreeFiles(paths) {
  const files = new Map;
  for (const path of paths) {
    const onDisk = machineryPath(path);
    if (!existsSync2(onDisk))
      continue;
    try {
      files.set(path, readFileSync3(onDisk, "utf8"));
    } catch {}
  }
  return files;
}
function treePaths() {
  const root = machineryPath(".github");
  if (!existsSync2(root))
    return [];
  const found = [];
  const walk = (dir, prefix) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const here = `${prefix}/${entry.name}`;
      if (entry.isDirectory())
        walk(join2(dir, entry.name), here);
      else
        found.push(here);
    }
  };
  walk(root, ".github");
  return found.sort();
}
function readLocalManifest() {
  const path = machineryPath(RELEASE_MANIFEST);
  if (!existsSync2(path)) {
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
    log2(`WARN could not read ${UPSTREAM_REPOSITORY}'s latest release: ${result.stderr.trim() || `gh exited ${result.code}`}`);
    return null;
  }
  const tag = result.stdout.trim();
  return tag === "" ? null : tag;
}

// src/entrypoints/machinery/update_atomaton.ts
var ref = defineScript(import.meta.url);
var COMMIT_NAME = "atomaton[bot]";
var COMMIT_EMAIL = "atomaton[bot]@users.noreply.github.com";
function updateBranchFor(tag) {
  return `atomaton/update-${tag}`;
}
function updateTitle(tag) {
  return `Take Atomaton ${tag} onto this tree`;
}
var MAX_LISTED = 15;
function updateBody(report) {
  const { plan } = report;
  const lines = [
    "<!-- atomaton:update -->",
    verdictSentence(plan.base, plan.target, report.conflicts.length, plan.kept.length),
    "",
    "## The plan",
    "",
    "| | |",
    "| --- | --- |",
    `| Files updated (untouched here) | ${plan.take.length} |`,
    `| Files added | ${plan.add.length} |`,
    `| Files removed (unmodified here) | ${plan.delete.length} |`,
    `| Files changed on **both** sides | ${plan.merge.length} |`,
    `| Resolved by the merge itself | ${report.merged.length} |`,
    `| Left with conflict markers | ${report.conflicts.length} |`,
    `| Paths the plan will not touch | ${plan.kept.length} |`,
    ""
  ];
  const list = (title, paths) => {
    if (paths.length === 0)
      return;
    lines.push(title, "", ...paths.slice(0, MAX_LISTED).map((path) => `- \`${path}\``));
    if (paths.length > MAX_LISTED)
      lines.push(`- \u2026and ${paths.length - MAX_LISTED} more`);
    lines.push("");
  };
  list("Conflicted \u2014 resolve here, then the checks finish the job:", report.conflicts);
  list("Merged cleanly by the machinery:", report.merged);
  list("Left alone for a person to look at:", plan.kept.map((entry) => entry.path));
  if (report.leftovers.length > 0) {
    lines.push("## Files upstream no longer ships", "", "Reported only \u2014 the manifest cannot tell these from files the adopter wrote,", "so nothing here deletes them on a guess:", "", ...report.leftovers.slice(0, MAX_LISTED).map((path) => `- \`${path}\``), ...report.leftovers.length > MAX_LISTED ? [`- \u2026and ${report.leftovers.length - MAX_LISTED} more`] : [], "");
  }
  lines.push("## What happens next", "", report.conflicts.length > 0 ? "This pull request does not merge on its own: the conflicted files above carry markers naming both sides. Comment on this pull request and ask an agent to resolve them \u2014 the checks will run on the resolved tree, and only then can this merge." : plan.kept.length > 0 ? "This pull request does not merge itself: the kept paths above are the plan declining to touch them without a person. Read them \u2014 each is a place this repository's words and the release disagree \u2014 and merge by hand when they read right, or ask an agent there to help." : "The repository's checks run on this pull request; when they pass, the update merges it and closes the notice.", "");
  return `${lines.join(`
`).trimEnd()}
`;
}
function stopComment(report, prNumber) {
  return [
    LLM_CONTEXT_TAG.write("exclude"),
    `Taking ${report.plan.target} stopped at a pull request: ${report.conflicts.length} file(s) changed on both sides.`,
    "",
    ...report.conflicts.map((path) => `- \`${path}\``),
    "",
    `The plan is applied on #${prNumber}, where the conflicts carry markers naming both sides. ` + "Comment on that pull request and ask an agent to resolve them \u2014 nothing merges until someone does."
  ].join(`
`);
}
function keptComment(report, prNumber) {
  return [
    LLM_CONTEXT_TAG.write("exclude"),
    `Taking ${report.plan.target} stopped for a look: ${report.plan.kept.length} path(s) the plan will not touch without a person.`,
    "",
    ...report.plan.kept.map((entry) => `- \`${entry.path}\` (${entry.upstream})`),
    "",
    `The plan is applied on #${prNumber}, which does not merge itself. Read the kept paths there \u2014 ` + "each is this repository's words or files against a release that moved elsewhere \u2014 and merge by hand when they read right, " + "or comment on that pull request and ask an agent to help."
  ].join(`
`);
}
function doneComment(plan, prNumber, merging) {
  return [
    LLM_CONTEXT_TAG.write("exclude"),
    `Taking ${plan.target} onto ${plan.base} is applied on #${prNumber}.`,
    merging ? "The repository's checks passed there, and the merge is made; the merge's push starts the deploy." : "The repository's checks run on it; once they pass, it can merge."
  ].join(`
`);
}
function log3(message) {
  console.error(`[update] ${message}`);
}
function git(args) {
  const run = Bun.spawnSync({ cmd: ["git", ...args], stdout: "pipe", stderr: "pipe" });
  return {
    code: run.exitCode ?? 1,
    stdout: run.stdout?.toString("utf8") ?? "",
    stderr: run.stderr?.toString("utf8") ?? ""
  };
}
function gitOrFail(args, what) {
  const result = git(args);
  if (result.code !== 0) {
    return { fail: `could not ${what}: ${result.stderr.trim() || `git exited ${result.code}`}` };
  }
  return { stdout: result.stdout };
}
function pushArgs(url, token, branch) {
  const github = /^https:\/\/github\.com\/(.+?)(?:\.git)?$/.exec(url);
  if (!github)
    return ["push", "--force", url, `HEAD:refs/heads/${branch}`];
  return [
    "-c",
    "http.extraheader=",
    "-c",
    "http.https://github.com/.extraheader=",
    "push",
    "--force",
    `https://x-access-token:${token}@github.com/${github[1]}`,
    `HEAD:refs/heads/${branch}`
  ];
}
function pushBranch(branch, token) {
  const origin = gitOrFail(["remote", "get-url", "origin"], "read the origin remote");
  if ("fail" in origin)
    return origin;
  const pushed = gitOrFail(pushArgs(origin.stdout.trim(), token, branch), `push ${branch}`);
  return "fail" in pushed ? pushed : { ok: true };
}
function defaultBranch(repo) {
  const result = ghRead("api", `repos/${repo}`, "--jq", ".default_branch");
  if (result.code !== 0) {
    log3(`WARN could not read ${repo}'s default branch: ${result.stderr.trim() || `gh exited ${result.code}`}`);
    return null;
  }
  const branch = result.stdout.trim();
  return branch === "" ? null : branch;
}
function openPrForHead(repo, branch) {
  const result = ghRead("pr", "list", "--repo", repo, "--head", branch, "--state", "open", "--json", "number");
  if (result.code !== 0)
    return 0;
  try {
    const parsed = JSON.parse(result.stdout || "[]");
    const number = Number(parsed[0]?.number ?? 0);
    return Number.isInteger(number) && number > 0 ? number : 0;
  } catch {
    return 0;
  }
}
function main() {
  const { values } = parseArgs({
    args: Bun.argv.slice(2),
    options: {
      repo: { type: "string" },
      source: { type: "string" },
      tag: { type: "string" }
    }
  });
  const repo = (values.repo ?? process.env.GITHUB_REPOSITORY ?? "").trim();
  if (!repo) {
    console.error("usage: update_atomaton.ts --repo OWNER/REPO [--source N] [--tag vX.Y.Z]");
    process.exit(2);
  }
  const source = values.source !== undefined && values.source !== "" ? Number(values.source) : 0;
  const token = process.env.ATOMATON_UPDATE_TOKEN?.trim() ?? "";
  if (!token) {
    console.error("::error::ATOMATON_UPDATE_TOKEN is not set. The update pushes a branch and asks for a merge, " + "which this workflow's own token may not do: a push from it would run none of the checks the " + "merge is meant to gate. Set the repository secret ATOMATON_UPDATE_TOKEN to a classic PAT " + "with repo and workflow scope, or a fine-grained token with Contents and Pull requests read " + "and write and Workflows write \u2014 the release ships workflows, and a push that updates one is " + "refused without it. Then run this again. Nothing has been changed.");
    process.exit(1);
  }
  const local = readLocalManifest();
  if (!("manifest" in local)) {
    console.error(`::error::${local.why}. An update cannot be applied to a tree that cannot say what it is on.`);
    process.exit(1);
  }
  const targetTag = (values.tag ?? "").trim() || latestReleaseTag();
  if (targetTag === null) {
    console.error(`::error::could not read ${UPSTREAM_REPOSITORY}'s latest release, so there is nothing to take. ` + "A failed read is not an answer of 'up to date'.");
    process.exit(1);
  }
  if (targetTag === local.manifest.version) {
    log3(`${local.manifest.version} is the release this tree is on; nothing to take`);
    if (source > 0) {
      const said = gh("issue", "comment", String(source), "--repo", repo, "--body", `${LLM_CONTEXT_TAG.write("exclude")}
This tree is already on ${targetTag}, which is the release asked for. Nothing to do.`);
      if (said.code !== 0)
        log3(`WARN could not answer in #${source}: ${said.stderr.trim()}`);
    }
    return;
  }
  const status = gitOrFail(["status", "--porcelain"], "read the working tree's status");
  if ("fail" in status) {
    console.error(`::error::${status.fail}. The update applies to a checkout, and this run is not standing in one it can trust.`);
    process.exit(1);
  }
  if (status.stdout.trim() !== "") {
    console.error("::error::the working tree is not clean. The update writes whole files and pushes a branch; " + "uncommitted work would be mixed into its commit. Run it from a clean checkout.");
    process.exit(1);
  }
  const baseRelease = releaseFiles(local.manifest.version);
  if ("why" in baseRelease) {
    console.error(`::error::this tree's own release (${local.manifest.version}) could not be read: ${baseRelease.why}`);
    process.exit(1);
  }
  const targetRelease = releaseFiles(targetTag);
  if ("why" in targetRelease) {
    console.error(`::error::the target release (${targetTag}) could not be read: ${targetRelease.why}`);
    process.exit(1);
  }
  const paths = new Set([...baseRelease.files.files.keys(), ...targetRelease.files.files.keys()]);
  const tree = readTreeFiles(paths);
  const plan = planUpdate(local.manifest.version, targetTag, baseRelease.files.files, targetRelease.files.files, tree);
  if (plan.take.length + plan.add.length + plan.delete.length + plan.merge.length === 0) {
    log3(`${targetTag} changes nothing here; nothing to apply`);
    if (source > 0) {
      const said = gh("issue", "comment", String(source), "--repo", repo, "--body", `${LLM_CONTEXT_TAG.write("exclude")}
${targetTag} differs from ${local.manifest.version} by name only: applying it would change nothing on this tree.`);
      if (said.code !== 0)
        log3(`WARN could not answer in #${source}: ${said.stderr.trim()}`);
    }
    return;
  }
  const leftovers = leftoversOf(targetRelease.files.manifest, treePaths());
  const applied = applyPlan(machineryPath("."), plan, baseRelease.files.files, targetRelease.files.files, tree);
  if ("why" in applied) {
    console.error(`::error::the plan could not be applied: ${applied.why}. The tree is left as it was before this run.`);
    process.exit(1);
  }
  const { conflicts, merged } = applied.result;
  log3(`applied: ${applied.result.taken.length} taken, ${applied.result.added.length} added, ` + `${applied.result.removed.length} removed, ${merged.length} merged, ${conflicts.length} conflicted`);
  const branch = updateBranchFor(targetTag);
  const branched = gitOrFail(["switch", "-C", branch], `create the branch ${branch}`);
  if ("fail" in branched) {
    console.error(`::error::${branched.fail}. The tree holds the applied plan but nothing has been committed.`);
    process.exit(1);
  }
  const staged = gitOrFail(["add", "-A"], "stage the applied plan");
  if ("fail" in staged) {
    console.error(`::error::${staged.fail}. The tree holds the applied plan but nothing has been committed.`);
    process.exit(1);
  }
  const committed = gitOrFail([
    "-c",
    `user.name=${COMMIT_NAME}`,
    "-c",
    `user.email=${COMMIT_EMAIL}`,
    "commit",
    "-m",
    updateTitle(targetTag),
    "-m",
    conflicts.length === 0 ? `Applied by the update machinery from ${plan.base}. Nothing here collided.` : `Applied by the update machinery from ${plan.base}. ${conflicts.length} file(s) carry conflict markers for a person or an agent to resolve on the pull request.`
  ], "commit the applied plan");
  if ("fail" in committed) {
    console.error(`::error::${committed.fail}. The tree holds the applied plan but nothing has been committed.`);
    process.exit(1);
  }
  const pushed = pushBranch(branch, token);
  if ("fail" in pushed) {
    console.error(`::error::${pushed.fail}. The commit exists locally as ${branch}; nothing has been opened upstream.`);
    process.exit(1);
  }
  const base = defaultBranch(repo);
  if (base === null) {
    console.error(`::error::could not read ${repo}'s default branch, so no pull request can be opened against it.`);
    process.exit(1);
  }
  const report = { plan, conflicts, merged, leftovers };
  const existing = openPrForHead(repo, branch);
  let prNumber;
  if (existing > 0) {
    const edited = gh("pr", "edit", String(existing), "--repo", repo, "--title", updateTitle(targetTag), "--body", updateBody(report));
    if (edited.code !== 0) {
      console.error(`::error::could not update the pull request #${existing}: ${edited.stderr.trim() || `gh exited ${edited.code}`}`);
      process.exit(1);
    }
    prNumber = existing;
    log3(`updated pull request #${existing}`);
  } else {
    const created = ghAs(token, "pr", "create", "--repo", repo, "--base", base, "--head", branch, "--title", updateTitle(targetTag), "--body", updateBody(report));
    if (created.code !== 0) {
      console.error(`::error::could not open the pull request: ${created.stderr.trim() || `gh exited ${created.code}`}`);
      process.exit(1);
    }
    const number = /\/pull\/(\d+)/.exec(created.stdout.trim())?.[1];
    if (number === undefined) {
      console.error(`::error::the pull request was opened (${created.stdout.trim()}) but its number could not be read, ` + "so the merge request and the notices cannot reference it.");
      process.exit(1);
    }
    prNumber = Number(number);
    log3(`opened pull request #${prNumber}`);
  }
  const notice = findOpenNotice(repo);
  const selfMerging = conflicts.length === 0 && plan.kept.length === 0;
  let merging = false;
  if (selfMerging) {
    let seen = "";
    for (let attempt = 0;attempt < 90; attempt++) {
      const rollup = readPullRequest(repo, prNumber, "statusCheckRollup");
      seen = rollup.known ? String((rollup.value.statusCheckRollup ?? []).filter((check) => check.name === "atomaton-check").length) : "0";
      if (seen !== "0")
        break;
      Bun.sleepSync(1e4);
    }
    if (seen === "0" || seen === "") {
      log3(`WARN the required check never registered on #${prNumber} in fifteen minutes \u2014 the pull request stands, ` + "and merges when a person or a run asks again.");
    } else if (gh("pr", "checks", String(prNumber), "--repo", repo, "--watch", "--fail-fast", "--interval", "10").code !== 0) {
      log3(`WARN the checks on #${prNumber} did not pass \u2014 the pull request stands with them.`);
    } else {
      const requested = ghAs(token, "pr", "merge", String(prNumber), "--repo", repo, "--merge");
      merging = requested.code === 0;
      if (!merging) {
        log3(`WARN the merge could not be made on #${prNumber}: ${requested.stderr.trim() || `gh exited ${requested.code}`}`);
      }
    }
  }
  if (!selfMerging && plan.kept.length > 0) {
    const said = keptComment(report, prNumber);
    if (notice !== null && notice !== 0) {
      const told = gh("issue", "comment", String(notice), "--repo", repo, "--body", said);
      if (told.code !== 0)
        log3(`WARN could not report to the notice #${notice}: ${told.stderr.trim()}`);
    }
    if (source > 0 && source !== notice) {
      const told = gh("issue", "comment", String(source), "--repo", repo, "--body", said);
      if (told.code !== 0)
        log3(`WARN could not answer in #${source}: ${told.stderr.trim()}`);
    }
    log3(`update to ${targetTag} is applied on #${prNumber}, held for a person: ${plan.kept.length} kept path(s)`);
    return;
  }
  if (conflicts.length > 0) {
    const said = stopComment(report, prNumber);
    if (notice !== null && notice !== 0) {
      const told = gh("issue", "comment", String(notice), "--repo", repo, "--body", said);
      if (told.code !== 0)
        log3(`WARN could not report to the notice #${notice}: ${told.stderr.trim()}`);
    }
    if (source > 0 && source !== notice) {
      const told = gh("issue", "comment", String(source), "--repo", repo, "--body", said);
      if (told.code !== 0)
        log3(`WARN could not answer in #${source}: ${told.stderr.trim()}`);
    }
    log3(`stopped at pull request #${prNumber}: ${conflicts.length} conflicted file(s)`);
    return;
  }
  if (merging && notice !== null && notice !== 0) {
    const closed = gh("issue", "close", String(notice), "--repo", repo, "--comment", `${NOTICE_MARKER}
${LLM_CONTEXT_TAG.write("exclude")}
${doneComment(plan, prNumber, merging)}`);
    if (closed.code !== 0)
      log3(`WARN could not close the notice #${notice}: ${closed.stderr.trim()}`);
  }
  if (source > 0 && source !== notice) {
    const said = gh("issue", "comment", String(source), "--repo", repo, "--body", doneComment(plan, prNumber, merging));
    if (said.code !== 0)
      log3(`WARN could not answer in #${source}: ${said.stderr.trim()}`);
  }
  log3(`update to ${targetTag} is on #${prNumber}${merging ? ", merged after the repository's checks passed" : ""}`);
}
if (import.meta.main)
  main();
export {
  doneComment,
  keptComment,
  pushArgs,
  ref,
  stopComment,
  updateBody,
  updateBranchFor,
  updateTitle
};
