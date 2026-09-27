//! When a model has stopped saying things and started repeating itself.
//!
//! # What this is not
//!
//! Not a quality metric, and not a length ceiling. It watches one thing: whether the
//! recent output has stopped being language. A reasoning model that has fallen into a
//! loop writes `Let me write. / Writing. / Let me write.` until it reaches
//! `max_tokens`, and every one of those tokens is billed. The loop is visible in the
//! token stream long before the ceiling is reached, and that is the whole point of
//! watching it there rather than counting tokens at the end.
//!
//! # Why vocabulary size rather than the frequency of one word
//!
//! The obvious rule -- "one word is more than a quarter of the window" -- was tried
//! and thrown out, for two reasons that both matter.
//!
//! It misses long cycles. A model repeating a twenty-word sentence verbatim puts each
//! of those words at a twentieth of the window, well under any threshold that ordinary
//! prose does not also cross. The pathology is not "one word is frequent"; it is "the
//! same few words keep coming back", and a cycle of any length is exactly that.
//!
//! And it needs a language. Folding `write`/`writing`/`writes` together is what makes a
//! two-word cycle visible to a per-word count, and that fold is English morphology --
//! meaningless for Japanese, Chinese or Korean, where the same loop would go unnoticed.
//!
//! Counting *distinct* tokens has neither problem. A window of ordinary prose uses a
//! large fraction of its tokens only once; a window of a loop, whatever the cycle's
//! length, uses a handful. Measured on English prose, roughly a third to a half of a
//! five-hundred-token window is distinct. A two-word cycle is two. A hundred-word cycle
//! is a hundred. The gap is an order of magnitude wide, and it is the same gap in every
//! language, because it is a property of repetition rather than of grammar.
//!
//! # What this catches, and what it deliberately does not
//!
//! It catches a cycle of up to about seventy-five words, which is every reasoning loop
//! observed and every one the issue describes. Beyond that the cycle's own vocabulary
//! approaches that of prose and the two stop being separable by this measure -- and a
//! model repeating a hundred-word paragraph verbatim is a different failure, one that
//! the output ceiling bounds anyway.
//!
//! **No detection of repetition across turns.** A model that says the same sentence in
//! two different turns is doing something a person does; the pathology this exists for
//! is inside one completion, where nothing can interrupt it but the ceiling.
//!
//! **No detection of a repeated *phrase* as such.** It does not need one: a repeated
//! phrase is a repeated cycle, and the vocabulary of the window collapses either way.

use std::collections::{HashMap, VecDeque};

/// How many recent tokens the count is taken over.
///
/// Five hundred, which is long enough that a cycle is seen many times over and short
/// enough that the window is about what the model is doing *now* rather than what it
/// has said so far. A window that grew with the output would make the threshold mean
/// something different at every point in the run.
///
/// It is also far below any output ceiling in use -- the smallest here is 8192 -- so a
/// loop is always visible in the window long before the ceiling is reached, which is
/// the whole point of watching the stream rather than counting tokens at the end.
pub const WINDOW_TOKENS: usize = 500;

/// The share of the window that must be distinct tokens for it to be prose.
///
/// Fifteen per cent. Ordinary prose is a third to a half; a cycle of seventy-five
/// words is exactly this; anything shorter is well below. The threshold sits in the
/// middle of a gap that is an order of magnitude wide, so nothing here is a close call.
pub const MIN_DISTINCT_RATIO: f64 = 0.15;

/// How many tokens must have been seen before any verdict is given.
///
/// Two hundred. A short completion -- a one-line answer, a tool call with no
/// reasoning -- never reaches it, so it can never be cut short by this. Without a
/// floor, the first few tokens of every response would be judged against a window that
/// is mostly empty, and a model that opened with `OK. OK.` would be stopped before it
/// had said anything.
pub const MIN_TOKENS: usize = 200;

/// Whether a character is one of the scripts that do not put spaces between words.
///
/// Hiragana, katakana, the CJK ideograph blocks, and Hangul. Text in these scripts
/// arrives as one long run with no whitespace, so splitting on whitespace alone would
/// make a whole Japanese sentence a single token -- and a single token is a vocabulary
/// of one, which this detector would read as a loop. Splitting them per character is
/// what makes the measure mean the same thing in every language.
fn is_unspaced_script(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF     // Hiragana, Katakana
        | 0x3400..=0x4DBF   // CJK Unified Ideographs Extension A
        | 0x4E00..=0x9FFF   // CJK Unified Ideographs
        | 0xF900..=0xFAFF   // CJK Compatibility Ideographs
        | 0xAC00..=0xD7AF   // Hangul Syllables
    )
}

/// Split a chunk of streamed text into the tokens the window counts.
///
/// Whitespace and punctuation both end a token, because a model in a loop writes
/// `write. write. write.` as readily as `write write write`, and a tokeniser that kept
/// the full stop would count three different words.
///
/// A run in a script that does not use spaces is split per character, for the reason
/// [`is_unspaced_script`] gives.
///
/// A chunk boundary can fall inside a word -- SSE deltas are not word-aligned -- so a
/// word split across two deltas is counted as two. That is a real limitation and it is
/// the safe direction: it can only ever *raise* the distinct count, and a raised count
/// delays a verdict rather than inventing one.
pub fn tokenise(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '\'') {
        if word.is_empty() {
            continue;
        }
        if word.chars().any(is_unspaced_script) {
            for ch in word.chars() {
                out.push(ch.to_lowercase().to_string());
            }
        } else {
            out.push(word.to_lowercase());
        }
    }
    out
}

/// What the detector saw, when it saw it.
///
/// A sentence for the person who has to read it, not for a log grep -- the same rule
/// `tool_loop::LoopTracker` follows, and for the same reason: the reader is somebody
/// deciding whether the run was cut short correctly.
#[derive(Debug, Clone, PartialEq)]
pub struct Repetition {
    /// How many distinct tokens the window held.
    pub distinct: usize,
    /// How many tokens the window held.
    pub window: usize,
    /// The most frequent token, for the sentence. Not what the verdict is based on --
    /// see the module comment -- but it is what a person wants to see.
    pub word: String,
    /// How many times that token appeared.
    pub count: usize,
}

impl Repetition {
    /// The share of the window that was distinct, as a percentage.
    pub fn distinct_percent(&self) -> f64 {
        if self.window == 0 {
            return 0.0;
        }
        (self.distinct as f64 / self.window as f64) * 100.0
    }
}

impl std::fmt::Display for Repetition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Aborting: the last {} tokens used only {} distinct words ({:.0}%), and '{}' \
             alone appeared {} times. This is a reasoning loop rather than an answer, and \
             it would have run to the output ceiling.",
            self.window,
            self.distinct,
            self.distinct_percent(),
            self.word,
            self.count,
        )
    }
}

/// So the abort path can carry it as an `anyhow::Error`.
///
/// The delta handler returns `Err` to stop the request, and that error travels back
/// through the adapter to the inference loop. As a plain struct it could not, and the
/// alternative -- a string -- would lose the fields the caller wants to report.
impl std::error::Error for Repetition {}

/// Watches the streamed output for its vocabulary collapsing.
///
/// One detector per completion. Fed every delta as it arrives, and asked after each
/// whether the run should stop. The counts are over a sliding window rather than
/// consecutive tokens, so a single ordinary sentence does not clear a loop that is
/// genuinely under way -- which is the opposite of `tool_loop`'s rule, and correct
/// here: a tool call is a discrete act that either learned something or did not, while
/// a stream of tokens has no such boundary.
#[derive(Debug)]
pub struct RepetitionDetector {
    window: VecDeque<String>,
    counts: HashMap<String, usize>,
    /// The most recent verdict, so a caller that keeps feeding deltas after deciding
    /// to stop does not get a second, different sentence for the same loop.
    tripped: Option<Repetition>,
}

impl Default for RepetitionDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl RepetitionDetector {
    pub fn new() -> Self {
        Self {
            window: VecDeque::with_capacity(WINDOW_TOKENS),
            counts: HashMap::new(),
            tripped: None,
        }
    }

    /// Feed one delta, and say whether the output has become a loop.
    ///
    /// `Some` is returned once and then again on every later call, so a caller that
    /// has not yet acted on it cannot lose it. The verdict does not change once
    /// reached: the loop it describes is the one that was detected, not whatever the
    /// window holds by the time somebody reads it.
    pub fn observe(&mut self, delta: &str) -> Option<Repetition> {
        if let Some(tripped) = &self.tripped {
            return Some(tripped.clone());
        }

        for token in tokenise(delta) {
            self.push(token);
        }

        if self.window.len() < MIN_TOKENS {
            return None;
        }

        let distinct = self.counts.len();
        let ratio = distinct as f64 / self.window.len() as f64;
        if ratio >= MIN_DISTINCT_RATIO {
            return None;
        }

        // The most frequent token, and among equals the one seen most recently. A
        // `HashMap` iteration order is not stable, so picking the maximum by iterating
        // it would name a different word on different runs for the same output -- and
        // the sentence this produces is read by a person deciding whether the run was
        // cut short correctly. Scanning the window backwards is both deterministic and
        // the more useful answer: the word the model is stuck on now.
        let count = self.counts.values().copied().max().unwrap_or(0);
        let word = self
            .window
            .iter()
            .rev()
            .find(|token| self.counts.get(*token) == Some(&count))
            .cloned()?;

        let verdict = Repetition {
            distinct,
            window: self.window.len(),
            word,
            count,
        };
        tracing::warn!("{}", verdict);
        self.tripped = Some(verdict.clone());
        Some(verdict)
    }

    /// Add one token to the window, evicting the oldest when it is full.
    fn push(&mut self, token: String) {
        if self.window.len() == WINDOW_TOKENS {
            if let Some(evicted) = self.window.pop_front() {
                if let Some(count) = self.counts.get_mut(&evicted) {
                    *count -= 1;
                    if *count == 0 {
                        self.counts.remove(&evicted);
                    }
                }
            }
        }
        *self.counts.entry(token.clone()).or_insert(0) += 1;
        self.window.push_back(token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paragraphs of the kind an agent writes, all different from one another.
    ///
    /// A single paragraph repeated is a loop and the detector is right to say so --
    /// that is what `a_long_cycle_is_caught` covers. What the prose tests need is a
    /// long run of *varied* text, which is what this is.
    const VARIED_PROSE: [&str; 5] = [
        "I need to look at the failing test first. The test asserts that the parser \
         rejects a malformed header, and it is failing because the parser now accepts \
         one. Let me read the parser and see what changed.",
        "The change was in the header check, which used to compare the name \
         case-sensitively and now lowercases it. That is the bug: the check must stay \
         case-sensitive for the value and case-insensitive for the name.",
        "I will fix the check and run the test again to confirm. If it passes, I should \
         also add a case for the empty header, which the old code handled by accident \
         and the new one may not.",
        "Before changing anything else, let me search for other callers of this \
         function. A fix in one place that leaves three others broken is worse than no \
         fix, because it looks like progress.",
        "The repository has a contract test for the tool sets, so a change to the \
         defaults file will be caught. I should run the full suite rather than only the \
         one test, since the header check is shared.",
    ];

    /// The shape the issue names: a model stuck on one word, written as a sentence
    /// each time so no two adjacent tokens are equal.
    #[test]
    fn a_repeated_word_in_sentences_is_caught() {
        let mut detector = RepetitionDetector::new();
        let mut verdict = None;
        for _ in 0..200 {
            verdict = detector.observe("Let me write. ");
            if verdict.is_some() {
                break;
            }
        }
        let verdict = verdict.expect("a loop of one word must be caught");
        assert!(
            verdict.distinct <= 4,
            "only a handful of words: {}",
            verdict.distinct
        );
    }

    /// The cycle shape: two words alternating, so a detector remembering only the
    /// previous token would see no repetition at all.
    #[test]
    fn an_alternating_cycle_is_caught() {
        let mut detector = RepetitionDetector::new();
        let mut verdict = None;
        for _ in 0..200 {
            verdict = detector.observe("write writing ");
            if verdict.is_some() {
                break;
            }
        }
        assert!(verdict.is_some(), "write/writing is a loop");
    }

    /// The case a per-word count cannot see: a long sentence repeated verbatim, where
    /// no single word is frequent enough to stand out.
    #[test]
    fn a_long_cycle_is_caught() {
        let sentence = "I need to check whether the parser handles the header \
            correctly before I change anything else in this file. ";
        let mut detector = RepetitionDetector::new();
        let mut verdict = None;
        for _ in 0..200 {
            verdict = detector.observe(sentence);
            if verdict.is_some() {
                break;
            }
        }
        assert!(
            verdict.is_some(),
            "a repeated sentence is a loop, however long it is"
        );
    }

    /// The acceptance criterion that matters most: ordinary long-form reasoning must
    /// not be cut short. This is a paragraph of the kind an agent writes, with the
    /// usual English word frequencies, and it must pass.
    #[test]
    fn ordinary_prose_is_not_a_loop() {
        let prose = "I need to look at the failing test first. The test asserts that \
            the parser rejects a malformed header, and it is failing because the \
            parser now accepts one. Let me read the parser and see what changed. \
            The change was in the header check, which used to compare the name \
            case-sensitively and now lowercases it. That is the bug: the check \
            must stay case-sensitive for the value and case-insensitive for the \
            name. I will fix the check and run the test again to confirm.";
        let mut detector = RepetitionDetector::new();
        assert_eq!(
            detector.observe(prose),
            None,
            "a paragraph of reasoning is not a loop"
        );
    }

    /// The same paragraph repeated to fill the window, which is the harder case: a
    /// long run of ordinary prose must still not trip, however much of it there is.
    ///
    /// The paragraphs differ, because repeating one paragraph verbatim IS a loop and
    /// the detector is right to say so -- that is what `a_long_cycle_is_caught`
    /// covers. What this test is about is a long run of *varied* prose.
    #[test]
    fn a_long_run_of_ordinary_prose_is_not_a_loop() {
        let mut detector = RepetitionDetector::new();
        for paragraph in VARIED_PROSE.iter().cycle().take(40) {
            assert_eq!(
                detector.observe(paragraph),
                None,
                "ordinary prose must never trip, at any length"
            );
        }
        assert!(
            detector.window.len() >= MIN_TOKENS,
            "the window must have been full, or this proves nothing"
        );
    }

    /// Japanese has no spaces, so a whole sentence would be one token -- and a
    /// vocabulary of one, which this detector would read as a loop. The per-character
    /// split is what stops that, and this is the test that says so.
    #[test]
    fn japanese_prose_is_not_a_loop() {
        let prose = "まず失敗しているテストを確認します。パーサーが不正なヘッダーを\
            拒否することを検証していますが、現在は受け入れてしまっているため失敗して\
            います。ヘッダーの検査が名前を小文字に変換するよう変更されたことが原因です。\
            値を大文字小文字を区別して比較し、名前は区別しないように修正します。";
        let mut detector = RepetitionDetector::new();
        assert_eq!(
            detector.observe(prose),
            None,
            "Japanese prose is not a loop"
        );
    }

    /// And a Japanese loop is caught, which the English-only stemmer could not do.
    #[test]
    fn a_japanese_loop_is_caught() {
        let mut detector = RepetitionDetector::new();
        let mut verdict = None;
        for _ in 0..200 {
            verdict = detector.observe("書く。書く。書く。");
            if verdict.is_some() {
                break;
            }
        }
        assert!(verdict.is_some(), "a Japanese loop must be caught");
    }

    /// A short answer never reaches the floor, so it can never be cut short.
    #[test]
    fn a_short_completion_is_never_judged() {
        let mut detector = RepetitionDetector::new();
        assert_eq!(detector.observe("OK. OK. OK. OK. OK."), None);
        assert!(detector.window.len() < MIN_TOKENS);
    }

    /// The verdict is reached once and then repeated unchanged, so a caller that has
    /// not acted on it yet cannot lose it or be given a different one.
    #[test]
    fn the_verdict_does_not_change_once_reached() {
        let mut detector = RepetitionDetector::new();
        let mut first = None;
        for _ in 0..200 {
            if let Some(v) = detector.observe("write ") {
                first = Some(v);
                break;
            }
        }
        let first = first.expect("caught");
        let again = detector
            .observe("something else entirely ")
            .expect("still caught");
        assert_eq!(first, again);
    }

    /// The window slides: a loop that has left the window must stop counting, or a
    /// long run would eventually trip on its own history.
    #[test]
    fn the_window_forgets_what_has_left_it() {
        let mut detector = RepetitionDetector::new();
        // Fill the window with a loop.
        for _ in 0..200 {
            detector.observe("write ");
        }
        assert!(detector.tripped.is_some(), "the loop was caught");

        // A fresh detector, filled with varied prose, must not trip.
        let mut prose = RepetitionDetector::new();
        for paragraph in VARIED_PROSE.iter().cycle().take(40) {
            assert_eq!(prose.observe(paragraph), None);
        }
    }

    /// The threshold is a share, not a count, so the same vocabulary at the same
    /// density trips in a full window and not in a nearly empty one.
    #[test]
    fn the_threshold_is_a_share_of_the_window() {
        // A cycle of ten words is a loop.
        let mut looping = RepetitionDetector::new();
        let mut verdict = None;
        for _ in 0..200 {
            verdict = looping.observe("one two three four five six seven eight nine ten ");
            if verdict.is_some() {
                break;
            }
        }
        assert!(verdict.is_some(), "a ten-word cycle is a loop");

        // Varied prose is not, however long the run.
        let mut prose = RepetitionDetector::new();
        for paragraph in VARIED_PROSE.iter().cycle().take(40) {
            assert_eq!(
                prose.observe(paragraph),
                None,
                "ordinary prose is not a loop"
            );
        }
    }

    #[test]
    fn tokenise_splits_on_punctuation_as_well_as_space() {
        assert_eq!(
            tokenise("write. write, write!"),
            vec!["write", "write", "write"]
        );
        assert_eq!(tokenise(""), Vec::<String>::new());
        assert_eq!(tokenise("   "), Vec::<String>::new());
    }

    /// Case is folded, so `Write` and `write` are one word -- and nothing else is.
    /// The English stemmer that used to live here is gone; this is what replaced it.
    #[test]
    fn tokenise_folds_case_and_nothing_else() {
        assert_eq!(
            tokenise("Write WRITE write"),
            vec!["write", "write", "write"]
        );
        assert_eq!(
            tokenise("write writing writes"),
            vec!["write", "writing", "writes"],
            "no stemming: these are three words, and the vocabulary measure does not need them folded"
        );
    }

    /// A script that does not use spaces is split per character, so a Japanese
    /// sentence is many tokens rather than one.
    #[test]
    fn unspaced_scripts_are_split_per_character() {
        assert_eq!(tokenise("書く"), vec!["書", "く"]);
        assert_eq!(tokenise("書く。書く。"), vec!["書", "く", "書", "く"]);
        // Latin text is untouched by that rule.
        assert_eq!(tokenise("hello world"), vec!["hello", "world"]);
    }
}
