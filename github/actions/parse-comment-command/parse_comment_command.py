#!/usr/bin/env python3
from __future__ import annotations

import os
import re
import sys

COMMAND_RE = re.compile(r"^/([a-z][a-z0-9-]*)")
# atoma internal dispatch: <!-- atoma:dispatch=AGENT -->
DISPATCH_RE = re.compile(r"<!--\s*atoma:dispatch\s*=\s*([a-z][a-z0-9-]*)\s*-->")


def parse_agent(body: str) -> str:
    if not body:
        return ""
    for line in body.splitlines():
        stripped = line.strip()
        # Try slash command first
        match = COMMAND_RE.match(stripped)
        if match:
            return match.group(1)
        # Try dispatch comment format
        match = DISPATCH_RE.match(stripped)
        if match:
            return match.group(1)
    return ""


def main() -> int:
    body = os.environ.get("ATOMA_COMMENT_BODY", "")
    agent = parse_agent(body)
    matched = "true" if agent else "false"

    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
      with open(github_output, "a", encoding="utf-8") as f:
          f.write(f"matched={matched}\n")
          f.write(f"agent={agent}\n")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())