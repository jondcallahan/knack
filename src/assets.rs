//! Application icon and image assets: default GPUI Kit component icons,
//! extra Lucide icons, official provider SVG logos, and the app icon.

use std::borrow::Cow;

use gpui_kit::{
    AssetSource, Result, SharedString,
    assets::{Assets, icon_assets},
    component::{Icon, IconName},
};

icon_assets!(
    ExtraIcons,
    [
        FileCode,
        FolderPlus,
        Layers,
        Link,
        Pencil,
        Slash,
        Sparkles,
        SquarePen,
        Terminal,
        Trash,
        Unlink,
        X,
    ]
);

const EMBEDDED_ASSETS: &[(&str, &[u8])] = &[
    (
        "icons/providers/claude.svg",
        include_bytes!("../assets/providers/claude.svg"),
    ),
    (
        "icons/providers/codex.svg",
        include_bytes!("../assets/providers/codex.svg"),
    ),
    (
        "icons/providers/cursor.svg",
        include_bytes!("../assets/providers/cursor.svg"),
    ),
    (
        "icons/providers/gemini.svg",
        include_bytes!("../assets/providers/gemini.svg"),
    ),
    (
        "icons/providers/opencode.svg",
        include_bytes!("../assets/providers/opencode.svg"),
    ),
    ("icons/app-icon.png", include_bytes!("../assets/icon.png")),
];

pub fn agent_icon(agent_id: &str) -> Icon {
    match agent_id {
        "claude" => Icon::empty().path("icons/providers/claude.svg"),
        "codex" => Icon::empty().path("icons/providers/codex.svg"),
        "cursor" => Icon::empty().path("icons/providers/cursor.svg"),
        "gemini" => Icon::empty().path("icons/providers/gemini.svg"),
        "opencode" => Icon::empty().path("icons/providers/opencode.svg"),
        _ => Icon::new(IconName::Bot),
    }
}

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = EMBEDDED_ASSETS.iter().find(|(p, _)| *p == path) {
            return Ok(Some(Cow::Borrowed(*bytes)));
        }
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.extend(
            EMBEDDED_ASSETS
                .iter()
                .map(|(p, _)| *p)
                .filter(|p| p.starts_with(path))
                .map(SharedString::from),
        );
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_embedded_provider_logos_and_app_icon() {
        for path in [
            "icons/providers/claude.svg",
            "icons/providers/codex.svg",
            "icons/providers/cursor.svg",
            "icons/providers/gemini.svg",
            "icons/providers/opencode.svg",
            "icons/app-icon.png",
        ] {
            let bytes = AppAssets
                .load(path)
                .expect("asset lookup succeeds")
                .expect("embedded asset exists");
            assert!(!bytes.is_empty(), "{path} should not be empty");
        }
    }
}
