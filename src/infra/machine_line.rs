//! The grammar shared by every `ATOMA_*` line atoma writes for a machine to read.
//!
//! There are three of them and they are read by three different callers: the
//! environment learns what an agent cost from `ATOMA_TOKEN_USAGE`, what a defect in
//! the tools file means from `ATOMA_CONFIG_FINDING`, and why an inference failed from
//! `ATOMA_LLM_ERROR`. Each is an `ATOMA_` name, a colon, then space-separated
//! `key=value`.
//!
//! **The fields are the contract and the sentence beside them is not.** A caller that
//! greps the English is a caller whose tooling atoma breaks by rewording a warning --
//! so a machine line carries fields and nothing else, and the prose stays free to be
//! good English for a person.
//!
//! The grammar has to be uniform even so, because a reader that receives a value with
//! a space in it sees two fields, and one that receives a stray comma sees two list
//! entries. [`field`] is what stops that, and it is one function rather than one per
//! line because a second encoder is a second answer to "what makes a value safe".

/// One value, with the three characters that would break the line's own grammar
/// percent-encoded as their UTF-8 bytes.
///
/// Whitespace would split one field into two, a comma would split one list entry into
/// two, and `%` has to be encoded for either of those to be reversible. Everything
/// else is written as it is, including the `_` and `*` that real names and globs are
/// made of: encoding those would make the common case unreadable to buy nothing.
///
/// A value that arrives already percent-encoded -- a URL-escaped message from a
/// provider, say -- is escaped again, because `%` is the escape character and one
/// that passes through unescaped makes the encoding ambiguous for every reader at
/// once. Ambiguity is the failure this exists to prevent; a `%25` where a reader
/// expected `%20` is legible, and two fields where one was meant is not.
pub(crate) fn field(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '%' => out.push_str("%25"),
            ',' => out.push_str("%2C"),
            c if c.is_whitespace() => {
                let mut buf = [0u8; 4];
                for byte in c.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{:02X}", byte));
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Several values as one comma-separated field. Empty is written as nothing.
pub(crate) fn field_list(values: &[String]) -> String {
    let mut out = String::new();
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&field(value));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{field, field_list};

    /// The three characters that would break the grammar, each encoded as the bytes
    /// that make it reversible.
    #[test]
    fn the_escape_character_and_the_list_separator_are_themselves_encoded() {
        assert_eq!(field("100%"), "100%25");
        assert_eq!(field("a,b"), "a%2Cb");
        assert_eq!(field("read_text_file"), "read_text_file");
    }

    /// A value with a space in it would otherwise become two fields for every reader
    /// at once, which is the failure the encoder exists to prevent.
    #[test]
    fn a_value_with_a_space_in_it_cannot_become_two_fields() {
        assert_eq!(field("my files").split_whitespace().count(), 1);
        assert_eq!(field("my files"), "my%20files");
        assert_eq!(field("read *"), "read%20*");
    }

    /// Nothing is encoded twice and nothing that need not be encoded is: the `_` and
    /// `*` of real names and globs survive byte for byte.
    #[test]
    fn ordinary_names_and_globs_survive_byte_for_byte() {
        assert_eq!(field("files_ro__*"), "files_ro__*");
        assert_eq!(field("github__get_issue"), "github__get_issue");
    }

    /// The list separator is the comma, so an empty list writes nothing rather than a
    /// separator that would read as one empty entry.
    #[test]
    fn an_empty_list_writes_nothing() {
        assert_eq!(field_list(&[]), "");
        assert_eq!(field_list(&["read".to_string()]), "read");
        assert_eq!(
            field_list(&["read".to_string(), "grep".to_string()]),
            "read,grep"
        );
    }

    /// A value that already looks encoded is escaped again, because a `%` that passes
    /// through is a `%` no reader can tell from the escape character.
    #[test]
    fn an_already_escaped_value_is_escaped_again() {
        assert_eq!(field("100%25"), "100%2525");
    }
}
