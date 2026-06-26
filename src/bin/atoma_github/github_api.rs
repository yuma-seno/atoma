//! GitHub API helper functions for atoma-github.

use anyhow::{Context, Result};

/// Get the GitHub token from environment.
pub fn get_token() -> Result<String> {
    std::env::var("GH_TOKEN")
        .or_else(|_| std::env::var("GITHUB_TOKEN"))
        .context("GH_TOKEN or GITHUB_TOKEN is required")
}

/// Get owner/repo from git remote or environment.
pub fn get_owner_repo() -> Result<(String, String)> {
    if let (Ok(owner), Ok(repo)) = (
        std::env::var("GITHUB_REPOSITORY_OWNER"),
        std::env::var("GITHUB_REPOSITORY_NAME"),
    ) {
        return Ok((owner, repo));
    }
    if let Ok(full) = std::env::var("GITHUB_REPOSITORY") {
        if let Some((owner, repo)) = full.split_once('/') {
            return Ok((owner.to_string(), repo.to_string()));
        }
    }
    // Fall back to gh repo view
    let output = std::process::Command::new("gh")
        .args(["repo", "view", "--json", "owner,name"])
        .output()
        .context("Failed to run gh repo view")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).context("Failed to parse gh repo view output")?;
    let owner = parsed["owner"]["login"]
        .as_str()
        .context("Missing owner in gh output")?
        .to_string();
    let name = parsed["name"]
        .as_str()
        .context("Missing name in gh output")?
        .to_string();
    Ok((owner, name))
}

/// Make a GitHub API GET request.
pub async fn api_get(path: &str) -> Result<serde_json::Value> {
    let token = get_token()?;
    let client = reqwest::Client::new();
    let url = format!("https://api.github.com{}", path);
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "atoma-github")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .with_context(|| format!("Failed to GET {}", url))?;
    let body: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse GitHub API response")?;
    Ok(body)
}

/// Make a GitHub API POST request.
pub async fn api_post(path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    let token = get_token()?;
    let client = reqwest::Client::new();
    let url = format!("https://api.github.com{}", path);
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "atoma-github")
        .header("Accept", "application/vnd.github.v3+json")
        .json(body)
        .send()
        .await
        .with_context(|| format!("Failed to POST {}", url))?;
    let body: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse GitHub API response")?;
    Ok(body)
}