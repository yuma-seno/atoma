#!/usr/bin/env python3
from __future__ import annotations

import os
import re
import sys

COMMAND_RE = re.compile(r"^/([a-z][a-z0-9-]*)")


def parse_agent(body: str) -> str:
    first_line = body.splitlines()[0].strip() if body else ""
    match = COMMAND_RE.match(first_line)
    return match.group(1) if match else ""


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