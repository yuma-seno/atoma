//! Keep this process's memory and environment out of reach of the tools it
//! spawns.
//!
//! # Why
//!
//! MCP servers are children of this process and run as the same OS user. On
//! Linux that means, by default, that a tool server can read
//! `/proc/<atoma>/environ` and — depending on the host — `/proc/<atoma>/mem`.
//! Since this process holds the provider API key for the whole run, and will
//! hold every credential a project declares, "the tools cannot see the
//! credentials" is otherwise a claim with nothing behind it.
//!
//! Measured on a GitHub Actions `ubuntu-24.04` runner, all as the same user:
//!
//! | Read                          | Result   |
//! |-------------------------------|----------|
//! | a parent's `/proc/…/environ`  | allowed  |
//! | a parent's `/proc/…/mem`      | refused  |
//! | another user's `environ`      | refused  |
//!
//! So the boundary is not "same user or not", it is "in an environment block or
//! only in the heap". `environ` needs only `PTRACE_MODE_READ`, which Yama
//! permits for the same user; `mem` needs attach, which `ptrace_scope=1`
//! refuses for a non-descendant.
//!
//! # Why not rely on `ptrace_scope`
//!
//! Because it is the host's setting, not ours. `0` allows attaching to any
//! process of the same user regardless of ancestry, a kernel built without Yama
//! has no such file at all, and the sysctl is not namespaced — a container
//! inherits whatever the host chose. Self-hosted runners are an intended
//! deployment, so depending on a value nobody here controls would make the
//! guarantee conditional on luck.
//!
//! `PR_SET_DUMPABLE(0)` does not depend on it. A non-dumpable process has its
//! sensitive `/proc/<pid>/` entries owned by root, so reads of `environ` and
//! `mem` by the same user are refused and attaching requires `CAP_SYS_PTRACE`.
//!
//! # What this is not
//!
//! Not a substitute for keeping credentials out of environment blocks in the
//! first place. This protects THIS process; the workflow step that invokes it is
//! a different process that cannot be made non-dumpable, and a child holds
//! whatever it was given. And `unsetenv` cannot help either way: `/proc/<pid>/
//! environ` reflects the block placed on the stack at `execve`, which glibc's
//! `setenv`/`unsetenv` do not rewrite — so a value that was ever in this
//! process's environment stays visible there for its lifetime. Only never
//! putting it there works.
//!
//! # Cost
//!
//! One, and it is a real one: no debugger, `strace` or `perf` can attach to a
//! running `atoma` as the same user any more — those need `sudo`. Hence
//! `--no-process-protection`, which is for that and nothing else.
//!
//! Core dumps are also lost, which for a process holding credentials is a
//! feature. `/proc/<pid>/status` and `cmdline` stay readable, so ordinary
//! process monitoring is unaffected.

/// Make this process unreadable to other processes of the same user.
///
/// Verifies the result rather than assuming it: `prctl` reports success on
/// nothing that matters here, so the flag is read back. A failure warns and
/// continues — refusing to run because a hardening step did not take would trade
/// a weaker guarantee for no run at all, and the log is what says which one this
/// is.
#[cfg(target_os = "linux")]
pub fn harden_against_same_user_inspection() {
    // SAFETY: `prctl` with PR_SET_DUMPABLE takes an int and touches nothing this
    // program owns. The call is always sound; only its effect is interesting.
    let set = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) };
    // SAFETY: as above, and PR_GET_DUMPABLE returns the flag as the exit value.
    let now = unsafe { libc::prctl(libc::PR_GET_DUMPABLE) };

    if set == 0 && now == 0 {
        tracing::debug!("process protection: this process is not dumpable; tool servers cannot read its memory or environment");
        return;
    }

    tracing::warn!(
        set_result = set,
        dumpable = now,
        "process protection did not take effect: a tool server running as this user may be able to read this process's environment and memory. Credentials held here are not confined on this host."
    );
}

/// Nothing to do: the exposure this addresses is `/proc`, which is Linux's.
///
/// Deliberately silent rather than warning. On macOS a developer running `atoma`
/// by hand would get a warning on every invocation about a mechanism that has no
/// equivalent to reach for, which teaches people to ignore warnings.
#[cfg(not(target_os = "linux"))]
pub fn harden_against_same_user_inspection() {}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// The property the design depends on, asserted rather than assumed.
    ///
    /// Runs in-process, so it also proves the call is reachable and does not
    /// abort. Reading `/proc/self/environ` still works afterwards -- a process
    /// can always read itself -- which is why this checks the flag rather than a
    /// read.
    #[test]
    fn the_process_becomes_non_dumpable() {
        // SAFETY: see `harden_against_same_user_inspection`.
        let before = unsafe { libc::prctl(libc::PR_GET_DUMPABLE) };
        assert_eq!(before, 1, "a normal process starts dumpable");

        harden_against_same_user_inspection();

        // SAFETY: as above.
        let after = unsafe { libc::prctl(libc::PR_GET_DUMPABLE) };
        assert_eq!(after, 0, "PR_SET_DUMPABLE(0) must take effect");
    }
}
