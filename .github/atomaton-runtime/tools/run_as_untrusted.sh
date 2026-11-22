#!/usr/bin/env bash
# run_as_untrusted.sh — run one tool server as atomaton-untrusted.
#
# The runner starts `atoma` as atomaton-tools, and atoma starts every server as
# its own child. This is the one line that puts the shell server, and every server
# a project adds, on the other uid.
#
# ## Why sudo rather than a second atoma
#
# The drop has to happen per server and after atoma has read the credentials file:
# atoma reads that file as atomaton-tools, then deletes it, and only then starts
# servers. A whole second atoma would need a second file. `sudo -u` is one
# exec, and the rule that allows it is narrow: atomaton-tools may run commands as
# atomaton-untrusted, and as nobody else.
#
# ## Why `env` after sudo
#
# Ubuntu's sudoers sets `secure_path`, which replaces PATH for the command sudo
# starts. A server is named by a command rather than a path, so without this it
# would be looked up in a PATH it was not launched with. HOME is passed for the
# same reason: it is the runner's, deliberately, and not writable by either tool
# user.
#
# `-E` is the opposite direction: it keeps the environment atoma gave the server,
# which is how a third-party server receives the credential its own `env:` block
# named. The sudoers rule carries SETENV for exactly this. Preserving more than
# that would not be an escalation -- the child is the untrusted user, so nothing
# it is handed reaches the credentials of the class it just left.
#
# ## Where there is no second user
#
# `check-live-tools.sh` starts every server of a release candidate, and
# `atomaton-check` starts a pull request's, on a machine where the two users were
# never created. There the variable is unset and the command runs as whoever
# launched the check -- which is correct, because those jobs hold no credential
# for the wrapper to keep away from anything. The variable is set in the agent
# step's environment; a run that reaches a server without it is not a run this
# project starts.
set -euo pipefail

command_name="$1"
shift

# Resolve before any sudo, so a bare command is looked up in the PATH the caller
# passed down rather than in sudo's secure_path. An absolute path resolves to
# itself only when it is executable, which is the property that matters.
resolved=$(command -v "$command_name") || {
  echo "run_as_untrusted: ${command_name} was not found on PATH" >&2
  exit 127
}

if [ -z "${ATOMATON_UNTRUSTED_USER:-}" ]; then
  exec "$resolved" "$@"
fi

exec sudo -n -E -u "$ATOMATON_UNTRUSTED_USER" -- env HOME="$HOME" PATH="$PATH" "$resolved" "$@"
