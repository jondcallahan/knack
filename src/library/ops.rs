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

const MAX_SKILL_NAME_LEN: usize = 64;

fn validated_slug(name: &str) -> Result<String> {
    let slug = super::slug(name);
    if slug.is_empty() {
        bail!("Give the skill a name.");
    }
    if slug.len() > MAX_SKILL_NAME_LEN {
        bail!("Skill name must be at most {MAX_SKILL_NAME_LEN} characters.");
    }
    Ok(slug)
}

/// A new skill folder named after `name`, holding `text` as its SKILL.md.
/// Returns the folder. Claims the folder with `create_dir` and cleans up on
/// write failure.
pub fn create(dir: &Path, name: &str, text: &str) -> Result<PathBuf> {
    let slug = validated_slug(name)?;
    std::fs::create_dir_all(dir)?;
    let folder = dir.join(&slug);
    match std::fs::create_dir(&folder) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            bail!("{} already exists.", folder.display());
        }
        Err(err) => return Err(err.into()),
    }
    if let Err(err) = std::fs::write(folder.join("SKILL.md"), text) {
        let _ = std::fs::remove_dir_all(&folder);
        return Err(err.into());
    }
    Ok(canonical(&folder))
}

/// Creates a skill in the primary folder (preferring `preferred_primary` if it
/// is among `dirs`) and symlinks it into every other chosen folder. Preflights all
/// destinations and rolls back entries created by this call on error.
pub fn create_many(
    dirs: &[PathBuf],
    preferred_primary: &Path,
    name: &str,
    text: &str,
) -> Result<PathBuf> {
    let slug = validated_slug(name)?;
    let mut unique: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        if !unique.iter().any(|existing| existing == dir) {
            unique.push(dir.clone());
        }
    }
    if unique.is_empty() {
        bail!("Choose at least one destination folder.");
    }
    for dir in &unique {
        let target = dir.join(&slug);
        if target.exists() || is_link(&target) {
            bail!("{} already exists.", target.display());
        }
    }

    let primary = unique
        .iter()
        .find(|d| d.as_path() == preferred_primary)
        .unwrap_or(&unique[0])
        .clone();
    let created = create(&primary, &slug, text)?;
    let mut created_links: Vec<PathBuf> = Vec::new();
    for dir in &unique {
        if dir != &primary {
            if let Err(err) = link(&created, dir) {
                for entry in &created_links {
                    let _ = std::fs::remove_file(entry);
                }
                let _ = std::fs::remove_dir_all(&created);
                return Err(err);
            }
            created_links.push(dir.join(&slug));
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
