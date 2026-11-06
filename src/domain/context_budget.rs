//! The context budget: when a session's prompt has grown past what its model's
//! window holds, and what a compaction does about it.
//!
//! # The failure this exists for
//!
//! A prompt token is resent on every turn, and tool results are most of it. Past
//! the model's window the provider answers 400 and nothing retries it -- so a long
//! run dies mid-work, and the session it saved can only be resumed after whoever
//! embeds atoma rebuilds it by hand.
//!
//! # A compaction REPLACES the session, and the replacement is saved
//!
//! Summarize-and-project (keep the full session, shrink what one request carries)
//! was considered and rejected: the summary is generated once and its cost is
//! paid again on every later turn, while the truth of the conversation now lives
//! in two places. A compaction here instead issues a NEW session -- a summary and
//! the most recent turns -- and saves it to the SAME path: the session file stays
//! the one truth, and the summary is paid for once.
//!
//! Whoever embedded atoma keeps the old state: a `before_compaction` hook (see
//! `infra::hooks`) receives the session's path before the replacement is written,
//! and archiving it there is that caller's decision, not this crate's -- atoma is
//! a stateless CLI and does not know where archives live.
//!
//! # Turn boundaries only
//!
//! The check runs at the top of the inference loop's iteration, beside the three
//! existing stops. A session is saved whole between exchanges, so cutting there
//! is what leaves a resumable state; cutting inside a tool batch is what leaves
//! an unanswered call.
//!
//! # Characters, not tokens, for the recent-turns window
//!
//! The summary is the model's; the recent turns are selected by a character
//! budget at four characters to the token, the same estimate the delivery
//! template measured with. An under-count for Japanese, which is the right
//! direction to be wrong in: it keeps fewer turns than the budget claims rather
//! than more.
use serde_json::Value;

use crate::domain::session::Message;

/// Four characters to a token, the estimate the delivery template measured with.
pub const CHARS_PER_TOKEN: usize = 4;

/// How much of the context budget the most recent turns keep, as a share.
///
/// A third: the summary holds the story, the recent turns hold where the story
/// stopped. Most of the budget goes to the summary because "what was decided and
/// why" is what a resumed run cannot reconstruct from tool results alone -- and
/// because the turns nearest the cut are the ones the summary has seen least of.
pub const RECENT_SHARE: f64 = 1.0 / 3.0;

/// Why a compaction ran: the usage the provider last reported, against the
/// definition's limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetExceeded {
    /// The prompt tokens the provider reported for the last inference.
    pub prompt_tokens: u64,
    /// The limit the agent definition declared.
    pub context_limit: u64,
}

/// Whether the last inference's prompt has crossed the declared limit.
///
/// `None` when the definition declares no limit -- every agent written before
/// this field existed runs unchanged -- or when no usage was reported, which is
/// Copilot's case: per-request billing and no token counts. A limit with no
/// measurement behind it must never fire, and it never does here.
pub fn budget_exceeded(
    context_limit: Option<u64>,
    last_prompt_tokens: Option<u64>,
) -> Option<BudgetExceeded> {
    let limit = context_limit?;
    let prompt = last_prompt_tokens?;
    (prompt >= limit).then_some(BudgetExceeded {
        prompt_tokens: prompt,
        context_limit: limit,
    })
}

/// The message that opens a compacted session: what happened to the history.
///
/// A user message, because that is the role the model reads as a fact about the
/// world rather than as one of its own thoughts. It states what was replaced and
/// what stands in for it, so a model reading the compacted session does not take
/// the summary for the whole conversation or wonder where the beginning went.
pub fn compaction_notice(dropped_messages: usize, kept_messages: usize) -> String {
    format!(
        "[session compacted: the {dropped_messages} earliest messages were replaced by the \
         summary below; the {kept_messages} most recent messages after it are verbatim.]"
    )
}

/// The instruction the summarizing inference is asked with.
///
/// Addressed to the model that has just been reading the conversation: it names
/// what a resuming run needs and cannot recover any other way -- decisions and
/// their reasons, open threads, the state of the work -- and forbids narration
/// of the transcript, which is what a summary prompt otherwise produces.
pub const SUMMARY_PROMPT: &str = "\
The conversation above is about to be compacted: everything before your answer \
will be replaced by your summary, and the run continues from it with only the \
most recent turns kept verbatim. Write that summary. State what the work is, \
what has been decided and why, what is done, what is still open, and anything a \
continuing run must not redo or must not forget. Do not narrate the transcript; \
write the state of the work.";

/// One compacted session, ready to save over the old one.
#[derive(Debug)]
pub struct Compacted {
    pub messages: Vec<Message>,
    /// How many messages the summary replaced. Recorded, so the run's report can
    /// say what was lost without anyone counting again.
    pub dropped_messages: usize,
}

/// Compact `messages` around a summary, keeping the most recent turns.
///
/// The caller has already produced `summary` (one inference with
/// [`SUMMARY_PROMPT`]); this assembles the replacement and measures nothing on
/// trust: the character budget is spent on the messages THEMSELVES, newest
/// first, so whatever the session's shape, the kept suffix is the part nearest
/// the work.
///
/// The suffix never splits a tool batch: a `tool` message answers an assistant
/// message's `tool_calls`, and a selection that kept the call and dropped its
/// answer would be a session no provider accepts. The kept range extends back to
/// the assistant message that opened the oldest kept batch.
///
/// Returns `None` when compaction has nothing to offer: a keep that reaches back
/// to the session's own opening has nothing before it to summarize, and a
/// replacement must itself fit the window -- a summary that does not fit would be
/// paid for a session the next turn exceeds again. The caller ends the run
/// instead, with the session intact.
pub fn compact(messages: &[Message], summary: &str, context_limit: u64) -> Option<Compacted> {
    if messages.is_empty() {
        return None;
    }
    // The whole window, in characters: `context_limit` is in tokens and
    // `CHARS_PER_TOKEN` is characters per token, so this multiplies. The
    // recent-turns budget is a third of the result. `RECENT_SHARE` is a share
    // and the arithmetic sits here rather than in a constant so it reads in one
    // place.
    let limit_chars = (context_limit as usize).saturating_mul(CHARS_PER_TOKEN);
    let budget_chars = (limit_chars as f64 * RECENT_SHARE) as usize;

    let mut keep_from = messages.len();
    let mut kept_chars = 0usize;
    let mut batches = 0usize;
    while keep_from > 0 {
        let start = batch_start(messages, keep_from - 1);
        let batch_chars: usize = messages[start..keep_from].iter().map(message_chars).sum();
        // The first batch is kept whatever it costs -- a session whose newest
        // exchange alone exceeds the budget has no smaller selection to offer.
        // Whether that selection can be compacted at all is the fit check below,
        // not a reason to walk further back.
        if batches > 0 && kept_chars + batch_chars > budget_chars {
            break;
        }
        keep_from = start;
        kept_chars += batch_chars;
        batches += 1;
    }

    // A compaction must DROP at least one whole batch: a keep that reaches back
    // to the session's own opening has nothing before it to summarize (the
    // summary would restate the kept turns). Dropping everything older than a
    // single kept batch is a legitimate compaction -- that batch is the anchor
    // the resumed run continues from -- so the number of kept batches is not
    // itself a refusal reason.
    if keep_from == 0 {
        return None;
    }

    let kept = &messages[keep_from..];
    let notice = compaction_notice(keep_from, kept.len());
    // The replacement has to fit the window it replaces, and the summary is the
    // model's output: its size is not ours to assume. If the notice, the summary
    // and the newest batch -- the smallest keep there is -- overflow the limit,
    // a summary would be paid for a session the next turn exceeds again, so the
    // caller ends the run with the session intact instead.
    const NOTICE_JOINER: usize = "\n\n---\n\n".len();
    if notice.len() + summary.len() + NOTICE_JOINER + kept_chars > limit_chars {
        return None;
    }

    let mut out = Vec::with_capacity(kept.len() + 1);
    out.push(Message::user(&format!("{}\n\n---\n\n{}", notice, summary)));
    out.extend(kept.iter().cloned());
    Some(Compacted {
        messages: out,
        dropped_messages: keep_from,
    })
}

/// Where the exchange containing `index` begins: the assistant message whose
/// `tool_calls` the tool messages after it answer, or `index` itself when it is
/// not part of a batch.
fn batch_start(messages: &[Message], index: usize) -> usize {
    let mut start = index;
    while start > 0 {
        let previous = &messages[start - 1];
        let previous_is_part = previous.tool_calls.is_some()
            || (previous.role == "tool" && messages[start].role == "tool");
        if !previous_is_part {
            break;
        }
        start -= 1;
    }
    start
}

fn message_chars(message: &Message) -> usize {
    let text = message.content.as_ref().map(|content| match content {
        Value::String(text) => text.len(),
        other => other.to_string().len(),
    });
    let calls = message
        .tool_calls
        .as_ref()
        .map(|calls| {
            calls
                .iter()
                .map(|c| c.function.arguments.len())
                .sum::<usize>()
        })
        .unwrap_or(0);
    text.unwrap_or(0) + calls
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::session::ToolCallFunction;

    /// Module-scope helpers, named for the tests that read them: the session
    /// shapes below are built from these three constructors alone.
    fn user(text: &str) -> Message {
        Message::user(text)
    }

    fn tool(id: &str, text: &str) -> Message {
        Message::tool(id, text)
    }

    fn assistant_with_calls(id: &str, args: &str) -> Message {
        Message::assistant(
            None,
            Some(vec![crate::domain::session::ToolCall {
                id: id.to_string(),
                type_: "function".to_string(),
                function: ToolCallFunction {
                    name: "read".to_string(),
                    arguments: args.to_string(),
                },
            }]),
        )
    }

    /// A session shaped like a real one: turns of [user, assistant+calls, tool].
    fn turn(turn_index: usize, result_chars: usize) -> Vec<Message> {
        vec![
            user(&format!(
                "turn {turn_index}: please look at file {turn_index}"
            )),
            assistant_with_calls(
                &format!("call-{turn_index}"),
                &format!("{{\"path\":\"f{turn_index}\"}}"),
            ),
            tool(&format!("call-{turn_index}"), &"x".repeat(result_chars)),
        ]
    }

    #[test]
    fn no_limit_never_fires() {
        assert!(budget_exceeded(None, Some(1_000_000)).is_none());
        // A limit with no usage reported never fires either: Copilot reports no
        // token counts, and a limit with no measurement must not guess.
        assert!(budget_exceeded(Some(1_000), None).is_none());
    }

    #[test]
    fn the_limit_fires_at_and_past_itself() {
        let hit = budget_exceeded(Some(100_000), Some(100_000)).unwrap();
        assert_eq!(hit.prompt_tokens, 100_000);
        assert!(budget_exceeded(Some(100_000), Some(99_999)).is_none());
    }

    #[test]
    fn a_compaction_keeps_the_recent_turns_and_replaces_the_rest() {
        let mut messages = vec![user("the task")];
        for i in 0..60 {
            messages.extend(turn(i, 100));
        }
        let limit = 4_000; // tokens → 16k chars; a third of that is ~5.3k chars, about 37 of these turns
        let out = compact(&messages, "the work so far", limit).unwrap();
        assert!(out.dropped_messages > 0);
        assert_eq!(
            out.messages.len(),
            1 + (messages.len() - out.dropped_messages)
        );
        let text = out.messages[0].content.as_ref().unwrap().as_str().unwrap();
        assert!(text.contains("[session compacted:"));
        assert!(text.contains("the work so far"));
        // The kept suffix is VERBATIM: it is the tail of the original.
        assert_eq!(
            out.messages
                .last()
                .unwrap()
                .content
                .as_ref()
                .unwrap()
                .as_str()
                .unwrap(),
            messages
                .last()
                .unwrap()
                .content
                .as_ref()
                .unwrap()
                .as_str()
                .unwrap()
        );
    }

    /// A `tool` message answers its assistant message's `tool_calls`, and a
    /// selection that kept the call and dropped the answer (or the reverse) is a
    /// conversation no provider accepts. The kept range extends back to the
    /// assistant that opened the oldest kept batch.
    #[test]
    fn the_kept_suffix_never_splits_a_tool_batch() {
        // Limit 100_000 tokens → 400k chars; a third is ~133k chars of recent
        // budget: with 2,000-char results, several turns are kept and several are
        // dropped -- both sides of the boundary exist, which is what this test
        // needs.
        let mut messages = vec![user("the task")];
        for i in 0..100 {
            messages.extend(turn(i, 2_000));
        }
        let out = compact(&messages, "summary", 100_000).unwrap();
        let kept = &out.messages[1..];
        // Every tool message in the kept suffix is preceded (possibly across other
        // tool messages) by the assistant carrying its tool_call id.
        for (i, m) in kept.iter().enumerate() {
            if m.role != "tool" {
                continue;
            }
            let id = m.tool_call_id.as_ref().unwrap();
            let answered = kept[..i].iter().rev().any(|earlier| {
                earlier
                    .tool_calls
                    .as_ref()
                    .is_some_and(|calls| calls.iter().any(|c| &c.id == id))
            });
            assert!(
                answered,
                "tool message answering {id} kept without its call"
            );
        }
        // And the first kept message opens a batch: an assistant with calls, or a
        // user turn -- never a bare tool result whose call was dropped.
        assert_ne!(kept.first().unwrap().role, "tool");
    }

    /// A session whose smallest replacement still overflows the window has no
    /// compaction to offer: a summary would be paid for a session the next turn
    /// exceeds again. `None` is the caller's signal to end the run with the
    /// session intact.
    #[test]
    fn a_session_whose_smallest_replacement_overflows_is_not_compacted() {
        // Limit 1_000 tokens → 4k chars total → ~1.3k chars of recent budget.
        // Every turn below is larger than that budget on its own, so the walk
        // keeps the newest batch; the notice and summary on top of it still
        // exceed the 4k window, so there is nothing to offer.
        let mut messages = vec![user("the task")];
        for i in 0..3 {
            messages.extend(turn(i, 5_000));
        }
        assert!(compact(&messages, "summary", 1_000).is_none());
    }

    #[test]
    fn a_session_that_fits_the_recent_window_has_nothing_to_summarize() {
        let mut messages = vec![user("the task")];
        for i in 0..3 {
            messages.extend(turn(i, 100));
        }
        // ~430 chars, far under a third of 4,000 tokens (16k chars): keeping it
        // all is not a compaction, and a summary would restate the kept turns.
        assert!(compact(&messages, "summary", 4_000).is_none());
    }

    /// The observed failure (#441): the summary succeeded, only the newest batch
    /// fit the recent budget, and the old `batches <= 1` refusal threw the work
    /// away. One batch is the anchor the resumed run needs; the replacement fits
    /// the window, so it is a compaction.
    #[test]
    fn a_single_kept_batch_is_enough_when_the_replacement_fits() {
        let mut messages = vec![user("the task")];
        for i in 0..3 {
            messages.extend(turn(i, 100));
        }
        messages.extend(turn(3, 5_300));

        // 4,000 tokens → 16k chars, ~5.3k of recent budget: the newest batch
        // (~5.3k chars) is the only one that fits, and everything older is
        // dropped.
        let out = compact(&messages, "summary", 4_000).unwrap();
        assert!(out.dropped_messages > 0);
        assert_eq!(out.messages.len() - 1, 2, "only the newest batch is kept");
    }

    /// The recent window is a third of the FULL window in characters. The budget
    /// used to divide by `CHARS_PER_TOKEN` instead of multiplying it, making the
    /// window a forty-eighth -- the observed session then kept one batch where it
    /// should have kept dozens.
    #[test]
    fn the_recent_window_is_a_third_of_the_window_in_characters() {
        let mut messages = vec![user("the task")];
        for i in 0..40 {
            messages.extend(turn(i, 100));
        }
        // 4,000 tokens → 16k chars; a third is ~5.3k chars, about 37 turns.
        let out = compact(&messages, "s", 4_000).unwrap();
        let kept = out.messages.len() - 1;
        assert!(out.dropped_messages > 0);
        assert!(kept > 60, "kept {kept} messages; expected dozens of turns");
    }

    /// The summary is the model's output, so its size is not ours to assume. One
    /// that does not fit the window is not a compaction -- the caller ends with
    /// the session intact instead.
    #[test]
    fn a_summary_that_does_not_fit_the_window_is_not_compacted() {
        let mut messages = vec![user("the task")];
        for i in 0..60 {
            messages.extend(turn(i, 100));
        }
        let summary = "x".repeat(20_000);
        assert!(compact(&messages, &summary, 4_000).is_none());
    }

    #[test]
    fn the_notice_counts_what_it_dropped_and_kept() {
        // Limit 500 tokens → 2k chars total, ~666 chars of recent budget:
        // several turns are dropped and several kept.
        let mut messages = vec![user("the task")];
        for i in 0..10 {
            messages.extend(turn(i, 50));
        }
        let out = compact(&messages, "s", 500).unwrap();
        let dropped = out.dropped_messages;
        let kept = out.messages.len() - 1;
        let text = out.messages[0].content.as_ref().unwrap().as_str().unwrap();
        assert!(text.contains(&format!("{dropped} earliest messages")));
        assert!(text.contains(&format!("{kept} most recent messages")));
    }
}
