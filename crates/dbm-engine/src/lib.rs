//! UI-independent database engines and local storage shared by every DBM
//! frontend.
//!
//! The Tauri desktop shell in `apps/desktop/src-tauri` and the platform-native
//! frontends under `experiments/` link this crate directly. Nothing in here may
//! depend on a presentation toolkit: the same sessions, schema browsing,
//! table pages, query execution, profile storage, and credential handling must
//! behave identically no matter which shell is driving them.

#![deny(unsafe_code)]

pub mod error;
pub mod keyring_store;
pub mod models;
pub mod mysql;
pub mod postgres;
pub mod redis;
pub mod session;
pub mod state;
pub mod storage;

mod util;
