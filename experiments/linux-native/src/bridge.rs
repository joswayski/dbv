//! Bridges the shared engine's async API onto the GTK main loop.

use std::future::Future;
use std::sync::OnceLock;

use dbm_engine::error::AppError;
use gtk4::glib;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("dbm-engine")
            .build()
            .expect("the DBM engine runtime must start")
    })
}

/// Runs an engine future on the tokio runtime and delivers its result back on
/// the GTK main thread. The callback therefore may touch widgets directly.
pub fn spawn<F, T>(future: F, on_done: impl FnOnce(Result<T, AppError>) + 'static)
where
    F: Future<Output = Result<T, AppError>> + Send + 'static,
    T: Send + 'static,
{
    let (sender, receiver) = tokio::sync::oneshot::channel();
    runtime().spawn(async move {
        let _ = sender.send(future.await);
    });
    glib::spawn_future_local(async move {
        if let Ok(result) = receiver.await {
            on_done(result);
        }
    });
}
