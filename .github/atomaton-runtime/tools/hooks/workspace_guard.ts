#!/usr/bin/env bun
// @bun

// src/entrypoints/tools/hooks/workspace_guard.ts
import { readdirSync, statSync } from "fs";
import { join } from "path";

// src/domain/work/workspace.ts
var WORKSPACE_PATH = "/tmp/atomaton-workspace";
var WORKSPACE_SENTENCE = `Anything under ${WORKSPACE_PATH} survives into the next run on this issue and is shared with the other ` + `agents working on it. Nothing else outside the repository survives. Put notes, scratch scripts and ` + `intermediate output there rather than in the repository, where they would be committed as part of the work.`;

// src/domain/work/workspace-size.ts
var MAX_TOTAL_BYTES = 5 * 1024 * 1024;
var MAX_FILE_BYTES = 1024 * 1024;
var MAX_FILES = 200;
var NAMED = 3;
function readable(bytes) {
  if (bytes >= 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  if (bytes >= 1024)
    return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} bytes`;
}
function overLimitNotice(usage, path) {
  const reasons = [];
  if (usage.bytes > MAX_TOTAL_BYTES) {
    reasons.push(`it holds ${readable(usage.bytes)}, over the ${readable(MAX_TOTAL_BYTES)} limit`);
  }
  if (usage.files > MAX_FILES) {
    reasons.push(`it holds ${usage.files} files, over the limit of ${MAX_FILES}`);
  }
  const huge = usage.largest.filter((f) => f.bytes > MAX_FILE_BYTES);
  if (huge.length > 0) {
    const subject = huge.length === 1 ? "one file is" : `${huge.length} files are`;
    reasons.push(`${subject} over the ${readable(MAX_FILE_BYTES)} single-file limit`);
  }
  if (reasons.length === 0)
    return;
  const named = usage.largest.slice(0, NAMED).map((f) => `  ${readable(f.bytes).padStart(9)}  ${f.path}`).join(`
`);
  return [
    `--- ${path} is over its limit; this is not part of the answer above ---`,
    `Nothing you have put there will reach the next run on this issue: ${reasons.join(", and ")}.`,
    "",
    "The largest are:",
    named,
    "",
    `Delete what the next run does not need. ${path} is an ordinary directory, so the ` + "tools you already use remove a file there. Build output, dependency trees and logs do not " + "belong in it; notes, scratch scripts and intermediate results do. If what is there is only " + "useful to you now, leave it and let it go when this run ends."
  ].join(`
`);
}

// src/entrypoints/tools/hooks/workspace_guard.ts
var WALK_CEILING = MAX_FILES + 1;
var KEEP_LARGEST = 5;
function measure(root) {
  const usage = { bytes: 0, files: 0, largest: [] };
  const stack = [root];
  while (stack.length > 0 && usage.files < WALK_CEILING) {
    const dir = stack.pop();
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (usage.files >= WALK_CEILING)
        break;
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        stack.push(path);
        continue;
      }
      if (!entry.isFile())
        continue;
      let size;
      try {
        size = statSync(path).size;
      } catch {
        continue;
      }
      usage.files += 1;
      usage.bytes += size;
      usage.largest.push({ path, bytes: size });
      if (usage.largest.length > KEEP_LARGEST * 4) {
        usage.largest.sort((a, b) => b.bytes - a.bytes);
        usage.largest.length = KEEP_LARGEST;
      }
    }
  }
  usage.largest.sort((a, b) => b.bytes - a.bytes);
  usage.largest.length = Math.min(usage.largest.length, KEEP_LARGEST);
  return usage;
}
async function main() {
  try {
    await new Response(Bun.stdin.stream()).text();
  } catch {}
  try {
    const notice = overLimitNotice(measure(WORKSPACE_PATH), WORKSPACE_PATH);
    if (notice !== undefined) {
      console.error(`[workspace_guard] the workspace is over its limit; telling the agent`);
      console.log(JSON.stringify({ notice }));
    }
  } catch (error) {
    console.error(`[workspace_guard] could not measure ${WORKSPACE_PATH}: ${error.message}`);
  }
}
if (import.meta.main)
  await main();
