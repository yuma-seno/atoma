#!/usr/bin/env bun
// @bun

// src/entrypoints/tools/hooks/note_file_opened.ts
import { writeFileSync } from "fs";

// src/domain/work/search-streak.ts
var TOOLS_THAT_OPEN = /(^|__)read$/;
function toolOpens(tool) {
  return TOOLS_THAT_OPEN.test(tool.trim());
}

// src/entrypoints/tools/lib/search-streak-file.ts
function streakFile() {
  const opsLog = process.env.ATOMATON_OPS_LOG;
  if (!opsLog)
    return;
  const dir = opsLog.replace(/[/\\][^/\\]*$/, "");
  return dir === opsLog ? undefined : `${dir}/search-streak`;
}

// src/entrypoints/tools/hooks/note_file_opened.ts
async function main() {
  let tool = "";
  try {
    const raw = await new Response(Bun.stdin.stream()).text();
    tool = String(JSON.parse(raw).tool ?? "");
  } catch {
    return;
  }
  if (!toolOpens(tool))
    return;
  const file = streakFile();
  if (!file)
    return;
  try {
    writeFileSync(file, "0");
  } catch {}
}
if (import.meta.main)
  await main();
