//! Finds every skill and slash command on disk.
//!
//! A skill is a folder with a `SKILL.md` (or `SKILL.md.disabled`) in an agent's
//! skills folder, at the home folder or in a project. The same folder is often
//! reached from several agents through symlinks (`~/.claude/skills/x ->
//! ~/.agents/skills/x`), so skills are keyed by their resolved path and each
//! way in is a [`Place`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::frontmatter;
use super::{AGENTS, Installed, Kind, Library, Place, Root, Scope, ScopeId, Skill, tilde};

const SKIP: [&str; 7] = ["node_modules", "build", "dist", "target", "Pods", "DerivedData", "vendor"];
const PROJECT_DEPTH: usize = 3;
const MAX_FILES: usize = 40;

pub fn scan(folders: &[String], home: &Path) -> Library {
    let projects = projects(folders, home);
    let roots: Vec<Root> = std::iter::once(Root::global(home))
        .chain(projects.iter().cloned())
        .collect();

    let mut skills: Vec<Skill> = Vec::new();
    let mut by_real: HashMap<PathBuf, usize> = HashMap::new();
    let mut installed = Vec::new();

    for root in &roots {
        for agent in AGENTS {
            let dir = root.path.join(agent.dir);
            if !dir.is_dir() {
                continue;
            }
            installed.push(Installed {
                root: root.id.clone(),
                agent,
                dir: dir.clone(),
            });
            for entry in sorted_children(&dir) {
                let Some((md, disabled)) = skill_file(&entry) else {
                    continue;
                };
                let real = canonical(&entry);
                let place = Place {
                    agent,
                    entry: entry.clone(),
                    file: md,
                    is_link: is_link(&entry),
                };
                if let Some(&ix) = by_real.get(&real) {
                    skills[ix].places.push(place);
                    continue;
                }
                by_real.insert(real.clone(), skills.len());
                skills.push(read_skill(&real, disabled, root, place));
            }
        }
    }

    for root in &roots {
        let dir = root.path.join(".claude/commands");
        if dir.is_dir() {
            read_commands(&dir, &dir, root, &mut skills, &mut by_real);
        }
    }

    skills.sort_by(|a, b| natural(&a.title).cmp(&natural(&b.title)));
    mark_drift(&mut skills);
    let scopes = scopes(&skills, &projects, home);

    Library {
        skills,
        scopes,
        installed,
        folders: folders.to_vec(),
    }
}

/// Watched folders, and the projects inside them that keep skills or commands.
fn projects(folders: &[String], home: &Path) -> Vec<Root> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for folder in folders {
        let path = super::expand(folder, home);
        if path.is_dir() {
            walk(&path, 0, home, &mut seen, &mut found);
        }
    }
    found.sort_by(|a: &Root, b: &Root| natural(&a.label).cmp(&natural(&b.label)));
    found
}

fn walk(path: &Path, depth: usize, home: &Path, seen: &mut HashSet<PathBuf>, found: &mut Vec<Root>) {
    let real = canonical(path);
    if real != home && has_skills(path) && seen.insert(real) {
        found.push(Root::project(path));
    }
    if depth >= PROJECT_DEPTH {
        return;
    }
    for child in sorted_children(path) {
        let name = child.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') || SKIP.contains(&name) || is_link(&child) || !child.is_dir() {
            continue;
        }
        walk(&child, depth + 1, home, seen, found);
    }
}

fn has_skills(path: &Path) -> bool {
    AGENTS.iter().any(|a| path.join(a.dir).is_dir()) || path.join(".claude/commands").is_dir()
}

fn skill_file(entry: &Path) -> Option<(PathBuf, bool)> {
    let on = entry.join("SKILL.md");
    if on.is_file() {
        return Some((on, false));
    }
    let off = entry.join("SKILL.md.disabled");
    off.is_file().then_some((off, true))
}

fn read_skill(real: &Path, disabled: bool, root: &Root, place: Place) -> Skill {
    let file = real.join(if disabled { "SKILL.md.disabled" } else { "SKILL.md" });
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let header = frontmatter::parse(&text);
    let folder = real
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = header
        .get("name")
        .filter(|n| !n.is_empty())
        .unwrap_or(&folder)
        .to_string();
    let title = frontmatter::display_name(real).unwrap_or_else(|| frontmatter::title(&name, &header.body));
    Skill {
        id: real.to_path_buf(),
        kind: Kind::Skill,
        disabled,
        folder,
        title,
        description: header.get("description").unwrap_or_default().to_string(),
        license: header.get("license").unwrap_or_default().to_string(),
        tools: header.get("allowed-tools").unwrap_or_default().to_string(),
        root: root.id.clone(),
        root_label: root.label.clone(),
        modified: modified(&file),
        files: supporting_files(real),
        name,
        file,
        body: header.body,
        text,
        places: vec![place],
        copies: 1,
        drift: false,
    }
}

fn read_commands(
    dir: &Path,
    base: &Path,
    root: &Root,
    skills: &mut Vec<Skill>,
    by_real: &mut HashMap<PathBuf, usize>,
) {
    for path in sorted_children(dir) {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            read_commands(&path, base, root, skills, by_real);
            continue;
        }
        let disabled = name.ends_with(".md.disabled");
        if !disabled && !name.ends_with(".md") {
            continue;
        }
        let real = canonical(&path);
        if by_real.contains_key(&real) {
            continue;
        }
        let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().into_owned();
        let stem = rel.trim_end_matches(".disabled").trim_end_matches(".md");
        let command = stem.replace('/', ":");
        let text = std::fs::read_to_string(&real).unwrap_or_default();
        let header = frontmatter::parse(&text);
        let description = header
            .get("description")
            .filter(|d| !d.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                header
                    .body
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty() && !l.starts_with('#'))
                    .unwrap_or_default()
                    .to_string()
            });
        let claude = AGENTS.iter().find(|a| a.id == "claude").expect("claude agent");
        by_real.insert(real.clone(), skills.len());
        skills.push(Skill {
            id: real.clone(),
            kind: Kind::Command,
            disabled,
            folder: command.clone(),
            title: frontmatter::title(&command, ""),
            name: command,
            description,
            license: String::new(),
            tools: header.get("allowed-tools").unwrap_or_default().to_string(),
            root: root.id.clone(),
            root_label: root.label.clone(),
            modified: modified(&real),
            files: Vec::new(),
            file: real.clone(),
            body: header.body,
            text,
            places: vec![Place {
                agent: claude,
                entry: path.clone(),
                file: path.clone(),
                is_link: is_link(&path),
            }],
            copies: 1,
            drift: false,
        });
    }
}

/// The same name in more than one place, with different text: copies that
/// have drifted apart.
fn mark_drift(skills: &mut [Skill]) {
    let mut texts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for skill in skills.iter() {
        texts.entry(skill.name.to_lowercase()).or_default().push(skill.text.clone());
    }
    for skill in skills.iter_mut() {
        let same = &texts[&skill.name.to_lowercase()];
        skill.copies = same.len();
        skill.drift = same.iter().collect::<HashSet<_>>().len() > 1;
    }
}

fn scopes(skills: &[Skill], projects: &[Root], home: &Path) -> Vec<Scope> {
    let mut scopes = vec![Scope {
        id: ScopeId::All,
        label: "All skills".into(),
        count: skills.len(),
        path: None,
    }];
    for agent in AGENTS {
        let dir = home.join(agent.dir);
        if !dir.is_dir() {
            continue;
        }
        let count = skills
            .iter()
            .filter(|s| s.root.is_global() && s.places.iter().any(|p| p.agent.id == agent.id))
            .count();
        scopes.push(Scope {
            id: ScopeId::Agent(agent.id),
            label: if agent.id == "agents" { "~/.agents".into() } else { agent.label.into() },
            count,
            path: Some(tilde(&dir, home)),
        });
    }
    for project in projects {
        scopes.push(Scope {
            id: ScopeId::Project(project.path.clone()),
            label: project.label.clone(),
            count: skills.iter().filter(|s| s.root == project.id).count(),
            path: Some(tilde(&project.path, home)),
        });
    }
    scopes
}

fn supporting_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    collect_files(dir, dir, &mut out);
    out.sort();
    out
}

fn collect_files(dir: &Path, base: &Path, out: &mut Vec<String>) {
    for path in sorted_children(dir) {
        if out.len() >= MAX_FILES {
            return;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_files(&path, base, out);
        } else if name != "SKILL.md" && name != "SKILL.md.disabled" {
            out.push(path.strip_prefix(base).unwrap_or(&path).to_string_lossy().into_owned());
        }
    }
}

fn sorted_children(dir: &Path) -> Vec<PathBuf> {
    let mut children: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    children.retain(|p| {
        !p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'))
    });
    children.sort();
    children
}

pub fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn is_link(path: &Path) -> bool {
    path.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink())
}

pub fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// A sort key that orders "Skill 2" before "Skill 10" and ignores case.
fn natural(text: &str) -> Vec<(u8, String)> {
    let mut key = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        let digit = c.is_ascii_digit();
        let mut part = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() != digit {
                break;
            }
            part.push(c);
            chars.next();
        }
        if digit {
            key.push((0, format!("{:0>20}", part)));
        } else {
            key.push((1, part.to_lowercase()));
        }
    }
    key
}
