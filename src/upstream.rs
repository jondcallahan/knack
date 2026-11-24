//! Asks GitHub what each skill's source folder holds now, and held when it
//! was installed.
//!
//! One recursive tree listing per repository (plus one per installed version
//! that differs from the latest), through `curl` like the update check. Signed
//! in with `GITHUB_TOKEN`, `GH_TOKEN` or the GitHub CLI when available; GitHub
//! allows 60 unsigned requests an hour otherwise.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::Path;
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

/// What `skills update` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOutcome {
    Updated,
    AlreadyCurrent,
}

/// Install the latest version of the skill named `name` (its folder) with the
/// `skills` CLI. `root` is the project, or the home folder for a global skill.
pub fn update(source: &Source, name: &str, root: &Path, home: &Path) -> Result<UpdateOutcome> {
    let npx = find_npx()
        .context("Updating needs Node.js. Install it from nodejs.org, then try again.")?;
    // npx prefers a `skills` installed in the current project, which would
    // let a project choose what runs. Resolve it from a folder of Knack's own.
    let prefix = std::env::temp_dir().join("knack-npx");
    std::fs::create_dir_all(&prefix).context("create npx folder")?;
    let bin = npx.parent().map(Path::to_path_buf).unwrap_or_default();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let output = Command::new(&npx)
        .arg("--prefix")
        .arg(&prefix)
        .args(["-y", "skills", "update"])
        .arg(if source.global { "-g" } else { "-p" })
        .arg(name)
        .current_dir(root)
        .env("HOME", home)
        .env("PATH", path)
        // Knack sends no analytics, so neither does the CLI on its behalf.
        .env("DO_NOT_TRACK", "1")
        .env("DISABLE_TELEMETRY", "1")
        .stdin(Stdio::null())
        .output()
        .context("run the skills CLI")?;
    let text = strip_ansi(&format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ));
    outcome(output.status.success(), &text, name)
}

/// Read what the CLI printed: it exits 0 even when nothing matched or a
/// check failed.
fn outcome(success: bool, text: &str, name: &str) -> Result<UpdateOutcome> {
    if let Some(failure) = text.lines().map(str::trim).find(|l| l.starts_with('✗')) {
        bail!("Couldn't update {name}: {}", failure.trim_start_matches('✗').trim());
    }
    if success && text.contains("✓ Updated") {
        return Ok(UpdateOutcome::Updated);
    }
    if success && text.contains("up to date") {
        return Ok(UpdateOutcome::AlreadyCurrent);
    }
    let last = text
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && !l.starts_with("npm notice"))
        .unwrap_or("");
    bail!("Couldn't update {name}: {last}")
}

/// `npx`, found the way the user's terminal would find it. Apps opened from
/// Finder don't get the shell's PATH, so ask the login shell.
fn find_npx() -> Option<std::path::PathBuf> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| s.starts_with('/') && Path::new(s).exists())
        .unwrap_or_else(|| "/bin/zsh".into());
    let from_shell = Command::new(shell)
        .args(["-l", "-c", "command -v npx"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|out| out.lines().map(str::trim).rfind(|l| l.starts_with('/')).map(Into::into));
    from_shell.or_else(|| {
        ["/opt/homebrew/bin/npx", "/usr/local/bin/npx"]
            .iter()
            .map(std::path::PathBuf::from)
            .find(|p| p.exists())
    })
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // CSI sequences end with a letter.
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
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
    fn reads_what_the_cli_did() {
        let ok = |text| outcome(true, text, "x").ok();
        assert_eq!(ok("Updating x…\n  ✓ Updated x\n"), Some(UpdateOutcome::Updated));
        assert_eq!(ok("✓ All global skills are up to date"), Some(UpdateOutcome::AlreadyCurrent));
        let err = outcome(true, "Updating x…\n\nNo installed skills found matching: x\n", "x");
        assert_eq!(
            err.unwrap_err().to_string(),
            "Couldn't update x: No installed skills found matching: x"
        );
        assert!(outcome(false, "✓ Updated x", "x").is_err());
        let failed = "✗ Failed to check skills from acme/skills\n✓ All global skills are up to date\n";
        assert_eq!(
            outcome(true, failed, "x").unwrap_err().to_string(),
            "Couldn't update x: Failed to check skills from acme/skills"
        );
    }

    #[test]
    fn strips_terminal_colors() {
        assert_eq!(strip_ansi("\u{1b}[31merror\u{1b}[0m: no"), "error: no");
    }

    #[test]
    fn encodes_refs_and_paths() {
        assert_eq!(encode("feature/x y"), "feature%2Fx%20y");
    }
}
