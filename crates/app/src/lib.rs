//! # empire-app
//!
//! The user interface for the Empire reimplementation: a CGA-style screen
//! drawn into an indexed framebuffer, menus and dialogs modelled on the 1987
//! Interstel release, and the new-game sequence (without the manual-lookup
//! security check). No I/O: a host feeds input and time and reads pixels.

pub mod app;
pub mod commands;
pub mod dialog;
pub mod editor;
pub mod font;
pub mod gfx;
pub mod input;
pub mod layout;
pub mod mapdraw;
pub mod play;
pub mod text;
pub mod theme;
pub mod tiles;
pub mod ui;

pub use app::{App, HostRequest, Resolution, Settings};
pub use input::{Key, KeyEvent};
