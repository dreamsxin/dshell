//! Semantic color tokens and mapping into the gpui-component theme.
//!
//! Palettes follow `docs/UI_DESIGN.md` §5.1. Business code references names such
//! as `tokens.surface_raised`; it never writes a literal color.

use gpui::{App, Hsla, px, rgb, rgba};
use gpui_component::{Theme, ThemeMode};
use serde::{Deserialize, Serialize};

use crate::profiles::TabColor;

use super::tokens;

/// Which UI palette is active. Terminal palettes stay independent.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UiTheme {
    #[default]
    Dark,
    Light,
}

#[allow(dead_code)]
impl UiTheme {
    pub const DEFAULT: Self = Self::Dark;

    pub const fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }

    pub const fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    pub const fn mode(self) -> ThemeMode {
        match self {
            Self::Dark => ThemeMode::Dark,
            Self::Light => ThemeMode::Light,
        }
    }

    pub fn tokens(self) -> ThemeTokens {
        ThemeTokens::for_theme(self)
    }
}

/// The semantic color set for one palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemeTokens {
    pub theme: UiTheme,
    pub bg_canvas: Hsla,
    pub surface: Hsla,
    pub surface_raised: Hsla,
    pub surface_overlay: Hsla,
    pub border_subtle: Hsla,
    pub border_strong: Hsla,
    pub text_primary: Hsla,
    pub text_secondary: Hsla,
    pub text_disabled: Hsla,
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
    pub tab_colors: [Hsla; 8],
    /// Higher-contrast variants for colored text on tinted backgrounds.
    pub accent_text: Hsla,
    pub success_text: Hsla,
    pub warning_text: Hsla,
    pub danger_text: Hsla,
    pub focus_ring: Hsla,
    /// Text drawn on top of `accent`, `danger` and `success` fills.
    pub on_accent: Hsla,
    /// Modal backdrop.
    pub scrim: Hsla,
}

impl ThemeTokens {
    pub fn for_theme(theme: UiTheme) -> Self {
        match theme {
            UiTheme::Dark => Self {
                theme,
                bg_canvas: rgb(0x0F1115).into(),
                surface: rgb(0x15181E).into(),
                surface_raised: rgb(0x1B1F27).into(),
                surface_overlay: rgb(0x20252F).into(),
                border_subtle: rgb(0x262C36).into(),
                border_strong: rgb(0x333B48).into(),
                text_primary: rgb(0xE7EAF0).into(),
                text_secondary: rgb(0xA0A9B8).into(),
                text_disabled: rgb(0x5C6575).into(),
                accent: rgb(0x3B82F6).into(),
                accent_hover: rgb(0x60A5FA).into(),
                success: rgb(0x22C55E).into(),
                warning: rgb(0xF59E0B).into(),
                danger: rgb(0xEF4444).into(),
                tab_colors: [
                    rgb(0x3B82F6).into(),
                    rgb(0x22C55E).into(),
                    rgb(0xF59E0B).into(),
                    rgb(0xEF4444).into(),
                    rgb(0x22D3EE).into(),
                    rgb(0x2DD4BF).into(),
                    rgb(0xF472B6).into(),
                    rgb(0xA78BFA).into(),
                ],
                accent_text: rgb(0x60A5FA).into(),
                success_text: rgb(0x4ADE80).into(),
                warning_text: rgb(0xFBBF24).into(),
                danger_text: rgb(0xF87171).into(),
                focus_ring: rgb(0x60A5FA).into(),
                on_accent: rgb(0xFFFFFF).into(),
                scrim: rgba(0x0000008C).into(),
            },
            UiTheme::Light => Self {
                theme,
                bg_canvas: rgb(0xF5F6F8).into(),
                surface: rgb(0xFFFFFF).into(),
                surface_raised: rgb(0xFFFFFF).into(),
                surface_overlay: rgb(0xFFFFFF).into(),
                border_subtle: rgb(0xE4E7EC).into(),
                border_strong: rgb(0xD0D5DD).into(),
                text_primary: rgb(0x101828).into(),
                text_secondary: rgb(0x475467).into(),
                text_disabled: rgb(0x98A2B3).into(),
                accent: rgb(0x2563EB).into(),
                accent_hover: rgb(0x1D4ED8).into(),
                success: rgb(0x16A34A).into(),
                warning: rgb(0xD97706).into(),
                danger: rgb(0xDC2626).into(),
                tab_colors: [
                    rgb(0x2563EB).into(),
                    rgb(0x16A34A).into(),
                    rgb(0xD97706).into(),
                    rgb(0xDC2626).into(),
                    rgb(0x0E7490).into(),
                    rgb(0x0F766E).into(),
                    rgb(0xBE185D).into(),
                    rgb(0x6D28D9).into(),
                ],
                accent_text: rgb(0x1D4ED8).into(),
                success_text: rgb(0x166534).into(),
                warning_text: rgb(0x92400E).into(),
                danger_text: rgb(0x991B1B).into(),
                focus_ring: rgb(0x2563EB).into(),
                on_accent: rgb(0xFFFFFF).into(),
                scrim: rgba(0x10182852).into(),
            },
        }
    }

    pub fn tab_color(self, color: TabColor) -> Option<Hsla> {
        let index = match color {
            TabColor::Default => return None,
            TabColor::Custom(custom_rgb) => return Some(rgb(custom_rgb).into()),
            TabColor::Blue => 0,
            TabColor::Green => 1,
            TabColor::Amber => 2,
            TabColor::Red => 3,
            TabColor::Cyan => 4,
            TabColor::Teal => 5,
            TabColor::Pink => 6,
            TabColor::Violet => 7,
        };
        Some(self.tab_colors[index])
    }
}

/// Applies a palette to the gpui-component global theme so that library
/// controls (buttons, inputs, scrollbars, popovers) share the design tokens.
pub fn install(cx: &mut App, theme: UiTheme) {
    let tokens = theme.tokens();
    Theme::change(theme.mode(), None, cx);

    let component = Theme::global_mut(cx);
    component.radius = px(tokens::RADIUS_CONTROL);
    component.radius_lg = px(tokens::RADIUS_PANEL);

    let colors = &mut component.colors;
    colors.background = tokens.surface_raised;
    colors.foreground = tokens.text_primary;
    colors.border = tokens.border_strong;
    colors.input = tokens.border_strong;
    colors.caret = tokens.accent;
    colors.ring = tokens.focus_ring;
    colors.selection = tokens.accent.opacity(0.28);

    colors.muted = tokens.surface;
    colors.muted_foreground = tokens.text_disabled;

    colors.primary = tokens.accent;
    colors.primary_hover = tokens.accent_hover;
    colors.primary_active = tokens.accent_hover;
    colors.primary_foreground = tokens.on_accent;

    colors.secondary = tokens.surface_raised;
    colors.secondary_hover = tokens.surface_overlay;
    colors.secondary_active = tokens.border_subtle;
    colors.secondary_foreground = tokens.text_primary;

    colors.accent = tokens.accent;
    colors.accent_foreground = tokens.on_accent;

    colors.danger = tokens.danger;
    colors.danger_hover = tokens.danger;
    colors.danger_active = tokens.danger;
    colors.danger_foreground = tokens.on_accent;

    colors.success = tokens.success;
    colors.success_foreground = tokens.on_accent;
    colors.warning = tokens.warning;
    colors.warning_foreground = tokens.on_accent;
    colors.info = tokens.accent;
    colors.info_foreground = tokens.on_accent;

    colors.sidebar = tokens.surface;
    colors.sidebar_foreground = tokens.text_primary;
    colors.sidebar_accent = tokens.accent.opacity(0.12);
    colors.sidebar_accent_foreground = tokens.text_primary;
    colors.sidebar_border = tokens.border_subtle;

    colors.list = tokens.surface_raised;
    colors.list_hover = tokens.surface_overlay;
    colors.list_active = tokens.accent.opacity(0.14);
    colors.list_active_border = tokens.accent;
    colors.popover = tokens.surface_overlay;
    colors.popover_foreground = tokens.text_primary;

    colors.scrollbar = tokens.surface;
    colors.scrollbar_thumb = tokens.border_strong;
    colors.scrollbar_thumb_hover = tokens.text_disabled;
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Rgba;

    fn relative_luminance(color: Hsla) -> f32 {
        fn channel(value: f32) -> f32 {
            if value <= 0.039_28 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        let Rgba { r, g, b, .. } = color.into();
        0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    }

    fn contrast_ratio(first: Hsla, second: Hsla) -> f32 {
        let first = relative_luminance(first);
        let second = relative_luminance(second);
        let (lighter, darker) = if first > second {
            (first, second)
        } else {
            (second, first)
        };
        (lighter + 0.05) / (darker + 0.05)
    }

    fn composite(overlay: Hsla, base: Hsla) -> Hsla {
        let overlay: Rgba = overlay.into();
        let base: Rgba = base.into();
        let alpha = overlay.a;
        Rgba {
            r: overlay.r * alpha + base.r * (1.0 - alpha),
            g: overlay.g * alpha + base.g * (1.0 - alpha),
            b: overlay.b * alpha + base.b * (1.0 - alpha),
            a: 1.0,
        }
        .into()
    }

    #[test]
    fn body_text_meets_wcag_aa_on_every_surface() {
        for theme in [UiTheme::Dark, UiTheme::Light] {
            let tokens = theme.tokens();
            for surface in [
                tokens.bg_canvas,
                tokens.surface,
                tokens.surface_raised,
                tokens.surface_overlay,
            ] {
                assert!(
                    contrast_ratio(tokens.text_primary, surface) >= 4.5,
                    "{theme:?} primary text contrast is too low"
                );
                assert!(
                    contrast_ratio(tokens.text_secondary, surface) >= 4.5,
                    "{theme:?} secondary text contrast is too low"
                );
            }
        }
    }

    #[test]
    fn component_and_status_colors_meet_contrast_targets() {
        for theme in [UiTheme::Dark, UiTheme::Light] {
            let tokens = theme.tokens();
            for surface in [
                tokens.bg_canvas,
                tokens.surface,
                tokens.surface_raised,
                tokens.surface_overlay,
            ] {
                assert!(
                    contrast_ratio(tokens.focus_ring, surface) >= 3.0,
                    "{theme:?} focus ring contrast is too low"
                );
                assert!(
                    contrast_ratio(tokens.accent, surface) >= 3.0,
                    "{theme:?} accent contrast is too low"
                );
            }
            for (tone, text) in [
                (tokens.accent, tokens.accent_text),
                (tokens.success, tokens.success_text),
                (tokens.warning, tokens.warning_text),
                (tokens.danger, tokens.danger_text),
            ] {
                for surface in [
                    tokens.surface,
                    tokens.surface_raised,
                    tokens.surface_overlay,
                ] {
                    let tinted = composite(tone.opacity(0.14), surface);
                    assert!(
                        contrast_ratio(text, tinted) >= 4.5,
                        "{theme:?} pill text contrast is too low"
                    );
                }
            }
            assert!(contrast_ratio(tokens.on_accent, tokens.accent) >= 3.0);
            assert!(contrast_ratio(tokens.on_accent, tokens.danger) >= 3.0);
            for color in tokens.tab_colors {
                for surface in [tokens.surface, tokens.surface_raised] {
                    assert!(
                        contrast_ratio(color, surface) >= 3.0,
                        "{theme:?} tab color contrast is too low"
                    );
                }
            }
        }
    }

    #[test]
    fn semantic_colors_stay_distinct_within_a_palette() {
        for theme in [UiTheme::Dark, UiTheme::Light] {
            let tokens = theme.tokens();
            assert_ne!(tokens.accent, tokens.success);
            assert_ne!(tokens.accent, tokens.warning);
            assert_ne!(tokens.accent, tokens.danger);
            assert_ne!(tokens.border_subtle, tokens.border_strong);
            assert_ne!(tokens.surface, tokens.border_subtle);
            assert_ne!(tokens.text_primary, tokens.text_secondary);
            assert_eq!(tokens.theme, theme);
        }
    }

    #[test]
    fn default_ui_theme_is_dark() {
        assert_eq!(UiTheme::DEFAULT, UiTheme::Dark);
        assert!(UiTheme::default().is_dark());
    }

    #[test]
    fn custom_tab_color_uses_the_requested_rgb_value() {
        let tokens = UiTheme::Dark.tokens();
        assert_eq!(
            tokens.tab_color(TabColor::Custom(0x35a8c6)),
            Some(rgb(0x35a8c6).into())
        );
    }
}
