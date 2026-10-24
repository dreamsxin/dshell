//! Reusable presentational components built from design tokens.
//!
//! Components take `&ThemeTokens` plus plain data and never read global app
//! state. Callers attach handlers through the returned element.
//!
//! The catalog is defined up front for the staged UI rollout; later phases use
//! the remaining entries.

#![allow(dead_code)]

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Div, ElementId, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Stateful, Styled, div, px, relative,
};
use gpui_component::{
    Icon, IconName, Sizable as _, Size, StyledExt as _,
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

/// Two-line list row: title, subtitle and always-visible row actions.
///
/// Actions stay visible (rather than hover-only) so keyboard users can reach
/// them with `Tab`; callers attach navigation handlers to the returned row.
pub struct SidebarRow {
    tokens: ThemeTokens,
    id: ElementId,
    title: SharedString,
    subtitle: SharedString,
    selected: bool,
    actions: Option<AnyElement>,
}

impl SidebarRow {
    pub fn new(
        tokens: &ThemeTokens,
        id: impl Into<ElementId>,
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
    ) -> Self {
        Self {
            tokens: *tokens,
            id: id.into(),
            title: title.into(),
            subtitle: subtitle.into(),
            selected: false,
            actions: None,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn actions(mut self, actions: impl IntoElement) -> Self {
        self.actions = Some(actions.into_any_element());
        self
    }

    pub fn into_row(self) -> Stateful<Div> {
        let tokens = &self.tokens;
        div()
            .id(self.id)
            .flex()
            .items_center()
            .gap(px(tokens::SPACE_2))
            .h(px(tokens::LIST_ROW_HEIGHT))
            .px(px(tokens::SPACE_2))
            .rounded(px(tokens::RADIUS_CONTROL))
            .border_l_2()
            .border_color(if self.selected {
                tokens.accent
            } else {
                tokens.surface
            })
            .bg(if self.selected {
                tokens.accent.opacity(0.14)
            } else {
                tokens.surface
            })
            .hover(|style| style.bg(tokens.surface_overlay))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.0))
                    .child(
                        div()
                            .overflow_hidden()
                            .text_size(px(tokens::FONT_BODY))
                            .text_color(tokens.text_primary)
                            .child(self.title),
                    )
                    .child(
                        div()
                            .overflow_hidden()
                            .text_size(px(tokens::FONT_CAPTION))
                            .text_color(tokens.text_secondary)
                            .child(self.subtitle),
                    ),
            )
            .when_some(self.actions, |row, actions| row.child(actions))
    }
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

/// Thin transfer/inspection progress bar.
pub fn progress_bar(tokens: &ThemeTokens, ratio: f32) -> Div {
    let ratio = ratio.clamp(0.0, 1.0);
    div()
        .h(px(6.0))
        .w_full()
        .rounded(px(tokens::RADIUS_PILL))
        .bg(tokens.border_subtle)
        .child(
            div()
                .h_full()
                .w(relative(ratio))
                .rounded(px(tokens::RADIUS_PILL))
                .bg(tokens.accent),
        )
}

/// Inspector group header with a chevron indicating the expanded state.
pub fn collapsible_header(
    tokens: &ThemeTokens,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    open: bool,
) -> Stateful<Div> {
    div()
        .id(id)
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
        .child(label.into())
        .child(
            Icon::new(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .with_size(Size::Small)
            .text_color(tokens.text_secondary),
        )
}

/// Label/value row for the connection inspector.
pub fn info_row(
    tokens: &ThemeTokens,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(tokens::SPACE_2))
        .px(px(tokens::SPACE_3))
        .py(px(tokens::SPACE_1))
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(tokens::FONT_CAPTION))
                .text_color(tokens.text_secondary)
                .child(label.into()),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_size(px(tokens::FONT_CAPTION))
                .text_color(tokens.text_primary)
                .child(value.into()),
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

/// Full-window scrim used by every modal.
pub fn modal_overlay(tokens: &ThemeTokens, id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .size_full()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .p(px(tokens::SPACE_4))
        .bg(tokens.scrim)
}

/// Modal surface: raised background, 12 px radius, strong border and shadow.
pub fn modal_panel(tokens: &ThemeTokens, id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .min_h_0()
        .border_1()
        .border_color(tokens.border_strong)
        .rounded(px(tokens::RADIUS_PANEL))
        .shadow_lg()
        .overflow_hidden()
        .bg(tokens.surface_overlay)
        .text_color(tokens.text_primary)
}

/// 48 px modal title bar. Append actions to keep the title and actions apart.
pub fn modal_header(tokens: &ThemeTokens, title: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(tokens::MODAL_HEADER_HEIGHT))
        .flex_shrink_0()
        .px(px(tokens::SPACE_4))
        .border_b_1()
        .border_color(tokens.border_subtle)
        .bg(tokens.surface)
        .text_size(px(tokens::FONT_TITLE))
        .font_semibold()
        .text_color(tokens.text_primary)
        .child(title.into())
}

/// 60 px modal footer with right-aligned actions.
pub fn modal_footer(tokens: &ThemeTokens) -> Div {
    div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(tokens::SPACE_2))
        .h(px(tokens::MODAL_FOOTER_HEIGHT))
        .flex_shrink_0()
        .px(px(tokens::SPACE_4))
        .border_t_1()
        .border_color(tokens.border_subtle)
        .bg(tokens.surface)
}

/// Transient notification card: tone icon plus text. Callers append a close
/// action and position the card themselves.
pub fn toast(tokens: &ThemeTokens, tone: Tone, message: impl Into<SharedString>) -> Div {
    let color = tone.color(tokens);
    let icon = match tone {
        Tone::Success => IconName::CircleCheck,
        Tone::Warning => IconName::TriangleAlert,
        Tone::Danger => IconName::CircleX,
        Tone::Accent | Tone::Neutral => IconName::Info,
    };
    div()
        .flex()
        .items_center()
        .gap(px(tokens::SPACE_2))
        .min_w(px(280.0))
        .max_w(px(420.0))
        .py(px(tokens::SPACE_2))
        .pl(px(tokens::SPACE_3))
        .pr(px(tokens::SPACE_1))
        .rounded(px(tokens::RADIUS_CARD))
        .border_1()
        .border_color(color.opacity(0.5))
        .bg(tokens.surface_overlay)
        .shadow_lg()
        .text_size(px(tokens::FONT_BODY))
        .text_color(tokens.text_primary)
        .child(Icon::new(icon).with_size(Size::Small).text_color(color))
        .child(div().flex_1().min_w_0().child(message.into()))
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
