//! Middle pane: scope header, status filter (All / Enabled / Disabled),
//! and virtualized keyboard-navigable skill list.

use std::path::{Path, PathBuf};

use gpui_kit::{
    App, Context, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Styled as _, TestSupportExt as _, Window,
    assets::IconName as LucideIcon,
    component::{
        ActiveTheme as _, Icon, IconName, IndexPath, Sizable as _, StyledExt as _,
        button::{Button, ButtonVariants as _},
        empty::{Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
        h_flex,
        list::{List, ListDelegate, ListItem, ListState},
        tab::{Tab, TabBar},
        tag::Tag,
        v_flex,
    },
    div, prelude::FluentBuilder as _, px,
};

use crate::{
    library::{Kind, Skill},
    prefs::StatusFilter,
};

use super::KnackApp;

pub struct SkillListDelegate {
    items: Vec<Skill>,
    selected_index: Option<IndexPath>,
}

impl SkillListDelegate {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected_index: None,
        }
    }

    pub fn set_items(&mut self, items: Vec<Skill>, selected_id: Option<&Path>) -> Option<IndexPath> {
        self.items = items;
        self.selected_index = selected_id
            .and_then(|id| self.items.iter().position(|s| s.id == id))
            .map(IndexPath::new);
        self.selected_index
    }

    pub fn skill_at(&self, ix: IndexPath) -> Option<&Skill> {
        self.items.get(ix.row)
    }

    pub fn index_of(&self, id: &Path) -> Option<IndexPath> {
        self.items
            .iter()
            .position(|s| s.id == id)
            .map(IndexPath::new)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

impl ListDelegate for SkillListDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.items.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let skill = self.items.get(ix.row)?;
        let selected = self.selected_index == Some(ix);
        let id = ElementId::Name(format!("skill-row:{}", skill.id.display()).into());
        let theme = cx.theme();
        let radius_lg = theme.radius_lg;
        let muted_fg = theme.muted_foreground;
        let warning_fg = theme.warning;

        // Every row in GPUI Kit's List must share a uniform fixed height so
        // virtualization measures accurately.
        let mut chips: Vec<SharedString> = Vec::new();
        if skill.kind == Kind::Command {
            chips.push("/command".into());
        }
        for place in &skill.places {
            if !place.root.is_global() {
                let label: SharedString = place.root_label.clone().into();
                if !chips.contains(&label) {
                    chips.push(label);
                }
            }
        }
        for place in &skill.places {
            let label: SharedString = place.agent.label.into();
            if !chips.contains(&label) {
                chips.push(label);
            }
        }

        let desc = if skill.description.is_empty() {
            skill.name.clone()
        } else {
            skill.description.clone()
        };

        let desc_id = ElementId::Name(format!("skill-desc:{}", skill.id.display()).into());
        let chips_id = ElementId::Name(format!("skill-chips:{}", skill.id.display()).into());

        let row = v_flex()
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .gap_1p5()
            .when(skill.disabled, |this| this.opacity(0.55))
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .h(px(20.))
                    .flex_shrink_0()
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_sm()
                            .line_height(px(20.))
                            .font_semibold()
                            .child(skill.title.clone()),
                    )
                    .when(skill.disabled, |this| {
                        this.child(
                            Tag::secondary()
                                .outline()
                                .xsmall()
                                .flex_shrink_0()
                                .child("Disabled"),
                        )
                    })
                    .when(skill.drift, |this| {
                        this.child(
                            Icon::new(IconName::TriangleAlert)
                                .xsmall()
                                .text_color(warning_fg),
                        )
                    }),
            )
            .child(
                div()
                    .id(desc_id)
                    .test_support()
                    .w_full()
                    .min_w_0()
                    .h(px(36.))
                    .flex_shrink_0()
                    .text_xs()
                    .line_height(px(18.))
                    .text_color(muted_fg)
                    .line_clamp(2)
                    .text_ellipsis()
                    .child(desc),
            )
            .child(
                h_flex()
                    .id(chips_id)
                    .test_support()
                    .w_full()
                    .min_w_0()
                    .h(px(20.))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .items_center()
                    .gap_1()
                    .children(chips.into_iter().take(4).map(|chip| {
                        Tag::secondary()
                            .xsmall()
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .child(chip)
                    })),
            );

        Some(
            ListItem::new(id)
                .w_full()
                .px_3()
                .py_2p5()
                .my_0p5()
                .rounded(radius_lg)
                .selected(selected)
                .accessibility_label(SharedString::from(skill.title.clone()))
                .child(row),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected_index = ix;
        cx.notify();
    }
}

pub fn render_skill_list(
    app: &KnackApp,
    _window: &mut Window,
    cx: &mut Context<KnackApp>,
) -> impl IntoElement {
    let (scoped_total, enabled_count, disabled_count) = app.scope_counts();
    let shown_count = app.skill_list.read(cx).delegate().len();
    let query = app.search_query(cx);
    let query_trimmed = query.trim().to_string();
    let status = app.prefs.status;

    let border_color = cx.theme().border;
    let muted_fg = cx.theme().muted_foreground;
    let mono_font = cx.theme().mono_font_family.clone();

    let status_tabs = TabBar::new("status-filter")
        .segmented()
        .small()
        .w_full()
        .selected_index(status.index())
        .on_click(cx.listener(|this, ix: &usize, window, cx| {
            this.set_status_filter(StatusFilter::from_index(*ix), window, cx);
        }))
        .child(Tab::new().label("All").flex_1())
        .child(
            Tab::new()
                .label(format!("Enabled {enabled_count}"))
                .flex_1(),
        )
        .child(
            Tab::new()
                .label(format!("Disabled {disabled_count}"))
                .flex_1(),
        );

    let body = if shown_count == 0 && !app.scanning {
        if !query_trimmed.is_empty() {
            let title = format!("No results for \"{query_trimmed}\"");
            v_flex()
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .px_4()
                .child(
                    Empty::new()
                        .header(
                            EmptyHeader::new()
                                .media(
                                    EmptyMedia::new()
                                        .with_variant(EmptyMediaVariant::Icon)
                                        .child(Icon::new(IconName::Search)),
                                )
                                .title(EmptyTitle::new().child(title)),
                        )
                        .content(
                            EmptyContent::new().child(
                                Button::new("clear-search")
                                    .small()
                                    .label("Clear search")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.clear_search(window, cx);
                                    })),
                            ),
                        ),
                )
                .into_any_element()
        } else if status != StatusFilter::All && scoped_total > 0 {
            let label = match status {
                StatusFilter::Enabled => "No enabled skills here",
                StatusFilter::Disabled => "No disabled skills here",
                StatusFilter::All => "No skills here",
            };
            v_flex()
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .px_4()
                .child(
                    Empty::new()
                        .header(
                            EmptyHeader::new()
                                .media(
                                    EmptyMedia::new()
                                        .with_variant(EmptyMediaVariant::Icon)
                                        .child(Icon::new(LucideIcon::Sparkles)),
                                )
                                .title(EmptyTitle::new().child(label)),
                        )
                        .content(
                            EmptyContent::new().child(
                                Button::new("show-all-status")
                                    .small()
                                    .label("Show all")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.set_status_filter(StatusFilter::All, window, cx);
                                    })),
                            ),
                        ),
                )
                .into_any_element()
        } else {
            v_flex()
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .px_4()
                .child(
                    Empty::new()
                        .header(
                            EmptyHeader::new()
                                .media(
                                    EmptyMedia::new()
                                        .with_variant(EmptyMediaVariant::Icon)
                                        .child(Icon::new(LucideIcon::Sparkles)),
                                )
                                .title(EmptyTitle::new().child("No skills here"))
                                .description(EmptyDescription::new().child(
                                    "Create one, or add a folder that holds your projects.",
                                )),
                        )
                        .content(
                            EmptyContent::new().child(
                                Button::new("empty-new-skill")
                                    .primary()
                                    .small()
                                    .label("New skill")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.start_new_skill(window, cx);
                                    })),
                            ),
                        ),
                )
                .into_any_element()
        }
    } else {
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(List::new(&app.skill_list).px_2().py_1p5())
            .into_any_element()
    };

    v_flex()
        .size_full()
        .min_w_0()
        .bg(cx.theme().background)
        .child(
            v_flex()
                .px_3p5()
                .pt_2p5()
                .pb_2p5()
                .gap_1p5()
                .border_b_1()
                .border_color(border_color)
                .child(status_tabs)
                .when(!query_trimmed.is_empty(), |this| {
                    this.child(
                        div()
                            .font_family(mono_font.clone())
                            .text_xs()
                            .text_color(muted_fg)
                            .child(format!("{shown_count} of {scoped_total}")),
                    )
                }),
        )
        .child(body)
}

pub fn selected_skill_id(
    list: &gpui_kit::Entity<ListState<SkillListDelegate>>,
    ix: IndexPath,
    cx: &App,
) -> Option<PathBuf> {
    list.read(cx).delegate().skill_at(ix).map(|s| s.id.clone())
}
