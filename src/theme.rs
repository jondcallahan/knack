//! One Light and One Dark syntax-highlighting palettes wired onto GPUI Kit's
//! active light and dark `ThemeConfig` entries so code blocks in Markdown
//! previews and the inline Markdown editor follow macOS appearance.

use std::rc::Rc;

use gpui_kit::{
    App,
    component::{
        highlighter::{
            FontStyle, FontWeightContent, HighlightThemeStyle, SyntaxColors, ThemeStyle,
        },
        theme::Theme,
    },
    rgb, rgba,
};

struct Palette {
    editor_fg: u32,
    editor_bg: u32,
    active_line: u32,
    line_number: u32,
    active_line_number: u32,
    invisible: u32,
    comment: u32,
    keyword: u32,
    function: u32,
    string: u32,
    cyan: u32,
    constant: u32,
    type_name: u32,
    tag_or_prop: u32,
    punctuation: u32,
}

const ONE_LIGHT: Palette = Palette {
    editor_fg: 0x383A42,
    editor_bg: 0xFFFFFF,
    active_line: 0xF5F5F5,
    line_number: 0x9D9D9F,
    active_line_number: 0x383A42,
    invisible: 0xA0A1A766,
    comment: 0x7F848E,
    keyword: 0xA626A4,
    function: 0x4078F2,
    string: 0x50A14F,
    cyan: 0x0184BC,
    constant: 0x986801,
    type_name: 0xC18401,
    tag_or_prop: 0xE45649,
    punctuation: 0x525560,
};

const ONE_DARK: Palette = Palette {
    editor_fg: 0xABB2BF,
    editor_bg: 0x0A0A0A,
    active_line: 0x171717,
    line_number: 0x5C6370,
    active_line_number: 0xABB2BF,
    invisible: 0x5C637066,
    comment: 0x7F848E,
    keyword: 0xC678DD,
    function: 0x61AFEF,
    string: 0x98C379,
    cyan: 0x56B6C2,
    constant: 0xD19A66,
    type_name: 0xE5C07B,
    tag_or_prop: 0xE06C75,
    punctuation: 0x8B92A0,
};

fn build_theme_style(
    color: Option<u32>,
    font_style: Option<FontStyle>,
    font_weight: Option<FontWeightContent>,
) -> ThemeStyle {
    // `ThemeStyle` is `#[non_exhaustive]` without `Default` in `gpui-component` 0.11.0.
    let mut style: ThemeStyle =
        serde_json::from_str("{}").expect("empty ThemeStyle JSON deserializes");
    style.color = color.map(|hex| rgb(hex).into());
    style.font_style = font_style;
    style.font_weight = font_weight;
    style
}

fn color_style(hex: u32) -> ThemeStyle {
    build_theme_style(Some(hex), None, None)
}

fn italic_color(hex: u32) -> ThemeStyle {
    build_theme_style(Some(hex), Some(FontStyle::Italic), None)
}

fn semibold_color(hex: u32) -> ThemeStyle {
    build_theme_style(Some(hex), None, Some(FontWeightContent::Semibold))
}

fn font_style_only(style: FontStyle) -> ThemeStyle {
    build_theme_style(None, Some(style), None)
}

fn font_weight_only(weight: FontWeightContent) -> ThemeStyle {
    build_theme_style(None, None, Some(weight))
}

fn build_highlight_theme(p: &Palette) -> HighlightThemeStyle {
    let mut syntax = SyntaxColors::default();
    syntax.comment = Some(italic_color(p.comment));
    syntax.comment_doc = Some(italic_color(p.comment));

    syntax.keyword = Some(color_style(p.keyword));
    syntax.preproc = Some(color_style(p.keyword));

    syntax.function = Some(color_style(p.function));
    syntax.label = Some(color_style(p.function));
    syntax.link_text = Some(color_style(p.function));

    syntax.string = Some(color_style(p.string));
    syntax.text_literal = Some(color_style(p.string));
    syntax.text_code_span = Some(color_style(p.string));

    syntax.string_escape = Some(color_style(p.cyan));
    syntax.string_regex = Some(color_style(p.cyan));
    syntax.string_special = Some(color_style(p.cyan));
    syntax.string_special_symbol = Some(color_style(p.cyan));
    syntax.operator = Some(color_style(p.cyan));
    syntax.link_uri = Some(italic_color(p.cyan));

    syntax.number = Some(color_style(p.constant));
    syntax.boolean = Some(color_style(p.constant));
    syntax.constant = Some(color_style(p.constant));
    syntax.attribute = Some(color_style(p.constant));

    syntax.type_ = Some(color_style(p.type_name));
    syntax.constructor = Some(color_style(p.type_name));
    syntax.enum_ = Some(color_style(p.type_name));
    syntax.variant = Some(color_style(p.type_name));

    syntax.tag = Some(color_style(p.tag_or_prop));
    syntax.tag_doctype = Some(color_style(p.tag_or_prop));
    syntax.property = Some(color_style(p.tag_or_prop));
    syntax.variable_special = Some(color_style(p.tag_or_prop));
    syntax.punctuation_list_marker = Some(color_style(p.tag_or_prop));
    syntax.punctuation_special = Some(color_style(p.tag_or_prop));
    syntax.title = Some(semibold_color(p.tag_or_prop));

    syntax.variable = Some(color_style(p.editor_fg));
    syntax.embedded = Some(color_style(p.editor_fg));

    syntax.punctuation = Some(color_style(p.punctuation));
    syntax.punctuation_bracket = Some(color_style(p.punctuation));
    syntax.punctuation_delimiter = Some(color_style(p.punctuation));

    syntax.emphasis = Some(font_style_only(FontStyle::Italic));
    syntax.emphasis_strong = Some(font_weight_only(FontWeightContent::Bold));

    let mut style = HighlightThemeStyle::default();
    style.editor_foreground = Some(rgb(p.editor_fg).into());
    style.editor_background = Some(rgb(p.editor_bg).into());
    style.editor_active_line = Some(rgb(p.active_line).into());
    style.editor_line_number = Some(rgb(p.line_number).into());
    style.editor_active_line_number = Some(rgb(p.active_line_number).into());
    style.editor_invisible = Some(rgba(p.invisible).into());
    style.syntax = syntax;
    style
}

/// Returns the Atom / Zed **One Light** syntax highlighting theme.
pub fn one_light_highlight() -> HighlightThemeStyle {
    build_highlight_theme(&ONE_LIGHT)
}

/// Returns the Atom / Zed **One Dark** syntax highlighting theme.
pub fn one_dark_highlight() -> HighlightThemeStyle {
    build_highlight_theme(&ONE_DARK)
}

/// Installs One Light and One Dark syntax highlighting into the registered
/// light and dark `ThemeConfig`s and reloads the active highlight theme.
pub fn install(cx: &mut App) {
    let mut light = (*Theme::global(cx).light_theme).clone();
    light.highlight = Some(one_light_highlight());

    let mut dark = (*Theme::global(cx).dark_theme).clone();
    dark.highlight = Some(one_dark_highlight());

    let mode = Theme::global(cx).mode;
    Theme::update(cx, |theme| {
        theme.light_theme = Rc::new(light);
        theme.dark_theme = Rc::new(dark);
    });
    Theme::change(mode, None, cx);
}

#[cfg(test)]
mod tests {
    use gpui_kit::{FontStyle, rgb};

    use super::{one_dark_highlight, one_light_highlight};

    #[test]
    fn builds_distinct_one_light_and_one_dark_tokens() {
        let light = one_light_highlight();
        let dark = one_dark_highlight();

        for (style, kw, str_hex, fn_hex, typ, prop, num, op) in [
            (
                &light, 0xA626A4, 0x50A14F, 0x4078F2, 0xC18401, 0xE45649, 0x986801, 0x0184BC,
            ),
            (
                &dark, 0xC678DD, 0x98C379, 0x61AFEF, 0xE5C07B, 0xE06C75, 0xD19A66, 0x56B6C2,
            ),
        ] {
            let s = &style.syntax;
            assert_eq!(s.style("keyword").and_then(|v| v.color), Some(rgb(kw).into()));
            assert_eq!(s.style("string").and_then(|v| v.color), Some(rgb(str_hex).into()));
            assert_eq!(s.style("function").and_then(|v| v.color), Some(rgb(fn_hex).into()));
            assert_eq!(s.style("type").and_then(|v| v.color), Some(rgb(typ).into()));
            assert_eq!(s.style("property").and_then(|v| v.color), Some(rgb(prop).into()));
            assert_eq!(s.style("number").and_then(|v| v.color), Some(rgb(num).into()));
            assert_eq!(s.style("operator").and_then(|v| v.color), Some(rgb(op).into()));

            let comment = s.style("comment").expect("comment style");
            assert_eq!(comment.color, Some(rgb(0x7F848E).into()));
            assert_eq!(comment.font_style, Some(FontStyle::Italic));
        }
    }
}
