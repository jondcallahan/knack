//! Asks GitHub what each skill's source folder holds now, and held when it
//! was installed.
//!
//! One recursive tree listing per repository (plus one per installed version
//! that differs from the latest), through `curl` like the update check. Signed
//! in with `GITHUB_TOKEN`, `GH_TOKEN` or the GitHub CLI when available; GitHub
//! allows 60 unsigned requests an hour otherwise.

use std::collections::HashMap;
use std::io::Write as _;
use std::process::{Command, Stdio};
use std::rc::Rc;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::library::{Baseline, Check, InstalledForm, Remote, RemoteEntry, Source, installed_tree};

const API: &str = "https://api.github.com";

/// What GitHub reports for each source. Sources it couldn't check (offline,
/// rate limited, a private repository) are left out.
pub fn check(sources: &[Source]) -> Remote {
    let token = token();
    let mut cache: HashMap<(String, String), Option<Rc<Tree>>> = HashMap::new();
    let mut listing = |repo: &str, treeish: &str| {
        cache
            .entry((repo.to_string(), treeish.to_string()))
            .or_insert_with(|| {
                let endpoint = format!("repos/{repo}/git/trees/{}?recursive=1", encode(treeish));
                // A missing repository reads as private or unreachable
                // rather than as removed, so it is left unknown.
                get::<Tree>(&endpoint, token.as_deref())
                    .ok()
                    .flatten()
                    .filter(|tree| !tree.truncated)
                    .map(Rc::new)
            })
            .clone()
    };

    let mut remote = Remote::new();
    for source in sources {
        let Some(head) = listing(&source.repo, source.git_ref.as_deref().unwrap_or("HEAD")) else {
            continue;
        };
        let latest = match head.folder(&source.path).map(|entries| installed_tree(&entries)) {
            Some(InstalledForm::Tree(tree)) => Some(tree),
            Some(InstalledForm::NotASkill) | None => None,
            Some(InstalledForm::Unknowable) => continue,
        };
        let as_tree = |installed| match installed {
            InstalledForm::Tree(tree) => Some(tree),
            InstalledForm::NotASkill | InstalledForm::Unknowable => None,
        };
        let installed = match &source.baseline {
            Some(Baseline::Folder(sha)) if head.folder_sha(&source.path) == Some(sha) => latest.clone(),
            Some(Baseline::Folder(sha)) => listing(&source.repo, sha)
                .and_then(|tree| tree.folder(""))
                .and_then(|entries| as_tree(installed_tree(&entries))),
            Some(Baseline::Commit(commit)) => listing(&source.repo, commit)
                .and_then(|tree| tree.folder(&source.path))
                .and_then(|entries| as_tree(installed_tree(&entries))),
            None => None,
        };
        remote.insert(source.key(), Check { latest, installed });
    }
    remote
}

#[derive(Deserialize)]
struct Tree {
    sha: String,
    tree: Vec<Entry>,
    #[serde(default)]
    truncated: bool,
}

#[derive(Deserialize)]
struct Entry {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}

impl Tree {
    /// The entries under `path`, relative to it; `None` when it isn't a folder.
    fn folder(&self, path: &str) -> Option<Vec<RemoteEntry>> {
        if !path.is_empty() && !self.tree.iter().any(|e| e.kind == "tree" && e.path == path) {
            return None;
        }
        let prefix = if path.is_empty() { String::new() } else { format!("{path}/") };
        Some(
            self.tree
                .iter()
                .filter_map(|e| {
                    Some(RemoteEntry {
                        path: e.path.strip_prefix(&prefix)?.to_string(),
                        mode: e.mode.clone(),
                        sha: e.sha.clone(),
                    })
                })
                .collect(),
        )
    }

    fn folder_sha(&self, path: &str) -> Option<&String> {
        if path.is_empty() {
            return Some(&self.sha);
        }
        self.tree
            .iter()
            .find(|e| e.kind == "tree" && e.path == path)
            .map(|e| &e.sha)
    }
}

/// GET an API endpoint; `None` when GitHub says it doesn't exist.
fn get<T: for<'de> Deserialize<'de>>(endpoint: &str, token: Option<&str>) -> Result<Option<T>> {
    let url = format!("{API}/{endpoint}");
    let mut child = Command::new("/usr/bin/curl")
        .args([
            "-sS",
            "--proto",
            "=https",
            "--max-time",
            "10",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "X-GitHub-Api-Version: 2022-11-28",
            "-A",
            "Knack",
            "-w",
            "\n%{http_code}",
            "--config",
            "-",
            &url,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("run curl")?;
    // The token goes through stdin so it stays out of the process list.
    if let Some(mut stdin) = child.stdin.take()
        && let Some(token) = token
    {
        let _ = writeln!(stdin, "header = \"Authorization: Bearer {token}\"");
    }
    let output = child.wait_with_output().context("wait for curl")?;
    if !output.status.success() {
        bail!(
            "GitHub request failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let (body, status) = text.rsplit_once('\n').unwrap_or(("", &text));
    match status.trim() {
        "200" => Ok(Some(
            serde_json::from_str(body).context("parse GitHub response")?,
        )),
        "404" => Ok(None),
        code => bail!("GitHub answered {code}"),
    }
}

/// A GitHub token from the environment or the GitHub CLI, when signed in.
fn token() -> Option<String> {
    token_from_env().or_else(token_from_gh).filter(|t| {
        // It is written into curl's config, so allow only token characters.
        !t.is_empty()
            && t.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
    })
}

fn token_from_env() -> Option<String> {
    ["GITHUB_TOKEN", "GH_TOKEN"]
        .iter()
        .filter_map(|var| std::env::var(var).ok())
        .map(|t| t.trim().to_string())
        .find(|t| !t.is_empty())
}

fn token_from_gh() -> Option<String> {
    // Apps opened from Finder don't inherit the shell's PATH.
    ["/opt/homebrew/bin/gh", "/usr/local/bin/gh", "gh"]
        .iter()
        .find_map(|gh| {
            let output = Command::new(gh)
                .args(["auth", "token", "--hostname", "github.com"])
                .stderr(Stdio::null())
                .output()
                .ok()?;
            let token = String::from_utf8(output.stdout).ok()?.trim().to_string();
            (output.status.success() && !token.is_empty()).then_some(token)
        })
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_refs_and_paths() {
        assert_eq!(encode("feature/x y"), "feature%2Fx%20y");
    }
}
