//! When a run has stopped being work.
//!
//! # What this is not
//!
//! Not a progress metric, and not a cost ceiling. Every threshold below was measured
//! against 341 stored sessions and 5,746 tool calls, and the measurement threw out
//! more candidates than it kept:
//!
//! ```text
//!   candidate                            fires on   verdict
//!   any failures in a row (>=5)          9.4%       REJECTED -- 99% of failure runs
//!                                                   recover, median 21 successes after.
//!                                                   The failure rate's p75 is 33%:
//!                                                   failing constantly is how these
//!                                                   agents work.
//!   results already seen, in a row       2.6%       weak, and 0 on the run it was
//!                                                   proposed for
//!   the same call and the same answer    0.3%       KEPT
//!   a cycle inside the last six calls    0.0%       KEPT
//!   nothing coming back, in a row        0.0%       KEPT
//! ```
//!
//! A guard that never fires on healthy data is not a useless guard, it is a guard with
//! no false positives. The ones kept here are insurance: cheap, exact, and pointed at a
//! pathology that costs money if it ever happens.
//!
//! # What is deliberately absent
//!
//! **The expensive runs are not caught here, and cannot be.** Measured, the three most
//! expensive sessions in the store made 124-188 searches while opening almost nothing:
//! no repeats, no cycles, no echoes, new information nearly every call. They were
//! working hard and arriving nowhere, and nothing in the local shape of the call
//! sequence separates that from working hard and arriving.
//!
//! That is caught one layer out, by a hook that refuses a search when nothing found by
//! the previous ones has been opened -- which turns an unmeasurable waste into a
//! deterministic refusal, and a deterministic refusal is what
//! `MAX_IDENTICAL_TOOL_FAILURES` below already stops.
use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};

/// Identical calls failing in a row.
///
/// Measured: 2 sessions of 341 reach 3, none reaches 4. Nearly inert, which is correct
/// -- failure is not the signal. It stays because a call that fails the same way
/// forever is a true pathology, and because a refusal from a `before_tool` hook is a
/// failure that cannot change, so this is what stops an agent ignoring one.
pub const MAX_IDENTICAL_TOOL_FAILURES: u8 = 3;

/// The same call returning the same answer, in a row.
///
/// Measured: 1 session of 341 reaches 3, none reaches 4.
///
/// A tautology rather than a heuristic. The same arguments came back with the same
/// bytes, so the second call informed nobody of anything -- and unlike a repeat with a
/// *different* answer (66% of all repeats, and legitimate: the world moved) there is
/// nothing here that could have been learned.
pub const MAX_IDENTICAL_ANSWERS: u8 = 4;

/// Steps of a cycle, in a row.
///
/// A cycle is A, B, A, B: each call differs from the one before it, so a tracker that
/// remembers only the previous call is blind to it by construction. That was the shape
/// of the old tracker, and this is why it kept a single slot.
///
/// Measured: 2 sessions of 341 reach 3, none reaches 4.
pub const MAX_CYCLE_STEPS: u8 = 4;

/// How far back a repeat still counts as a cycle.
///
/// Six. Long enough for A,B,A,B and A,B,C,A,B,C, short enough that revisiting a file
/// after a detour is not mistaken for one.
const CYCLE_WINDOW: usize = 6;

/// Calls returning nothing, in a row.
///
/// Measured: no session of 341 reaches 8. Pure insurance -- an agent probing at
/// something that does not exist, over and over, learns nothing each time and there is
/// no bound on how long it can do that.
pub const MAX_EMPTY_RESULTS: u8 = 8;

/// A result short enough to hold nothing: an empty grep, an empty directory.
const EMPTY_RESULT_CHARS: usize = 2;

/// What one call did, as far as deciding whether the run is still working.
#[derive(Debug)]
pub enum CallOutcome<'a> {
    /// The tool answered. The text is what it answered with.
    Answered(&'a str),
    /// The tool failed, or its arguments could not be parsed.
    Failed,
}

fn hash_of(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Watches the call sequence for the shapes that mean the run has stopped being work.
///
/// One tracker per run. Every rule is a counter over *consecutive* calls, so a single
/// productive call clears whichever one was climbing -- which is why the thresholds can
/// be as low as they are without touching a healthy run.
#[derive(Default)]
pub struct LoopTracker {
    failing_signature: Option<String>,
    consecutive_failures: u8,

    answered_signature: Option<String>,
    answered_hash: Option<u64>,
    consecutive_identical_answers: u8,

    recent: VecDeque<String>,
    consecutive_cycle_steps: u8,

    consecutive_empty_results: u8,
}

impl LoopTracker {
    /// Record one call, and say whether the run should stop.
    ///
    /// `signature` identifies the call: the tool's name and its canonicalised
    /// arguments. `Some(reason)` is a sentence for the person who has to read it, not
    /// for a log grep.
    pub fn record(&mut self, signature: &str, outcome: CallOutcome<'_>) -> Option<String> {
        // Checked before the counters are updated for this call, because the cycle
        // window must not contain the call being judged.
        let cycled = self.recent.iter().any(|s| s == signature)
            && self.recent.back().map(String::as_str) != Some(signature);

        self.recent.push_back(signature.to_string());
        if self.recent.len() > CYCLE_WINDOW {
            self.recent.pop_front();
        }

        if cycled {
            self.consecutive_cycle_steps = self.consecutive_cycle_steps.saturating_add(1);
        } else {
            self.consecutive_cycle_steps = 0;
        }

        match outcome {
            CallOutcome::Failed => {
                if self.failing_signature.as_deref() == Some(signature) {
                    self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                } else {
                    self.failing_signature = Some(signature.to_string());
                    self.consecutive_failures = 1;
                }
                // A failure answers nothing, so neither the echo nor the emptiness
                // counter can be climbing through it.
                self.answered_signature = None;
                self.answered_hash = None;
                self.consecutive_identical_answers = 0;
                self.consecutive_empty_results = 0;
            }
            CallOutcome::Answered(text) => {
                self.failing_signature = None;
                self.consecutive_failures = 0;

                let hash = hash_of(text);
                let same_answer = self.answered_signature.as_deref() == Some(signature)
                    && self.answered_hash == Some(hash);
                self.consecutive_identical_answers = if same_answer {
                    self.consecutive_identical_answers.saturating_add(1)
                } else {
                    1
                };
                self.answered_signature = Some(signature.to_string());
                self.answered_hash = Some(hash);

                if text.trim().len() <= EMPTY_RESULT_CHARS {
                    self.consecutive_empty_results = self.consecutive_empty_results.saturating_add(1);
                } else {
                    self.consecutive_empty_results = 0;
                }
            }
        }

        self.verdict(signature)
    }

    /// Clear everything a successful, informative call would clear.
    ///
    /// For the caller that knows a call succeeded but has no text to offer -- an image,
    /// a tool whose whole answer is its `_meta`. Treated as productive, because it is:
    /// something came back that was not there before.
    pub fn record_informative(&mut self) {
        self.failing_signature = None;
        self.consecutive_failures = 0;
        self.answered_signature = None;
        self.answered_hash = None;
        self.consecutive_identical_answers = 0;
        self.consecutive_empty_results = 0;
    }

    fn verdict(&self, signature: &str) -> Option<String> {
        let tool = signature.split(':').next().unwrap_or(signature);

        if self.consecutive_failures >= MAX_IDENTICAL_TOOL_FAILURES {
            return Some(format!(
                "Aborting after {} identical failed calls to '{}'. Change the tool or arguments before retrying.",
                MAX_IDENTICAL_TOOL_FAILURES, tool,
            ));
        }
        if self.consecutive_identical_answers >= MAX_IDENTICAL_ANSWERS {
            return Some(format!(
                "Aborting: '{}' was called {} times in a row with the same arguments and returned the same answer every time. Nothing was learned; the run is going round.",
                tool, MAX_IDENTICAL_ANSWERS,
            ));
        }
        if self.consecutive_cycle_steps >= MAX_CYCLE_STEPS {
            return Some(format!(
                "Aborting: the last {} calls repeat a cycle of earlier ones, ending at '{}'. Each differs from the one before it, so this is a loop rather than a retry.",
                MAX_CYCLE_STEPS + 1, tool,
            ));
        }
        if self.consecutive_empty_results >= MAX_EMPTY_RESULTS {
            return Some(format!(
                "Aborting: the last {} calls returned nothing at all, most recently '{}'. Whatever is being looked for is not where it is being looked for.",
                MAX_EMPTY_RESULTS, tool,
            ));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok<'a>(text: &'a str) -> CallOutcome<'a> {
        CallOutcome::Answered(text)
    }

    #[test]
    fn identical_failures_reach_the_threshold() {
        let mut t = LoopTracker::default();
        assert!(t.record("shell:{\"x\":1}", CallOutcome::Failed).is_none());
        assert!(t.record("shell:{\"x\":1}", CallOutcome::Failed).is_none());
        let reason = t.record("shell:{\"x\":1}", CallOutcome::Failed).unwrap();
        assert!(reason.contains("identical failed calls"), "{reason}");
    }

    /// Measured: the failure rate's p75 is 33%, and 99% of failure runs recover with a
    /// median of 21 successes after. Failing and trying something else is how these
    /// agents work, and a guard that stopped it would stop the work.
    #[test]
    fn failing_and_trying_something_else_is_not_a_loop() {
        let mut t = LoopTracker::default();
        for i in 0..40 {
            let sig = format!("shell:{{\"cmd\":{i}}}");
            assert!(
                t.record(&sig, CallOutcome::Failed).is_none(),
                "different calls failing is ordinary probing, at call {i}"
            );
        }
    }

    #[test]
    fn a_success_clears_a_climbing_failure_count() {
        let mut t = LoopTracker::default();
        t.record("shell:{}", CallOutcome::Failed);
        t.record("shell:{}", CallOutcome::Failed);
        t.record("shell:{}", ok("it worked"));
        assert!(t.record("shell:{}", CallOutcome::Failed).is_none());
    }

    /// The same arguments came back with the same bytes. Nothing could have been
    /// learned -- unlike a repeat with a different answer, which is 66% of all repeats
    /// and legitimate.
    #[test]
    fn the_same_call_returning_the_same_answer_is_a_loop() {
        let mut t = LoopTracker::default();
        let mut last = None;
        for _ in 0..MAX_IDENTICAL_ANSWERS {
            last = t.record("read:{\"path\":\"a.ts\"}", ok("contents of a"));
        }
        let reason = last.expect("four identical answers is going round");
        assert!(reason.contains("same answer"), "{reason}");
    }

    #[test]
    fn the_same_call_returning_something_new_is_work() {
        let mut t = LoopTracker::default();
        for i in 0..40 {
            let answer = format!("run {i} of the test suite");
            assert!(
                t.record("shell:{\"cmd\":\"bun test\"}", ok(&answer)).is_none(),
                "the world moved, so the call was worth making"
            );
        }
    }

    /// The shape a single-slot tracker cannot see: every call differs from the one
    /// before it.
    #[test]
    fn an_alternating_pair_is_a_cycle() {
        let mut t = LoopTracker::default();
        let mut last = None;
        for i in 0..8 {
            let sig = if i % 2 == 0 { "read:{\"path\":\"a\"}" } else { "read:{\"path\":\"b\"}" };
            let answer = format!("answer {i}");
            last = t.record(sig, ok(&answer));
            if last.is_some() {
                break;
            }
        }
        let reason = last.expect("A,B,A,B,A,B is a loop");
        assert!(reason.contains("cycle"), "{reason}");
    }

    #[test]
    fn a_three_step_cycle_is_one_too() {
        let mut t = LoopTracker::default();
        let mut last = None;
        for i in 0..12 {
            let sig = format!("read:{{\"path\":\"{}\"}}", ["a", "b", "c"][i % 3]);
            last = t.record(&sig, ok(&format!("answer {i}")));
            if last.is_some() {
                break;
            }
        }
        assert!(last.is_some(), "A,B,C,A,B,C is a loop");
    }

    /// Revisiting a file after doing something else is not a cycle, and this is the
    /// false positive the window length is chosen against.
    #[test]
    fn coming_back_to_a_file_after_a_detour_is_not_a_cycle() {
        let mut t = LoopTracker::default();
        let path = |n: usize| format!("read:{{\"path\":\"f{n}\"}}");
        for round in 0..6 {
            // Seven distinct calls, then back to the first: further back than the
            // window, so nothing is flagged.
            for n in 0..7 {
                let sig = path(n);
                assert!(
                    t.record(&sig, ok(&format!("r{round} f{n}"))).is_none(),
                    "round {round}, file {n}"
                );
            }
        }
    }

    #[test]
    fn nothing_coming_back_over_and_over_is_a_loop() {
        let mut t = LoopTracker::default();
        let mut last = None;
        for i in 0..MAX_EMPTY_RESULTS {
            // Different searches every time, so only the emptiness is the signal.
            last = t.record(&format!("shell:{{\"cmd\":\"grep x{i}\"}}"), ok(""));
        }
        let reason = last.expect("eight empty answers in a row");
        assert!(reason.contains("returned nothing"), "{reason}");
    }

    #[test]
    fn one_answer_with_content_clears_the_emptiness_count() {
        let mut t = LoopTracker::default();
        for i in 0..20 {
            let sig = format!("shell:{{\"cmd\":\"grep x{i}\"}}");
            let answer = if i % 4 == 3 { "src/foo.ts:12: match" } else { "" };
            assert!(t.record(&sig, ok(answer)).is_none(), "call {i}");
        }
    }

    /// The measured shape of the expensive runs: 124-188 searches, every one returning
    /// something new, no repeats and no cycles. Nothing here can see it, and the module
    /// comment says why.
    #[test]
    fn searching_forever_with_new_answers_is_invisible_here() {
        let mut t = LoopTracker::default();
        for i in 0..200 {
            let sig = format!("shell:{{\"cmd\":\"grep -rn pattern{i}\"}}");
            let answer = format!("src/file{i}.ts:{i}: a match nobody will open");
            assert!(
                t.record(&sig, ok(&answer)).is_none(),
                "this is what the hook is for, not this module"
            );
        }
    }
}
