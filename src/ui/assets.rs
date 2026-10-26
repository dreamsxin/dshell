//! Built-in SVG assets used by the icon components.
//!
//! `gpui-component` deliberately leaves icon files to the application.  Keeping
//! the small Lucide-style set here makes the binary self-contained and avoids a
//! runtime asset directory that can get out of sync with the UI.

use std::borrow::Cow;

use anyhow::Result;
use gpui::{AssetSource, SharedString};

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let Some(name) = path
            .strip_prefix("icons/")
            .and_then(|path| path.strip_suffix(".svg"))
        else {
            return Ok(None);
        };

        Ok(Some(Cow::Borrowed(icon_svg(name).as_bytes())))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

fn icon_svg(name: &str) -> &'static str {
    match name {
        "plus" => PLUS,
        "minus" => MINUS,
        "close" | "window-close" => CLOSE,
        "check" => CHECK,
        "dash" => DASH,
        "search" => SEARCH,
        "settings" | "settings-2" => SETTINGS,
        "ellipsis" => ELLIPSIS,
        "ellipsis-vertical" => ELLIPSIS_VERTICAL,
        "panel-left" | "panel-left-open" | "panel-left-close" => PANEL_LEFT,
        "panel-right" | "panel-right-open" | "panel-right-close" => PANEL_RIGHT,
        "panel-bottom" | "panel-bottom-open" => PANEL_BOTTOM,
        "layout-dashboard" => LAYOUT_DASHBOARD,
        "chevron-down" => CHEVRON_DOWN,
        "chevron-up" => CHEVRON_UP,
        "chevron-left" => CHEVRON_LEFT,
        "chevron-right" => CHEVRON_RIGHT,
        "chevrons-up-down" => CHEVRONS_UP_DOWN,
        "arrow-down" => ARROW_DOWN,
        "arrow-left" => ARROW_LEFT,
        "arrow-right" => ARROW_RIGHT,
        "arrow-up" => ARROW_UP,
        "folder" | "folder-closed" => FOLDER,
        "folder-open" => FOLDER_OPEN,
        "file" => FILE,
        "lock" => LOCK,
        "lock-open" | "unlock" => LOCK_OPEN,
        "globe" => GLOBE,
        "book-open" => BOOK_OPEN,
        "square-terminal" => TERMINAL,
        "copy" => COPY,
        "replace" => REPLACE,
        "delete" => DELETE,
        "eye" => EYE,
        "eye-off" => EYE_OFF,
        "star" => STAR,
        "star-off" => STAR_OFF,
        "palette" => PALETTE,
        "moon" => MOON,
        "sun" => SUN,
        "user" | "circle-user" => USER,
        "info" => INFO,
        "circle-check" => CIRCLE_CHECK,
        "circle-x" => CIRCLE_X,
        "triangle-alert" => TRIANGLE_ALERT,
        "loader" | "loader-circle" => LOADER,
        "resize-corner" => RESIZE_CORNER,
        "maximize" | "window-maximize" => MAXIMIZE,
        "minimize" | "window-minimize" => MINIMIZE,
        "window-restore" => RESTORE,
        "heart" | "heart-off" => HEART,
        "thumbs-up" => THUMBS_UP,
        "thumbs-down" => THUMBS_DOWN,
        "bell" => BELL,
        "bot" => BOT,
        "building-2" => BUILDING,
        "calendar" => CALENDAR,
        "case-sensitive" => CASE_SENSITIVE,
        "chart-pie" => CHART_PIE,
        "asterisk" => ASTERISK,
        "frame" => FRAME,
        "gallery-vertical-end" => GALLERY,
        "github" => GITHUB,
        "inbox" => INBOX,
        "map" => MAP,
        "sort-ascending" => SORT_ASCENDING,
        "sort-descending" => SORT_DESCENDING,
        "a-large-small" => A_LARGE_SMALL,
        "external-link" => EXTERNAL_LINK,
        _ => GENERIC,
    }
}

macro_rules! svg_icon {
    ($body:literal) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
            $body,
            r#"</svg>"#
        )
    };
}

const GENERIC: &str =
    svg_icon!(r#"<rect x="5" y="5" width="14" height="14" rx="3"/><path d="M8 12h8M12 8v8"/>"#);
const PLUS: &str = svg_icon!(r#"<path d="M12 5v14M5 12h14"/>"#);
const MINUS: &str = svg_icon!(r#"<path d="M5 12h14"/>"#);
const CLOSE: &str = svg_icon!(r#"<path d="m6 6 12 12M18 6 6 18"/>"#);
const CHECK: &str = svg_icon!(r#"<path d="m5 12 4 4L19 6"/>"#);
const DASH: &str = svg_icon!(r#"<path d="M5 12h14"/>"#);
const SEARCH: &str = svg_icon!(r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/>"#);
const SETTINGS: &str = svg_icon!(
    r#"<path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.64 5.64l1.42 1.42M16.94 16.94l1.42 1.42M5.64 18.36l1.42-1.42M16.94 7.06l1.42-1.42"/><circle cx="12" cy="12" r="4"/>"#
);
const ELLIPSIS: &str = svg_icon!(
    r#"<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>"#
);
const ELLIPSIS_VERTICAL: &str = svg_icon!(
    r#"<circle cx="12" cy="5" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="12" cy="19" r="1"/>"#
);
const PANEL_LEFT: &str =
    svg_icon!(r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M8 4v16"/>"#);
const PANEL_RIGHT: &str =
    svg_icon!(r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M16 4v16"/>"#);
const PANEL_BOTTOM: &str =
    svg_icon!(r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 15h18"/>"#);
const LAYOUT_DASHBOARD: &str = svg_icon!(
    r#"<rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/>"#
);
const CHEVRON_DOWN: &str = svg_icon!(r#"<path d="m6 9 6 6 6-6"/>"#);
const CHEVRON_UP: &str = svg_icon!(r#"<path d="m6 15 6-6 6 6"/>"#);
const CHEVRON_LEFT: &str = svg_icon!(r#"<path d="m15 6-6 6 6 6"/>"#);
const CHEVRON_RIGHT: &str = svg_icon!(r#"<path d="m9 6 6 6-6 6"/>"#);
const CHEVRONS_UP_DOWN: &str = svg_icon!(r#"<path d="m7 9 5-5 5 5M7 15l5 5 5-5"/>"#);
const ARROW_DOWN: &str = svg_icon!(r#"<path d="M12 5v14M19 12l-7 7-7-7"/>"#);
const ARROW_LEFT: &str = svg_icon!(r#"<path d="M19 12H5M12 19l-7-7 7-7"/>"#);
const ARROW_RIGHT: &str = svg_icon!(r#"<path d="M5 12h14M12 5l7 7-7 7"/>"#);
const ARROW_UP: &str = svg_icon!(r#"<path d="M12 19V5M5 12l7-7 7 7"/>"#);
const FOLDER: &str = svg_icon!(
    r#"<path d="M3 6a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/>"#
);
const FOLDER_OPEN: &str =
    svg_icon!(r#"<path d="m3 7 2 12h14l2-10H5"/><path d="M3 7V5a1 1 0 0 1 1-1h6l2 3h8"/>"#);
const FILE: &str = svg_icon!(r#"<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v5h5M9 13h6M9 17h6"/>"#);
const LOCK: &str = svg_icon!(
    r#"<rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 1 1 8 0v4"/>"#
);
const LOCK_OPEN: &str = svg_icon!(
    r#"<rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 7.5-1.8"/>"#
);
const GLOBE: &str = svg_icon!(
    r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>"#
);
const BOOK_OPEN: &str = svg_icon!(
    r#"<path d="M2 5a3 3 0 0 1 3-2h5v17H5a3 3 0 0 0-3 2ZM22 5a3 3 0 0 0-3-2h-5v17h5a3 3 0 0 1 3 2Z"/>"#
);
const TERMINAL: &str = svg_icon!(
    r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/>"#
);
const COPY: &str = svg_icon!(
    r#"<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M15 9V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h3"/>"#
);
const REPLACE: &str = svg_icon!(
    r#"<path d="m7 7 3-3 3 3M10 4v10a4 4 0 0 0 4 4h3"/><path d="m17 17-3 3-3-3M14 20V10a4 4 0 0 0-4-4H7"/>"#
);
const DELETE: &str = svg_icon!(r#"<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>"#);
const EYE: &str = svg_icon!(
    r#"<path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6S2 12 2 12Z"/><circle cx="12" cy="12" r="2"/>"#
);
const EYE_OFF: &str = svg_icon!(
    r#"<path d="m3 3 18 18M10.6 6.2A10.7 10.7 0 0 1 12 6c6.5 0 10 6 10 6a18 18 0 0 1-3.1 3.8M6.2 6.2C3.4 7.9 2 12 2 12s3.5 6 10 6a10.5 10.5 0 0 0 2-.2"/>"#
);
const STAR: &str = svg_icon!(
    r#"<path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9Z"/>"#
);
const STAR_OFF: &str = svg_icon!(
    r#"<path d="m3 3 18 18M8.2 8.2 3 9.6l4.5 4.4-1.1 6.2 5.6-3 2.4 1.3M15.8 15.8l4.7 2.5-1.1-6.2L21 9.6l-6.2-.9L12 3l-1.1 2.2"/>"#
);
const PALETTE: &str = svg_icon!(
    r#"<path d="M12 3a9 9 0 0 0 0 18h1.5a1.5 1.5 0 0 0 0-3H12a2 2 0 0 1 0-4h2a7 7 0 0 0 0-14Z"/><circle cx="7.5" cy="10" r=".8"/><circle cx="9" cy="6.5" r=".8"/><circle cx="14" cy="6.5" r=".8"/><circle cx="17" cy="10" r=".8"/>"#
);
const MOON: &str =
    svg_icon!(r#"<path d="M20.5 15.5A8.5 8.5 0 0 1 8.5 3.5 8.5 8.5 0 1 0 20.5 15.5Z"/>"#);
const SUN: &str = svg_icon!(
    r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>"#
);
const USER: &str = svg_icon!(r#"<circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/>"#);
const INFO: &str = svg_icon!(r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>"#);
const CIRCLE_CHECK: &str =
    svg_icon!(r#"<circle cx="12" cy="12" r="9"/><path d="m8 12 2.5 2.5L16 9"/>"#);
const CIRCLE_X: &str =
    svg_icon!(r#"<circle cx="12" cy="12" r="9"/><path d="m9 9 6 6M15 9l-6 6"/>"#);
const TRIANGLE_ALERT: &str =
    svg_icon!(r#"<path d="m12 3 10 18H2Z"/><path d="M12 9v4M12 17h.01"/>"#);
const LOADER: &str = svg_icon!(r#"<path d="M12 3a9 9 0 1 1-9 9"/>"#);
const RESIZE_CORNER: &str = svg_icon!(r#"<path d="M15 3h6v6M21 3l-8 8M9 21H3v-6M3 21l8-8"/>"#);
const MAXIMIZE: &str = svg_icon!(r#"<path d="M8 3H3v5M16 3h5v5M21 16v5h-5M3 16v5h5"/>"#);
const MINIMIZE: &str = svg_icon!(r#"<path d="M5 12h14"/>"#);
const RESTORE: &str = svg_icon!(
    r#"<rect x="7" y="7" width="13" height="13" rx="2"/><path d="M17 7V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h2"/>"#
);
const HEART: &str = svg_icon!(
    r#"<path d="M20.8 8.8c0 5-8.8 10.2-8.8 10.2S3.2 13.8 3.2 8.8A4.8 4.8 0 0 1 12 6a4.8 4.8 0 0 1 8.8 2.8Z"/>"#
);
const THUMBS_UP: &str = svg_icon!(
    r#"<path d="M7 10v10H4V10h3ZM7 20h9a2 2 0 0 0 2-1.6l1-6A2 2 0 0 0 17 10h-4l.6-3.2A2.3 2.3 0 0 0 11.3 4L7 10"/>"#
);
const THUMBS_DOWN: &str = svg_icon!(
    r#"<path d="M7 14V4H4v10h3ZM7 4h9a2 2 0 0 1 2 1.6l1 6A2 2 0 0 1 17 14h-4l.6 3.2a2.3 2.3 0 0 1-2.3 2.8L7 14"/>"#
);
const BELL: &str =
    svg_icon!(r#"<path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9M10 21h4"/>"#);
const BOT: &str = svg_icon!(
    r#"<rect x="4" y="7" width="16" height="12" rx="3"/><path d="M12 3v4M8 12h.01M16 12h.01M8 16h8"/>"#
);
const BUILDING: &str =
    svg_icon!(r#"<path d="M4 21V5l8-3 8 3v16M4 9h16M8 13h2M14 13h2M8 17h2M14 17h2"/>"#);
const CALENDAR: &str = svg_icon!(
    r#"<rect x="3" y="5" width="18" height="16" rx="2"/><path d="M16 3v4M8 3v4M3 10h18"/>"#
);
const CASE_SENSITIVE: &str =
    svg_icon!(r#"<path d="M5 17 9 7l4 10M6.5 13h5M14 17l3-8 3 8M15.2 14h3.6"/>"#);
const CHART_PIE: &str =
    svg_icon!(r#"<path d="M21 12a9 9 0 1 1-9-9v9Z"/><path d="M12 3a9 9 0 0 1 9 9h-9Z"/>"#);
const ASTERISK: &str =
    svg_icon!(r#"<path d="M12 3v18M3 12h18M5.6 5.6l12.8 12.8M18.4 5.6 5.6 18.4"/>"#);
const FRAME: &str = svg_icon!(
    r#"<path d="M8 3H3v5M16 3h5v5M21 16v5h-5M3 16v5h5"/><rect x="7" y="7" width="10" height="10" rx="1"/>"#
);
const GALLERY: &str = svg_icon!(
    r#"<rect x="3" y="3" width="18" height="18" rx="2"/><path d="m3 16 5-5 4 4 3-3 6 6M16 8h.01"/>"#
);
const GITHUB: &str = svg_icon!(
    r#"<path d="M9 19c-4 1.5-4-2-5.5-2.5M14 22v-3.9a3.4 3.4 0 0 0-.9-2.6c3 0 6.1-1.5 6.1-6.5a5 5 0 0 0-1.3-3.5A4.6 4.6 0 0 0 17.8 2S16.6 1.6 14 3.3a13.4 13.4 0 0 0-5 0C6.4 1.6 5.2 2 5.2 2a4.6 4.6 0 0 0-.1 3.5A5 5 0 0 0 3.8 9c0 5 3.1 6.5 6.1 6.5a3.4 3.4 0 0 0-.9 2.6V22"/>"#
);
const INBOX: &str = svg_icon!(r#"<path d="M4 4h16v13H4zM4 13h4l2 3h4l2-3h4"/>"#);
const MAP: &str =
    svg_icon!(r#"<path d="m3 6 6-3 6 3 6-3v15l-6 3-6-3-6 3Z"/><path d="M9 3v15M15 6v15"/>"#);
const SORT_ASCENDING: &str = svg_icon!(r#"<path d="M4 17h6M4 12h10M4 7h14"/>"#);
const SORT_DESCENDING: &str = svg_icon!(r#"<path d="M4 7h14M4 12h10M4 17h6"/>"#);
const A_LARGE_SMALL: &str =
    svg_icon!(r#"<path d="M5 18 9 6l4 12M6.5 14h5M15 18l3-8 3 8M16.2 15h3.6"/>"#);
const EXTERNAL_LINK: &str = svg_icon!(
    r#"<path d="M14 3h7v7M21 3l-9 9"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>"#
);

#[cfg(test)]
mod tests {
    use super::AppAssets;
    use gpui::AssetSource;

    #[test]
    fn loads_known_and_unknown_icons() {
        let assets = AppAssets;
        let known = assets.load("icons/plus.svg").unwrap().unwrap();
        let fallback = assets
            .load("icons/not-in-the-catalog.svg")
            .unwrap()
            .unwrap();
        let lock = assets.load("icons/lock.svg").unwrap().unwrap();
        let unlock = assets.load("icons/unlock.svg").unwrap().unwrap();
        assert!(known.windows(5).any(|window| window == b"<svg "));
        assert!(fallback.windows(5).any(|window| window == b"<svg "));
        assert_ne!(lock.as_ref(), fallback.as_ref());
        assert_ne!(unlock.as_ref(), fallback.as_ref());
        assert!(assets.load("images/logo.svg").unwrap().is_none());
    }
}
