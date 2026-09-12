//! Presentation-agnostic workbench logic shared by every DBM frontend.
//!
//! Nothing here depends on a UI toolkit. The Tauri/React app ports this
//! behavior in TypeScript; the native frontends call these functions directly
//! so presets, CSV output, connection-URL import, and statement targeting stay
//! identical on every platform.

#![deny(unsafe_code)]

pub mod connection_url;
pub mod format;
pub mod sql_target;
