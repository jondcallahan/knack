use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use super::*;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn skill_md(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\nDo it.\n")
}

/// A home folder with a skill shared by link, a disabled skill, a project
/// with its own copy and a slash command.
fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(temp.path()).unwrap();
    write(&home.join(".agents/skills/shared/SKILL.md"), &skill_md("shared", "Shared skill"));
    write(&home.join(".agents/skills/shared/scripts/run.sh"), "echo hi");
    fs::create_dir_all(home.join(".claude/skills")).unwrap();
    symlink(home.join(".agents/skills/shared"), home.join(".claude/skills/shared")).unwrap();
    write(&home.join(".agents/skills/quiet/SKILL.md.disabled"), &skill_md("quiet", "Off"));
    write(&home.join("dev/app/.agents/skills/shared/SKILL.md"), &skill_md("shared", "Drifted copy"));
    write(&home.join("dev/app/.claude/commands/ship.md"), "Ship the branch.\n");
    write(&home.join("dev/app/node_modules/pkg/.agents/skills/no/SKILL.md"), &skill_md("no", "Skipped"));
    (temp, home)
}

#[test]
fn scan_dedupes_links_and_finds_projects_commands_and_disabled() {
    let (_temp, home) = fixture();
    let library = scan(&["~/dev".to_string()], &home);

    let names: Vec<_> = library.skills.iter().map(|s| (s.name.as_str(), s.kind)).collect();
    assert_eq!(
        names,
        [("quiet", Kind::Skill), ("shared", Kind::Skill), ("shared", Kind::Skill), ("ship", Kind::Command)]
    );

    let global = library.skills.iter().find(|s| s.name == "shared" && s.root.is_global()).unwrap();
    let agents: Vec<_> = global.places.iter().map(|p| (p.agent.id, p.is_link)).collect();
    assert_eq!(agents, [("agents", false), ("claude", true)]);
    assert_eq!(global.files, ["scripts/run.sh"]);
    assert!(global.drift, "the project's copy has different text");
    assert_eq!(global.copies, 2);

    let quiet = library.skills.iter().find(|s| s.name == "quiet").unwrap();
    assert!(quiet.disabled);

    let command = library.skills.iter().find(|s| s.kind == Kind::Command).unwrap();
    assert_eq!(command.description, "Ship the branch.");
    assert_eq!(command.root_label, "dev/app");

    let labels: Vec<_> = library.scopes.iter().map(|s| (s.label.as_str(), s.count)).collect();
    assert_eq!(labels, [("All skills", 4), ("~/.agents", 2), ("Claude Code", 1), ("dev/app", 2)]);
}

#[test]
fn disable_and_enable_rename_the_file() {
    let (_temp, home) = fixture();
    let file = home.join(".agents/skills/shared/SKILL.md");
    let off = set_enabled(&file, false).unwrap();
    assert_eq!(off, home.join(".agents/skills/shared/SKILL.md.disabled"));
    let library = scan(&[], &home);
    let shared = library.skills.iter().find(|s| s.name == "shared").unwrap();
    assert!(shared.disabled);
    assert_eq!(shared.places.len(), 2, "the link follows the rename");
    assert_eq!(set_enabled(&off, true).unwrap(), file);
}

#[test]
fn save_refuses_a_file_changed_since_it_was_read() {
    let (_temp, home) = fixture();
    let file = home.join(".agents/skills/quiet/SKILL.md.disabled");
    let seen = Some(std::time::SystemTime::UNIX_EPOCH);
    assert!(matches!(save(&file, "x", seen, false).unwrap(), SaveOutcome::ChangedOnDisk));
    assert!(matches!(save(&file, "x", seen, true).unwrap(), SaveOutcome::Saved(_)));
    assert_eq!(fs::read_to_string(&file).unwrap(), "x");
}

#[test]
fn link_unlink_create_and_trash() {
    let (_temp, home) = fixture();
    let created = create(&home.join(".agents/skills"), "New Thing", &skill_md("new-thing", "New")).unwrap();
    assert!(created.ends_with("new-thing"));
    assert!(create(&home.join(".agents/skills"), "new thing", "").is_err());

    let codex = home.join(".codex/skills");
    link(&created, &codex).unwrap();
    assert!(codex.join("new-thing/SKILL.md").is_file());
    assert!(unlink(&created).is_err(), "the original is not removed");
    unlink(&codex.join("new-thing")).unwrap();
    assert!(!codex.join("new-thing").exists());
}

#[test]
fn create_many_creates_primary_and_links_others() {
    let (_temp, home) = fixture();
    let agents = home.join(".agents/skills");
    let claude = home.join(".claude/skills");
    let codex = home.join(".codex/skills");

    let created = create_many(
        &[claude.clone(), agents.clone(), codex.clone()],
        &agents,
        "Multi Target",
        &skill_md("multi-target", "Shared across three"),
    )
    .unwrap();
    assert_eq!(created, agents.join("multi-target"));
    assert!(created.join("SKILL.md").is_file());
    assert!(scan::is_link(&claude.join("multi-target")));
    assert!(scan::is_link(&codex.join("multi-target")));

    let library = scan(&[], &home);
    let found = library
        .skills
        .iter()
        .find(|s| s.name == "multi-target")
        .expect("multi-target scanned");
    assert_eq!(found.places.len(), 3);
    assert!(!found.drift);
}
