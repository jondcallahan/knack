//! Agent skills on disk: what exists, where it is reached from, and the
//! changes Knack makes to it.
//!
//! Everything here is plain file-system work with no UI, so it runs on a
//! background executor and is tested without a window.

mod frontmatter;
mod ops;
mod scan;
mod source;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub use frontmatter::{
    Frontmatter, compose, compose_preserving, parse as parse_frontmatter, slug,
};
pub use ops::{SaveOutcome, create, create_many, link, save, set_enabled, trash, unlink};
pub use scan::scan;
pub use source::{Baseline, Check, InstalledForm, Local, Remote, RemoteEntry, Source, Upstream, installed_tree};

/// An agent's skills folder, relative to the home folder or a project.
#[derive(Debug, PartialEq, Eq)]
pub struct Agent {
    pub id: &'static str,
    pub label: &'static str,
    pub global_dir: &'static str,
    pub dir: &'static str,
}

impl Agent {
    pub fn dir_for(&self, root: &RootId) -> &'static str {
        if root.is_global() {
            self.global_dir
        } else {
            self.dir
        }
    }
}

pub static AGENTS: &[Agent] = &[
    Agent { id: "agents", label: ".agents", global_dir: ".agents/skills", dir: ".agents/skills" },
    Agent { id: "claude", label: "Claude Code", global_dir: ".claude/skills", dir: ".claude/skills" },
    Agent { id: "codex", label: "Codex", global_dir: ".codex/skills", dir: ".codex/skills" },
    Agent { id: "cursor", label: "Cursor", global_dir: ".cursor/skills", dir: ".cursor/skills" },
    Agent { id: "gemini", label: "Gemini", global_dir: ".gemini/skills", dir: ".gemini/skills" },
    Agent { id: "opencode", label: "OpenCode", global_dir: ".config/opencode/skills", dir: ".opencode/skills" },
];

/// Where skills are installed: the home folder, or a project.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RootId {
    Global,
    Project(PathBuf),
}

impl RootId {
    pub fn is_global(&self) -> bool {
        matches!(self, RootId::Global)
    }
}

#[derive(Debug, Clone)]
pub struct Root {
    pub id: RootId,
    /// "Global", or the project's parent and name ("dev/knack").
    pub label: String,
    pub path: PathBuf,
}

impl Root {
    fn global(home: &Path) -> Self {
        Self {
            id: RootId::Global,
            label: "Global".into(),
            path: home.to_path_buf(),
        }
    }

    fn project(path: &Path) -> Self {
        let name = |p: Option<&Path>| {
            p.and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        Self {
            id: RootId::Project(path.to_path_buf()),
            label: format!("{}/{}", name(path.parent()), name(Some(path))),
            path: path.to_path_buf(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Skill,
    /// A Claude Code slash command: one Markdown file in `.claude/commands`.
    Command,
}

/// One way into a skill: an entry in an agent's folder, which may be the
/// folder itself or a symlink to it.
#[derive(Debug, Clone)]
pub struct Place {
    pub root: RootId,
    pub root_label: String,
    pub agent: &'static Agent,
    /// The entry in the agent's folder (`~/.claude/skills/x`).
    pub entry: PathBuf,
    /// The instructions file reached through the entry.
    pub file: PathBuf,
    pub is_link: bool,
}

#[derive(Debug, Clone)]
pub struct Skill {
    /// The resolved folder (or, for a command, file); stable across rescans.
    pub id: PathBuf,
    pub kind: Kind,
    /// Its file ends in `.disabled`, so agents don't load it.
    pub disabled: bool,
    pub folder: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub license: String,
    pub tools: String,
    pub root: RootId,
    pub root_label: String,
    /// The resolved instructions file.
    pub file: PathBuf,
    pub body: String,
    pub text: String,
    pub modified: Option<SystemTime>,
    pub files: Vec<String>,
    pub places: Vec<Place>,
    /// How many skills share this name, this one included.
    pub copies: usize,
    /// Those copies don't all have the same text.
    pub drift: bool,
    /// The GitHub repository the `skills` CLI installed it from.
    pub source: Option<Source>,
    /// How it compares with that source.
    pub upstream: Option<Upstream>,
}

impl Skill {
    pub fn place(&self, agent: &str) -> Option<&Place> {
        self.places.iter().find(|p| p.agent.id == agent)
    }

    pub fn place_in(&self, root: &RootId, agent: &str) -> Option<&Place> {
        self.places
            .iter()
            .find(|p| &p.root == root && p.agent.id == agent)
    }

    pub fn links(&self) -> impl Iterator<Item = &Place> {
        self.places.iter().filter(|p| p.is_link)
    }

    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || [&self.name, &self.title, &self.description, &self.body]
                .iter()
                .any(|field| field.to_lowercase().contains(&query))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ScopeId {
    All,
    Agent(&'static str),
    Project(PathBuf),
}

impl ScopeId {
    pub fn contains(&self, skill: &Skill) -> bool {
        match self {
            ScopeId::All => true,
            ScopeId::Agent(id) => skill.places.iter().any(|p| p.root.is_global() && p.agent.id == *id),
            ScopeId::Project(path) => skill
                .places
                .iter()
                .any(|p| matches!(&p.root, RootId::Project(p_root) if p_root == path)),
        }
    }

    /// A stable string for saving the selection.
    pub fn key(&self) -> String {
        match self {
            ScopeId::All => "all".into(),
            ScopeId::Agent(id) => format!("agent:{id}"),
            ScopeId::Project(path) => format!("project:{}", path.display()),
        }
    }

    pub fn from_key(key: &str) -> Self {
        if let Some(id) = key.strip_prefix("agent:")
            && let Some(agent) = AGENTS.iter().find(|a| a.id == id)
        {
            return ScopeId::Agent(agent.id);
        }
        if let Some(path) = key.strip_prefix("project:") {
            return ScopeId::Project(PathBuf::from(path));
        }
        ScopeId::All
    }
}

#[derive(Debug, Clone)]
pub struct Scope {
    pub id: ScopeId,
    pub label: String,
    pub count: usize,
    pub path: Option<String>,
}

/// An agent folder that exists, where a new skill can be saved or linked.
#[derive(Debug, Clone)]
pub struct Installed {
    pub root: RootId,
    pub agent: &'static Agent,
    pub dir: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct Library {
    pub skills: Vec<Skill>,
    pub scopes: Vec<Scope>,
    pub installed: Vec<Installed>,
    pub folders: Vec<String>,
}

impl Library {
    pub fn skill(&self, id: &Path) -> Option<&Skill> {
        self.skills.iter().find(|s| s.id == id)
    }

    /// The skills installed from GitHub, one per source folder.
    pub fn sources(&self) -> Vec<Source> {
        let mut seen = std::collections::HashSet::new();
        self.skills
            .iter()
            .filter_map(|s| s.source.clone())
            .filter(|s| seen.insert(s.key()))
            .collect()
    }

    /// Compare each skill with what GitHub reported for its source.
    pub fn apply_remote(&mut self, remote: &Remote) {
        for skill in &mut self.skills {
            skill.upstream = skill.source.as_ref().map(|s| s.status(remote));
        }
    }
}

pub fn home() -> PathBuf {
    if let Some(overridden) = std::env::var_os("KNACK_HOME")
        && !overridden.is_empty()
    {
        let path = PathBuf::from(overridden);
        return std::fs::canonicalize(&path).unwrap_or(path);
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// `~/x` for a path under the home folder.
pub fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

pub fn expand(path: &str, home: &Path) -> PathBuf {
    if path == "~" {
        home.to_path_buf()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(path)
    }
}

#[cfg(test)]
mod tests;
