//! Left sidebar: Library, Agents, Projects, and Watched Folders.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::{
    App, Context, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Styled as _, TestSupportExt as _, Window,
    assets::IconName as LucideIcon,
    component::{
        ActiveTheme as _, Icon, IconName, Sizable as _,
        button::{Button, ButtonVariants as _},
        h_flex,
        sidebar::{Sidebar, SidebarGroup, SidebarMenu, SidebarMenuItem},
    },
    div, px,
};

use crate::{
    assets::agent_icon,
    library::{ScopeId, tilde},
};

use super::KnackApp;

pub fn render_sidebar(app: &KnackApp, _window: &mut Window, cx: &mut Context<KnackApp>) -> impl IntoElement {
    let active_scope = app.prefs.scope_id();
    let total_count = app.library.skills.len();

    let agent_scopes: Vec<_> = app
        .library
        .scopes
        .iter()
        .filter(|s| matches!(s.id, ScopeId::Agent(_)))
        .cloned()
        .collect();

    let project_scopes: Vec<_> = app
        .library
        .scopes
        .iter()
        .filter(|s| matches!(s.id, ScopeId::Project(_)))
        .cloned()
        .collect();

    let folders: Vec<(String, String)> = app
        .prefs
        .folders
        .iter()
        .map(|raw| {
            let expanded = crate::library::expand(raw, &app.home);
            let shown = tilde(&expanded, &app.home);
            (raw.clone(), shown)
        })
        .collect();

    let mut sidebar = Sidebar::new("knack-sidebar")
        .collapsible(false)
        .w_full()
        .border_r_0()
        .child(
            SidebarGroup::new("Library").child(
                SidebarMenu::new().child(
                    SidebarMenuItem::new("All skills")
                        .icon(Icon::new(LucideIcon::Layers))
                        .py_1()
                        .label_style(truncate_label_style())
                        .active(active_scope == ScopeId::All)
                        .suffix(move |_, cx| count_badge(total_count, cx))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.select_scope(ScopeId::All, window, cx);
                        })),
                ),
            ),
        );

    if !agent_scopes.is_empty() {
        let items = agent_scopes.into_iter().map(|scope| {
            let id = scope.id.clone();
            let count = scope.count;
            let active = active_scope == id;
            let agent_id = match &id {
                ScopeId::Agent(agent_id) => *agent_id,
                _ => "agents",
            };
            SidebarMenuItem::new(SharedString::from(scope.label))
                .icon(agent_icon(agent_id))
                .py_1()
                .label_style(truncate_label_style())
                .active(active)
                .suffix(move |_, cx| count_badge(count, cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_scope(id.clone(), window, cx);
                }))
        });
        sidebar = sidebar.child(SidebarGroup::new("Agents").child(SidebarMenu::new().children(items)));
    }

    if !project_scopes.is_empty() {
        let items = project_scopes.into_iter().map(|scope| {
            let id = scope.id.clone();
            let count = scope.count;
            let active = active_scope == id;
            SidebarMenuItem::new(SharedString::from(scope.label))
                .icon(Icon::new(IconName::Folder))
                .py_1()
                .label_style(truncate_label_style())
                .active(active)
                .suffix(move |_, cx| count_badge(count, cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_scope(id.clone(), window, cx);
                }))
        });
        sidebar = sidebar.child(SidebarGroup::new("Projects").child(SidebarMenu::new().children(items)));
    }

    // Watched folders group with inline remove buttons and "Add folder…" trigger.
    let mut folder_menu = SidebarMenu::new();
    for (raw, shown) in folders {
        let remove_raw = raw.clone();
        let remove_label = SharedString::from(format!("Stop watching {shown}"));
        let remove_id = ElementId::Name(format!("unwatch:{raw}").into());
        let open_path = PathBuf::from(crate::library::expand(&raw, &app.home));
        let on_remove = Rc::new(cx.listener(move |this, _, window, cx| {
            this.remove_folder(&remove_raw, window, cx);
        }));
        folder_menu = folder_menu.child(
            SidebarMenuItem::new(SharedString::from(shown))
                .icon(Icon::new(IconName::FolderClosed))
                .py_1()
                .label_style(truncate_label_style())
                .on_click(cx.listener(move |_this, _, _, cx| {
                    cx.reveal_path(&open_path);
                }))
                .suffix(move |_, _| {
                    Button::new(remove_id.clone())
                        .flex_shrink_0()
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(LucideIcon::X))
                        .accessibility_label(remove_label.clone())
                        .tooltip(remove_label.clone())
                        .on_click({
                            let on_remove = on_remove.clone();
                            move |ev, window, cx| {
                                cx.stop_propagation();
                                on_remove(ev, window, cx);
                            }
                        })
                }),
        );
    }

    folder_menu = folder_menu.child(
        SidebarMenuItem::new("Add folder…")
            .icon(Icon::new(LucideIcon::FolderPlus))
            .py_1()
            .label_style(truncate_label_style())
            .on_click(cx.listener(|this, _, window, cx| {
                this.prompt_add_folder(window, cx);
            })),
    );

    sidebar = sidebar.child(SidebarGroup::new("Watched folders").child(folder_menu));

    div()
        .id("sidebar-pane")
        .test_support()
        .size_full()
        .overflow_hidden()
        .child(sidebar)
}

fn truncate_label_style() -> gpui_kit::StyleRefinement {
    let mut el = div()
        .block()
        .min_w_0()
        .overflow_x_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .line_height(px(20.));
    el.style().clone()
}

fn count_badge(count: usize, cx: &App) -> gpui_kit::Div {
    h_flex()
        .flex_shrink_0()
        .font_family(cx.theme().mono_font_family.clone())
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .pl_1()
        .min_w(px(18.))
        .justify_end()
        .child(count.to_string())
}
