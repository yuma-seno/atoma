//! Command implementations for atoma-github subcommands.

use anyhow::Result;

pub async fn create_pr(
    _title: &str,
    _description: &str,
    _linked_issue: Option<u64>,
    _dispatch_agent: Option<&str>,
) -> Result<()> {
    eprintln!("atoma-github create-pr: not yet implemented");
    Ok(())
}

pub async fn push_commits(_pr: u64, _dispatch_agent: Option<&str>) -> Result<()> {
    eprintln!("atoma-github push-commits: not yet implemented");
    Ok(())
}

pub async fn create_sub_issue(
    _title: &str,
    _body: &str,
    _parent_issue: u64,
    _notify_agent: Option<&str>,
    _trigger_agent: Option<&str>,
) -> Result<()> {
    eprintln!("atoma-github create-sub-issue: not yet implemented");
    Ok(())
}

pub async fn add_label(_issue: u64, _label: &str) -> Result<()> {
    eprintln!("atoma-github add-label: not yet implemented");
    Ok(())
}

pub async fn close_issue(_issue: u64, _comment: Option<&str>) -> Result<()> {
    eprintln!("atoma-github close-issue: not yet implemented");
    Ok(())
}

pub async fn fetch_events(
    _type_: &str,
    _number: u64,
    _max_diff_chars: u32,
    _out: Option<&str>,
) -> Result<()> {
    eprintln!("atoma-github fetch-events: not yet implemented");
    Ok(())
}

pub async fn build_context(
    _events: &str,
    _agent_name: &str,
    _session: Option<&str>,
    _orchestration_file: Option<&str>,
    _out: Option<&str>,
) -> Result<()> {
    eprintln!("atoma-github build-context: not yet implemented");
    Ok(())
}