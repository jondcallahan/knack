//! Application-wide actions, keyboard shortcuts, and native macOS menu bar.

use gpui_kit::{App, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType, actions};

actions!(
    knack,
    [
        About,
        CheckForUpdates,
        Quit,
        CloseWindow,
        MinimizeWindow,
        ZoomWindow,
        ToggleFullscreen,
        HideApp,
        HideOthers,
        ShowAllApps,
        NewSkill,
        FocusSearch,
        Rescan,
        EditSkill,
        SaveSkill,
        Cancel,
        OpenInEditor,
        CopyPath,
        RevealInFinder,
        ToggleEnabled,
        MoveToTrash,
        AddFolder,
        SubmitNewSkill,
    ]
);

pub const KEY_CONTEXT: &str = "Knack";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        // Standard macOS window & app shortcuts (global so they always work).
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-m", MinimizeWindow, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullscreen, None),
        KeyBinding::new("cmd-h", HideApp, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        // Knack shortcuts (global so they work regardless of which pane or widget has focus).
        KeyBinding::new("cmd-n", NewSkill, None),
        KeyBinding::new("cmd-f", FocusSearch, None),
        KeyBinding::new("cmd-r", Rescan, None),
        KeyBinding::new("cmd-e", EditSkill, None),
        KeyBinding::new("cmd-s", SaveSkill, None),
        KeyBinding::new("cmd-o", OpenInEditor, None),
        KeyBinding::new("cmd-shift-c", CopyPath, None),
        KeyBinding::new("cmd-shift-r", RevealInFinder, None),
        KeyBinding::new("cmd-shift-d", ToggleEnabled, None),
        KeyBinding::new("cmd-shift-o", AddFolder, None),
        KeyBinding::new("cmd-enter", SubmitNewSkill, None),
        KeyBinding::new("cmd-enter", SubmitNewSkill, Some("Input")),
        KeyBinding::new("cmd-f", FocusSearch, Some("Input")),
        KeyBinding::new("escape", Cancel, Some(KEY_CONTEXT)),
    ]);
}

/// `toggle_label` names what Enable / Disable does to the selected skill:
/// "Enable" or "Disable".
pub fn install_menus(toggle_label: &'static str, cx: &mut App) {
    use gpui_kit::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};

    cx.set_menus(vec![
        Menu::new("Knack").items(vec![
            MenuItem::action("About Knack", About),
            MenuItem::action("Check for Updates", CheckForUpdates),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide Knack", HideApp),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAllApps),
            MenuItem::separator(),
            MenuItem::action("Quit Knack", Quit),
        ]),
        Menu::new("File").items(vec![
            MenuItem::action("New Skill…", NewSkill),
            MenuItem::action("Add Watched Folder…", AddFolder),
            MenuItem::separator(),
            MenuItem::action("Save", SaveSkill),
            MenuItem::action("Open in Editor", OpenInEditor),
            MenuItem::action("Reveal in Finder", RevealInFinder),
            MenuItem::separator(),
            MenuItem::action("Move to Trash…", MoveToTrash),
            MenuItem::separator(),
            MenuItem::action("Close Window", CloseWindow),
        ]),
        Menu::new("Edit").items(vec![
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
            MenuItem::separator(),
            MenuItem::action("Copy Path", CopyPath),
        ]),
        Menu::new("Skill").items(vec![
            MenuItem::action("Edit", EditSkill),
            MenuItem::action(toggle_label, ToggleEnabled),
        ]),
        Menu::new("View").items(vec![
            MenuItem::action("Search Skills", FocusSearch),
            MenuItem::action("Rescan", Rescan),
            MenuItem::separator(),
            MenuItem::action("Enter Full Screen", ToggleFullscreen),
        ]),
        Menu::new("Window").items(vec![
            MenuItem::action("Minimize", MinimizeWindow),
            MenuItem::action("Zoom", ZoomWindow),
            MenuItem::separator(),
            MenuItem::action("Close", CloseWindow),
        ]),
    ]);
}
