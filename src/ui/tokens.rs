//! Non-color design tokens: spacing, sizing, radii, type scale and motion.
//!
//! Everything is expressed on the 4 px grid described in `docs/UI_DESIGN.md` §5.
//! Values are plain `f32` because GPUI's [`Pixels`](gpui::Pixels) has no public
//! const constructor; call sites wrap them in `px(...)`.
//!
//! The full scale is defined up front; later UI phases consume the entries that
//! are not yet referenced.

#![allow(dead_code)]

// Spacing scale (§5.2).
pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_5: f32 = 24.0;
pub const SPACE_6: f32 = 32.0;

// Application shell (§5.3).
pub const COMMAND_BAR_HEIGHT: f32 = 52.0;
pub const SIDEBAR_WIDTH: f32 = 248.0;
pub const SIDEBAR_COLLAPSED_WIDTH: f32 = 48.0;
pub const SIDEBAR_MIN_WIDTH: f32 = 200.0;
pub const SIDEBAR_MAX_WIDTH: f32 = 360.0;
pub const INSPECTOR_WIDTH: f32 = 380.0;
pub const INSPECTOR_MIN_WIDTH: f32 = 300.0;
pub const INSPECTOR_MAX_WIDTH: f32 = 520.0;
pub const TAB_BAR_HEIGHT: f32 = 40.0;
pub const STATUS_BAR_HEIGHT: f32 = 28.0;
pub const LIST_ROW_HEIGHT: f32 = 56.0;

// Controls (§5.3).
pub const CONTROL_HEIGHT_SM: f32 = 28.0;
pub const CONTROL_HEIGHT: f32 = 32.0;
pub const CONTROL_HEIGHT_LG: f32 = 36.0;
pub const INPUT_MULTILINE_HEIGHT: f32 = 96.0;
pub const ICON_SIZE_INLINE: f32 = 16.0;
pub const ICON_SIZE_TOOLBAR: f32 = 20.0;
pub const SWITCH_WIDTH: f32 = 40.0;
pub const SWITCH_HEIGHT: f32 = 24.0;
pub const SWITCH_THUMB_SIZE: f32 = 16.0;
pub const SWITCH_INSET: f32 = 4.0;

// Panel and modal chrome.
pub const PANEL_HEADER_HEIGHT: f32 = 32.0;
pub const SIDEBAR_HEADER_HEIGHT: f32 = 48.0;
pub const MODAL_HEADER_HEIGHT: f32 = 48.0;
pub const MODAL_FOOTER_HEIGHT: f32 = 60.0;
pub const MODAL_WIDTH_CONFIRM: f32 = 440.0;
pub const MODAL_WIDTH_AUTH: f32 = 480.0;
pub const MODAL_WIDTH_KEYGEN: f32 = 560.0;
pub const MODAL_WIDTH_SESSION: f32 = 720.0;
pub const MODAL_HEIGHT_SESSION: f32 = 640.0;
pub const MODAL_WIDTH_SETTINGS: f32 = 760.0;
pub const MODAL_HEIGHT_SETTINGS: f32 = 600.0;
pub const SESSION_NAV_WIDTH: f32 = 176.0;
pub const PORT_FIELD_WIDTH: f32 = 112.0;
pub const QUICK_COMMAND_WIDTH: f32 = 152.0;
pub const QUICK_COMMAND_HEIGHT: f32 = 72.0;
pub const QUICK_COMMAND_PANEL_WIDTH: f32 = 360.0;
pub const MENU_WIDTH: f32 = 336.0;
pub const MENU_MAX_HEIGHT: f32 = 560.0;
pub const LAYOUT_MENU_WIDTH: f32 = 360.0;
pub const LAYOUT_PRESET_HEIGHT: f32 = 68.0;

// Radii (§5.4).
pub const RADIUS_CONTROL: f32 = 6.0;
pub const RADIUS_CARD: f32 = 8.0;
pub const RADIUS_PANEL: f32 = 12.0;
pub const RADIUS_PILL: f32 = 999.0;

// Strokes (§5.4).
pub const BORDER_WIDTH: f32 = 1.0;
pub const FOCUS_RING_WIDTH: f32 = 2.0;

// Type scale (§5.5).
pub const FONT_CAPTION: f32 = 12.0;
pub const FONT_BODY: f32 = 13.0;
pub const FONT_SUBTITLE: f32 = 14.0;
pub const FONT_TITLE: f32 = 16.0;
pub const FONT_DISPLAY: f32 = 20.0;

// Motion (§5.6).
pub const MOTION_QUICK_MS: u64 = 120;
pub const MOTION_MODAL_MS: u64 = 140;
pub const MOTION_PANEL_MS: u64 = 160;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_scale_is_ordered_and_on_the_four_pixel_grid() {
        let scale = [SPACE_1, SPACE_2, SPACE_3, SPACE_4, SPACE_5, SPACE_6];
        for pair in scale.windows(2) {
            assert!(pair[0] < pair[1]);
        }
        for value in scale {
            assert_eq!(value % 4.0, 0.0, "spacing {value} is off the 4 px grid");
        }
    }

    #[test]
    fn control_sizes_and_radii_are_positive_and_ordered() {
        assert!(CONTROL_HEIGHT_SM < CONTROL_HEIGHT);
        assert!(CONTROL_HEIGHT < CONTROL_HEIGHT_LG);
        assert!(RADIUS_CONTROL < RADIUS_CARD);
        assert!(RADIUS_CARD < RADIUS_PANEL);
        assert!(RADIUS_PANEL < RADIUS_PILL);
        assert!(FONT_CAPTION < FONT_BODY);
        assert!(FONT_BODY < FONT_SUBTITLE);
        assert!(FONT_SUBTITLE < FONT_TITLE);
        assert!(FONT_TITLE < FONT_DISPLAY);
        assert!(MOTION_QUICK_MS < MOTION_MODAL_MS);
        assert!(MOTION_MODAL_MS < MOTION_PANEL_MS);
    }

    #[test]
    fn modal_widths_follow_the_dialog_spec() {
        assert!(MODAL_WIDTH_CONFIRM < MODAL_WIDTH_AUTH);
        assert!(MODAL_WIDTH_AUTH < MODAL_WIDTH_KEYGEN);
        assert!(MODAL_WIDTH_KEYGEN < MODAL_WIDTH_SESSION);
        assert!(MODAL_WIDTH_SETTINGS > MODAL_WIDTH_SESSION);
    }
}
