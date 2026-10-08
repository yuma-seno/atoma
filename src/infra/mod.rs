pub mod config;
pub mod credentials;
pub mod hooks;
pub mod llm;
// Crate-internal like `llm::shared`: the grammar is for atoma's own lines, and a
// caller outside that links Rust is a caller that would rather grep the output.
pub(crate) mod machine_line;
pub mod mcp;
pub mod persistence;
pub mod process_protection;
pub mod template;
pub mod timeouts;
