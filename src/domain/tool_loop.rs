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
//!
//! # What a verdict carries
//!
//! A verdict is read by whoever has to decide what the stopped run does next, and
//! making that person dig through a run log to find out which file was being re-read is
//! half a verdict (#35). So it names the call: the tool, the arguments, where in the run
//! it happened, and -- when the answer repeated -- whether the output cap had cut that
//! answer short.
//!
//! Two shapes are deliberately *not* excluded from the guard:
//!
//! **A capped answer still counts.** The same arguments return the same truncation, so
//! re-reading cannot reach the part that was cut and the call is still going round. But
//! the verdict says the answer was capped, because that is why the model kept coming
//! back and it is the thing a resume instruction has to change: a narrower range, not
//! the same file again.
//!
//! **A compaction clears the history.** Compaction replaces what the model had read with
//! a summary, so reading the same file afterwards restores what the session dropped and
//! comes back with the same bytes because the file did not change.
//! [`LoopTracker::reset_after_compaction`] is what keeps that recovery from being
//! aborted.
//!
//! Counting is over *consecutive* calls: any other call, or the same call returning
//! different bytes, starts the count again. That is what separates four reads in a row
//! with nothing in between from a healthy run that comes back to a file after a detour,
//! and it is why [`MAX_IDENTICAL_ANSWERS`] can sit as low as it does.
use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use super::tool_output;

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
///
/// "In a row" is exact: another call in between, or the same call answering with
/// different bytes, resets the count. Four reads spread around other work are a run
/// consulting a file twice, which is ordinary; four with nothing in between and the
/// same bytes back are one call made four times (#35).
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

/// How much of a call's arguments a verdict shows.
///
/// What identifies a repeated call is at the front of an argument set -- a path, a
/// query, a command -- while a write carries a whole file behind it. A verdict is read
/// by a person in a terminal and carried whole in the run's own record, so a bounded
/// prefix plus the real size serves both better than either a cut-off JSON blob or a
/// page of file contents.
const MAX_ARGUMENTS_CHARS: usize = 200;

fn brief_arguments(arguments: &str) -> String {
    let total = arguments.chars().count();
    if total <= MAX_ARGUMENTS_CHARS {
        return arguments.to_string();
    }
    let prefix: String = arguments.chars().take(MAX_ARGUMENTS_CHARS).collect();
    format!("{prefix}… ({total} characters in all)")
}

/// Which rule fired.
///
/// The word travels in the `--output json` envelope, so a caller can act on a stopped
/// run without reading the sentence: `snake_case`, matching the ending words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopRule {
    /// The same call failed, in a row.
    IdenticalFailures,
    /// The same call returned the same answer, in a row.
    IdenticalAnswers,
    /// The last calls repeat a cycle of earlier ones.
    Cycle,
    /// Calls returning nothing, in a row.
    EmptyResults,
}

/// Why a run was stopped, in the terms whoever resumes it needs.
///
/// Two readers, one value: `Display` is the sentence a person sees, and the fields are
/// what the JSON envelope and the session's run history carry. A resumed run is offered
/// an instruction that names what the stopped one already read (#35), and building that
/// instruction from a sentence would be parsing prose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoopVerdict {
    pub rule: LoopRule,
    /// The tool the rule names: the repeated one, the last of the cycle, or the most
    /// recent empty answer.
    pub tool: String,
    /// Its arguments, shortened by [`MAX_ARGUMENTS_CHARS`].
    pub arguments: String,
    /// How many calls the rule counted, which is its threshold.
    pub count: u8,
    /// Which tool call of this run the verdict was reached on, counting from 1. The
    /// run of calls it names ended here.
    pub call: u64,
    /// Whether the repeated answer was truncated by the output cap. Only meaningful
    /// for [`LoopRule::IdenticalAnswers`].
    pub capped: bool,
    /// The last error, when the rule was identical failures.
    pub last_error: Option<String>,
}

impl LoopVerdict {
    /// The first call of the run the verdict names.
    pub fn first_call(&self) -> u64 {
        self.call
            .saturating_sub(u64::from(self.count).saturating_sub(1))
    }

    /// Carry the error that failed the call, for the person reading the abort.
    pub fn with_last_error(mut self, error: String) -> Self {
        self.last_error = Some(error);
        self
    }

    fn position(&self) -> String {
        let first = self.first_call();
        if first == self.call {
            format!("tool call {} of this run", self.call)
        } else {
            format!("tool calls {first}-{} of this run", self.call)
        }
    }
}

impl std::fmt::Display for LoopVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.rule {
            LoopRule::IdenticalFailures => write!(
                f,
                "Aborting after {} identical failed calls to '{}' with arguments {} ({}). \
                 Change the tool or arguments before retrying.",
                self.count,
                self.tool,
                self.arguments,
                self.position(),
            )?,
            LoopRule::IdenticalAnswers => {
                write!(
                    f,
                    "Aborting: '{}' was called {} times in a row with the same arguments {} ({}) \
                     and returned the same answer every time. Nothing was learned; the run is \
                     going round.",
                    self.tool,
                    self.count,
                    self.arguments,
                    self.position(),
                )?;
                if self.capped {
                    write!(
                        f,
                        " That answer was itself truncated by the output cap, so the same call \
                         cannot return what was cut: read a narrower range instead."
                    )?;
                }
            }
            LoopRule::Cycle => write!(
                f,
                "Aborting: the last {} calls repeat a cycle of earlier ones, ending at '{}' with \
                 arguments {} ({}). Each differs from the one before it, so this is a loop rather \
                 than a retry.",
                self.count,
                self.tool,
                self.arguments,
                self.position(),
            )?,
            LoopRule::EmptyResults => write!(
                f,
                "Aborting: the last {} calls returned nothing at all, most recently '{}' with \
                 arguments {} ({}). Whatever is being looked for is not where it is being looked \
                 for.",
                self.count,
                self.tool,
                self.arguments,
                self.position(),
            )?,
        }
        if let Some(error) = &self.last_error {
            write!(f, " Last error: {error}")?;
        }
        Ok(())
    }
}

/// So the abort path can carry it as an `anyhow::Error`, and a caller can downcast to
/// the fields rather than to a sentence.
impl std::error::Error for LoopVerdict {}

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
    /// Tool calls this tracker has judged. The run's position, which a verdict names.
    calls: u64,

    failing_signature: Option<String>,
    consecutive_failures: u8,

    answered_signature: Option<String>,
    answered_hash: Option<u64>,
    consecutive_identical_answers: u8,
    /// Whether the most recent answer was cut by the output cap.
    answer_capped: bool,

    recent: VecDeque<String>,
    consecutive_cycle_steps: u8,

    consecutive_empty_results: u8,
}

impl LoopTracker {
    /// Record one call, and say whether the run should stop.
    ///
    /// `signature` identifies the call: the tool's name and its canonicalised
    /// arguments. `Some(verdict)` carries a sentence for the person who has to read it
    /// and the same facts as fields, for whoever decides what the run does next.
    pub fn record(&mut self, signature: &str, outcome: CallOutcome<'_>) -> Option<LoopVerdict> {
        self.calls += 1;

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
                self.answer_capped = false;
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
                self.answer_capped = tool_output::is_capped(text);
                self.answered_signature = Some(signature.to_string());
                self.answered_hash = Some(hash);

                if text.trim().len() <= EMPTY_RESULT_CHARS {
                    self.consecutive_empty_results =
                        self.consecutive_empty_results.saturating_add(1);
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
        self.answer_capped = false;
        self.consecutive_empty_results = 0;
    }

    /// Forget what this run has read, because compaction replaced it.
    ///
    /// Reading the same file after a compaction is recovery rather than a loop: the
    /// session dropped the tool results the model was working from, so the re-read
    /// restores them, and it answers with the same bytes because nothing about the file
    /// changed. Without this the guard would abort a run doing the right thing (#35).
    ///
    /// The call position survives: it says where the run is, not what it has read.
    pub fn reset_after_compaction(&mut self) {
        self.failing_signature = None;
        self.consecutive_failures = 0;
        self.answered_signature = None;
        self.answered_hash = None;
        self.consecutive_identical_answers = 0;
        self.answer_capped = false;
        self.recent.clear();
        self.consecutive_cycle_steps = 0;
        self.consecutive_empty_results = 0;
    }

    fn verdict(&self, signature: &str) -> Option<LoopVerdict> {
        let (tool, arguments) = signature.split_once(':').unwrap_or((signature, ""));
        let verdict = |rule, count, capped| LoopVerdict {
            rule,
            tool: tool.to_string(),
            arguments: brief_arguments(arguments),
            count,
            call: self.calls,
            capped,
            last_error: None,
        };

        if self.consecutive_failures >= MAX_IDENTICAL_TOOL_FAILURES {
            return Some(verdict(
                LoopRule::IdenticalFailures,
                self.consecutive_failures,
                false,
            ));
        }
        if self.consecutive_identical_answers >= MAX_IDENTICAL_ANSWERS {
            return Some(verdict(
                LoopRule::IdenticalAnswers,
                self.consecutive_identical_answers,
                self.answer_capped,
            ));
        }
        if self.consecutive_cycle_steps >= MAX_CYCLE_STEPS {
            return Some(verdict(
                LoopRule::Cycle,
                self.consecutive_cycle_steps + 1,
                false,
            ));
        }
        if self.consecutive_empty_results >= MAX_EMPTY_RESULTS {
            return Some(verdict(
                LoopRule::EmptyResults,
                self.consecutive_empty_results,
                false,
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
        assert!(
            reason.to_string().contains("identical failed calls"),
            "{reason}"
        );
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
        assert!(reason.to_string().contains("same answer"), "{reason}");
    }

    #[test]
    fn the_same_call_returning_something_new_is_work() {
        let mut t = LoopTracker::default();
        for i in 0..40 {
            let answer = format!("run {i} of the test suite");
            assert!(
                t.record("shell:{\"cmd\":\"bun test\"}", ok(&answer))
                    .is_none(),
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
            let sig = if i % 2 == 0 {
                "read:{\"path\":\"a\"}"
            } else {
                "read:{\"path\":\"b\"}"
            };
            let answer = format!("answer {i}");
            last = t.record(sig, ok(&answer));
            if last.is_some() {
                break;
            }
        }
        let reason = last.expect("A,B,A,B,A,B is a loop");
        assert!(reason.to_string().contains("cycle"), "{reason}");
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
        assert!(reason.to_string().contains("returned nothing"), "{reason}");
    }

    #[test]
    fn one_answer_with_content_clears_the_emptiness_count() {
        let mut t = LoopTracker::default();
        for i in 0..20 {
            let sig = format!("shell:{{\"cmd\":\"grep x{i}\"}}");
            let answer = if i % 4 == 3 {
                "src/foo.ts:12: match"
            } else {
                ""
            };
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

    /// Whoever resumes the run must not have to dig through the log first: the verdict
    /// names the call that repeated and where in the run it was (#35, point 1).
    #[test]
    fn the_verdict_names_the_call_and_where_in_the_run_it_was() {
        let mut t = LoopTracker::default();
        for _ in 0..MAX_IDENTICAL_ANSWERS - 1 {
            assert!(t
                .record("read:{\"path\":\"src/x.ts\"}", ok("contents of x"))
                .is_none());
        }
        let verdict = t
            .record("read:{\"path\":\"src/x.ts\"}", ok("contents of x"))
            .expect("four identical reads is going round");

        assert_eq!(verdict.rule, LoopRule::IdenticalAnswers);
        assert_eq!(verdict.tool, "read");
        assert_eq!(verdict.arguments, "{\"path\":\"src/x.ts\"}");
        assert_eq!(verdict.count, MAX_IDENTICAL_ANSWERS);
        assert_eq!(verdict.call, u64::from(MAX_IDENTICAL_ANSWERS));
        assert_eq!(verdict.first_call(), 1);

        let message = verdict.to_string();
        assert!(message.contains("src/x.ts"), "{message}");
        assert!(message.contains("tool calls 1-4 of this run"), "{message}");
        assert!(message.contains("same answer"), "{message}");
    }

    /// A write's arguments are a whole file. What identifies the call is its path, and
    /// the verdict says how much it left out rather than printing the file again.
    #[test]
    fn a_verdict_shortens_arguments_that_are_a_whole_file() {
        let mut t = LoopTracker::default();
        let body = "x".repeat(5_000);
        let signature = format!("write:{{\"path\":\"a.ts\",\"content\":\"{body}\"}}");
        let mut last = None;
        for _ in 0..MAX_IDENTICAL_ANSWERS {
            last = t.record(&signature, ok("written"));
        }
        let verdict = last.expect("four identical writes is going round");
        assert!(verdict.arguments.len() < 400, "{}", verdict.arguments.len());
        assert!(verdict.arguments.contains("a.ts"), "{}", verdict.arguments);
        assert!(
            verdict.arguments.contains("characters in all"),
            "{}",
            verdict.arguments
        );
    }

    /// Four reads spread around other work are a run consulting a file twice, which is
    /// ordinary; only an unbroken run of the same call is a loop (#35, point 3).
    #[test]
    fn returning_to_a_file_after_other_work_is_not_a_loop() {
        let mut t = LoopTracker::default();
        for round in 0..4 {
            for _ in 0..MAX_IDENTICAL_ANSWERS - 1 {
                assert!(
                    t.record("read:{\"path\":\"a.ts\"}", ok("contents of a"))
                        .is_none(),
                    "round {round}"
                );
            }
            assert!(t
                .record("shell:{\"cmd\":\"cargo test\"}", ok("tests passed"))
                .is_none());
        }
    }

    /// The cap cut the answer, so the same call cannot return what was cut. The abort
    /// stays -- the call is still going round -- and says why re-reading cannot help
    /// (#35, point 4).
    #[test]
    fn a_capped_answer_says_that_reading_it_again_cannot_help() {
        let mut t = LoopTracker::default();
        let capped = "head\n\n[atoma: 5000 characters dropped from the middle; 8000 shown]\n\ntail";
        let mut last = None;
        for _ in 0..MAX_IDENTICAL_ANSWERS {
            last = t.record("read:{\"path\":\"big.log\"}", ok(capped));
        }
        let verdict = last.expect("four identical answers is going round");
        assert!(verdict.capped);
        let message = verdict.to_string();
        assert!(message.contains("truncated by the output cap"), "{message}");
    }

    /// Compaction replaced the results the model was reading, so the same call coming
    /// back afterwards is the session getting them back, not a loop (#35, point 4).
    #[test]
    fn a_compaction_clears_what_the_run_had_read() {
        let mut t = LoopTracker::default();
        for _ in 0..MAX_IDENTICAL_ANSWERS - 2 {
            assert!(t
                .record("read:{\"path\":\"a.ts\"}", ok("contents of a"))
                .is_none());
        }
        t.reset_after_compaction();
        for _ in 0..MAX_IDENTICAL_ANSWERS - 2 {
            assert!(
                t.record("read:{\"path\":\"a.ts\"}", ok("contents of a"))
                    .is_none(),
                "the re-read restores what compaction removed"
            );
        }
        assert_eq!(t.calls, 4, "the run is still where it was");
    }
}
