//! The changes Knack makes on disk. Each one is a single file-system step, so
//! a failure leaves things as they were and is reported as written.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context as _, Result, bail};

use super::scan::{canonical, is_link, modified};

pub enum SaveOutcome {
    Saved(Option<SystemTime>),
    /// Something else wrote the file after it was read; nothing was written.
    ChangedOnDisk,
}

/// Writes `text`, unless the file changed since `seen` and `force` is off.
pub fn save(path: &Path, text: &str, seen: Option<SystemTime>, force: bool) -> Result<SaveOutcome> {
    if !force && seen.is_some() && modified(path) != seen {
        return Ok(SaveOutcome::ChangedOnDisk);
    }
    std::fs::write(path, text).with_context(|| format!("Couldn't write {}", path.display()))?;
    Ok(SaveOutcome::Saved(modified(path)))
}

/// A new skill folder named after `name`, holding `text` as its SKILL.md.
/// Returns the folder.
pub fn create(dir: &Path, name: &str, text: &str) -> Result<PathBuf> {
    let slug = super::slug(name);
    if slug.is_empty() {
        bail!("Give the skill a name.");
    }
    let folder = dir.join(&slug);
    if folder.exists() {
        bail!("{} already exists.", folder.display());
    }
    std::fs::create_dir_all(&folder)?;
    std::fs::write(folder.join("SKILL.md"), text)?;
    Ok(canonical(&folder))
}

/// Creates a skill in the primary folder (preferring `preferred_primary` if it
/// is among `dirs`) and symlinks it into every other chosen folder.
pub fn create_many(
    dirs: &[PathBuf],
    preferred_primary: &Path,
    name: &str,
    text: &str,
) -> Result<PathBuf> {
    let mut unique: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        if !unique.iter().any(|existing| existing == dir) {
            unique.push(dir.clone());
        }
    }
    if unique.is_empty() {
        bail!("Choose at least one destination folder.");
    }
    let primary = unique
        .iter()
        .find(|d| d.as_path() == preferred_primary)
        .unwrap_or(&unique[0])
        .clone();
    let created = create(&primary, name, text)?;
    for dir in &unique {
        if dir != &primary {
            link(&created, dir)?;
        }
    }
    Ok(created)
}

/// Turns a skill or command on or off by adding or removing `.disabled` at
/// the end of its file name. Every agent linked to it follows. Returns the
/// file's new path.
pub fn set_enabled(file: &Path, enabled: bool) -> Result<PathBuf> {
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let target = match (enabled, name.strip_suffix(".disabled")) {
        (true, Some(on)) => file.with_file_name(on),
        (false, None) => file.with_file_name(format!("{name}.disabled")),
        _ => return Ok(file.to_path_buf()),
    };
    if target.exists() {
        bail!("{} already exists.", target.display());
    }
    std::fs::rename(file, &target)?;
    Ok(target)
}

/// Makes a skill available to another agent: a symlink to it in that agent's
/// folder.
pub fn link(skill: &Path, agent_dir: &Path) -> Result<()> {
    let name = skill.file_name().context("The skill has no folder name.")?;
    let entry = agent_dir.join(name);
    if entry.exists() || is_link(&entry) {
        bail!("{} already exists.", entry.display());
    }
    std::fs::create_dir_all(agent_dir)?;
    std::os::unix::fs::symlink(skill, &entry)?;
    Ok(())
}

/// Removes an agent's link to a skill. Refuses anything that is not a link,
/// so the original is never removed this way.
pub fn unlink(entry: &Path) -> Result<()> {
    if !is_link(entry) {
        bail!("That's the original, not a link.");
    }
    std::fs::remove_file(entry)?;
    Ok(())
}

/// Moves a skill to the Trash, removing the agents' links to it first so none
/// is left pointing at nothing.
pub fn trash(target: &Path, links: &[PathBuf]) -> Result<()> {
    for entry in links {
        if is_link(entry) {
            std::fs::remove_file(entry)?;
        }
    }
    trash::delete(target).context("Couldn't move it to the Trash.")
}
