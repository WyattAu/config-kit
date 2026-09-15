//! Hot reload: debounced, typed file watching (feature `hot-reload`).

use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use notify::Watcher;
use serde::de::DeserializeOwned;

use crate::error::ConfigError;

/// Events must settle for this long before the file is re-parsed.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// Watch `path` and invoke `handler` with the freshly parsed value every
/// time the file settles after a change.
///
/// The handler is boxed into a `Box<dyn Fn(T) + Send + Sync>` and run on
/// the watcher's worker thread; keep it quick and non-blocking. Parsing
/// uses the same format detection as loading (TOML core, JSON/YAML behind
/// their features).
///
/// Failure handling is typed and panic-free:
///
/// - startup failures surface as [`ConfigError::WatchFailed`] from this
///   call;
/// - reload-time failures (file deleted mid-run, malformed content, type
///   mismatch) never panic and never stop the watcher — they are recorded
///   and retrievable via [`ConfigWatcher::last_error`]. The previous valid
///   configuration stays in effect; the handler simply is not invoked.
///
/// The watcher observes the file itself. Editors that replace files
/// atomically via rename can detach platform-specific watches; writing in
/// place (the common `File::create`/write pattern) is fully supported.
///
/// # Example
///
/// ```
/// use config_kit::watch;
/// use serde::Deserialize;
/// use std::sync::Arc;
/// use std::sync::atomic::AtomicBool;
/// use std::sync::atomic::Ordering;
///
/// #[derive(Deserialize)]
/// struct Settings {
///     enabled: bool,
/// }
///
/// # fn demo() -> Result<(), Box<dyn std::error::Error>> {
/// let dir = tempfile::tempdir()?;
/// let path = dir.path().join("watched.toml");
/// std::fs::write(&path, "enabled = false")?;
///
/// let fired = Arc::new(AtomicBool::new(false));
/// let flag = Arc::clone(&fired);
/// let watcher = watch::<Settings, _>(&path, move |_settings| {
///     flag.store(true, Ordering::SeqCst);
/// })?;
/// // After the file is edited and settles (300 ms debounce), the handler
/// // fires with the new value. `ConfigWatcher::last_error` reports any
/// // reload-time parse failures.
/// watcher.stop();
/// # Ok(())
/// # }
/// # demo().expect("watcher demo");
/// ```
///
/// # Errors
///
/// [`ConfigError::WatchFailed`] when the platform watcher cannot be
/// created, the path cannot be registered (typically: it does not exist),
/// or the worker thread cannot be spawned.
pub fn watch<T, F>(path: &Path, handler: F) -> Result<ConfigWatcher, ConfigError>
where
    T: DeserializeOwned + Send + Sync + 'static,
    F: Fn(T) + Send + Sync + 'static,
{
    let (sender, receiver) = mpsc::channel();
    let mut platform =
        notify::recommended_watcher(sender.clone()).map_err(|source| ConfigError::WatchFailed {
            source: Box::new(source),
        })?;
    platform
        .watch(path, notify::RecursiveMode::NonRecursive)
        .map_err(|source| ConfigError::WatchFailed {
            source: Box::new(source),
        })?;

    let stop = Arc::new(AtomicBool::new(false));
    let errors: Arc<Mutex<Option<Arc<ConfigError>>>> = Arc::new(Mutex::new(None));
    let worker = Worker {
        receiver,
        watched: path.to_path_buf(),
        handler: Box::new(handler),
        errors: Arc::clone(&errors),
        stop: Arc::clone(&stop),
    };
    let handle = std::thread::Builder::new()
        .name("config-kit-watch".into())
        .spawn(move || worker.run())
        .map_err(|source| ConfigError::WatchFailed {
            source: Box::new(source),
        })?;

    Ok(ConfigWatcher {
        platform: Some(platform),
        handle: Some(handle),
        stop,
        errors,
    })
}

/// Handle to a running watcher. Dropping it without calling
/// [`stop`](Self::stop) still signals the worker, which exits within one
/// debounce window; [`stop`](Self::stop) additionally joins the thread.
#[derive(Debug)]
pub struct ConfigWatcher {
    platform: Option<notify::RecommendedWatcher>,
    handle: Option<std::thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    errors: Arc<Mutex<Option<Arc<ConfigError>>>>,
}

impl ConfigWatcher {
    /// The most recent reload failure, if any, recorded since the watcher
    /// started. Returns [`ConfigError::Poisoned`] when the error slot's
    /// lock is itself poisoned.
    #[must_use]
    pub fn last_error(&self) -> Option<Arc<ConfigError>> {
        match self.errors.lock() {
            Ok(slot) => slot.clone(),
            Err(_) => Some(Arc::new(ConfigError::Poisoned)),
        }
    }

    /// Signals the worker to stop, unregisters the platform watch, and
    /// joins the worker thread. Returns within roughly one debounce
    /// window (300 ms) at worst.
    pub fn stop(mut self) {
        self.stop_flag(true);
        self.platform = None;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    fn stop_flag(&self, stopped: bool) {
        self.stop.store(stopped, Ordering::Relaxed);
    }
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        self.stop_flag(true);
    }
}

/// The watcher's worker: debounce, parse, dispatch.
struct Worker<T: DeserializeOwned + Send + Sync + 'static> {
    receiver: mpsc::Receiver<Result<notify::Event, notify::Error>>,
    watched: PathBuf,
    handler: Box<dyn Fn(T) + Send + Sync>,
    errors: Arc<Mutex<Option<Arc<ConfigError>>>>,
    stop: Arc<AtomicBool>,
}

impl<T: DeserializeOwned + Send + Sync + 'static> Worker<T> {
    fn run(mut self) {
        let mut pending = false;
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
            match self.receiver.recv_timeout(DEBOUNCE) {
                // A matching event or a watch-level error both mean the
                // file may have changed; either schedules a re-parse.
                Ok(Ok(event)) => {
                    if event.paths.iter().any(|path| path == &self.watched) {
                        pending = true;
                    }
                }
                Ok(Err(_)) => pending = true,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if pending {
                        pending = false;
                        self.parse_and_fire();
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Parses the watched file and invokes the handler on success; records
    /// any failure in the shared error slot instead of panicking.
    fn parse_and_fire(&mut self) {
        match crate::builder::read_layer_value(&self.watched) {
            Ok(value) => match T::deserialize(value) {
                Ok(parsed) => (self.handler)(parsed),
                Err(source) => self.record(ConfigError::Parse {
                    path: self.watched.clone(),
                    source: Box::new(source),
                }),
            },
            Err(error) => self.record(error),
        }
    }

    /// Records a reload failure. A poisoned slot is unrecoverable-by-design
    /// (see [`ConfigError::Poisoned`]); the event is dropped.
    fn record(&self, error: ConfigError) {
        if let Ok(mut slot) = self.errors.lock() {
            *slot = Some(Arc::new(error));
        }
    }
}
