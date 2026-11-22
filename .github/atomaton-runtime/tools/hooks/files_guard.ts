#!/usr/bin/env bun
// @bun

// src/domain/work/search-streak.ts
var MAX_SEARCHES_WITHOUT_OPENING = 15;
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

// src/entrypoints/tools/hooks/files_guard.ts
var SEARCHES = new Set(["grep", "glob", "list"]);
function allow() {
  console.log(JSON.stringify({ allow: true }));
}
async function main() {
  let tool = "";
  try {
    const raw = await new Response(Bun.stdin.stream()).text();
    tool = String(JSON.parse(raw).tool ?? "");
  } catch {
    allow();
    return;
  }
  if (!SEARCHES.has(tool.trim().replace(/^.*__/, ""))) {
    allow();
    return;
  }
  const file = streakFile();
  const streak = nextStreak(readStreak(file), "search");
  writeStreak(file, streak);
  const refusal = refusalReason(streak);
  if (refusal) {
    console.log(JSON.stringify({ allow: false, reason: refusal }));
    return;
  }
  allow();
}
if (import.meta.main)
  await main();
