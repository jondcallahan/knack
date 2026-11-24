//! UI integration tests for KnackApp running in a headless window
//! against a temporary fixture home directory.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AppContext as _, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions,
    component::WindowExt as _,
    px, size,
};
use knack::{
    actions,
    library::{Kind, Upstream},
    ui::{DetailMode, KnackApp},
};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn skill_md(name: &str, description: &str, body: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\n{body}\n")
}

fn setup_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(temp.path()).unwrap();
    write(
        &home.join(".agents/skills/shared/SKILL.md"),
        &skill_md("shared", "Shared global skill", "Unique body keyword: nebula"),
    );
    write(&home.join(".agents/skills/shared/scripts/run.sh"), "echo hi");
    fs::create_dir_all(home.join(".claude/skills")).unwrap();
    symlink(
        home.join(".agents/skills/shared"),
        home.join(".claude/skills/shared"),
    )
    .unwrap();
    fs::create_dir_all(home.join(".codex/skills")).unwrap();
    write(
        &home.join(".agents/skills/quiet/SKILL.md.disabled"),
        &skill_md("quiet", "Disabled skill", "Sleeps quietly."),
    );
    write(
        &home.join("dev/app/.claude/commands/ship.md"),
        "Ship the branch cleanly.\n",
    );
    let prefs_path = home.join(".config/knack/prefs.json");
    (temp, home, prefs_path)
}

fn open_knack(
    cx: &mut TestAppContext,
    home: PathBuf,
    prefs_path: PathBuf,
) -> (gpui_kit::AnyWindowHandle, Entity<KnackApp>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        knack::theme::install(cx);
        actions::bind_keys(cx);
        cx.on_action(|_: &actions::Quit, cx| KnackApp::handle_global_quit(cx));
        cx.on_action(|_: &actions::CloseWindow, cx| KnackApp::handle_global_close_window(cx));
    });
    let (handle, app) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1200.), px(780.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| KnackApp::new_in(home, prefs_path, window, cx)),
        )
        .expect("open test window")
    });
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
    (handle, app)
}

#[gpui_kit::test]
fn scans_searches_body_and_toggles_enabled_on_disk(cx: &mut TestAppContext) {
    let (_temp, home, prefs_path) = setup_fixture();
    let (handle, app) = open_knack(cx, home.clone(), prefs_path);

    cx.update(|cx| {
        let app = app.read(cx);
        assert!(!app.is_scanning());
        assert_eq!(app.library().skills.len(), 3);
        assert_eq!(app.filtered_skill_count(cx), 3);
    });

    // Search matches skill body text ("nebula").
    cx.update_window(handle, |_, window, cx| {
        window.click("search-input", cx);
        window.input("nebula", cx);
    })
    .unwrap();

    cx.update(|cx| {
        assert_eq!(app.read(cx).filtered_skill_count(cx), 1);
    });

    // Click the filtered row to open `shared`, then toggle its Enabled switch off.
    let shared_row_id = gpui_kit::SharedString::from(format!(
        "skill-row:{}",
        home.join(".agents/skills/shared").display()
    ));
    cx.update_window(handle, |_, window, cx| {
        window.click(shared_row_id.clone(), cx);
        window.render_frame(cx);
    })
    .unwrap();

    cx.update(|cx| {
        let selected = app.read(cx).selected_skill().expect("selected shared");
        assert_eq!(selected.name, "shared");
        assert!(!selected.disabled);
    });

    cx.update_window(handle, |_, window, cx| {
        window.click("skill-enabled-switch", cx);
    })
    .unwrap();
    cx.run_until_parked();

    assert!(home.join(".agents/skills/shared/SKILL.md.disabled").is_file());
    cx.update(|cx| {
        let selected = app.read(cx).selected_skill().expect("still selected");
        assert!(selected.disabled);
    });
}

#[gpui_kit::test]
fn edit_save_and_agent_sharing_work_through_ui(cx: &mut TestAppContext) {
    let (_temp, home, prefs_path) = setup_fixture();
    let (handle, app) = open_knack(cx, home.clone(), prefs_path);

    let shared_row_id = gpui_kit::SharedString::from(format!(
        "skill-row:{}",
        home.join(".agents/skills/shared").display()
    ));
    cx.update_window(handle, |_, window, cx| {
        window.click(shared_row_id.clone(), cx);
        window.render_frame(cx);
    })
    .unwrap();

    // Share with Codex by clicking its pill.
    cx.update_window(handle, |_, window, cx| {
        window.click("agent-pill:codex", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(home.join(".codex/skills/shared/SKILL.md").is_file());

    // Stop sharing with Claude Code by clicking its link pill.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("agent-pill:claude", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!home.join(".claude/skills/shared").exists());

    // Enter edit mode, verify Save is disabled until dirty, type text, save.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("edit-skill", cx);
        window.render_frame(cx);
    })
    .unwrap();

    cx.update(|cx| {
        assert_eq!(app.read(cx).detail_mode(), DetailMode::Edit);
    });

    cx.update_window(handle, |_, window, cx| {
        window.input("# Extra note\n", cx);
        window.click("save-edit", cx);
    })
    .unwrap();
    cx.run_until_parked();

    let saved = fs::read_to_string(home.join(".agents/skills/shared/SKILL.md")).unwrap();
    assert!(saved.contains("# Extra note"));
    cx.update(|cx| {
        assert_eq!(app.read(cx).detail_mode(), DetailMode::Read);
    });
}

#[gpui_kit::test]
fn creates_new_skill_from_pasted_frontmatter(cx: &mut TestAppContext) {
    let (_temp, home, prefs_path) = setup_fixture();
    let (handle, app) = open_knack(cx, home.clone(), prefs_path);

    // Pasting a full SKILL.md into the focused Instructions editor should
    // supply name, description, and keep extra frontmatter keys automatically.
    let pasted = "---\nname: pasted-skill\ndescription: Parsed from header\nmodel: sonnet\n---\n\n# Pasted Skill\n\nFollow these steps.";
    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(pasted.into()));

    cx.update_window(handle, |_, window, cx| {
        window.click("toolbar-new", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-v", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("new-skill-create", cx);
    })
    .unwrap();
    cx.run_until_parked();

    let created = home.join(".agents/skills/pasted-skill/SKILL.md");
    assert!(created.is_file(), "expected pasted skill to be created on disk");
    let contents = fs::read_to_string(&created).unwrap();
    assert!(contents.contains("name: \"pasted-skill\""));
    assert!(contents.contains("description: \"Parsed from header\""));
    assert!(contents.contains("model: sonnet"));
    assert!(contents.contains("Follow these steps."));

    cx.update(|cx| {
        assert_eq!(app.read(cx).detail_mode(), DetailMode::Read);
        let sel = app.read(cx).selected_skill().expect("new skill selected");
        assert_eq!(sel.name, "pasted-skill");
        assert_eq!(sel.kind, Kind::Skill);
    });
}

#[gpui_kit::test]
fn detects_external_edit_conflict_and_allows_overwrite(cx: &mut TestAppContext) {
    let (_temp, home, prefs_path) = setup_fixture();
    let (handle, app) = open_knack(cx, home.clone(), prefs_path);

    let shared_file = home.join(".agents/skills/shared/SKILL.md");
    let shared_row_id = gpui_kit::SharedString::from(format!(
        "skill-row:{}",
        home.join(".agents/skills/shared").display()
    ));

    cx.update_window(handle, |_, window, cx| {
        window.click(shared_row_id.clone(), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("edit-skill", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.input("My local edit", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();

    // Simulate another editor modifying the file on disk with a newer mtime.
    fs::write(
        &shared_file,
        "---\nname: shared\ndescription: External\n---\nExternal change\n",
    )
    .unwrap();
    fs::File::options()
        .write(true)
        .open(&shared_file)
        .unwrap()
        .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
        .unwrap();

    cx.update_window(handle, |_, window, cx| {
        window.click("save-edit", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update(|cx| {
        assert!(
            app.read(cx).has_conflict(),
            "expected conflict, mode={:?}",
            app.read(cx).detail_mode()
        );
        assert_eq!(app.read(cx).detail_mode(), DetailMode::Edit);
    });

    // Overwrite forces the save and leaves edit mode.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("conflict-overwrite", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update(|cx| {
        assert!(!app.read(cx).has_conflict());
        assert_eq!(app.read(cx).detail_mode(), DetailMode::Read);
    });
    let final_text = fs::read_to_string(&shared_file).unwrap();
    assert!(final_text.contains("My local edit"));
}

#[gpui_kit::test]
fn skill_list_rows_do_not_clip_description_or_overlap_chips(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(temp.path()).unwrap();
    let long_desc = "This is a deliberately long multi-line skill description that should wrap cleanly across two lines inside the skill card and truncate with an ellipsis rather than getting clipped vertically or spilling past the card border.";
    write(
        &home.join(".agents/skills/alpha/SKILL.md"),
        &skill_md("alpha", long_desc, "Body alpha"),
    );
    write(
        &home.join(".agents/skills/beta/SKILL.md"),
        &skill_md("beta", "Short description", "Body beta"),
    );
    let prefs_path = home.join(".config/knack/prefs.json");
    let (handle, _app) = open_knack(cx, home.clone(), prefs_path);

    let alpha_path = home.join(".agents/skills/alpha");
    let beta_path = home.join(".agents/skills/beta");
    let alpha_row_id = gpui_kit::SharedString::from(format!("skill-row:{}", alpha_path.display()));
    let alpha_desc_id = gpui_kit::SharedString::from(format!("skill-desc:{}", alpha_path.display()));
    let alpha_chips_id = gpui_kit::SharedString::from(format!("skill-chips:{}", alpha_path.display()));
    let beta_row_id = gpui_kit::SharedString::from(format!("skill-row:{}", beta_path.display()));

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let row = window.find(alpha_row_id.clone());
        let desc = window.find(alpha_desc_id.clone());
        let chips = window.find(alpha_chips_id.clone());
        let second_row = window.find(beta_row_id.clone());

        assert!(
            desc.bounds().size.height >= px(36.),
            "expected full 2-line description height >= 36px, got {:?}",
            desc.bounds().size.height
        );
        assert!(
            chips.bounds().top() >= desc.bounds().bottom(),
            "chips top {:?} overlapped description bottom {:?}",
            chips.bounds().top(),
            desc.bounds().bottom()
        );
        assert!(
            desc.bounds().right() <= row.bounds().right(),
            "description right {:?} escaped row right {:?}",
            desc.bounds().right(),
            row.bounds().right()
        );
        assert!(
            second_row.bounds().top() > row.bounds().bottom(),
            "adjacent rows should have vertical separation: first bottom {:?}, second top {:?}",
            row.bounds().bottom(),
            second_row.bounds().top()
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn shortcuts_work_immediately_on_launch_including_cmd_w_and_about(cx: &mut TestAppContext) {
    let (_temp, home, prefs_path) = setup_fixture();
    let (handle, app) = open_knack(cx, home, prefs_path);

    // Without clicking anywhere first, Cmd+N -> Escape -> Cmd+E -> Escape work.
    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-n", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(app.read(cx).detail_mode(), DetailMode::New));

    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(app.read(cx).detail_mode(), DetailMode::Read));

    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-e", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(app.read(cx).detail_mode(), DetailMode::Edit));

    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(app.read(cx).detail_mode(), DetailMode::Read));

    // Cmd+R spins the refresh icon for the 600ms soft-close cycle even on fast scans.
    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-r", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(app.read(cx).is_rescan_spinning());
    });
    cx.background_executor
        .advance_clock(Duration::from_millis(550));
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(app.read(cx).is_rescan_spinning());
    });
    cx.background_executor
        .advance_clock(Duration::from_millis(100));
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(!app.read(cx).is_rescan_spinning());
    });

    // Cmd+F focuses search; Escape blurs search without clearing entered text.
    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-f", cx);
        window.render_frame(cx);
        window.input("nebula", cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        assert!(app.read(cx).is_search_focused(window, cx));
        assert_eq!(app.read(cx).search_text(cx), "nebula");
        assert_eq!(app.read(cx).filtered_skill_count(cx), 1);
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        assert!(!app.read(cx).is_search_focused(window, cx));
        assert_eq!(app.read(cx).search_text(cx), "nebula");
        assert_eq!(app.read(cx).filtered_skill_count(cx), 1);
    })
    .unwrap();

    // About action opens a dialog and Escape closes it.
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(actions::About), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
    })
    .unwrap();

    // Cmd+W closes the window.
    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-w", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(cx.windows().is_empty(), "expected Cmd+W to close the window");
    });
}

#[gpui_kit::test]
fn sidebar_items_fit_without_vertical_or_horizontal_clipping(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(temp.path()).unwrap();
    write(
        &home.join(".claude/skills/one/SKILL.md"),
        &skill_md("one", "Claude skill", "Body"),
    );
    write(
        &home.join(".codex/skills/two/SKILL.md"),
        &skill_md("two", "Codex skill", "Body"),
    );
    write(
        &home.join("dev/sunburntimer-sunpos-very-long-project-name/.agents/skills/demo/SKILL.md"),
        &skill_md("demo", "Project skill", "Body"),
    );
    let prefs_path = home.join(".config/knack/prefs.json");
    let (handle, _app) = open_knack(cx, home, prefs_path);

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let pane = window.find("sidebar-pane");
        // Groups: 0 Library ("0-0-0"), 1 Agents ("1-0-0", "1-0-1"), 2 Projects ("2-0-0"), 3 Watched folders ("3-0-0", "3-0-1").
        for id in ["0-0-0", "1-0-0", "1-0-1", "2-0-0", "3-0-0"] {
            let item = window.within("knack-sidebar").find(id);
            assert!(
                item.bounds().size.height >= px(28.),
                "sidebar item {id} height too small: {:?}",
                item.bounds().size.height
            );
            assert!(
                item.bounds().right() <= pane.bounds().right(),
                "sidebar item {id} right {:?} overflowed pane right {:?}",
                item.bounds().right(),
                pane.bounds().right()
            );
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn new_skill_save_to_multi_select_links_selected_agents(cx: &mut TestAppContext) {
    let (_temp, home, prefs_path) = setup_fixture();
    let (handle, app) = open_knack(cx, home.clone(), prefs_path);

    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-n", cx);
        window.render_frame(cx);
        window.click("new-dir-select", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();

    // Select Claude Code and Codex in addition to the default ~/.agents/skills.
    cx.update_window(handle, |_, window, cx| {
        window.click("dir-option:~/.claude/skills", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click("dir-option:~/.codex/skills", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        // Close the multi-select popup with Escape, then fill name & description.
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click("new-name-input", cx);
        window.input("multi-skill", cx);
        window.click("new-description-input", cx);
        window.input("Saved to three destinations", cx);
        window.click("new-skill-create", cx);
    })
    .unwrap();
    cx.run_until_parked();

    assert!(home.join(".agents/skills/multi-skill/SKILL.md").is_file());
    assert!(home.join(".claude/skills/multi-skill/SKILL.md").is_file());
    assert!(home.join(".codex/skills/multi-skill/SKILL.md").is_file());
    assert!(
        home.join(".claude/skills/multi-skill")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        home.join(".codex/skills/multi-skill")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );

    cx.update(|cx| {
        let sel = app.read(cx).selected_skill().expect("created skill selected");
        assert_eq!(sel.name, "multi-skill");
        assert_eq!(sel.places.len(), 3);
    });
}

#[gpui_kit::test]
fn uses_one_light_and_one_dark_syntax_themes(cx: &mut TestAppContext) {
    use gpui_kit::{
        component::{
            highlighter::FontStyle,
            theme::{Theme, ThemeMode},
        },
        rgb,
    };

    cx.update(|cx| {
        gpui_kit::init(cx);
        knack::theme::install(cx);

        Theme::change(ThemeMode::Light, None, cx);
        let light = &Theme::global(cx).highlight_theme.style.syntax;
        assert_eq!(
            light.keyword.and_then(|s| s.color),
            Some(rgb(0xA626A4).into())
        );
        let comment = light.comment.expect("light comment style");
        assert_eq!(comment.color, Some(rgb(0x7F848E).into()));
        assert_eq!(comment.font_style, Some(FontStyle::Italic));

        Theme::change(ThemeMode::Dark, None, cx);
        let dark = &Theme::global(cx).highlight_theme.style.syntax;
        assert_eq!(
            dark.keyword.and_then(|s| s.color),
            Some(rgb(0xC678DD).into())
        );
        assert_eq!(
            dark.string.and_then(|s| s.color),
            Some(rgb(0x98C379).into())
        );
    });
}

#[gpui_kit::test]
fn update_check_detects_newer_release_from_feed(cx: &mut TestAppContext) {
    let (temp, home, prefs_path) = setup_fixture();
    let feed_path = temp.path().join("latest.json");
    fs::write(
        &feed_path,
        r#"{
          "version": "9.9.9",
          "tag": "v9.9.9",
          "published_at": "2026-11-24T00:00:00Z",
          "notes": "Test update release.",
          "release_url": "https://github.com/jondcallahan/knack/releases/tag/v9.9.9",
          "dmg_url": "https://github.com/jondcallahan/knack/releases/download/v9.9.9/Knack-9.9.9-arm64.dmg",
          "zip_url": "https://github.com/jondcallahan/knack/releases/download/v9.9.9/Knack-9.9.9-arm64.zip",
          "zip_sha256": ""
        }"#,
    )
    .unwrap();

    let (handle, app) = open_knack(cx, home, prefs_path);
    let url = format!("file://{}", feed_path.display());
    cx.update_window(handle, |_, window, cx| {
        app.update(cx, |this, cx| {
            this.check_updates_from_url(url.clone(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let _ = window.find("toolbar-update");
    })
    .unwrap();

    cx.update(|cx| {
        let update = app
            .read(cx)
            .available_update()
            .expect("newer release detected");
        assert_eq!(update.version, "9.9.9");
    });
}

#[gpui_kit::test]
fn creates_normalized_slug_and_preserves_yaml_metadata(cx: &mut TestAppContext) {
    let (_temp, home, prefs_path) = setup_fixture();
    let (handle, _app) = open_knack(cx, home.clone(), prefs_path);

    let pasted = "---\nname: Old Name\ndescription: Parsed from header\nmetadata:\n  author: acme\n  version: \"1.0\"\n---\n\n# Body\n\nFollow these steps.";
    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(pasted.into()));

    cx.update_window(handle, |_, window, cx| {
        window.click("toolbar-new", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-v", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click("new-name-input", cx);
        window.input("My Skill", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("new-skill-create", cx);
    })
    .unwrap();
    cx.run_until_parked();

    let created = home.join(".agents/skills/my-skill/SKILL.md");
    assert!(created.is_file(), "expected normalized my-skill folder");
    let contents = fs::read_to_string(&created).unwrap();
    assert!(contents.contains("name: \"my-skill\"\n"), "got:\n{contents}");
    assert!(
        contents.contains("metadata:\n  author: acme\n  version: \"1.0\"\n"),
        "expected nested metadata mapping to stay intact, got:\n{contents}"
    );
}

#[gpui_kit::test]
fn cmd_q_prompts_when_editor_is_dirty(cx: &mut TestAppContext) {
    let (_temp, home, prefs_path) = setup_fixture();
    let (handle, app) = open_knack(cx, home, prefs_path);

    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-e", cx);
        window.render_frame(cx);
        window.input("# Unsaved note\n", cx);
        window.render_frame(cx);
        window.press("cmd-q", cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
        // Second Cmd+Q and Cmd+W while dialog has focus must still guard dirty edits.
        window.press("cmd-q", cx);
        window.render_frame(cx);
        window.press("cmd-w", cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(app.read(cx).detail_mode(), DetailMode::Edit);
    });
}

#[gpui_kit::test]
fn project_scope_detail_unlinks_cross_root_project_symlink(cx: &mut TestAppContext) {
    let (_temp, home, prefs_path) = setup_fixture();
    write(
        &home.join(".agents/skills/cross/SKILL.md"),
        &skill_md("cross", "Global original linked into project", "Cross body"),
    );
    fs::create_dir_all(home.join("dev/app/.claude/skills")).unwrap();
    symlink(
        home.join(".agents/skills/cross"),
        home.join("dev/app/.claude/skills/cross"),
    )
    .unwrap();

    let (handle, app) = open_knack(cx, home.clone(), prefs_path);
    let cross_row_id = gpui_kit::SharedString::from(format!(
        "skill-row:{}",
        home.join(".agents/skills/cross").display()
    ));

    // Switch sidebar to project `dev/app` ("2-0-0"), select `cross`, and click Claude link pill.
    cx.update_window(handle, |_, window, cx| {
        window.within("knack-sidebar").click("2-0-0", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click(cross_row_id.clone(), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click("agent-pill:claude", cx);
    })
    .unwrap();
    cx.run_until_parked();

    assert!(
        !home.join("dev/app/.claude/skills/cross").exists(),
        "expected project-scope unlink to remove the project symlink"
    );
    assert!(
        home.join(".agents/skills/cross/SKILL.md").is_file(),
        "expected global original to remain intact"
    );
    cx.update(|cx| {
        assert!(app.read(cx).library().skills.iter().any(|s| s.name == "cross"));
    });
}

#[gpui_kit::test]
fn cmd_q_with_about_focused_prompts_discard_and_suppresses_duplicates(cx: &mut TestAppContext) {
    let (_temp, home, prefs_path) = setup_fixture();
    let (handle, app) = open_knack(cx, home, prefs_path);

    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-e", cx);
        window.render_frame(cx);
        window.input("# Unsaved note\n", cx);
        window.render_frame(cx);
        window.dispatch_action(Box::new(actions::About), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
        window.press("cmd-q", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // Escape closes Discard, leaving About still open below it.
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
        // Repeated Cmd+Q / Cmd+W open at most one Discard dialog.
        window.press("cmd-q", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("cmd-q", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("cmd-w", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // First Escape closes the single stacked Discard prompt; About stays open.
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
        // Second Escape closes About.
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
    })
    .unwrap();

    cx.update(|cx| {
        assert_eq!(app.read(cx).detail_mode(), DetailMode::Edit);
        assert!(!cx.windows().is_empty());
    });
}

#[gpui_kit::test]
fn cmd_w_with_about_focused_closes_clean_window(cx: &mut TestAppContext) {
    let (_temp, home, prefs_path) = setup_fixture();
    let (handle, _app) = open_knack(cx, home, prefs_path);

    cx.update_window(handle, |_, window, cx| {
        window.activate_window();
        window.dispatch_action(Box::new(actions::About), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
        window.press("cmd-w", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update(|cx| {
        assert!(cx.windows().is_empty());
    });
}

#[gpui_kit::test]
fn flags_a_skill_edited_since_it_was_installed_from_github(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(temp.path()).unwrap();
    // Installed into a project by `npx skills add acme/skills` as "hi\n",
    // whose content hash the lock file records; `pristine` is untouched,
    // `tweaked` edited.
    let project = home.join("dev/app");
    write(&project.join(".agents/skills/pristine/SKILL.md"), "hi\n");
    write(&project.join(".agents/skills/tweaked/SKILL.md"), "hi, edited\n");
    let hash = "b0ebaa711c8956779316c1ecbbd45c42d693ce66ff2c0bd60a81ef1cfeaeac32";
    write(
        &project.join("skills-lock.json"),
        &format!(
            r#"{{"version":2,"skills":{{
                "pristine":{{"source":"acme/skills","sourceType":"github","skillPath":"skills/pristine/SKILL.md","computedHash":"{hash}"}},
                "tweaked":{{"source":"acme/skills","sourceType":"github","skillPath":"skills/tweaked/SKILL.md","computedHash":"{hash}"}}}}}}"#
        ),
    );
    let prefs_path = home.join(".config/knack/prefs.json");
    let (handle, app) = open_knack(cx, home.clone(), prefs_path);

    for (name, edited) in [("pristine", false), ("tweaked", true)] {
        let row = gpui_kit::SharedString::from(format!(
            "skill-row:{}",
            project.join(format!(".agents/skills/{name}")).display()
        ));
        cx.update_window(handle, |_, window, cx| {
            window.click(row, cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let skill = app.read(cx).selected_skill().unwrap();
            assert_eq!(skill.name, name);
            assert_eq!(skill.source.as_ref().unwrap().repo, "acme/skills");
            assert_eq!(
                skill.upstream,
                Some(if edited { Upstream::Edited } else { Upstream::Current })
            );
            assert_eq!(window.try_find("source-alert").is_some(), edited, "{name}");
            assert!(window.try_find("source-link").is_some(), "{name} links its source");
            assert!(
                window.try_find("source-update").is_none(),
                "{name}: nothing newer on GitHub, so nothing to update"
            );
        })
        .unwrap();
    }
}
