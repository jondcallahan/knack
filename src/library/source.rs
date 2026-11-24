//! Skills installed from GitHub by the `skills` CLI, and how far each copy has
//! moved from its source.
//!
//! The CLI records what it installed in a lock file: `~/.agents/.skill-lock.json`
//! for global skills and `skills-lock.json` at a project's root. Global entries
//! keep the Git tree hash of the skill's folder in the repository
//! (`skillFolderHash`); project entries keep a SHA-256 of the installed files
//! (`computedHash`) and, lately, the commit.
//!
//! Installing drops some files (`metadata.json`, `__pycache__`) and copies
//! links as files, so a folder on disk is compared with the *installed form*
//! of a repository folder: the Git tree hash of what installing it would write,
//! worked out from GitHub's tree listing without downloading files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::RootId;

/// Files and folders the `skills` CLI leaves out of a skill's content.
const EXCLUDE_FILES: [&str; 1] = ["metadata.json"];
const EXCLUDE_DIRS: [&str; 3] = [".git", "__pycache__", "__pypackages__"];

/// Where a skill came from and what was installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// `owner/repo` on GitHub.
    pub repo: String,
    /// The branch, tag or commit it tracks; the default branch when unset.
    pub git_ref: Option<String>,
    /// The skill's folder in the repository, `""` for the repository root.
    pub path: String,
    /// Global skills update with `-g`.
    pub global: bool,
    /// The Git tree hash of the folder on disk.
    pub tree: String,
    /// What the lock file alone says about the folder on disk.
    pub local: Local,
    /// Where GitHub keeps the version that was installed.
    pub baseline: Option<Baseline>,
}

/// The folder on disk against what was installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Local {
    AsInstalled,
    Edited,
    /// The lock file keeps no hash of the installed files; GitHub can tell.
    Unknown,
}

/// The source on GitHub against what was installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceState {
    /// Not asked yet, or GitHub couldn't answer.
    Unchecked,
    /// Same files as the folder on disk.
    Same,
    Unchanged,
    Updated,
    /// Differs from the folder on disk; what was installed is unknown.
    Unknown,
    Gone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Baseline {
    /// The Git tree of the skill's folder.
    Folder(String),
    /// The commit it was installed from.
    Commit(String),
}

/// What GitHub reported for a skill's source, as installed-form tree hashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// The current version; `None` when the skill is gone from the source.
    pub latest: Option<String>,
    /// The version that was installed, when GitHub could tell.
    pub installed: Option<String>,
}

/// One entry of a GitHub tree listing, with its path inside the skill.
pub struct RemoteEntry {
    pub path: String,
    pub mode: String,
    pub sha: String,
}

/// How a skill's folder compares with its source on GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upstream {
    /// Same files as the source.
    Current,
    /// The source changed; the folder is as installed.
    Behind,
    /// The folder was edited since install; the source didn't change (or
    /// hasn't been checked yet).
    Edited,
    /// Both the folder and the source changed since install.
    Diverged,
    /// The source no longer has this skill.
    Gone,
}

/// What GitHub reported, by [`Source::key`].
pub type Remote = HashMap<String, Check>;

impl Source {
    /// The key for this source's folder in a [`Remote`].
    /// Copies installed at different versions get different keys.
    pub fn key(&self) -> String {
        let baseline = match &self.baseline {
            Some(Baseline::Folder(sha) | Baseline::Commit(sha)) => sha.as_str(),
            None => "",
        };
        format!(
            "{}@{}:{}#{baseline}",
            self.repo,
            self.git_ref.as_deref().unwrap_or(""),
            self.path
        )
    }

    pub fn url(&self) -> String {
        let git_ref = self.git_ref.as_deref().unwrap_or("HEAD");
        if self.path.is_empty() {
            format!("https://github.com/{}/tree/{git_ref}", self.repo)
        } else {
            format!(
                "https://github.com/{}/tree/{git_ref}/{}",
                self.repo, self.path
            )
        }
    }

    /// The folder no longer has the files it had when scanned.
    pub fn changed_since_scan(&self, dir: &Path) -> bool {
        tree_hash(&collect(dir)) != self.tree
    }

    pub fn status(&self, remote: &Remote) -> Upstream {
        let check = remote.get(&self.key());
        let reported = check.and_then(|c| c.installed.as_ref());
        // GitHub's copy of what was installed settles what the lock can't.
        let local = match (self.local, reported) {
            (Local::Unknown, Some(installed)) if *installed == self.tree => Local::AsInstalled,
            (Local::Unknown, Some(_)) => Local::Edited,
            (local, _) => local,
        };
        let installed = match local {
            Local::AsInstalled => Some(&self.tree),
            _ => reported,
        };
        let source = match check.map(|c| &c.latest) {
            None => SourceState::Unchecked,
            Some(None) => SourceState::Gone,
            Some(Some(latest)) if *latest == self.tree => SourceState::Same,
            Some(Some(latest)) => match installed {
                Some(installed) if installed == latest => SourceState::Unchanged,
                Some(_) => SourceState::Updated,
                None => SourceState::Unknown,
            },
        };
        match (local, source) {
            (_, SourceState::Gone) => Upstream::Gone,
            (_, SourceState::Same) => Upstream::Current,
            (Local::AsInstalled, SourceState::Updated) => Upstream::Behind,
            (Local::AsInstalled, SourceState::Unchecked | SourceState::Unchanged | SourceState::Unknown) => {
                Upstream::Current
            }
            (Local::Edited, SourceState::Updated) => Upstream::Diverged,
            (Local::Edited, SourceState::Unchecked | SourceState::Unchanged | SourceState::Unknown) => {
                Upstream::Edited
            }
            (Local::Unknown, SourceState::Unchecked) => Upstream::Current,
            // Differs from GitHub, and nothing says what was installed:
            // report only that the copy isn't what GitHub has.
            (Local::Unknown, SourceState::Unchanged | SourceState::Updated | SourceState::Unknown) => {
                Upstream::Edited
            }
        }
    }
}

#[derive(Deserialize)]
struct LockFile {
    #[serde(default)]
    skills: HashMap<String, LockEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockEntry {
    #[serde(default)]
    source: String,
    #[serde(default)]
    source_type: String,
    #[serde(default)]
    source_url: String,
    #[serde(default, rename = "ref")]
    git_ref: Option<String>,
    #[serde(default)]
    skill_path: String,
    #[serde(default)]
    skill_folder_hash: String,
    #[serde(default)]
    computed_hash: String,
    #[serde(default)]
    commit: String,
}

/// The lock entries for one root, by skill folder name.
#[derive(Default)]
pub struct Locks {
    global: bool,
    entries: HashMap<String, LockEntry>,
}

impl Locks {
    pub fn read(root: &RootId, path: &Path) -> Self {
        let file: PathBuf = match root {
            RootId::Global => path.join(".agents/.skill-lock.json"),
            RootId::Project(_) => path.join("skills-lock.json"),
        };
        let entries = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<LockFile>(&text).ok())
            .map(|lock| lock.skills)
            .unwrap_or_default();
        Self {
            global: root.is_global(),
            entries,
        }
    }

    /// The source of the skill in `dir`, when the lock file installed it from
    /// GitHub.
    pub fn source(&self, folder: &str, dir: &Path) -> Option<Source> {
        let entry = self.entries.get(folder)?;
        if entry.source_type != "github" {
            return None;
        }
        let repo = github_repo(&entry.source, &entry.source_url)?;
        let path = folder_path(&entry.skill_path);
        let files = collect(dir);
        let tree = tree_hash(&files);
        let local = if is_hex(&entry.computed_hash, 64) {
            let locked = entry.computed_hash.to_lowercase();
            // Version 1 lock files hold the CLI's older hash.
            if content_hash(&files) == locked || legacy_content_hash(&files) == locked {
                Local::AsInstalled
            } else {
                Local::Edited
            }
        } else {
            Local::Unknown
        };
        let baseline = if is_hex(&entry.skill_folder_hash, 40) {
            Some(Baseline::Folder(entry.skill_folder_hash.to_lowercase()))
        } else if is_hex(&entry.commit, 40) {
            Some(Baseline::Commit(entry.commit.to_lowercase()))
        } else {
            None
        };
        Some(Source {
            repo,
            git_ref: entry.git_ref.clone().filter(|r| !r.is_empty()),
            path,
            global: self.global,
            tree,
            local,
            baseline,
        })
    }
}

/// `owner/repo` from a lock entry's `source` or `sourceUrl`.
fn github_repo(source: &str, url: &str) -> Option<String> {
    let from_url = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("git@github.com:"))
        .map(|rest| rest.trim_end_matches('/').trim_end_matches(".git"));
    let candidate = from_url.unwrap_or(source);
    let mut parts = candidate.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    let valid = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    (valid(owner) && valid(repo)).then(|| format!("{owner}/{repo}"))
}

/// The skill's folder from its `SKILL.md` path in the repository.
fn folder_path(skill_path: &str) -> String {
    let path = skill_path.replace('\\', "/");
    let lower = path.to_lowercase();
    let folder = if lower.ends_with("skill.md") {
        &path[..path.len() - "skill.md".len()]
    } else {
        &path[..]
    };
    folder.trim_matches('/').to_string()
}

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A file in a skill folder, by its path relative to the folder.
struct File {
    path: String,
    content: Vec<u8>,
    executable: bool,
}

/// Every file in a skill folder as the `skills` CLI sees it. A disabled
/// `SKILL.md.disabled` counts as the `SKILL.md` it was installed as.
fn collect(dir: &Path) -> Vec<File> {
    let root = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut files = Vec::new();
    walk(&root, &root, "", &mut Vec::new(), &mut files);
    for file in &mut files {
        if file.path == "SKILL.md.disabled" {
            file.path = "SKILL.md".into();
        }
    }
    files
}

fn walk(root: &Path, dir: &Path, prefix: &str, ancestors: &mut Vec<PathBuf>, out: &mut Vec<File>) {
    let real = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if ancestors.contains(&real) {
        return;
    }
    ancestors.push(real);
    let Ok(entries) = std::fs::read_dir(dir) else {
        ancestors.pop();
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let target = if kind.is_symlink() {
            match std::fs::canonicalize(&path) {
                // Like the CLI, follow links only within the skill.
                Ok(target) if target.starts_with(root) => target,
                _ => continue,
            }
        } else {
            path.clone()
        };
        let Ok(meta) = std::fs::metadata(&target) else {
            continue;
        };
        if meta.is_dir() {
            if !EXCLUDE_DIRS.contains(&name.as_str()) {
                walk(root, &path, &format!("{prefix}{name}/"), ancestors, out);
            }
        } else if meta.is_file() && !EXCLUDE_FILES.contains(&name.as_str()) {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(content) = std::fs::read(&target) {
                out.push(File {
                    path: format!("{prefix}{name}"),
                    content,
                    executable: meta.permissions().mode() & 0o111 != 0,
                });
            }
        }
    }
    ancestors.pop();
}

/// The Git tree hash of the files on disk.
fn tree_hash(files: &[File]) -> String {
    let blobs: Vec<Blob> = files
        .iter()
        .map(|f| Blob {
            path: f.path.clone(),
            executable: f.executable,
            sha: object("blob", &f.content),
        })
        .collect();
    hex(&tree(blobs.iter().map(|b| (b.path.as_str(), b)).collect()))
}

/// A repository folder once installed.
#[derive(Debug, PartialEq, Eq)]
pub enum InstalledForm {
    /// Its Git tree hash.
    Tree(String),
    /// It has no `SKILL.md`, so it isn't a skill.
    NotASkill,
    /// It holds links, whose installed form depends on what they point to.
    Unknowable,
}

/// The installed form of a repository folder, from GitHub's listing of it.
pub fn installed_tree(entries: &[RemoteEntry]) -> InstalledForm {
    if !entries.iter().any(|e| e.path == "SKILL.md" && e.mode.starts_with("100")) {
        return InstalledForm::NotASkill;
    }
    let mut blobs = Vec::new();
    for entry in entries {
        let parts: Vec<&str> = entry.path.split('/').collect();
        let excluded = parts.iter().enumerate().any(|(i, part)| {
            if i + 1 < parts.len() {
                EXCLUDE_DIRS.contains(part)
            } else {
                EXCLUDE_FILES.contains(part)
            }
        });
        match entry.mode.as_str() {
            _ if excluded => {}
            "100644" | "100755" => blobs.push(Blob {
                path: entry.path.clone(),
                executable: entry.mode == "100755",
                sha: match unhex(&entry.sha) {
                    Some(sha) => sha,
                    None => return InstalledForm::Unknowable,
                },
            }),
            "120000" => return InstalledForm::Unknowable,
            // Folders come with their files; submodules don't install.
            _ => {}
        }
    }
    InstalledForm::Tree(hex(&tree(blobs.iter().map(|b| (b.path.as_str(), b)).collect())))
}

struct Blob {
    path: String,
    executable: bool,
    sha: [u8; 20],
}

fn tree(files: Vec<(&str, &Blob)>) -> [u8; 20] {
    // name -> (is_dir, mode and hash)
    let mut entries: Vec<(String, bool, Vec<u8>)> = Vec::new();
    let mut dirs: HashMap<&str, Vec<(&str, &Blob)>> = HashMap::new();
    for (path, blob) in files {
        match path.split_once('/') {
            Some((dir, rest)) => dirs.entry(dir).or_default().push((rest, blob)),
            None => {
                let mode = if blob.executable { "100755" } else { "100644" };
                let mut entry = format!("{mode} {path}\0").into_bytes();
                entry.extend_from_slice(&blob.sha);
                entries.push((path.to_string(), false, entry));
            }
        }
    }
    for (dir, files) in dirs {
        let mut entry = format!("40000 {dir}\0").into_bytes();
        entry.extend_from_slice(&tree(files));
        entries.push((dir.to_string(), true, entry));
    }
    // Git orders a folder as if its name ended in "/".
    entries.sort_by(|a, b| {
        let key = |(name, is_dir, _): &(String, bool, Vec<u8>)| {
            let mut key = name.as_bytes().to_vec();
            if *is_dir {
                key.push(b'/');
            }
            key
        };
        key(a).cmp(&key(b))
    });
    let body: Vec<u8> = entries.into_iter().flat_map(|(_, _, e)| e).collect();
    object("tree", &body)
}

fn object(kind: &str, body: &[u8]) -> [u8; 20] {
    let mut hasher = sha1_smol::Sha1::new();
    hasher.update(format!("{kind} {}\0", body.len()).as_bytes());
    hasher.update(body);
    hasher.digest().bytes()
}

/// The `computedHash` the `skills` CLI writes to `skills-lock.json`.
fn content_hash(files: &[File]) -> String {
    let mut entries: Vec<(&[u8], Vec<u8>)> = files
        .iter()
        .map(|f| (f.path.as_bytes(), normalize_line_endings(&f.content)))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let mut hasher = Sha256::new();
    for (path, content) in entries {
        hasher.update(path);
        hasher.update(b"\0");
        hasher.update(content.len().to_string().as_bytes());
        hasher.update(b"\0");
        hasher.update(&content);
    }
    hex(&hasher.finalize())
}

/// The `computedHash` in version 1 lock files: each path and its bytes, in
/// JavaScript's `localeCompare` order (case-insensitive first, for paths).
fn legacy_content_hash(files: &[File]) -> String {
    let mut sorted: Vec<&File> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()).then(a.path.cmp(&b.path)));
    let mut hasher = Sha256::new();
    for file in sorted {
        hasher.update(file.path.as_bytes());
        hasher.update(&file.content);
    }
    hex(&hasher.finalize())
}

fn normalize_line_endings(content: &[u8]) -> Vec<u8> {
    if content.contains(&0) || !content.windows(2).any(|w| w == b"\r\n") {
        return content.to_vec();
    }
    let mut out = Vec::with_capacity(content.len());
    let mut i = 0;
    while i < content.len() {
        if content[i] == b'\r' && content.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(content[i]);
        i += 1;
    }
    out
}

fn unhex(text: &str) -> Option<[u8; 20]> {
    if !is_hex(text, 40) {
        return None;
    }
    let mut out = [0; 20];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    /// A skill folder whose hashes were taken with `git write-tree` and the
    /// `skills` CLI's `computeSkillFolderHash`.
    fn demo() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("demo");
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::write(dir.join("SKILL.md"), "---\nname: demo\n---\nDo it.\r\n").unwrap();
        fs::write(dir.join("scripts/run.sh"), "echo hi\n").unwrap();
        fs::set_permissions(
            dir.join("scripts/run.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        // Sorts before the `scripts` folder in Git's order, after it bytewise.
        fs::write(dir.join("scripts.md"), "notes\n").unwrap();
        fs::write(dir.join("metadata.json"), "{}\n").unwrap();
        (temp, dir)
    }

    const DEMO_TREE: &str = "82dd5076db005dccb397dc1b686a7b97247c7dec";
    const DEMO_CONTENT: &str = "1451da95aa21527d9fc002f85f06ee2f118de5f9fcc01ce2bb30a629ebb0d371";

    #[test]
    fn hashes_match_git_and_the_skills_cli() {
        let (_temp, dir) = demo();
        let files = collect(&dir);
        assert_eq!(tree_hash(&files), DEMO_TREE);
        assert_eq!(content_hash(&files), DEMO_CONTENT);
    }

    #[test]
    fn a_disabled_skill_hashes_as_installed() {
        let (_temp, dir) = demo();
        fs::rename(dir.join("SKILL.md"), dir.join("SKILL.md.disabled")).unwrap();
        assert_eq!(tree_hash(&collect(&dir)), DEMO_TREE);
    }

    fn global_lock(home: &Path, hash: &str) {
        fs::create_dir_all(home.join(".agents")).unwrap();
        fs::write(
            home.join(".agents/.skill-lock.json"),
            format!(
                r#"{{"version":3,"skills":{{"demo":{{"source":"acme/skills","sourceType":"github",
                "sourceUrl":"https://github.com/acme/skills.git","skillPath":"skills/demo/SKILL.md",
                "skillFolderHash":"{hash}"}}}}}}"#
            ),
        )
        .unwrap();
    }

    fn check(latest: Option<&str>, installed: Option<&str>) -> Check {
        Check {
            latest: latest.map(str::to_string),
            installed: installed.map(str::to_string),
        }
    }

    #[test]
    fn global_lock_tells_update_from_local_edit() {
        let (temp, dir) = demo();
        // The repository folder, which has `metadata.json`; installed, it
        // hashes as DEMO_TREE.
        let raw = "205c1d1a89d43129d4cd0033fbf7ca107f05c524";
        global_lock(temp.path(), raw);
        let locks = Locks::read(&RootId::Global, temp.path());
        let source = locks.source("demo", &dir).unwrap();
        assert_eq!(source.repo, "acme/skills");
        assert_eq!(source.path, "skills/demo");
        assert_eq!(
            source.url(),
            "https://github.com/acme/skills/tree/HEAD/skills/demo"
        );
        assert_eq!(source.baseline, Some(Baseline::Folder(raw.into())));
        assert_eq!(source.local, Local::Unknown);
        assert!(!source.changed_since_scan(&dir));

        let newer = "f".repeat(40);
        let mut remote = Remote::new();
        assert_eq!(source.status(&remote), Upstream::Current, "not checked yet");
        remote.insert(source.key(), check(Some(DEMO_TREE), Some(DEMO_TREE)));
        assert_eq!(source.status(&remote), Upstream::Current);
        remote.insert(source.key(), check(Some(&newer), Some(DEMO_TREE)));
        assert_eq!(source.status(&remote), Upstream::Behind);
        remote.insert(source.key(), check(None, Some(DEMO_TREE)));
        assert_eq!(source.status(&remote), Upstream::Gone);

        fs::write(dir.join("SKILL.md"), "edited\n").unwrap();
        assert!(source.changed_since_scan(&dir));
        let source = locks.source("demo", &dir).unwrap();
        remote.insert(source.key(), check(Some(DEMO_TREE), Some(DEMO_TREE)));
        assert_eq!(source.status(&remote), Upstream::Edited);
        remote.insert(source.key(), check(Some(&newer), Some(DEMO_TREE)));
        assert_eq!(source.status(&remote), Upstream::Diverged);
        remote.insert(source.key(), check(Some(&newer), None));
        assert_eq!(source.status(&remote), Upstream::Edited, "installed version unknown");
    }

    #[test]
    fn installed_tree_drops_what_installing_drops() {
        let entry = |path: &str, mode: &str, sha: &str| RemoteEntry {
            path: path.into(),
            mode: mode.into(),
            sha: sha.into(),
        };
        // `git ls-tree -r` of the demo folder with its `metadata.json`.
        let mut entries = vec![
            entry("SKILL.md", "100644", "602f2b0a601bd598c418a937a307d7b86a0ea36c"),
            entry("metadata.json", "100644", "0967ef424bce6791893e9a57bb952f80fd536e93"),
            entry("scripts", "040000", "0000000000000000000000000000000000000000"),
            entry("scripts.md", "100644", "bfa655111293037a5564088d1a9bbca4cbcf446b"),
            entry("scripts/run.sh", "100755", "8b2fe5434fec16870a71cd8b272c7fcf6d352536"),
        ];
        assert_eq!(installed_tree(&entries), InstalledForm::Tree(DEMO_TREE.into()));
        entries.push(entry("link.md", "120000", "0967ef424bce6791893e9a57bb952f80fd536e93"));
        assert_eq!(installed_tree(&entries), InstalledForm::Unknowable);
        assert_eq!(installed_tree(&entries[1..]), InstalledForm::NotASkill, "a link, no SKILL.md");
    }

    #[test]
    fn project_lock_uses_the_content_hash() {
        let (temp, dir) = demo();
        let lock = |commit: &str| {
            fs::write(
                temp.path().join("skills-lock.json"),
                format!(
                    r#"{{"version":2,"skills":{{"demo":{{"source":"acme/skills","sourceType":"github",
                    "skillPath":"skills/demo/SKILL.md","computedHash":"{DEMO_CONTENT}","commit":"{commit}"}}}}}}"#
                ),
            )
            .unwrap();
        };
        lock("");
        let root = RootId::Project(temp.path().to_path_buf());
        let source = Locks::read(&root, temp.path())
            .source("demo", &dir)
            .unwrap();
        assert_eq!(source.local, Local::AsInstalled);
        assert_eq!(source.baseline, None);
        let newer = "f".repeat(40);
        let mut remote = Remote::new();
        remote.insert(source.key(), check(Some(&newer), None));
        assert_eq!(source.status(&remote), Upstream::Behind);

        fs::write(dir.join("scripts.md"), "changed\n").unwrap();
        let source = Locks::read(&root, temp.path())
            .source("demo", &dir)
            .unwrap();
        assert_eq!(source.local, Local::Edited);
        assert_eq!(source.status(&Remote::new()), Upstream::Edited, "offline");
        assert_eq!(source.status(&remote), Upstream::Edited, "installed version unknown");

        // With the commit, GitHub tells what was installed.
        lock(&"a".repeat(40));
        let source = Locks::read(&root, temp.path())
            .source("demo", &dir)
            .unwrap();
        assert_eq!(source.baseline, Some(Baseline::Commit("a".repeat(40))));
        assert!(!remote.contains_key(&source.key()), "another version, another key");
        remote.insert(source.key(), check(Some(&newer), Some(DEMO_TREE)));
        assert_eq!(source.status(&remote), Upstream::Diverged);
    }

    #[test]
    fn version_1_lock_uses_the_legacy_hash() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("demo");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "hi\n").unwrap();
        // The CLI's legacy hash of this folder: sha256 of "SKILL.md" then its bytes.
        let expected = "b76971028744ae881c07b3b954a74282c4f2c1a8f3392f4676a8ad4dd2e1adc6";
        fs::write(
            temp.path().join("skills-lock.json"),
            format!(
                r#"{{"version":1,"skills":{{"demo":{{"source":"acme/skills","sourceType":"github",
                "skillPath":"skills/demo/SKILL.md","computedHash":"{expected}"}}}}}}"#
            ),
        )
        .unwrap();
        let root = RootId::Project(temp.path().to_path_buf());
        let source = Locks::read(&root, temp.path()).source("demo", &dir).unwrap();
        assert_eq!(source.local, Local::AsInstalled);
    }

    #[test]
    fn ignores_skills_not_from_github() {
        let (temp, dir) = demo();
        fs::create_dir_all(temp.path().join(".agents")).unwrap();
        fs::write(
            temp.path().join(".agents/.skill-lock.json"),
            r#"{"version":3,"skills":{"demo":{"source":"x","sourceType":"local"}}}"#,
        )
        .unwrap();
        assert!(
            Locks::read(&RootId::Global, temp.path())
                .source("demo", &dir)
                .is_none()
        );
        assert!(
            Locks::read(&RootId::Global, &dir)
                .source("demo", &dir)
                .is_none(),
            "no lock file"
        );
    }

    #[test]
    fn folder_path_drops_skill_md() {
        assert_eq!(folder_path("skills/x/SKILL.md"), "skills/x");
        assert_eq!(folder_path("SKILL.md"), "");
        assert_eq!(folder_path("skills/x"), "skills/x");
    }

    #[test]
    fn repo_from_source_or_url() {
        assert_eq!(
            github_repo("cursor/plugins", "https://github.com/cursor/plugins.git").as_deref(),
            Some("cursor/plugins")
        );
        assert_eq!(github_repo("a/b", "").as_deref(), Some("a/b"));
        assert_eq!(github_repo("../x", "").as_deref(), None);
    }
}
