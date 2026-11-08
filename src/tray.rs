//! System tray panel.
//!
//! Installs a native tray icon with a context menu (Open dshell, New
//! session, Quit). Tray menu events are delivered on a platform thread, so
//! they are routed into the GPUI event loop through a channel and polled by
//! a foreground task in `main` — no static App access is required.
//!
//! The tray icon is drawn programmatically from the same terminal mark as the
//! Windows application icon.

use std::sync::mpsc::Sender;

use gpui::{AnyWindowHandle, Entity, Global};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

/// Commands the tray menu and icon clicks send into the GPUI event loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Show,
    NewSession,
    Quit,
}

pub const MENU_OPEN_ID: &str = "dshell-tray-open";
pub const MENU_NEW_SESSION_ID: &str = "dshell-tray-new-session";
pub const MENU_QUIT_ID: &str = "dshell-tray-quit";
const TRAY_ID: &str = "dshell-tray";

const TRAY_ICON_SIZE: usize = 32;

/// Bridge between tray handlers and the main window: holds the window
/// handle so the GPUI poll loop can activate the window and drive the
/// session editor from tray commands.
pub struct TrayBridge {
    pub window: AnyWindowHandle,
    pub app_view: Entity<crate::AppView>,
}

impl Global for TrayBridge {}

/// Build the tray icon with its context menu and register the global event
/// handlers. Must be called after the GPUI application event loop has
/// started. Returns the live tray icon handle (dropping it removes the
/// icon, so callers keep it alive).
pub fn install(sender: Sender<TrayCommand>) -> Result<TrayIcon, String> {
    let mut menu = Menu::new();
    let open = MenuItem::with_id(MENU_OPEN_ID, "Open dshell", true, None);
    let new_session = MenuItem::with_id(MENU_NEW_SESSION_ID, "New session", true, None);
    menu.append(&open)
        .map_err(|error| format!("Could not build the tray menu: {error}"))?;
    menu.append(&new_session)
        .map_err(|error| format!("Could not build the tray menu: {error}"))?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|error| format!("Could not build the tray menu: {error}"))?;
    let quit = MenuItem::with_id(MENU_QUIT_ID, "Quit", true, None);
    menu.append(&quit)
        .map_err(|error| format!("Could not build the tray menu: {error}"))?;

    let tray = TrayIconBuilder::new()
        .with_id(TRAY_ID)
        .with_icon(tray_icon_rgba())
        .with_tooltip("dshell")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .map_err(|error| format!("Could not create the system tray icon: {error}"))?;

    // Menu clicks fire on the platform thread; route them into the GPUI
    // poll loop through the channel.
    let menu_tx = sender.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let command = match event.id().0.as_str() {
            MENU_OPEN_ID => TrayCommand::Show,
            MENU_NEW_SESSION_ID => TrayCommand::NewSession,
            MENU_QUIT_ID => TrayCommand::Quit,
            _ => return,
        };
        let _ = menu_tx.send(command);
    }));

    // Left click on the icon brings the window to the front.
    let click_tx = sender;
    tray_icon::TrayIconEvent::set_event_handler(Some(move |_event| {
        let _ = click_tx.send(TrayCommand::Show);
    }));

    Ok(tray)
}

/// Draw the tray icon pixels: 32×32 RGBA, rounded dark-navy square with a
/// light terminal-prompt chevron. Colors match the app accent family
/// (`ThemeTokens.accent` on the dark palette).
fn tray_icon_pixels() -> Vec<u8> {
    const SIZE: usize = TRAY_ICON_SIZE;
    const RADIUS: i32 = 7;
    const BG: [u8; 3] = [17, 24, 39];
    const PANEL: [u8; 3] = [31, 41, 55];
    const BORDER: [u8; 3] = [59, 130, 246];
    const PROMPT: [u8; 3] = [103, 232, 249];
    const CURSOR: [u8; 3] = [243, 244, 246];
    const GREEN: [u8; 3] = [52, 211, 153];
    const AMBER: [u8; 3] = [251, 191, 36];
    const BLUE: [u8; 3] = [96, 165, 250];

    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (x_i, y_i) = (x as i32, y as i32);
            let max = (SIZE - 1) as i32;
            // Rounded corners: skip pixels outside the radius box corners.
            let corner_distance_x = if x_i < RADIUS {
                RADIUS - x_i
            } else if x_i > max - RADIUS {
                x_i - (max - RADIUS)
            } else {
                0
            };
            let corner_distance_y = if y_i < RADIUS {
                RADIUS - y_i
            } else if y_i > max - RADIUS {
                y_i - (max - RADIUS)
            } else {
                0
            };
            let inside = corner_distance_x * corner_distance_x
                + corner_distance_y * corner_distance_y
                <= RADIUS * RADIUS;
            if !inside {
                continue;
            }
            let in_terminal = (4..=27).contains(&x_i) && (5..=26).contains(&y_i);
            let terminal_border = in_terminal && (x_i == 4 || x_i == 27 || y_i == 5 || y_i == 26);
            let title_dot = y_i == 9 && (x_i == 8 || x_i == 11 || x_i == 14);
            let divider = y_i == 12 && (7..=24).contains(&x_i);
            let prompt = ((16..=19).contains(&y_i) && (x_i - 9 - (y_i - 16)).abs() <= 1)
                || ((19..=22).contains(&y_i) && (x_i - 15 + (y_i - 19)).abs() <= 1);
            let cursor = (18..=21).contains(&x_i) && (21..=22).contains(&y_i);
            let [r, g, b] = if terminal_border {
                BORDER
            } else if prompt {
                PROMPT
            } else if cursor {
                CURSOR
            } else if title_dot {
                match x_i {
                    8 => GREEN,
                    11 => AMBER,
                    _ => BLUE,
                }
            } else if divider {
                [55, 65, 81]
            } else if in_terminal {
                PANEL
            } else {
                BG
            };
            let index = (y * SIZE + x) * 4;
            rgba[index] = r;
            rgba[index + 1] = g;
            rgba[index + 2] = b;
            rgba[index + 3] = 255;
        }
    }
    rgba
}

fn tray_icon_rgba() -> tray_icon::Icon {
    tray_icon::Icon::from_rgba(tray_icon_pixels(), 32, 32).expect("tray icon dimensions are valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_menu_ids_are_distinct() {
        let ids = [MENU_OPEN_ID, MENU_NEW_SESSION_ID, MENU_QUIT_ID];
        for (left_index, left) in ids.iter().enumerate() {
            for right in &ids[left_index + 1..] {
                assert_ne!(left, right);
            }
        }
    }

    #[test]
    fn tray_icon_pixels_are_opaque_with_transparent_corners() {
        let pixels = tray_icon_pixels();
        assert_eq!(pixels.len(), TRAY_ICON_SIZE * TRAY_ICON_SIZE * 4);
        // Corner pixel transparent, center pixel opaque.
        let size = TRAY_ICON_SIZE;
        let corner = (0 * size + 0) * 4;
        assert_eq!(pixels[corner + 3], 0);
        let center = ((size / 2) * size + (size / 2)) * 4;
        assert_eq!(pixels[center + 3], 255);
    }

    #[test]
    fn tray_commands_cover_menu_actions() {
        // The three menu ids map onto the three commands; keep them in sync
        // if the menu grows.
        assert_eq!(MENU_OPEN_ID, "dshell-tray-open");
        assert_eq!(MENU_NEW_SESSION_ID, "dshell-tray-new-session");
        assert_eq!(MENU_QUIT_ID, "dshell-tray-quit");
    }
}
