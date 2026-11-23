//! Root application view: owns library state, persistence, async scans & file
//! operations, dialogs, notifications, actions, and the 3-pane layout.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui_kit::{
    Animation, AnimationExt as _, AnyWindowHandle, App, AppContext as _, ClipboardItem, Context,
    Entity, FocusHandle, Focusable, Global, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, PathPromptOptions, Render, Styled as _, Subscription, Task,
    Transformation, WeakEntity, Window,
    assets::IconName as LucideIcon,
    base::animation::cubic_bezier,
    component::{
        ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, TitleBar, WindowExt as _,
        button::{Button, ButtonVariant, ButtonVariants as _},
        h_flex,
        input::{EditorState, Input, InputEvent, InputState},
        list::{ListEvent, ListState},
        notification::Notification,
        resizable::{ResizablePanelEvent, ResizableState, h_resizable, resizable_panel},
        theme::Theme,
        v_flex,
    },
    div, img, percentage, prelude::FluentBuilder as _, px,
};

use crate::{
    actions::{
        About, AddFolder, Cancel, CheckForUpdates, CloseWindow, CopyPath, EditSkill, FocusSearch,
        HideApp, HideOthers, KEY_CONTEXT, MinimizeWindow, MoveToTrash, NewSkill, OpenInEditor,
        Quit, Rescan, RevealInFinder, SaveSkill, ShowAllApps, SubmitNewSkill, ToggleEnabled,
        ToggleFullscreen, ZoomWindow,
    },
    library::{self, Kind, Library, SaveOutcome, ScopeId, Skill, expand, home, scan, tilde},
    prefs::{
        MAX_LIST_WIDTH, MAX_SIDE_WIDTH, MIN_LIST_WIDTH, MIN_SIDE_WIDTH, Prefs, StatusFilter,
    },
    updater::{self, ReleaseManifest, UpdateCheck},
};

use super::{
    detail::{DetailMode, render_detail},
    new_skill::{NewSkillForm, render_new_skill},
    sidebar::render_sidebar,
    skill_list::{SkillListDelegate, render_skill_list, selected_skill_id},
};

const RESCAN_SPIN_DURATION: Duration = Duration::from_millis(600);

#[derive(Default)]
struct OpenKnackWindows(Vec<(AnyWindowHandle, WeakEntity<KnackApp>)>);

impl Global for OpenKnackWindows {}

pub struct KnackApp {
    pub(super) focus_handle: FocusHandle,
    pub(super) prefs: Prefs,
    prefs_path: PathBuf,
    pub(super) home: PathBuf,
    pub(super) library: Library,
    pub(super) scanning: bool,
    pub(super) mode: DetailMode,
    pub(super) search_input: Entity<InputState>,
    pub(super) editor_input: Entity<EditorState>,
    pub(super) draft_seen: Option<SystemTime>,
    pub(super) conflict: bool,
    pub(super) saving: bool,
    pub(super) resizable_state: Entity<ResizableState>,
    pub(super) skill_list: Entity<ListState<SkillListDelegate>>,
    pub(super) new_skill: NewSkillForm,
    rescan_epoch: usize,
    rescan_spinning: bool,
    available_update: Option<ReleaseManifest>,
    checking_updates: bool,
    installing_update: bool,
    auto_check_updates: bool,
    initial_mode_applied: bool,
    allow_close_dirty: bool,
    discard_prompt_open: bool,
    new_skill_session: u64,
    next_op_id: u64,
    _subscriptions: Vec<Subscription>,
    _scan_task: Option<Task<()>>,
    _spin_task: Option<Task<()>>,
    active_ops: Vec<(u64, Task<()>)>,
    _update_task: Option<Task<()>>,
}

impl KnackApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let auto_check = std::env::var_os("KNACK_NO_UPDATE_CHECK").is_none();
        Self::new_with_options(home(), Prefs::path(), auto_check, window, cx)
    }

    pub fn new_in(
        home: PathBuf,
        prefs_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_with_options(home, prefs_path, false, window, cx)
    }

    fn new_with_options(
        home: PathBuf,
        prefs_path: PathBuf,
        auto_check_updates: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let prefs = if prefs_path == Prefs::path() {
            Prefs::load()
        } else {
            Prefs::load_from(&prefs_path)
        };
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search skills"));
        let editor_input = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("markdown")
                .soft_wrap(true)
                .line_number(true)
        });
        let resizable_state = cx.new(|_| ResizableState::default());
        let skill_list = cx.new(|cx| ListState::new(SkillListDelegate::new(), window, cx));
        let new_skill = NewSkillForm::new(&[], &prefs.new_dirs, &home, window, cx);

        let mut subscriptions = Vec::new();

        // Sync Light / Dark theme whenever macOS appearance changes unless forced via env.
        if std::env::var_os("KNACK_APPEARANCE").is_none() {
            subscriptions.push(cx.observe_window_appearance(window, |_, window, cx| {
                Theme::sync_system_appearance(Some(window), cx);
            }));
        }

        // Persist dragged pane widths.
        subscriptions.push(cx.subscribe(
            &resizable_state,
            |this, state, event: &ResizablePanelEvent, cx| {
                if matches!(event, ResizablePanelEvent::Resized) {
                    let sizes = state.read(cx).sizes();
                    if sizes.len() >= 2 {
                        let side = f32::from(sizes[0]).clamp(MIN_SIDE_WIDTH, MAX_SIDE_WIDTH);
                        let list = f32::from(sizes[1]).clamp(MIN_LIST_WIDTH, MAX_LIST_WIDTH);
                        if (this.prefs.side_width - side).abs() >= 1.0
                            || (this.prefs.list_width - list).abs() >= 1.0
                        {
                            this.prefs.side_width = side;
                            this.prefs.list_width = list;
                            this.save_prefs();
                        }
                    }
                }
            },
        ));

        // Search input drives live filtering.
        subscriptions.push(cx.subscribe_in(
            &search_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.refresh_filtered_list(window, cx);
                    cx.notify();
                }
            },
        ));

        // Editor changes update dirty state for the Save button.
        subscriptions.push(cx.subscribe_in(
            &editor_input,
            window,
            |_this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        ));

        // Skill list selection (click or keyboard arrows / Enter).
        subscriptions.push(cx.subscribe_in(
            &skill_list,
            window,
            |this, list, event: &ListEvent, window, cx| match event {
                ListEvent::Select(ix) | ListEvent::Confirm(ix) => {
                    if let Some(id) = selected_skill_id(&list, *ix, cx) {
                        this.request_select_skill(id, window, cx);
                    }
                }
                ListEvent::Cancel => {}
            },
        ));

        let weak_close = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            let Some(entity) = weak_close.upgrade() else {
                return true;
            };
            entity.update(cx, |this, cx| {
                if this.allow_close_dirty || !this.is_dirty(cx) {
                    return true;
                }
                this.confirm_discard_if_dirty(window, cx, |this, window, _cx| {
                    this.allow_close_dirty = true;
                    window.remove_window();
                });
                false
            })
        });

        let weak_self = cx.entity().downgrade();
        let handle = window.window_handle();
        let windows = cx.default_global::<OpenKnackWindows>();
        windows.0.retain(|(_, w)| w.upgrade().is_some());
        windows.0.push((handle, weak_self));

        let mut app = Self {
            focus_handle,
            prefs,
            prefs_path,
            home,
            library: Library::default(),
            scanning: true,
            mode: DetailMode::Read,
            search_input,
            editor_input,
            draft_seen: None,
            conflict: false,
            saving: false,
            resizable_state,
            skill_list,
            new_skill,
            rescan_epoch: 0,
            rescan_spinning: false,
            available_update: None,
            checking_updates: false,
            installing_update: false,
            auto_check_updates,
            initial_mode_applied: false,
            allow_close_dirty: false,
            discard_prompt_open: false,
            new_skill_session: 0,
            next_op_id: 0,
            _subscriptions: subscriptions,
            _scan_task: None,
            _spin_task: None,
            active_ops: Vec::new(),
            _update_task: None,
        };

        app.trigger_scan(window, cx);
        if app.auto_check_updates {
            app.start_update_check(updater::feed_url(), false, window, cx);
        }
        app
    }

    pub(super) fn save_prefs(&self) {
        self.prefs.save_to(&self.prefs_path);
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn is_scanning(&self) -> bool {
        self.scanning
    }

    pub fn is_rescan_spinning(&self) -> bool {
        self.rescan_spinning
    }

    pub fn is_search_focused(&self, window: &Window, cx: &App) -> bool {
        self.search_input.focus_handle(cx).is_focused(window)
    }

    pub fn search_text(&self, cx: &App) -> String {
        self.search_query(cx)
    }

    pub fn selected_skill(&self) -> Option<&Skill> {
        self.current_skill()
    }

    pub fn detail_mode(&self) -> DetailMode {
        self.mode
    }

    pub fn has_conflict(&self) -> bool {
        self.conflict
    }

    pub fn filtered_skill_count(&self, cx: &App) -> usize {
        self.skill_list.read(cx).delegate().len()
    }

    pub fn available_update(&self) -> Option<&ReleaseManifest> {
        self.available_update.as_ref()
    }

    pub fn check_updates_from_url(
        &mut self,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_update_check(url, false, window, cx);
    }

    pub(super) fn search_query(&self, cx: &App) -> String {
        self.search_input.read(cx).value().to_string()
    }

    pub(super) fn current_skill(&self) -> Option<&Skill> {
        let id = self.prefs.selected_path()?;
        self.library.skill(&id)
    }

    pub(super) fn is_dirty(&self, cx: &App) -> bool {
        if self.mode != DetailMode::Edit {
            return false;
        }
        let Some(skill) = self.current_skill() else {
            return false;
        };
        self.editor_input.read(cx).value().as_ref() != skill.text
    }

    pub(super) fn scope_counts(&self) -> (usize, usize, usize) {
        let scope = self.prefs.scope_id();
        let mut total = 0;
        let mut enabled = 0;
        let mut disabled = 0;
        for skill in &self.library.skills {
            if scope.contains(skill) {
                total += 1;
                if skill.disabled {
                    disabled += 1;
                } else {
                    enabled += 1;
                }
            }
        }
        (total, enabled, disabled)
    }

    pub(super) fn trigger_user_rescan(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rescan_epoch = self.rescan_epoch.wrapping_add(1);
        self.rescan_spinning = true;
        let epoch = self.rescan_epoch;
        self._spin_task = Some(cx.spawn_in(window, async move |this, cx| {
            let mut current_epoch = epoch;
            loop {
                cx.background_executor()
                    .timer(RESCAN_SPIN_DURATION)
                    .await;
                let keep_spinning = this
                    .update(cx, |this, cx| {
                        if this.rescan_epoch != current_epoch {
                            return false;
                        }
                        if this.scanning {
                            this.rescan_epoch = this.rescan_epoch.wrapping_add(1);
                            current_epoch = this.rescan_epoch;
                            cx.notify();
                            true
                        } else {
                            this.rescan_spinning = false;
                            cx.notify();
                            false
                        }
                    })
                    .unwrap_or(false);
                if !keep_spinning {
                    break;
                }
            }
        }));
        self.trigger_scan(window, cx);
    }

    pub(super) fn trigger_scan(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.scanning = true;
        let folders = self.prefs.folders.clone();
        let home = self.home.clone();

        self._scan_task = Some(cx.spawn_in(window, async move |this, cx| {
            let scanned = cx
                .background_executor()
                .spawn(async move { scan(&folders, &home) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.library = scanned;
                this.scanning = false;

                // Reset scope to All if the previously saved scope no longer exists.
                let scope = this.prefs.scope_id();
                if scope != ScopeId::All && !this.library.scopes.iter().any(|s| s.id == scope) {
                    this.prefs.set_scope(&ScopeId::All);
                    this.save_prefs();
                }

                // Clear selection if the previously selected skill no longer exists.
                if let Some(sel) = this.prefs.selected_path()
                    && this.library.skill(&sel).is_none()
                {
                    this.prefs.set_selected(None);
                    if this.mode == DetailMode::Edit {
                        this.mode = DetailMode::Read;
                    }
                    this.save_prefs();
                }

                this.new_skill.sync_destinations(
                    &this.library.installed,
                    &this.prefs.new_dirs,
                    &this.home,
                    window,
                    cx,
                );
                this.refresh_filtered_list(window, cx);
                if !this.initial_mode_applied {
                    this.initial_mode_applied = true;
                    this.apply_env_startup_state(window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn apply_env_startup_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Ok(slug) = std::env::var("KNACK_SELECT")
            && !slug.trim().is_empty()
            && let Some(skill) = self
                .library
                .skills
                .iter()
                .find(|s| s.name == slug)
                .cloned()
        {
            self.commit_select_skill(skill.id, window, cx);
        }
        match std::env::var("KNACK_START_MODE").ok().as_deref() {
            Some("edit") => self.enter_edit_mode(window, cx),
            Some("new") => self.open_new_skill_form(window, cx),
            _ => {}
        }
    }

    fn refresh_filtered_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scope = self.prefs.scope_id();
        let status = self.prefs.status;
        let query = self.search_query(cx);
        let filtered: Vec<Skill> = self
            .library
            .skills
            .iter()
            .filter(|s| scope.contains(s) && status.matches(s) && s.matches(&query))
            .cloned()
            .collect();

        // If nothing is selected yet (or current selection disappeared), pick
        // the first visible item on initial load only when in Read mode and none selected.
        if self.prefs.selected_path().is_none()
            && self.mode == DetailMode::Read
            && let Some(first) = filtered.first()
        {
            self.prefs.set_selected(Some(&first.id));
            self.save_prefs();
        }

        let selected = self.prefs.selected_path();
        self.skill_list.update(cx, |list, cx| {
            let ix = list.delegate_mut().set_items(filtered, selected.as_deref());
            list.set_selected_index(ix, window, cx);
        });
    }

    fn restore_list_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.prefs.selected_path();
        self.skill_list.update(cx, |list, cx| {
            let ix = selected
                .as_deref()
                .and_then(|id| list.delegate().index_of(id));
            list.set_selected_index(ix, window, cx);
        });
    }

    pub(super) fn select_scope(
        &mut self,
        scope: ScopeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.prefs.scope_id() == scope {
            return;
        }
        self.prefs.set_scope(&scope);
        self.save_prefs();
        self.refresh_filtered_list(window, cx);
        cx.notify();
    }

    pub(super) fn set_status_filter(
        &mut self,
        status: StatusFilter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.prefs.status == status {
            return;
        }
        self.prefs.status = status;
        self.save_prefs();
        self.refresh_filtered_list(window, cx);
        cx.notify();
    }

    pub(super) fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.refresh_filtered_list(window, cx);
        cx.notify();
    }

    pub fn handle_global_close_window(cx: &mut App) {
        let Some(active) = cx.active_window().or_else(|| {
            let windows = cx.windows();
            (windows.len() == 1).then(|| windows[0])
        }) else {
            return;
        };
        let entity = cx
            .try_global::<OpenKnackWindows>()
            .and_then(|g| g.0.iter().find(|(h, _)| *h == active).and_then(|(_, w)| w.upgrade()));
        cx.defer(move |cx| {
            let _ = active.update(cx, |_, window, cx| {
                if let Some(entity) = entity {
                    entity.update(cx, |this, cx| {
                        this.confirm_discard_if_dirty(window, cx, |this, window, _| {
                            this.allow_close_dirty = true;
                            window.remove_window();
                        });
                    });
                } else {
                    window.remove_window();
                }
            });
        });
    }

    pub fn handle_global_quit(cx: &mut App) {
        let open: Vec<(AnyWindowHandle, WeakEntity<KnackApp>)> = cx
            .try_global::<OpenKnackWindows>()
            .map(|g| g.0.clone())
            .unwrap_or_default();
        for (handle, weak) in open {
            let Some(entity) = weak.upgrade() else {
                continue;
            };
            if entity.read(cx).is_dirty(cx) {
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, cx| {
                        entity.update(cx, |this, cx| {
                            this.confirm_discard_if_dirty(window, cx, |this, _, cx| {
                                this.allow_close_dirty = true;
                                cx.quit();
                            });
                        });
                    });
                });
                return;
            }
        }
        cx.quit();
    }

    fn next_op(&mut self) -> u64 {
        self.next_op_id = self.next_op_id.wrapping_add(1);
        self.next_op_id
    }

    fn track_op(&mut self, id: u64, task: Task<()>) {
        self.active_ops.push((id, task));
    }

    fn finish_op(&mut self, id: u64) {
        self.active_ops.retain(|(op_id, _)| *op_id != id);
    }

    fn confirm_discard_if_dirty<F>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        proceed: F,
    ) where
        F: FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    {
        self.confirm_discard_with_cancel(window, cx, proceed, |_, _, _| {});
    }

    fn confirm_discard_with_cancel<F, C>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        proceed: F,
        on_cancel: C,
    ) where
        F: FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        C: FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    {
        if !self.is_dirty(cx) {
            proceed(self, window, cx);
            return;
        }
        if self.discard_prompt_open && window.has_active_dialog(cx) {
            return;
        }
        self.discard_prompt_open = true;
        let weak = cx.entity().downgrade();
        let proceed_cell = std::rc::Rc::new(std::cell::Cell::new(Some(proceed)));
        let cancel_cell = std::rc::Rc::new(std::cell::Cell::new(Some(on_cancel)));
        window.open_alert_dialog(cx, move |alert, _, _| {
            let weak_ok = weak.clone();
            let weak_cancel = weak.clone();
            let weak_close = weak.clone();
            let proceed_cell = proceed_cell.clone();
            let cancel_cell = cancel_cell.clone();
            alert
                .confirm()
                .title("Discard unsaved changes?")
                .ok_text("Discard")
                .ok_variant(ButtonVariant::Danger)
                .cancel_text("Keep editing")
                .on_ok(move |_, window, cx| {
                    let _ = weak_ok.update(cx, |this, cx| {
                        this.discard_prompt_open = false;
                        if let Some(f) = proceed_cell.take() {
                            f(this, window, cx);
                        }
                    });
                    true
                })
                .on_cancel(move |_, window, cx| {
                    let _ = weak_cancel.update(cx, |this, cx| {
                        this.discard_prompt_open = false;
                        if let Some(c) = cancel_cell.take() {
                            c(this, window, cx);
                        }
                    });
                    true
                })
                .on_close(move |_, _, cx| {
                    let _ = weak_close.update(cx, |this, _| {
                        this.discard_prompt_open = false;
                    });
                })
        });
    }

    fn request_select_skill(
        &mut self,
        id: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.prefs.selected_path().as_deref() == Some(id.as_path())
            && self.mode != DetailMode::New
        {
            return;
        }
        let target_id = id;
        self.confirm_discard_with_cancel(
            window,
            cx,
            move |this, window, cx| this.commit_select_skill(target_id, window, cx),
            |this, window, cx| this.restore_list_selection(window, cx),
        );
    }

    fn commit_select_skill(
        &mut self,
        id: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode == DetailMode::New {
            self.new_skill_session = self.new_skill_session.wrapping_add(1);
        }
        self.prefs.set_selected(Some(&id));
        self.save_prefs();
        self.mode = DetailMode::Read;
        self.conflict = false;
        self.restore_list_selection(window, cx);
        cx.notify();
    }

    pub(super) fn enter_edit_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(skill) = self.current_skill().cloned() else {
            return;
        };
        if self.mode == DetailMode::New {
            self.new_skill_session = self.new_skill_session.wrapping_add(1);
        }
        self.draft_seen = skill.modified;
        self.conflict = false;
        self.mode = DetailMode::Edit;
        self.editor_input.update(cx, |editor, cx| {
            editor.set_value(skill.text.clone(), window, cx);
            editor.focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn cancel_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_input.focus_handle(cx).is_focused(window) {
            self.focus_handle.focus(window, cx);
            cx.notify();
            return;
        }
        match self.mode {
            DetailMode::Edit => {
                self.confirm_discard_if_dirty(window, cx, |this, window, cx| {
                    this.mode = DetailMode::Read;
                    this.conflict = false;
                    this.focus_handle.focus(window, cx);
                    cx.notify();
                });
            }
            DetailMode::New => {
                self.new_skill_session = self.new_skill_session.wrapping_add(1);
                self.mode = DetailMode::Read;
                self.focus_handle.focus(window, cx);
                cx.notify();
            }
            DetailMode::Read => {}
        }
    }

    pub(super) fn reload_from_disk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.conflict = false;
        self.mode = DetailMode::Read;
        self.trigger_scan(window, cx);
    }

    pub(super) fn save_current(
        &mut self,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode != DetailMode::Edit || self.saving {
            return;
        }
        let Some(skill) = self.current_skill().cloned() else {
            return;
        };
        let draft = self.editor_input.read(cx).value().to_string();
        if !force && draft == skill.text {
            return;
        }
        let path = skill.file.clone();
        let seen = self.draft_seen;
        self.saving = true;
        let op_id = self.next_op();

        let task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { library::save(&path, &draft, seen, force) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                cx.defer_in(window, move |this, _, _| this.finish_op(op_id));
                this.saving = false;
                match result {
                    Ok(SaveOutcome::Saved(modified)) => {
                        this.draft_seen = modified;
                        this.mode = DetailMode::Read;
                        this.conflict = false;
                        window.push_notification(Notification::success("Saved"), cx);
                        this.trigger_scan(window, cx);
                    }
                    Ok(SaveOutcome::ChangedOnDisk) => {
                        this.conflict = true;
                        cx.notify();
                    }
                    Err(err) => {
                        window.push_notification(Notification::error(err.to_string()), cx);
                        cx.notify();
                    }
                }
            });
        });
        self.track_op(op_id, task);
        cx.notify();
    }

    pub(super) fn start_new_skill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard_if_dirty(window, cx, |this, window, cx| {
            this.open_new_skill_form(window, cx);
        });
    }

    fn open_new_skill_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_skill_session = self.new_skill_session.wrapping_add(1);
        self.mode = DetailMode::New;
        self.conflict = false;
        self.new_skill.sync_destinations(
            &self.library.installed,
            &self.prefs.new_dirs,
            &self.home,
            window,
            cx,
        );
        self.new_skill.reset(window, cx);
        cx.notify();
    }

    pub(super) fn submit_new_skill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode != DetailMode::New || !self.new_skill.can_create(cx) {
            return;
        }
        let dirs = self.new_skill.selected_dirs(&self.home, cx);
        let preferred_primary = expand("~/.agents/skills", &self.home);
        let (name, text) = self.new_skill.compose_skill_md(cx);
        let session = self.new_skill_session;
        self.new_skill.creating = true;
        self.new_skill.error = None;
        let op_id = self.next_op();

        let task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { library::create_many(&dirs, &preferred_primary, &name, &text) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                cx.defer_in(window, move |this, _, _| this.finish_op(op_id));
                let same_form = this.mode == DetailMode::New && this.new_skill_session == session;
                if same_form {
                    this.new_skill.creating = false;
                }
                match result {
                    Ok(created_id) => {
                        if same_form {
                            this.prefs.set_selected(Some(&created_id));
                            this.prefs.set_scope(&ScopeId::All);
                            this.save_prefs();
                            this.mode = DetailMode::Read;
                        }
                        window.push_notification(Notification::success("Skill created"), cx);
                        this.trigger_scan(window, cx);
                    }
                    Err(err) => {
                        if same_form {
                            this.new_skill.error = Some(err.to_string());
                            cx.notify();
                        } else {
                            window.push_notification(Notification::error(err.to_string()), cx);
                        }
                    }
                }
            });
        });
        self.track_op(op_id, task);
        cx.notify();
    }

    pub(super) fn toggle_skill_enabled(
        &mut self,
        skill_id: PathBuf,
        kind: Kind,
        file: PathBuf,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let op_id = self.next_op();
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { library::set_enabled(&file, enabled) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                cx.defer_in(window, move |this, _, _| this.finish_op(op_id));
                match result {
                    Ok(new_path) => {
                        // For a slash command, the skill ID is the file path itself,
                        // so keep it selected under its renamed path.
                        if kind == Kind::Command && this.prefs.selected_path().as_deref() == Some(&skill_id) {
                            this.prefs.set_selected(Some(&new_path));
                            this.save_prefs();
                        }
                        let msg = if enabled { "Enabled" } else { "Disabled" };
                        window.push_notification(Notification::success(msg), cx);
                        this.trigger_scan(window, cx);
                    }
                    Err(err) => {
                        window.push_notification(Notification::error(err.to_string()), cx);
                        cx.notify();
                    }
                }
            });
        });
        self.track_op(op_id, task);
    }

    pub(super) fn link_agent(
        &mut self,
        skill_dir: PathBuf,
        agent_dir: PathBuf,
        agent_label: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let op_id = self.next_op();
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { library::link(&skill_dir, &agent_dir) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                cx.defer_in(window, move |this, _, _| this.finish_op(op_id));
                match result {
                    Ok(()) => {
                        window.push_notification(
                            Notification::success(format!("Shared with {agent_label}")),
                            cx,
                        );
                        this.trigger_scan(window, cx);
                    }
                    Err(err) => {
                        window.push_notification(Notification::error(err.to_string()), cx);
                    }
                }
            });
        });
        self.track_op(op_id, task);
    }

    pub(super) fn unlink_agent(
        &mut self,
        entry: PathBuf,
        agent_label: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let op_id = self.next_op();
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { library::unlink(&entry) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                cx.defer_in(window, move |this, _, _| this.finish_op(op_id));
                match result {
                    Ok(()) => {
                        window.push_notification(
                            Notification::success(format!("Removed from {agent_label}")),
                            cx,
                        );
                        this.trigger_scan(window, cx);
                    }
                    Err(err) => {
                        window.push_notification(Notification::error(err.to_string()), cx);
                    }
                }
            });
        });
        self.track_op(op_id, task);
    }

    pub(super) fn confirm_trash_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(skill) = self.current_skill().cloned() else {
            return;
        };
        let link_paths: Vec<PathBuf> = skill.links().map(|p| p.entry.clone()).collect();
        let link_count = link_paths.len();
        let target = skill.id.clone();
        let title = format!("Move \"{}\" to the Trash?", skill.name);
        let description = (link_count > 0).then(|| {
            let noun = if link_count == 1 {
                "1 other agent".to_string()
            } else {
                format!("{link_count} other agents")
            };
            format!("Links to it from {noun} are removed too.")
        });

        let weak = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let weak = weak.clone();
            let target = target.clone();
            let link_paths = link_paths.clone();
            alert
                .confirm()
                .title(title.clone())
                .when_some(description.clone(), |this, desc| this.description(desc))
                .ok_text("Move to Trash")
                .ok_variant(ButtonVariant::Danger)
                .cancel_text("Cancel")
                .on_ok(move |_, window, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        this.trash_skill(target.clone(), link_paths.clone(), window, cx);
                    });
                    true
                })
        });
    }

    fn trash_skill(
        &mut self,
        target: PathBuf,
        links: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prefs.set_selected(None);
        self.save_prefs();
        self.mode = DetailMode::Read;
        self.conflict = false;
        let op_id = self.next_op();

        let task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { library::trash(&target, &links) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                cx.defer_in(window, move |this, _, _| this.finish_op(op_id));
                match result {
                    Ok(()) => {
                        window.push_notification(Notification::success("Moved to Trash"), cx);
                    }
                    Err(err) => {
                        window.push_notification(Notification::error(err.to_string()), cx);
                    }
                }
                this.trigger_scan(window, cx);
            });
        });
        self.track_op(op_id, task);
    }

    pub(super) fn copy_path(&self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let text = path.to_string_lossy().into_owned();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        window.push_notification(Notification::success("Path copied"), cx);
    }

    pub(super) fn prompt_add_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Watch folder for project skills".into()),
        });
        let home = self.home.clone();
        let op_id = self.next_op();
        let task = cx.spawn_in(window, async move |this, cx| {
            let picked = rx.await.ok().and_then(|r| r.ok()).flatten().and_then(|v| v.into_iter().next());
            let _ = this.update_in(cx, |this, window, cx| {
                cx.defer_in(window, move |this, _, _| this.finish_op(op_id));
                if let Some(picked) = picked {
                    let shown = tilde(&picked, &home);
                    if !this.prefs.folders.contains(&shown) {
                        this.prefs.folders.push(shown);
                        this.save_prefs();
                        this.trigger_scan(window, cx);
                    }
                }
            });
        });
        self.track_op(op_id, task);
    }

    pub(super) fn remove_folder(
        &mut self,
        folder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prefs.folders.retain(|f| f != folder);
        if matches!(self.prefs.scope_id(), ScopeId::Project(_)) {
            self.prefs.set_scope(&ScopeId::All);
        }
        self.save_prefs();
        self.trigger_scan(window, cx);
    }

    // Action handlers wired to shortcuts, menus, and ⋯ dropdown.
    fn open_about_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        let checking = self.checking_updates;
        let weak = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let muted_fg = cx.theme().muted_foreground;
            let mono_font = cx.theme().mono_font_family.clone();
            let weak_check = weak.clone();
            dialog
                .w(px(300.))
                .close_button(true)
                .overlay_closable(true)
                .keyboard(true)
                .child(
                    v_flex()
                        .w_full()
                        .items_center()
                        .gap_1p5()
                        .py_3()
                        .child(img("icons/app-icon.png").size(px(64.)).rounded(px(14.)))
                        .child(div().pt_1().text_lg().font_semibold().child("Knack"))
                        .child(
                            div()
                                .font_family(mono_font)
                                .text_xs()
                                .text_color(muted_fg)
                                .child(format!("Version {}", updater::CURRENT_VERSION)),
                        )
                        .child(
                            div()
                                .pt_1()
                                .text_xs()
                                .text_color(muted_fg)
                                .text_center()
                                .child("Browse, edit, and share AI agent skills."),
                        )
                        .child(
                            div().pt_1().child(
                                Button::new("about-check-updates")
                                    .xsmall()
                                    .outline()
                                    .label(if checking { "Checking…" } else { "Check for updates" })
                                    .on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        let _ = weak_check.update(cx, |this, cx| {
                                            this.start_update_check(
                                                updater::feed_url(),
                                                true,
                                                window,
                                                cx,
                                            );
                                        });
                                    }),
                            ),
                        ),
                )
        });
    }

    fn start_update_check(
        &mut self,
        url: String,
        user_initiated: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.checking_updates || self.installing_update {
            return;
        }
        self.checking_updates = true;
        self._update_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { updater::check_for_updates(&url) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.checking_updates = false;
                match result {
                    Ok(UpdateCheck::Available(manifest)) => {
                        this.available_update = Some(manifest.clone());
                        if user_initiated {
                            this.open_update_dialog(manifest, window, cx);
                        }
                        cx.notify();
                    }
                    Ok(UpdateCheck::UpToDate { current }) => {
                        this.available_update = None;
                        if user_initiated {
                            window.push_notification(
                                Notification::info(format!("Knack {current} is up to date.")),
                                cx,
                            );
                        }
                        cx.notify();
                    }
                    Err(err) => {
                        if user_initiated {
                            window.push_notification(Notification::warning(err.to_string()), cx);
                        }
                        cx.notify();
                    }
                }
            });
        }));
        cx.notify();
    }

    fn open_update_dialog(
        &self,
        manifest: ReleaseManifest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.entity().downgrade();
        let can_self_install =
            updater::current_app_bundle().is_some_and(|p| updater::can_self_update(&p));
        window.open_dialog(cx, move |dialog, _, cx| {
            let muted_fg = cx.theme().muted_foreground;
            let mono_font = cx.theme().mono_font_family.clone();
            let weak_install = weak.clone();
            let manifest_install = manifest.clone();
            let notes_url = if manifest.release_url.is_empty() {
                updater::RELEASES_URL.to_string()
            } else {
                manifest.release_url.clone()
            };
            let primary_label = if can_self_install {
                "Install & Relaunch"
            } else {
                "Download update"
            };
            let version_line =
                format!("Version {} → {}", updater::CURRENT_VERSION, manifest.version);
            let notes = manifest.notes.trim().to_string();

            dialog
                .w(px(380.))
                .close_button(true)
                .overlay_closable(true)
                .keyboard(true)
                .title("Update available")
                .child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .py_1()
                        .child(
                            div()
                                .font_family(mono_font)
                                .text_xs()
                                .text_color(muted_fg)
                                .child(version_line),
                        )
                        .when(!notes.is_empty(), |this| {
                            this.child(div().text_sm().child(notes.clone()))
                        }),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            Button::new("update-notes")
                                .small()
                                .ghost()
                                .label("Release notes")
                                .on_click(move |_, _, cx| {
                                    cx.open_url(&notes_url);
                                }),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Button::new("update-later")
                                        .small()
                                        .outline()
                                        .label("Later")
                                        .on_click(|_, window, cx| {
                                            window.close_dialog(cx);
                                        }),
                                )
                                .child(
                                    Button::new("update-install")
                                        .small()
                                        .primary()
                                        .label(primary_label)
                                        .on_click(move |_, window, cx| {
                                            window.close_dialog(cx);
                                            let _ = weak_install.update(cx, |this, cx| {
                                                this.install_available_update(
                                                    manifest_install.clone(),
                                                    window,
                                                    cx,
                                                );
                                            });
                                        }),
                                ),
                        ),
                )
        });
    }

    fn install_available_update(
        &mut self,
        manifest: ReleaseManifest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.installing_update {
            return;
        }
        let Some(dest_app) =
            updater::current_app_bundle().filter(|p| updater::can_self_update(p))
        else {
            let url = if !manifest.dmg_url.is_empty() {
                &manifest.dmg_url
            } else {
                &manifest.release_url
            };
            cx.open_url(url);
            return;
        };

        self.confirm_discard_if_dirty(window, cx, move |this, window, cx| {
            this.allow_close_dirty = true;
            this.start_update_install(manifest, dest_app, window, cx);
        });
    }

    fn start_update_install(
        &mut self,
        manifest: ReleaseManifest,
        dest_app: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.installing_update = true;
        window.push_notification(
            Notification::info(format!("Downloading Knack {}…", manifest.version)),
            cx,
        );
        self._update_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    updater::download_and_install(&manifest, &dest_app).map(|()| dest_app)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.installing_update = false;
                match result {
                    Ok(dest_app) => match updater::schedule_relaunch(&dest_app) {
                        Ok(()) => cx.quit(),
                        Err(err) => {
                            this.allow_close_dirty = false;
                            window.push_notification(Notification::error(err.to_string()), cx);
                            cx.notify();
                        }
                    },
                    Err(err) => {
                        this.allow_close_dirty = false;
                        window.push_notification(Notification::error(err.to_string()), cx);
                        cx.notify();
                    }
                }
            });
        }));
        cx.notify();
    }

    fn on_about(&mut self, _: &About, window: &mut Window, cx: &mut Context<Self>) {
        self.open_about_dialog(window, cx);
    }

    fn on_check_for_updates(
        &mut self,
        _: &CheckForUpdates,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_update_check(updater::feed_url(), true, window, cx);
    }

    fn on_quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard_if_dirty(window, cx, |this, _, cx| {
            this.allow_close_dirty = true;
            cx.quit();
        });
    }

    fn on_close_window(&mut self, _: &CloseWindow, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard_if_dirty(window, cx, |this, window, _| {
            this.allow_close_dirty = true;
            window.remove_window();
        });
    }

    fn on_minimize_window(
        &mut self,
        _: &MinimizeWindow,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        window.minimize_window();
    }

    fn on_zoom_window(&mut self, _: &ZoomWindow, window: &mut Window, _cx: &mut Context<Self>) {
        window.zoom_window();
    }

    fn on_toggle_fullscreen(
        &mut self,
        _: &ToggleFullscreen,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        window.toggle_fullscreen();
    }

    fn on_hide_app(&mut self, _: &HideApp, _window: &mut Window, cx: &mut Context<Self>) {
        cx.hide();
    }

    fn on_hide_others(&mut self, _: &HideOthers, _window: &mut Window, cx: &mut Context<Self>) {
        cx.hide_other_apps();
    }

    fn on_show_all_apps(&mut self, _: &ShowAllApps, _window: &mut Window, cx: &mut Context<Self>) {
        cx.unhide_other_apps();
    }

    fn on_new_skill(&mut self, _: &NewSkill, window: &mut Window, cx: &mut Context<Self>) {
        self.start_new_skill(window, cx);
    }

    fn on_focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.search_input.update(cx, |s, cx| s.focus(window, cx));
    }

    fn on_rescan(&mut self, _: &Rescan, window: &mut Window, cx: &mut Context<Self>) {
        self.trigger_user_rescan(window, cx);
    }

    fn on_edit_skill(&mut self, _: &EditSkill, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == DetailMode::Read {
            self.enter_edit_mode(window, cx);
        }
    }

    fn on_save_skill(&mut self, _: &SaveSkill, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == DetailMode::Edit {
            self.save_current(false, window, cx);
        }
    }

    fn on_submit_new_skill(
        &mut self,
        _: &SubmitNewSkill,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode == DetailMode::New {
            self.submit_new_skill(window, cx);
        }
    }

    fn on_cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_mode(window, cx);
    }

    fn on_open_in_editor(&mut self, _: &OpenInEditor, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(skill) = self.current_skill() {
            cx.open_with_system(&skill.file);
        }
    }

    fn on_copy_path(&mut self, _: &CopyPath, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(file) = self.current_skill().map(|s| s.file.clone()) {
            self.copy_path(&file, window, cx);
        }
    }

    fn on_reveal_in_finder(
        &mut self,
        _: &RevealInFinder,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(skill) = self.current_skill() {
            cx.reveal_path(&skill.file);
        }
    }

    fn on_toggle_enabled(
        &mut self,
        _: &ToggleEnabled,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(skill) = self.current_skill().cloned() {
            self.toggle_skill_enabled(skill.id, skill.kind, skill.file, skill.disabled, window, cx);
        }
    }

    fn on_move_to_trash(&mut self, _: &MoveToTrash, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode != DetailMode::New {
            self.confirm_trash_current(window, cx);
        }
    }

    fn on_add_folder(&mut self, _: &AddFolder, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_add_folder(window, cx);
    }

    fn render_title_bar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted_fg = cx.theme().muted_foreground;
        let is_read = self.mode == DetailMode::Read;

        let refresh_icon = if self.rescan_spinning {
            Icon::new(IconName::RefreshCw)
                .xsmall()
                .with_animation(
                    ("rescan-spin", self.rescan_epoch),
                    Animation::new(RESCAN_SPIN_DURATION)
                        .with_easing(cubic_bezier(0.22, 1.0, 0.36, 1.0)),
                    |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
                )
                .into_any_element()
        } else {
            Icon::new(IconName::RefreshCw).xsmall().into_any_element()
        };

        TitleBar::new().child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_3()
                .pr_2()
                .child(div().text_sm().font_semibold().child("Knack"))
                .child(
                    h_flex()
                        .items_center()
                        .gap_1p5()
                        .when_some(self.available_update.clone(), |row, manifest| {
                            let version = manifest.version.clone();
                            row.child(
                                Button::new("toolbar-update")
                                    .xsmall()
                                    .outline()
                                    .icon(Icon::new(IconName::ArrowDown))
                                    .label(format!("Update {version}"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_update_dialog(manifest.clone(), window, cx);
                                    })),
                            )
                        })
                        .child(
                            div().w(px(220.)).child(
                                Input::new(&self.search_input)
                                    .id("search-input")
                                    .xsmall()
                                    .prefix(Icon::new(IconName::Search).xsmall().text_color(muted_fg))
                                    .cleanable(true),
                            ),
                        )
                        .child(
                            Button::new("toolbar-rescan")
                                .ghost()
                                .xsmall()
                                .w_6()
                                .h_6()
                                .accessibility_label("Rescan")
                                .tooltip_with_action("Rescan", &Rescan, None)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.trigger_user_rescan(window, cx);
                                }))
                                .child(refresh_icon),
                        )
                        .child(
                            Button::new("toolbar-new")
                                .xsmall()
                                .map(|btn| if is_read { btn.primary() } else { btn.outline() })
                                .icon(Icon::new(LucideIcon::SquarePen))
                                .label("New skill")
                                .tooltip_with_action("New skill", &NewSkill, None)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.start_new_skill(window, cx);
                                })),
                        ),
                ),
        )
    }
}

impl Focusable for KnackApp {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for KnackApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let side_w = px(self.prefs.side_width);
        let list_w = px(self.prefs.list_width);

        let detail_pane = match self.mode {
            DetailMode::New => render_new_skill(self, window, cx).into_any_element(),
            DetailMode::Read | DetailMode::Edit => render_detail(self, window, cx).into_any_element(),
        };

        v_flex()
            .id("knack-root")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if window.focused(cx).is_none() {
                        this.focus_handle.focus(window, cx);
                    }
                }),
            )
            .on_action(cx.listener(Self::on_about))
            .on_action(cx.listener(Self::on_check_for_updates))
            .on_action(cx.listener(Self::on_quit))
            .on_action(cx.listener(Self::on_close_window))
            .on_action(cx.listener(Self::on_minimize_window))
            .on_action(cx.listener(Self::on_zoom_window))
            .on_action(cx.listener(Self::on_toggle_fullscreen))
            .on_action(cx.listener(Self::on_hide_app))
            .on_action(cx.listener(Self::on_hide_others))
            .on_action(cx.listener(Self::on_show_all_apps))
            .on_action(cx.listener(Self::on_new_skill))
            .on_action(cx.listener(Self::on_focus_search))
            .on_action(cx.listener(Self::on_rescan))
            .on_action(cx.listener(Self::on_edit_skill))
            .on_action(cx.listener(Self::on_save_skill))
            .on_action(cx.listener(Self::on_submit_new_skill))
            .on_action(cx.listener(Self::on_cancel))
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| {
                this.cancel_mode(window, cx);
            }))
            .on_action(cx.listener(Self::on_open_in_editor))
            .on_action(cx.listener(Self::on_copy_path))
            .on_action(cx.listener(Self::on_reveal_in_finder))
            .on_action(cx.listener(Self::on_toggle_enabled))
            .on_action(cx.listener(Self::on_move_to_trash))
            .on_action(cx.listener(Self::on_add_folder))
            .child(self.render_title_bar(window, cx))
            .child(
                div().flex_1().min_h_0().w_full().child(
                    h_resizable("knack-split")
                        .with_state(&self.resizable_state)
                        .child(
                            resizable_panel()
                                .size(side_w)
                                .size_range(px(MIN_SIDE_WIDTH)..px(MAX_SIDE_WIDTH))
                                .child(render_sidebar(self, window, cx)),
                        )
                        .child(
                            resizable_panel()
                                .size(list_w)
                                .size_range(px(MIN_LIST_WIDTH)..px(MAX_LIST_WIDTH))
                                .child(render_skill_list(self, window, cx)),
                        )
                        .child(
                            resizable_panel()
                                .size_range(px(360.)..px(4000.))
                                .child(detail_pane),
                        ),
                ),
            )
    }
}
