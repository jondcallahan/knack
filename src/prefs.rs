//! Persisted user preferences: watched folders, last scope/filter/selection,
//! default destinations for new skills, and resizable pane widths.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::library::{ScopeId, Skill};

pub const DEFAULT_SIDE_WIDTH: f32 = 236.0;
pub const DEFAULT_LIST_WIDTH: f32 = 324.0;
pub const MIN_SIDE_WIDTH: f32 = 180.0;
pub const MAX_SIDE_WIDTH: f32 = 380.0;
pub const MIN_LIST_WIDTH: f32 = 240.0;
pub const MAX_LIST_WIDTH: f32 = 540.0;

fn default_new_dirs() -> Vec<String> {
    vec!["~/.agents/skills".into()]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusFilter {
    #[default]
    All,
    Enabled,
    Disabled,
}

impl StatusFilter {
    pub fn matches(self, skill: &Skill) -> bool {
        match self {
            StatusFilter::All => true,
            StatusFilter::Enabled => !skill.disabled,
            StatusFilter::Disabled => skill.disabled,
        }
    }

    pub fn index(self) -> usize {
        match self {
            StatusFilter::All => 0,
            StatusFilter::Enabled => 1,
            StatusFilter::Disabled => 2,
        }
    }

    pub fn from_index(ix: usize) -> Self {
        match ix {
            1 => StatusFilter::Enabled,
            2 => StatusFilter::Disabled,
            _ => StatusFilter::All,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub folders: Vec<String>,
    pub scope: String,
    pub status: StatusFilter,
    pub selected: Option<String>,
    #[serde(default = "default_new_dirs")]
    pub new_dirs: Vec<String>,
    pub side_width: f32,
    pub list_width: f32,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            folders: vec!["~/dev".into()],
            scope: "all".into(),
            status: StatusFilter::All,
            selected: None,
            new_dirs: default_new_dirs(),
            side_width: DEFAULT_SIDE_WIDTH,
            list_width: DEFAULT_LIST_WIDTH,
        }
    }
}

impl Prefs {
    pub fn path() -> PathBuf {
        if let Some(overridden) = std::env::var_os("KNACK_PREFS")
            && !overridden.is_empty()
        {
            return PathBuf::from(overridden);
        }
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("knack")
            .join("prefs.json")
    }

    pub fn legacy_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("knack-gpui")
            .join("prefs.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        if path.exists() || std::env::var_os("KNACK_PREFS").is_some_and(|v| !v.is_empty()) {
            Self::load_from(&path)
        } else {
            Self::load_from(&Self::legacy_path())
        }
    }

    pub fn load_from(path: &Path) -> Self {
        let mut prefs = std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Prefs>(&raw).ok())
            .unwrap_or_default();
        prefs.side_width = prefs.side_width.clamp(MIN_SIDE_WIDTH, MAX_SIDE_WIDTH);
        prefs.list_width = prefs.list_width.clamp(MIN_LIST_WIDTH, MAX_LIST_WIDTH);
        if prefs.new_dirs.is_empty() {
            prefs.new_dirs = default_new_dirs();
        }
        prefs
    }

    pub fn save(&self) {
        self.save_to(&Self::path());
    }

    pub fn save_to(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn scope_id(&self) -> ScopeId {
        ScopeId::from_key(&self.scope)
    }

    pub fn set_scope(&mut self, scope: &ScopeId) {
        self.scope = scope.key();
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    }

    pub fn set_selected(&mut self, id: Option<&Path>) {
        self.selected = id.map(|p| p.to_string_lossy().into_owned());
    }
}
