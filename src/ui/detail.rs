//! Right pane: skill reading view (with Markdown & syntax-highlighted code
//! blocks), inline Markdown editor (with conflict detection), agent symlink
//! sharing pills, enable/disable toggle, supporting files, and empty state.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use gpui_kit::{
    Anchor, Context, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, TestSupportExt as _, Window,
    assets::IconName as LucideIcon,
    component::{
        ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
        alert::Alert,
        button::{Button, ButtonVariants as _},
        empty::{
            Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant,
            EmptyTitle,
        },
        h_flex,
        input::Editor,
        menu::{DropdownMenu as _, PopupMenuItem},
        scroll::ScrollableElement as _,
        separator::Separator,
        switch::Switch,
        tag::Tag,
        text::TextView,
        tooltip::Tooltip,
        v_flex,
    },
    div, prelude::FluentBuilder as _, px,
};

use crate::{
    actions::{
        Cancel, CopyPath, EditSkill, MoveToTrash, OpenInEditor, RevealInFinder, SaveSkill,
        ToggleEnabled,
    },
    library::{Kind, Skill, Upstream, tilde},
};

use super::KnackApp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailMode {
    Read,
    Edit,
    New,
}

pub fn render_detail(
    app: &KnackApp,
    _window: &mut Window,
    cx: &mut Context<KnackApp>,
) -> impl IntoElement {
    let Some(skill) = app.current_skill().cloned() else {
        return render_empty_detail(app, cx).into_any_element();
    };

    let theme = cx.theme();
    let muted_fg = theme.muted_foreground;
    let border_color = theme.border;
    let card_bg = theme.secondary;
    let radius_lg = theme.radius_lg;
    let mono_font = theme.mono_font_family.clone();
    let success_fg = theme.success;

    let is_edit = app.mode == DetailMode::Edit;
    let is_dirty = app.is_dirty(cx);
    let saving = app.saving;
    let conflict = app.conflict;
    let focus_handle = app.focus_handle.clone();

    if is_edit {
        let edit_actions = h_flex()
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .child(
                Button::new("cancel-edit")
                    .small()
                    .label("Cancel")
                    .tooltip_with_action("Cancel editing", &Cancel, None)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_mode(window, cx);
                    })),
            )
            .child(
                Button::new("save-edit")
                    .small()
                    .primary()
                    .label(if saving { "Saving…" } else { "Save" })
                    .disabled(!is_dirty || saving)
                    .loading(saving)
                    .tooltip_with_action("Save changes", &SaveSkill, None)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.save_current(false, window, cx);
                    })),
            );

        let edit_header = v_flex()
            .w_full()
            .max_w(px(900.))
            .px_8()
            .pt_6()
            .pb_3()
            .gap_2()
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .items_start()
                    .justify_between()
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .id("detail-title")
                                    .test_support()
                                    .text_xl()
                                    .font_semibold()
                                    .child(skill.title.clone()),
                            )
                            .child(
                                div()
                                    .font_family(mono_font.clone())
                                    .text_xs()
                                    .text_color(muted_fg)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(tilde(&skill.file, &app.home)),
                            ),
                    )
                    .child(edit_actions),
            )
            .when(conflict, |this| {
                this.child(
                    h_flex()
                        .id("conflict-banner")
                        .test_support()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_2()
                        .rounded(radius_lg)
                        .bg(cx.theme().warning.opacity(0.12))
                        .border_1()
                        .border_color(cx.theme().warning.opacity(0.4))
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .small()
                                .text_color(cx.theme().warning),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .child("This file changed on disk after you opened it."),
                        )
                        .child(
                            Button::new("conflict-reload")
                                .xsmall()
                                .outline()
                                .label("Reload")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.reload_from_disk(window, cx);
                                })),
                        )
                        .child(
                            Button::new("conflict-overwrite")
                                .xsmall()
                                .danger()
                                .label("Overwrite")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.save_current(true, window, cx);
                                })),
                        ),
                )
            });

        return v_flex()
            .size_full()
            .min_h_0()
            .bg(cx.theme().background)
            .child(edit_header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .max_w(px(900.))
                    .px_8()
                    .pb_6()
                    .child(Editor::new(&app.editor_input).h_full()),
            )
            .into_any_element();
    }

    let shown_path = tilde(&skill.file, &app.home);
    let file_for_toggle = skill.file.clone();
    let file_for_copy = skill.file.clone();
    let file_for_reveal = skill.file.clone();
    let file_for_open = skill.file.clone();
    let skill_id_for_toggle = skill.id.clone();
    let skill_kind = skill.kind;
    let is_disabled = skill.disabled;
    let places_len = skill.places.len();

    let header_actions = {
        let toggle_label = if is_disabled { "Enable" } else { "Disable" };
        let toggle_icon = if is_disabled {
            Icon::new(IconName::Check)
        } else {
            Icon::new(IconName::Ban)
        };
        let menu_focus = focus_handle.clone();
        h_flex()
            .flex_shrink_0()
            .items_center()
            .gap_1p5()
            .child(
                Button::new("edit-skill")
                    .small()
                    .outline()
                    .icon(Icon::new(LucideIcon::Pencil))
                    .label("Edit")
                    .tooltip_with_action("Edit", &EditSkill, None)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.enter_edit_mode(window, cx);
                    })),
            )
            .child(
                Button::new("skill-more")
                    .small()
                    .outline()
                    .icon(Icon::new(IconName::Ellipsis))
                    .accessibility_label("More actions")
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                        menu.action_context(menu_focus.clone())
                            .menu_with_icon("Copy path", Icon::new(IconName::Copy), Box::new(CopyPath))
                            .menu_with_icon(
                                "Reveal in Finder",
                                Icon::new(IconName::FolderOpen),
                                Box::new(RevealInFinder),
                            )
                            .menu_with_icon(
                                "Open in editor",
                                Icon::new(IconName::ExternalLink),
                                Box::new(OpenInEditor),
                            )
                            .item(
                                PopupMenuItem::new(toggle_label)
                                    .icon(toggle_icon.clone())
                                    .action(Box::new(ToggleEnabled)),
                            )
                            .separator()
                            .menu_with_icon(
                                "Move to Trash…",
                                Icon::new(LucideIcon::Trash),
                                Box::new(MoveToTrash),
                            )
                    }),
            )
    };

    // Enable / Disable card
    let status_card = h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .gap_3()
        .px_3()
        .py_2p5()
        .rounded(radius_lg)
        .bg(card_bg)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .child(if is_disabled { "Disabled" } else { "Enabled" }),
                )
                .when(!is_disabled && places_len > 1, |this| {
                    this.child(div().text_xs().text_color(muted_fg).child(format!(
                        "Affects all {places_len} agents that share it."
                    )))
                }),
        )
        .child(
            Switch::new("skill-enabled-switch")
                .checked(!is_disabled)
                .accessibility_label("Enabled")
                .on_change(cx.listener(move |this, enabled: &bool, window, cx| {
                    this.toggle_skill_enabled(
                        skill_id_for_toggle.clone(),
                        skill_kind,
                        file_for_toggle.clone(),
                        *enabled,
                        window,
                        cx,
                    );
                })),
        );

    // Agent sharing strip ("Available to") for skills (not slash commands)
    let agent_strip = if skill.kind == Kind::Skill {
        let sharing_root = match app.prefs.scope_id() {
            crate::library::ScopeId::Project(path) => {
                let proj_root = crate::library::RootId::Project(path);
                if skill.places.iter().any(|p| p.root == proj_root) {
                    proj_root
                } else {
                    skill.root.clone()
                }
            }
            crate::library::ScopeId::Agent(_) => {
                if skill.places.iter().any(|p| p.root.is_global()) {
                    crate::library::RootId::Global
                } else {
                    skill.root.clone()
                }
            }
            crate::library::ScopeId::All => skill.root.clone(),
        };
        let installed: Vec<_> = app
            .library
            .installed
            .iter()
            .filter(|i| i.root == sharing_root)
            .cloned()
            .collect();
        if installed.is_empty() {
            None
        } else {
            let pills = installed.into_iter().map(|inst| {
                let agent_id = inst.agent.id;
                let agent_label = inst.agent.label;
                let existing = skill.place_in(&inst.root, agent_id);
                let pill_id = ElementId::Name(format!("agent-pill:{agent_id}").into());

                match existing {
                    Some(place) if place.is_link => {
                        let entry = place.entry.clone();
                        Button::new(pill_id)
                            .xsmall()
                            .secondary()
                            .rounded(px(999.))
                            .icon(Icon::new(LucideIcon::Link).text_color(success_fg))
                            .label(agent_label)
                            .accessibility_label(format!("Stop sharing with {agent_label}"))
                            .tooltip("Shared by link. Click to stop sharing.")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.unlink_agent(entry.clone(), agent_label, window, cx);
                            }))
                            .into_any_element()
                    }
                    Some(_) => h_flex()
                        .id(pill_id)
                        .items_center()
                        .gap_1()
                        .px_2p5()
                        .h(px(24.))
                        .rounded(px(999.))
                        .bg(card_bg)
                        .text_xs()
                        .child(Icon::new(IconName::Check).xsmall().text_color(success_fg))
                        .child(agent_label)
                        .tooltip(|window, cx| Tooltip::new("The original lives here.").build(window, cx))
                        .into_any_element(),
                    None => {
                        let skill_dir = skill.id.clone();
                        let target_dir = inst.dir.clone();
                        let shown_dir = tilde(&target_dir, &app.home);
                        Button::new(pill_id)
                            .xsmall()
                            .outline()
                            .rounded(px(999.))
                            .icon(Icon::new(IconName::Plus))
                            .label(agent_label)
                            .accessibility_label(format!("Share with {agent_label}"))
                            .tooltip(format!("Share by linking into {shown_dir}"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.link_agent(
                                    skill_dir.clone(),
                                    target_dir.clone(),
                                    agent_label,
                                    window,
                                    cx,
                                );
                            }))
                            .into_any_element()
                    }
                }
            });

            Some(
                v_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .text_xs()
                            .font_medium()
                            .text_color(muted_fg)
                            .child("Available to"),
                    )
                    .child(h_flex().flex_wrap().gap_1p5().children(pills)),
            )
        }
    } else {
        None
    };

    // Location & supporting files card
    let location_card = v_flex()
        .w_full()
        .gap_2()
        .px_3()
        .py_2p5()
        .rounded(radius_lg)
        .bg(card_bg)
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .items_center()
                .gap_1p5()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .font_family(mono_font.clone())
                        .text_xs()
                        .child(shown_path),
                )
                .child(
                    Button::new("copy-path-btn")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::Copy))
                        .accessibility_label("Copy path")
                        .tooltip_with_action("Copy path", &CopyPath, None)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.copy_path(&file_for_copy, window, cx);
                        })),
                )
                .child(
                    Button::new("reveal-path-btn")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::FolderOpen))
                        .accessibility_label("Reveal in Finder")
                        .tooltip_with_action("Reveal in Finder", &RevealInFinder, None)
                        .on_click(cx.listener(move |_this, _, _, cx| {
                            cx.reveal_path(&file_for_reveal);
                        })),
                )
                .child(
                    Button::new("open-path-btn")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::ExternalLink))
                        .accessibility_label("Open in editor")
                        .tooltip_with_action("Open in editor", &OpenInEditor, None)
                        .on_click(cx.listener(move |_this, _, _, cx| {
                            cx.open_with_system(&file_for_open);
                        })),
                ),
        )
        .when(!skill.files.is_empty(), |this| {
            this.child(
                h_flex()
                    .flex_wrap()
                    .gap_1()
                    .children(skill.files.iter().map(|f| {
                        div()
                            .px_1p5()
                            .py_0p5()
                            .rounded(theme.radius)
                            .border_1()
                            .border_color(border_color)
                            .font_family(mono_font.clone())
                            .text_xs()
                            .text_color(muted_fg)
                            .child(f.clone())
                    })),
            )
        });

    let header = v_flex()
        .w_full()
        .max_w(px(860.))
        .px_8()
        .pt_7()
        .gap_3()
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .items_start()
                .justify_between()
                .gap_3()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .flex_wrap()
                                .child(
                                    div()
                                        .id("detail-title")
                                        .test_support()
                                        .text_2xl()
                                        .font_semibold()
                                        .child(skill.title.clone()),
                                )
                                .when(skill.kind == Kind::Command, |this| {
                                    this.child(Tag::secondary().small().child("/command"))
                                }),
                        )
                        .child(
                            div()
                                .font_family(mono_font.clone())
                                .text_xs()
                                .text_color(muted_fg)
                                .child(skill.name.clone()),
                        ),
                )
                .child(header_actions),
        )
        .when(!skill.description.is_empty(), |this| {
            this.child(
                div()
                    .text_sm()
                    .text_color(muted_fg)
                    .child(skill.description.clone()),
            )
        })
        .child(status_card)
        .when(skill.drift, |this| {
            this.child(Alert::warning(
                "drift-alert",
                format!(
                    "{} copies of this skill have different instructions.",
                    skill.copies
                ),
            ))
        })
        .when_some(source_alert(&skill), |this, alert| {
            this.child(div().id("source-alert").test_support().child(alert))
        })
        .when_some(agent_strip, |this, strip| this.child(strip))
        .child(location_card)
        .when(!skill.license.is_empty() || !skill.tools.is_empty() || skill.source.is_some(), |this| {
            this.child(
                h_flex()
                    .flex_wrap()
                    .items_center()
                    .gap_4()
                    .text_xs()
                    .text_color(muted_fg)
                    .when_some(skill.source.clone(), |row, source| {
                        let url = source.url();
                        row.child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .child("Source ·")
                                .child(
                                    Button::new("source-link")
                                        .link()
                                        .xsmall()
                                        .label(source.repo.clone())
                                        .tooltip(url.clone())
                                        .on_click(move |_, _, cx| cx.open_url(&url)),
                                ),
                        )
                    })
                    .when(!skill.license.is_empty(), |row| {
                        row.child(format!("License · {}", skill.license))
                    })
                    .when(!skill.tools.is_empty(), |row| {
                        row.child(format!("Allowed tools · {}", skill.tools))
                    }),
            )
        })
        .child(Separator::horizontal().mt_1());

    // Read mode: single scrollable canvas with metadata header + rendered Markdown.
    let md_id = markdown_element_id(&skill);
    let body_text = if skill.body.trim().is_empty() {
        "_No instructions written yet._".to_string()
    } else {
        skill.body.clone()
    };

    let scroll_id = ElementId::Name(format!("detail-read:{}", skill.id.display()).into());
    v_flex()
        .id(scroll_id)
        .size_full()
        .min_h_0()
        .overflow_y_scrollbar()
        .bg(cx.theme().background)
        .child(header)
        .child(
            div()
                .w_full()
                .max_w(px(860.))
                .px_8()
                .pt_4()
                .pb_10()
                .child(
                    TextView::markdown(md_id, SharedString::from(body_text))
                        .selectable(true)
                        .on_link_click(|href, _, _, cx| {
                            cx.open_url(href.as_ref());
                        }),
                ),
        )
        .into_any_element()
}

fn render_empty_detail(app: &KnackApp, cx: &mut Context<KnackApp>) -> impl IntoElement {
    let header = EmptyHeader::new()
        .media(
            EmptyMedia::new()
                .with_variant(EmptyMediaVariant::Icon)
                .child(Icon::new(IconName::BookOpen)),
        )
        .title(EmptyTitle::new().child("No skill selected"))
        .when(app.library.skills.is_empty(), |this| {
            this.description(
                EmptyDescription::new()
                    .child("Knack lists skills from Claude Code, Codex, and your projects here."),
            )
        });

    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .bg(cx.theme().background)
        .px_6()
        .child(
            Empty::new()
                .header(header)
                .content(
                    EmptyContent::new().child(
                        Button::new("detail-empty-new")
                            .primary()
                            .small()
                            .icon(Icon::new(IconName::Plus))
                            .label("New skill")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_new_skill(window, cx);
                            })),
                    ),
                ),
        )
}

/// How the skill compares with the GitHub repository it was installed from.
fn source_alert(skill: &Skill) -> Option<Alert> {
    let source = skill.source.as_ref()?;
    let repo = &source.repo;
    let update = source.update_command(&skill.folder);
    let (warning, title, message) = match skill.upstream? {
        Upstream::Current => return None,
        Upstream::Behind => (
            false,
            "Update available",
            format!("**{repo}** changed since you installed this skill. Run `{update}` to update."),
        ),
        Upstream::Edited => (
            false,
            "Edited locally",
            format!("This copy no longer matches what was installed from **{repo}**. `{update}` would replace your edits."),
        ),
        Upstream::Diverged => (
            true,
            "Update available",
            format!("**{repo}** changed, and so did this copy. `{update}` would replace your edits."),
        ),
        Upstream::Gone => (true, "Removed from source", format!("**{repo}** no longer has this skill.")),
    };
    let id = ElementId::Name(format!("source-alert:{}", skill.id.display()).into());
    let text = TextView::markdown(
        ElementId::Name(format!("source-alert-text:{}", skill.id.display()).into()),
        SharedString::from(message),
    );
    let alert = if warning { Alert::warning(id, text) } else { Alert::info(id, text) };
    Some(alert.title(title))
}

fn markdown_element_id(skill: &Skill) -> ElementId {
    let mut hasher = DefaultHasher::new();
    skill.id.hash(&mut hasher);
    skill.body.hash(&mut hasher);
    ElementId::Name(format!("skill-md:{:x}", hasher.finish()).into())
}
