//! Runs engine work on a tokio runtime and hands results back to the window
//! thread through a queue plus a `WM_APP` wake-up message.

use std::future::Future;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use dbm_engine::error::AppError;
use dbm_engine::models::{
    ConnectionProfile, DatabaseRef, QueryHistoryEntry, QueryResponse, SchemaNode, TablePage,
};
use dbm_engine::state::AppState;
use dbm_workbench::format;
use uuid::Uuid;

pub const WM_ENGINE_EVENT: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static ENGINE: OnceLock<Arc<AppState>> = OnceLock::new();
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static QUEUE: Mutex<Vec<EngineEvent>> = Mutex::new(Vec::new());

pub enum EngineEvent {
    Profiles(Vec<ConnectionProfile>),
    Connected {
        profile: ConnectionProfile,
        databases: Vec<DatabaseRef>,
        schema: Vec<SchemaNode>,
    },
    Schema {
        profile_id: Uuid,
        schema: Vec<SchemaNode>,
    },
    TablePage {
        tab: u64,
        result: Result<TablePage, String>,
    },
    Query {
        tab: u64,
        sql: String,
        result: Result<QueryResponse, String>,
    },
    History {
        tab: u64,
        entries: Vec<QueryHistoryEntry>,
    },
    Exported {
        rows: u64,
    },
    Error(String),
    Toast(String),
}

pub fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("dbm-engine")
            .build()
            .expect("the DBM engine runtime must start")
    })
}

pub fn install(engine: Arc<AppState>, window: isize) {
    let _ = ENGINE.set(engine);
    WINDOW.store(window, Ordering::SeqCst);
}

pub fn window() -> Option<windows::Win32::Foundation::HWND> {
    let handle = WINDOW.load(Ordering::SeqCst);
    (handle != 0).then_some(windows::Win32::Foundation::HWND(
        handle as *mut core::ffi::c_void,
    ))
}

pub fn engine() -> &'static Arc<AppState> {
    ENGINE.get().expect("engine installed")
}

/// Runs a future on the engine runtime and queues the mapped event.
pub fn spawn<F, T>(future: F, map: impl FnOnce(Result<T, AppError>) -> EngineEvent + Send + 'static)
where
    F: Future<Output = Result<T, AppError>> + Send + 'static,
    T: Send + 'static,
{
    runtime().spawn(async move {
        let result = future.await;
        let event = map(result);
        push(event);
    });
}

/// Queues an event that did not come from an engine future.
pub fn push(event: EngineEvent) {
    QUEUE.lock().expect("event queue").push(event);
    wake();
}

pub fn drain() -> Vec<EngineEvent> {
    std::mem::take(&mut *QUEUE.lock().expect("event queue"))
}

fn wake() {
    if let Some(window) = window() {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                window,
                WM_ENGINE_EVENT,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
            );
        }
    }
}

/// Converts an engine error into the message the UI shows.
pub fn error_text(error: &AppError) -> String {
    format::error_message(error)
}
