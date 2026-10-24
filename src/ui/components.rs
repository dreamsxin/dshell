//! Reusable presentational components built from design tokens.
//!
//! Components take `&ThemeTokens` plus plain data and never read global app
//! state. Callers attach handlers through the returned element.
//!
//! The catalog is defined up front for the staged UI rollout; later phases use
//! the remaining entries.

#![allow(dead_code)]

use gpui::{Div, ElementId, Entity, ParentElement as _, SharedString, Styled, div, px};
use gpui_component::{
    IconName, StyledExt as _,
    button::{Button, ButtonVariants as _},
    input::{InputState, TextInput},
};

use super::{theme::ThemeTokens, tokens};

/// Semantic tone used by pills and status dots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

impl Tone {
    pub fn color(self, tokens: &ThemeTokens) -> gpui::Hsla {
        match self {
            Self::Neutral => tokens.text_secondary,
            Self::Accent => tokens.accent,
            Self::Success => tokens.success,
            Self::Warning => tokens.warning,
            Self::Danger => tokens.danger,
        }
    }
}

/// Button emphasis, mapped onto the gpui-component variants so hover, active,
/// focus and disabled states stay consistent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

/// Button height tiers from §5.3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonSize {
    Small,
    Regular,
    Large,
}

impl ButtonSize {
    pub const fn height(self) -> f32 {
        match self {
            Self::Small => tokens::CONTROL_HEIGHT_SM,
            Self::Regular => tokens::CONTROL_HEIGHT,
            Self::Large => tokens::CONTROL_HEIGHT_LG,
        }
    }
}

/// A token-sized button with a fixed body font size.
///
/// Colors come from the installed gpui-component theme (which
/// [`super::theme::install`] fills from the same tokens), so hover, focus and
/// disabled states keep the library behavior.
pub fn button(
    _tokens: &ThemeTokens,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    size: ButtonSize,
) -> Button {
    let button = Button::new(id)
        .child(div().text_size(px(tokens::FONT_BODY)).child(label.into()))
        .h(px(size.height()))
        .px(px(tokens::SPACE_3))
        .rounded(px(tokens::RADIUS_CONTROL));
    match kind {
        ButtonKind::Primary => button.primary(),
        ButtonKind::Secondary => button,
        ButtonKind::Ghost => button.ghost(),
        ButtonKind::Danger => button.danger(),
    }
}

/// Square icon-only button used by the shell and toolbars.
pub fn icon_button(
    _tokens: &ThemeTokens,
    id: impl Into<ElementId>,
    icon: IconName,
    kind: ButtonKind,
    size: ButtonSize,
    tooltip: impl Into<SharedString>,
) -> Button {
    let button = Button::new(id)
        .icon(icon)
        .tooltip(tooltip)
        .h(px(size.height()))
        .w(px(size.height()))
        .rounded(px(tokens::RADIUS_CONTROL));
    match kind {
        ButtonKind::Primary => button.primary(),
        ButtonKind::Secondary => button,
        ButtonKind::Ghost => button.ghost(),
        ButtonKind::Danger => button.danger(),
    }
}

/// Label, input and optional caller-provided help/error rows.
pub fn field(
    tokens: &ThemeTokens,
    label: impl Into<SharedString>,
    input: &Entity<InputState>,
    masked: bool,
) -> Div {
    let input = TextInput::new(input);
    let input = if masked { input.mask_toggle() } else { input };
    div()
        .flex()
        .flex_col()
        .gap(px(tokens::SPACE_1))
        .child(label_text(tokens, label.into()))
        .child(input.h(px(tokens::CONTROL_HEIGHT_SM)).w_full())
}

pub fn label_text(tokens: &ThemeTokens, label: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(tokens::FONT_CAPTION))
        .text_color(tokens.text_secondary)
        .child(label.into())
}

pub fn help_text(tokens: &ThemeTokens, text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(tokens::FONT_CAPTION))
        .text_color(tokens.text_secondary)
        .child(text.into())
}

pub fn error_text(tokens: &ThemeTokens, text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(tokens::FONT_CAPTION))
        .text_color(tokens.danger)
        .child(text.into())
}

pub fn warning_text(tokens: &ThemeTokens, text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(tokens::FONT_CAPTION))
        .text_color(tokens.warning)
        .child(text.into())
}

pub fn section_label(tokens: &ThemeTokens, text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(tokens::FONT_CAPTION))
        .font_semibold()
        .text_color(tokens.text_secondary)
        .child(text.into())
}

/// In-form group heading, one step above [`section_label`].
pub fn subheading(tokens: &ThemeTokens, text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(tokens::FONT_SUBTITLE))
        .font_semibold()
        .text_color(tokens.text_primary)
        .child(text.into())
}

/// Sidebar group header: small, semi-bold, secondary text.
pub fn section_header(tokens: &ThemeTokens, text: impl Into<SharedString>) -> Div {
    div()
        .pt(px(tokens::SPACE_2))
        .px(px(tokens::SPACE_1))
        .text_size(px(tokens::FONT_CAPTION))
        .font_semibold()
        .text_color(tokens.text_secondary)
        .child(text.into())
}

/// Standard docked panel container. Callers set width and children.
pub fn panel(tokens: &ThemeTokens) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .min_h_0()
        .border_l_1()
        .border_color(tokens.border_subtle)
        .bg(tokens.surface_raised)
        .text_color(tokens.text_primary)
}

/// Panel title bar. Append one action container to keep `justify_between`.
pub fn panel_header(tokens: &ThemeTokens, title: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(tokens::PANEL_HEADER_HEIGHT))
        .flex_shrink_0()
        .px(px(tokens::SPACE_3))
        .border_b_1()
        .border_color(tokens.border_subtle)
        .bg(tokens.surface)
        .text_size(px(tokens::FONT_BODY))
        .font_semibold()
        .text_color(tokens.text_primary)
        .child(title.into())
}

pub fn empty_state(
    tokens: &ThemeTokens,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(px(tokens::SPACE_2))
        .p(px(tokens::SPACE_4))
        .child(
            div()
                .text_size(px(tokens::FONT_SUBTITLE))
                .font_semibold()
                .text_color(tokens.text_primary)
                .child(title.into()),
        )
        .child(
            div()
                .text_size(px(tokens::FONT_CAPTION))
                .text_color(tokens.text_secondary)
                .child(description.into()),
        )
}

/// Small status capsule: color plus text, never color alone.
pub fn pill(tokens: &ThemeTokens, label: impl Into<SharedString>, tone: Tone) -> Div {
    let color = tone.color(tokens);
    div()
        .flex()
        .items_center()
        .h(px(20.0))
        .px(px(tokens::SPACE_2))
        .rounded(px(tokens::RADIUS_PILL))
        .bg(color.opacity(0.14))
        .text_size(px(tokens::FONT_CAPTION))
        .text_color(color)
        .child(label.into())
}

pub fn status_dot(tokens: &ThemeTokens, tone: Tone) -> Div {
    div()
        .flex_shrink_0()
        .size(px(tokens::ICON_SIZE_INLINE * 0.5))
        .rounded(px(tokens::RADIUS_PILL))
        .bg(tone.color(tokens))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::UiTheme;

    #[test]
    fn button_height_tiers_follow_the_size_tokens() {
        assert_eq!(ButtonSize::Small.height(), tokens::CONTROL_HEIGHT_SM);
        assert_eq!(ButtonSize::Regular.height(), tokens::CONTROL_HEIGHT);
        assert_eq!(ButtonSize::Large.height(), tokens::CONTROL_HEIGHT_LG);
    }

    #[test]
    fn tones_resolve_to_semantic_colors() {
        let dark = UiTheme::Dark.tokens();
        assert_eq!(Tone::Accent.color(&dark), dark.accent);
        assert_eq!(Tone::Success.color(&dark), dark.success);
        assert_eq!(Tone::Warning.color(&dark), dark.warning);
        assert_eq!(Tone::Danger.color(&dark), dark.danger);
        assert_eq!(Tone::Neutral.color(&dark), dark.text_secondary);

        let light = UiTheme::Light.tokens();
        assert_eq!(Tone::Accent.color(&light), light.accent);
        assert_ne!(Tone::Accent.color(&light), Tone::Accent.color(&dark));
    }
}
