//! New skill creation form: paste a SKILL.md (frontmatter is parsed live and
//! extra keys are preserved) or author one from scratch, and choose one or more
//! destination folders.

use std::path::{Path, PathBuf};

use gpui_kit::{
    App, AppContext as _, Context, ElementId, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Styled as _, Subscription, TestSupportExt as _, Window,
    component::{
        ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, StyledExt as _,
        alert::Alert,
        button::{Button, ButtonVariants as _},
        combobox::{Combobox, ComboboxEvent, ComboboxState},
        h_flex,
        input::{Editor, EditorState, Input, InputEvent, InputState},
        scroll::ScrollableElement as _,
        searchable_list::{SearchableListItem, SearchableVec},
        v_flex,
    },
    div, prelude::FluentBuilder as _, px,
};

use crate::{
    assets::agent_icon,
    library::{Frontmatter, Installed, RootId, compose, expand, parse_frontmatter, tilde},
};

use super::KnackApp;

const DEFAULT_NEW_DIR: &str = "~/.agents/skills";
const KNOWN_KEYS: [&str; 4] = ["name", "description", "license", "allowed-tools"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirOption {
    pub short_title: SharedString,
    pub path: String,
    pub agent_id: &'static str,
}

impl SearchableListItem for DirOption {
    type Value = String;

    fn title(&self) -> SharedString {
        self.short_title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.path
    }

    fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty()
            || self.short_title.to_lowercase().contains(&q)
            || self.path.to_lowercase().contains(&q)
    }

    fn render(&self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let row_id = ElementId::Name(format!("dir-option:{}", self.path).into());
        h_flex()
            .id(row_id)
            .test_support()
            .w_full()
            .min_w_0()
            .items_center()
            .gap_2()
            .child(agent_icon(self.agent_id).xsmall())
            .child(div().flex_shrink_0().child(self.short_title.clone()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.path.clone()),
            )
    }
}

pub struct NewSkillForm {
    pub instructions: Entity<EditorState>,
    pub name: Entity<InputState>,
    pub description: Entity<InputState>,
    pub license: Entity<InputState>,
    pub tools: Entity<InputState>,
    pub dir_select: Entity<ComboboxState<SearchableVec<DirOption>>>,
    pub parsed: Frontmatter,
    pub creating: bool,
    pub error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl NewSkillForm {
    pub fn new(
        installed: &[Installed],
        default_dirs: &[String],
        home: &Path,
        window: &mut Window,
        cx: &mut Context<KnackApp>,
    ) -> Self {
        let instructions = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("markdown")
                .soft_wrap(true)
                .line_number(false)
                .placeholder("Paste a SKILL.md or write instructions in Markdown")
        });
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("my-skill"));
        let description = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("What it does, and when an agent should use it")
        });
        let license = cx.new(|cx| InputState::new(window, cx).placeholder("Optional, e.g. MIT"));
        let tools = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Optional, e.g. Bash, Read, Edit")
        });

        let (options, selected_indices) = build_dir_options(installed, default_dirs, home);
        let dir_select = cx.new(|cx| {
            ComboboxState::new(SearchableVec::new(options), selected_indices, window, cx)
                .multiple(true)
                .searchable(true)
        });

        let mut subscriptions = Vec::new();

        subscriptions.push(cx.subscribe_in(
            &instructions,
            window,
            |this, state, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let raw = state.read(cx).value().to_string();
                    this.new_skill.on_instructions_changed(&raw, window, cx);
                    cx.notify();
                }
            },
        ));

        for field in [&name, &description, &license, &tools] {
            subscriptions.push(cx.subscribe_in(
                field,
                window,
                |this, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.new_skill.error = None;
                        cx.notify();
                    }
                },
            ));
        }

        subscriptions.push(cx.subscribe_in(
            &dir_select,
            window,
            |this, _, event: &ComboboxEvent<SearchableVec<DirOption>>, _, cx| match event {
                ComboboxEvent::Change(dirs) | ComboboxEvent::Confirm(dirs) => {
                    this.prefs.new_dirs = dirs.clone();
                    this.save_prefs();
                    cx.notify();
                }
            },
        ));

        Self {
            instructions,
            name,
            description,
            license,
            tools,
            dir_select,
            parsed: Frontmatter::default(),
            creating: false,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn reset(&mut self, window: &mut Window, cx: &mut App) {
        self.parsed = Frontmatter::default();
        self.creating = false;
        self.error = None;
        self.instructions.update(cx, |s, cx| s.set_value("", window, cx));
        self.name.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.set_placeholder("my-skill", window, cx);
        });
        self.description.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.set_placeholder("What it does, and when an agent should use it", window, cx);
        });
        self.license.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.set_placeholder("Optional, e.g. MIT", window, cx);
        });
        self.tools.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.set_placeholder("Optional, e.g. Bash, Read, Edit", window, cx);
        });
        self.instructions.update(cx, |s, cx| s.focus(window, cx));
    }

    pub fn sync_destinations(
        &mut self,
        installed: &[Installed],
        preferred: &[String],
        home: &Path,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (options, selected_indices) = build_dir_options(installed, preferred, home);
        self.dir_select.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(options), window, cx);
            state.set_selected_indices(selected_indices, window, cx);
        });
    }

    fn on_instructions_changed(&mut self, text: &str, window: &mut Window, cx: &mut App) {
        self.error = None;
        self.parsed = parse_frontmatter(text);

        let name_ph = self
            .parsed
            .get("name")
            .filter(|v| !v.is_empty())
            .unwrap_or("my-skill")
            .to_string();
        let desc_ph = self
            .parsed
            .get("description")
            .filter(|v| !v.is_empty())
            .unwrap_or("What it does, and when an agent should use it")
            .to_string();
        let lic_ph = self
            .parsed
            .get("license")
            .filter(|v| !v.is_empty())
            .unwrap_or("Optional, e.g. MIT")
            .to_string();
        let tools_ph = self
            .parsed
            .get("allowed-tools")
            .filter(|v| !v.is_empty())
            .unwrap_or("Optional, e.g. Bash, Read, Edit")
            .to_string();

        self.name.update(cx, |s, cx| s.set_placeholder(name_ph, window, cx));
        self.description
            .update(cx, |s, cx| s.set_placeholder(desc_ph, window, cx));
        self.license
            .update(cx, |s, cx| s.set_placeholder(lic_ph, window, cx));
        self.tools
            .update(cx, |s, cx| s.set_placeholder(tools_ph, window, cx));
    }

    pub fn final_name(&self, cx: &App) -> String {
        let typed = self.name.read(cx).value().trim().to_string();
        if !typed.is_empty() {
            typed
        } else {
            self.parsed.get("name").unwrap_or_default().trim().to_string()
        }
    }

    pub fn final_description(&self, cx: &App) -> String {
        let typed = self.description.read(cx).value().trim().to_string();
        if !typed.is_empty() {
            typed
        } else {
            self.parsed
                .get("description")
                .unwrap_or_default()
                .trim()
                .to_string()
        }
    }

    pub fn can_create(&self, cx: &App) -> bool {
        !self.creating
            && !self.final_name(cx).is_empty()
            && !self.final_description(cx).is_empty()
            && !self.dir_select.read(cx).selected_values().is_empty()
    }

    pub fn kept_keys(&self) -> Vec<&str> {
        self.parsed
            .keys
            .iter()
            .map(|(k, _)| k.as_str())
            .filter(|k| !KNOWN_KEYS.contains(k))
            .collect()
    }

    pub fn selected_dirs(&self, home: &Path, cx: &App) -> Vec<PathBuf> {
        self.dir_select
            .read(cx)
            .selected_values()
            .into_iter()
            .map(|raw| expand(&raw, home))
            .collect()
    }

    pub fn compose_skill_md(&self, cx: &App) -> (String, String) {
        let raw_text = self.instructions.read(cx).value().to_string();
        let parsed = parse_frontmatter(&raw_text);
        let name = self.final_name(cx);
        let description = self.final_description(cx);
        let license = {
            let v = self.license.read(cx).value().trim().to_string();
            if !v.is_empty() {
                v
            } else {
                parsed.get("license").unwrap_or_default().trim().to_string()
            }
        };
        let tools = {
            let v = self.tools.read(cx).value().trim().to_string();
            if !v.is_empty() {
                v
            } else {
                parsed
                    .get("allowed-tools")
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            }
        };

        let mut keys = vec![
            ("name".to_string(), name.clone()),
            ("description".to_string(), description),
        ];
        if !license.is_empty() {
            keys.push(("license".to_string(), license));
        }
        if !tools.is_empty() {
            keys.push(("allowed-tools".to_string(), tools));
        }
        for (k, v) in &parsed.keys {
            if !KNOWN_KEYS.contains(&k.as_str()) {
                keys.push((k.clone(), v.clone()));
            }
        }

        let md = compose(&keys, &parsed.body);
        (name, md)
    }
}

fn build_dir_options(
    installed: &[Installed],
    preferred: &[String],
    home: &Path,
) -> (Vec<DirOption>, Vec<IndexPath>) {
    let mut options = vec![DirOption {
        short_title: ".agents".into(),
        path: DEFAULT_NEW_DIR.into(),
        agent_id: "agents",
    }];

    for item in installed {
        let shown = tilde(&item.dir, home);
        if shown == DEFAULT_NEW_DIR || options.iter().any(|o| o.path == shown) {
            continue;
        }
        let short_title = match &item.root {
            RootId::Global => SharedString::from(item.agent.label),
            RootId::Project(root_path) => {
                let parent = root_path
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                let name = root_path
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                SharedString::from(format!("{parent}/{name} · {}", item.agent.label))
            }
        };
        options.push(DirOption {
            short_title,
            path: shown,
            agent_id: item.agent.id,
        });
    }

    let mut selected_indices: Vec<IndexPath> = preferred
        .iter()
        .filter_map(|pref| options.iter().position(|o| &o.path == pref))
        .map(IndexPath::new)
        .collect();
    if selected_indices.is_empty() && !preferred.is_empty() {
        selected_indices.push(IndexPath::new(0));
    }

    (options, selected_indices)
}

pub fn render_new_skill(
    app: &KnackApp,
    _window: &mut Window,
    cx: &mut Context<KnackApp>,
) -> impl IntoElement {
    let form = &app.new_skill;
    let can_create = form.can_create(cx);
    let creating = form.creating;
    let kept = form.kept_keys();
    let kept_note = if kept.is_empty() {
        None
    } else {
        Some(format!(
            "Also keeps {} from the frontmatter.",
            kept.join(", ")
        ))
    };
    let error = form.error.clone();
    let muted_fg = cx.theme().muted_foreground;

    let field_label = |text: &'static str| {
        div()
            .text_xs()
            .font_medium()
            .text_color(muted_fg)
            .child(text)
    };

    v_flex()
        .id("new-skill-scroll")
        .size_full()
        .min_h_0()
        .overflow_y_scrollbar()
        .bg(cx.theme().background)
        .child(
            v_flex()
                .w_full()
                .max_w(px(760.))
                .px_8()
                .pt_7()
                .pb_10()
                .gap_4()
                .child(div().text_2xl().font_semibold().child("New skill"))
                .child(
                    v_flex()
                        .gap_1p5()
                        .child(field_label("Instructions"))
                        .child(Editor::new(&form.instructions).h(px(240.)))
                        .when_some(kept_note, |this, note| {
                            this.child(div().text_xs().text_color(muted_fg).child(note))
                        }),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap_3()
                        .items_start()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1p5()
                                .child(field_label("Name"))
                                .child(Input::new(&form.name).id("new-name-input")),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1p5()
                                .child(field_label("Save to"))
                                .child(
                                    div().id("new-dir-select").test_support().w_full().child(
                                        Combobox::new(&form.dir_select)
                                            .placeholder("Choose folders")
                                            .search_placeholder("Filter destinations…")
                                            .w_full(),
                                    ),
                                ),
                        ),
                )
                .child(
                    v_flex()
                        .gap_1p5()
                        .child(field_label("Description"))
                        .child(Input::new(&form.description).id("new-description-input")),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap_3()
                        .items_start()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1p5()
                                .child(field_label("License"))
                                .child(Input::new(&form.license)),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1p5()
                                .child(field_label("Allowed tools"))
                                .child(Input::new(&form.tools)),
                        ),
                )
                .when_some(error, |this, err| {
                    this.child(Alert::error("new-skill-error", err))
                })
                .child(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .pt_2()
                        .child(
                            Button::new("new-skill-cancel")
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_mode(window, cx);
                                })),
                        )
                        .child(
                            Button::new("new-skill-create")
                                .primary()
                                .label(if creating {
                                    "Creating…"
                                } else {
                                    "Create skill"
                                })
                                .disabled(!can_create)
                                .loading(creating)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.submit_new_skill(window, cx);
                                })),
                        ),
                ),
        )
}
