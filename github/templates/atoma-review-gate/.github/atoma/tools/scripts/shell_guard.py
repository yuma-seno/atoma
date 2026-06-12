#!/usr/bin/env python3
import json
import re
import sys

BLOCKED = [
    (r'\bgit\s+push\b', "git push is disabled. Use create_pr command to publish changes."),
    (r'\bgit\s+fetch\b', "git fetch is disabled."),
    (r'\bgit\s+pull\b', "git pull is disabled."),
    (r'\bgit\s+remote\b', "git remote is disabled."),
    (r'\bgh\b', "gh CLI is disabled. Use the provided scripts for GitHub interaction."),
    (r'\bcurl\b', "curl is disabled."),
    (r'\bwget\b', "wget is disabled."),
    (r'\bssh\b', "ssh is disabled."),
    (r'\bscp\b', "scp is disabled."),
    (r'\brsync\b', "rsync is disabled."),
    (r'\bpython3?\s.*-[cC]\b', "python -c  command execution is disabled."),
    (r'\bruby\s.*-[eE]\b', "ruby -e  command execution is disabled."),
    (r'\bperl\s.*-[eE]\b', "perl -e  command execution is disabled."),
    (r'\bnode\s.*-[eE]\b', "node -e  command execution is disabled."),
    (r'\bpython3?\s+<<', "python heredoc  command execution is disabled."),
    (r'\bruby\s+<<', "ruby heredoc  command execution is disabled."),
    (r'\bperl\s+<<', "perl heredoc  command execution is disabled."),
    (r'\bnode\s+<<', "node heredoc  command execution is disabled."),
    (r'\bbase64\b.*\|\s*(?:sh|bash|zsh|dash)', "base64  decoded shell execution is disabled."),
    (r'\bxxd\b.*\|\s*(?:sh|bash|zsh|dash)', "Binary  decoded shell execution is disabled."),
    (r'(?:^|\s|\||\;)\beval\b', "eval is disabled."),
    (r'(?:^|\s|\||\;)\bexec\b', "exec is disabled."),
    (r'(?:^|\s|\;)\bsource\b', "source is disabled."),
    (r'(?:^|\s|\;)\.\s', "source (.) is disabled."),
    (r'\bsh\s+(?:-[a-zA-Z]+\s+)*-c\b', "sh -c  command execution is disabled."),
    (r'\bbash\s+(?:-[a-zA-Z]+\s+)*-c\b', "bash -c  command execution is disabled."),
    (r'\bzsh\s+(?:-[a-zA-Z]+\s+)*-c\b', "zsh -c  command execution is disabled."),
    (r'\bdash\s+(?:-[a-zA-Z]+\s+)*-c\b', "dash -c  command execution is disabled."),
]
COMPILED = [(re.compile(pattern, re.MULTILINE), reason) for pattern, reason in BLOCKED]


def check_command(command: str):
    for pattern, reason in COMPILED:
        if pattern.search(command):
            return False, reason
    return True, ""


def main():
    try:
        data = json.load(sys.stdin)
    except Exception:
        print(json.dumps({"allow": False, "reason": "shell_guard: failed to parse hook input"}))
        return

    arguments = data.get("arguments", {})
    command = arguments.get("command") or arguments.get("cmd") or arguments.get("shell") or ""
    allow, reason = check_command(command)
    if allow:
        print(json.dumps({"allow": True}))
    else:
        print(json.dumps({"allow": False, "reason": reason}))


if __name__ == "__main__":
    main()
