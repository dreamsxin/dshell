//! Shared UI layer: design tokens, theme palettes and reusable components.
//!
//! Business code should reference semantic token names from [`theme::ThemeTokens`]
//! and the small component helpers in [`components`] instead of writing bare
//! colors, spacing or corner radii.

pub mod components;
pub mod theme;
pub mod tokens;
