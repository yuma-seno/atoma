//! Command implementations for atoma-github subcommands.
//!
//! These delegate to the `gh` CLI, which reads GH_TOKEN / GITHUB_TOKEN
//! from the environment for authentication.

use anyhow::{Context, Result};
use std::process::Command;

fn gh(args: &[&str]) -> Result<String> {
    let output = Command::new("gh")
        .args(args)
        .output()
        .context("Failed to execute gh")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub async fn create_pr(
    title: &str,
    body: &str,
    _linked_issue: Option<u64>,
    _dispatch_agent: Option<&str>,
) -> Result<()> {
    let repo = std::env::var("GITHUB_REPOSITORY")
        .or_else(|_| {
            let remote = gh(&["remote", "get-url", "origin"]).unwrap_or_default();
            let r = remote.trim_start_matches("https://github.com/").trim_end_matches(".git");
            if r.is_empty() { Err(std::env::VarError::NotPresent) } else { Ok(r.to_string()) }
        })
        .context("GITHUB_REPOSITORY not set and could not derive from git remote")?;

    let branch = std::env::var("BRANCH")
        .or_else(|_| std::env::var("GITHUB_HEAD_REF"))
        .unwrap_or_default();

    let branch = if branch.is_empty() {
        let b = gh(&["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
        if b == "HEAD" {
            let branches = gh(&["branch", "--format=%(refname:short)", "--points-at=HEAD"]).unwrap_or_default();
            branches.lines().next().unwrap_or(&b).to_string()
        } else { b }
    } else { branch };

    if branch.is_empty() || branch == "HEAD" {
        anyhow::bail!("Cannot determine current branch name. Run from a named branch or set BRANCH env var.");
    }

    let _ = gh(&["push", "--set-upstream", "origin", &branch]);
    let mut args = vec!["pr", "create", "--repo", &repo, "--title", title, "--head", &branch];
    if !body.is_empty() { args.push("--body"); args.push(body); }
    let out = gh(&args)?;
    let pr_num: u64 = out.rsplit('/').next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let url = format!("https://github.com/{}/pull/{}", repo, pr_num);
    println!("{{\"number\":{},\"url\":\"{}\"}}", pr_num, url);
    eprintln!("atoma-github create-pr: created PR #{} — {}", pr_num, out);
    Ok(())
}

pub async fn push_commits(pr: u64, dispatch_agent: Option<&str>) -> Result<()> {
    let branch = gh(&["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|_| "HEAD".to_string());
    gh(&["push", "-u", "origin", &branch])?;
    if let Some(agent) = dispatch_agent {
        let repo = std::env::var("GITHUB_REPOSITORY").unwrap_or_default();
        let comment = format!("<!-- atoma:dispatch={} -->", agent);
        let _ = gh(&["pr", "comment", &pr.to_string(), "--repo", &repo, "--body", &comment]);
    }
    eprintln!("atoma-github push-commits: pushed to {}", branch);
    Ok(())
}

pub async fn create_sub_issue(
    title: &str,
    body: &str,
    parent_issue: u64,
    _notify_agent: Option<&str>,
    _trigger_agent: Option<&str>,
) -> Result<()> {
    let repo = std::env::var("GITHUB_REPOSITORY").unwrap_or_default();
    let full_body = format!("<!-- atoma:parent=#{} -->\n{}", parent_issue, body);
    let out = gh(&["issue", "create", "--repo", &repo, "--title", title, "--body", &full_body])?;
    let num = out.rsplit('/').next().unwrap_or("?");
    eprintln!("atoma-github create-sub-issue: created issue #{} — {}", num, out);
    Ok(())
}

pub async fn add_label(issue: u64, label: &str) -> Result<()> {
    let repo = std::env::var("GITHUB_REPOSITORY").unwrap_or_default();
    gh(&["label", "create", label, "--repo", &repo, "--force"])?;
    gh(&["issue", "edit", &issue.to_string(), "--repo", &repo, "--add-label", label])?;
    eprintln!("atoma-github add-label: added '{}' to #{}", label, issue);
    Ok(())
}

pub async fn close_issue(issue: u64, _comment: Option<&str>) -> Result<()> {
    let repo = std::env::var("GITHUB_REPOSITORY").unwrap_or_default();
    gh(&["issue", "close", &issue.to_string(), "--repo", &repo])?;
    eprintln!("atoma-github close-issue: closed #{}", issue);
    Ok(())
}

pub async fn fetch_events(
    type_: &str,
    number: u64,
    _max_diff_chars: u32,
    out_path: Option<&str>,
) -> Result<()> {
    let repo = std::env::var("GITHUB_REPOSITORY").unwrap_or_default();
    let resource = match type_ {
        "pr" => "pulls",
        _ => "issues",
    };
    let out = gh(&["api", &format!("repos/{}/{}/{}/timeline", repo, resource, number), "--jq", "."])?;
    if let Some(path) = out_path {
        std::fs::write(path, &out).context("Failed to write events file")?;
    } else {
        println!("{}", out);
    }
    eprintln!("atoma-github fetch-events: fetched timeline for {} #{}", type_, number);
    Ok(())
}

pub async fn build_context(
    events_path: &str,
    agent_name: &str,
    _session: Option<&str>,
    _orchestration_file: Option<&str>,
    out_path: Option<&str>,
) -> Result<()> {
    // Simple pass-through — the real context building is in the prepare action
    let events = std::fs::read_to_string(events_path)
        .context("Failed to read events file")?;
    let context = serde_json::json!({
        "agent": agent_name,
        "events": serde_json::from_str::<serde_json::Value>(&events).unwrap_or_default(),
    });
    let out = serde_json::to_string_pretty(&context)?;
    if let Some(path) = out_path {
        std::fs::write(path, &out).context("Failed to write context file")?;
    } else {
        println!("{}", out);
    }
    eprintln!("atoma-github build-context: built context for {}", agent_name);
    Ok(())
}