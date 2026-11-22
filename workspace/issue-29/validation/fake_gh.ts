#!/usr/bin/env bun
// A stand-in for `gh`, for running plan_deploy.ts with no network and no token.
//
// plan_deploy.ts runs `[process.execPath, ATOMATON_FAKE_GH, ...args]` when
// ATOMATON_FAKE_GH is set, so this file receives the gh arguments and must print
// what gh would have printed *after* applying the --jq the caller passed.
//
// The answers are the shapes this repository actually has: a pull_request rule on
// the default branch, one existing tag (v0.4.8), and a merge that carries no tag.

const args = process.argv.slice(2);
const joined = args.join(" ");
const say = (text: string) => process.stdout.write(`${text}\n`);

if (joined.includes("/rules/branches/")) {
  say(
    JSON.stringify([
      { type: "deletion", parameters: null },
      { type: "pull_request", parameters: { required_approving_review_count: 0 } },
      {
        type: "required_status_checks",
        parameters: { required_status_checks: [{ context: "atomaton-check" }] },
      },
    ]),
  );
} else if (joined.includes("matching-refs/tags")) {
  say("refs/tags/v0.4.8 tagsha0f048");
} else if (joined.includes("/compare/") && joined.includes(".status")) {
  // Is the tag's commit on the branch? It is the branch head.
  say("identical");
} else if (joined.includes("/compare/") && joined.includes(".commits[].sha")) {
  // The commits a push to main brought in. None of them carries a tag, which is
  // the state before release-tag runs and pushes one.
  say("commit5a1");
} else {
  process.stderr.write(`fake gh: no answer for: ${joined}\n`);
  process.exit(1);
}
