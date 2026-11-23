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

#[test]
fn opencode_scans_global_config_dir_and_project_local_dir() {
    let temp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(temp.path()).unwrap();
    write(
        &home.join(".config/opencode/skills/global-oc/SKILL.md"),
        &skill_md("global-oc", "Global OpenCode skill"),
    );
    write(
        &home.join("dev/proj/.opencode/skills/local-oc/SKILL.md"),
        &skill_md("local-oc", "Project OpenCode skill"),
    );

    let library = scan(&["~/dev".to_string()], &home);
    assert!(library.skills.iter().any(|s| s.name == "global-oc" && s.root.is_global()));
    assert!(library.skills.iter().any(|s| s.name == "local-oc" && !s.root.is_global()));
    let oc_scope = library
        .scopes
        .iter()
        .find(|s| s.id == ScopeId::Agent("opencode"))
        .expect("opencode global scope");
    assert_eq!(oc_scope.count, 1);
    assert_eq!(oc_scope.path.as_deref(), Some("~/.config/opencode/skills"));
}

#[test]
fn cross_root_symlinks_belong_to_both_agent_and_project_scopes() {
    let temp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(temp.path()).unwrap();
    write(
        &home.join(".agents/skills/shared-cross/SKILL.md"),
        &skill_md("shared-cross", "Global original linked into project"),
    );
    fs::create_dir_all(home.join("dev/app/.claude/skills")).unwrap();
    symlink(
        home.join(".agents/skills/shared-cross"),
        home.join("dev/app/.claude/skills/shared-cross"),
    )
    .unwrap();

    let library = scan(&["~/dev".to_string()], &home);
    let skill = library
        .skills
        .iter()
        .find(|s| s.name == "shared-cross")
        .expect("shared-cross scanned");
    let app_root = RootId::Project(home.join("dev/app"));

    assert!(ScopeId::Agent("agents").contains(skill));
    assert!(!ScopeId::Agent("claude").contains(skill));
    assert!(ScopeId::Project(home.join("dev/app")).contains(skill));
    assert!(skill.place_in(&RootId::Global, "agents").is_some());
    assert!(skill.place_in(&RootId::Global, "claude").is_none());
    assert!(skill.place_in(&app_root, "claude").is_some());

    let project_scope = library
        .scopes
        .iter()
        .find(|s| s.id == ScopeId::Project(home.join("dev/app")))
        .expect("dev/app scope");
    assert_eq!(project_scope.count, 1);
}

#[test]
fn scans_nested_cursor_grouping_folders_and_guards_symlink_cycles() {
    let temp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(temp.path()).unwrap();
    write(
        &home.join(".cursor/skills/frontend/react-hooks/SKILL.md"),
        &skill_md("react-hooks", "Nested Cursor grouping folder skill"),
    );
    fs::create_dir_all(home.join(".cursor/skills/cycle")).unwrap();
    symlink(
        home.join(".cursor/skills"),
        home.join(".cursor/skills/cycle/back-to-root"),
    )
    .unwrap();

    let library = scan(&[], &home);
    assert!(
        library.skills.iter().any(|s| s.name == "react-hooks"),
        "expected grouped Cursor skill to be discovered"
    );
}

#[test]
fn create_many_preflights_and_rolls_back_on_collision() {
    let (_temp, home) = fixture();
    let agents = home.join(".agents/skills");
    let claude = home.join(".claude/skills");
    // Pre-existing skill in secondary destination (`shared`).
    let err = create_many(
        &[agents.clone(), claude.clone()],
        &agents,
        "shared",
        &skill_md("shared", "Collision"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("already exists"));

    // Simulate secondary collision added after another valid target (`codex`) when primary succeeds.
    let codex = home.join(".codex/skills");
    write(&codex.join("partial-fail/SKILL.md"), "blocker");
    assert!(
        create_many(
            &[agents.clone(), codex.clone()],
            &agents,
            "partial-fail",
            &skill_md("partial-fail", "Should not leave primary behind"),
        )
        .is_err()
    );
    assert!(
        !agents.join("partial-fail").exists(),
        "primary destination must remain clean after failure"
    );

    // Exercise actual post-create rollback when a secondary `link` fails after `create` + first `link`.
    let not_a_dir = home.join("not-a-dir");
    write(&not_a_dir, "file blocker");
    let bad_dest = not_a_dir.join("skills");
    assert!(
        create_many(
            &[agents.clone(), codex.clone(), bad_dest],
            &agents,
            "rollback-test",
            &skill_md("rollback-test", "Should roll back primary and earlier links"),
        )
        .is_err()
    );
    assert!(!agents.join("rollback-test").exists());
    assert!(!scan::is_link(&codex.join("rollback-test")));
}
