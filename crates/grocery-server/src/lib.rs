//! The grocery web service.
//!
//! Keeps the recipe index in SQLite up to date with the `.cook` files (a
//! rescan at startup, then on every change under `Recipes/`), stores
//! shopping lists, and serves both as JSON plus Markdown/plain-text exports.
//! See [`api`] for the routes. Changes are pushed to open clients as
//! server-sent events ([`Event`]), and [`web`] serves the web app.
//!
//! The service has no authentication: bind it to localhost or a private
//! network (Tailscale, WireGuard), or put an authenticating proxy in front.

pub mod api;
mod plans;
pub mod web;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, Result};
use grocery_core::library::Library;
use grocery_store::{Store, SyncReport};
use notify_debouncer_mini::notify::RecursiveMode;
use serde::Serialize;
use tokio::sync::broadcast;

/// Something changed; clients showing it should fetch it again.
///
/// Events carry no data on purpose: refetching is simple, and it's always
/// right, even for a client that missed events while it was offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Event {
    /// A list was created, changed or deleted.
    List { id: i64 },
    /// The recipe index changed.
    Recipes,
    /// Someone's meal plan changed.
    Plan { person: i64 },
    /// People were added, renamed or removed.
    People,
}

pub struct App {
    pub library: Library,
    store: Mutex<Store>,
    events: broadcast::Sender<Event>,
}

impl App {
    pub fn new(library: Library, store: Store) -> Arc<Self> {
        // Subscribers that fall this far behind get a "resync" instead.
        let (events, _) = broadcast::channel(64);
        Arc::new(Self {
            library,
            store: Mutex::new(store),
            events,
        })
    }

    pub fn notify(&self, event: Event) {
        // An error only means nobody is listening right now.
        let _ = self.events.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    /// The store, for one short operation. Blocking: call from
    /// `spawn_blocking` or a non-async thread.
    pub fn store(&self) -> MutexGuard<'_, Store> {
        // A panic mid-operation rolls back its transaction, so the
        // connection is still consistent; keep serving.
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Re-index `Recipes/`; with `force`, re-parse every file.
    pub fn sync(&self, force: bool) -> grocery_store::Result<SyncReport> {
        let report = self.store().sync_recipes(&self.library, force)?;
        if report.changed() {
            self.notify(Event::Recipes);
        }
        Ok(report)
    }
}

pub struct Config {
    pub library: PathBuf,
    pub db: PathBuf,
    pub listen: SocketAddr,
}

/// Run the service until Ctrl-C or SIGTERM. Builds its own Tokio runtime so
/// the CLI can stay synchronous.
pub fn run(config: Config) -> Result<()> {
    tokio::runtime::Runtime::new()
        .context("starting the async runtime")?
        .block_on(serve(config))
}

async fn serve(config: Config) -> Result<()> {
    let library = Library::new(&config.library);
    let recipes = library.recipes_dir();
    std::fs::create_dir_all(&recipes).with_context(|| format!("creating {}", recipes.display()))?;
    let store =
        Store::open(&config.db).with_context(|| format!("opening {}", config.db.display()))?;
    let app = App::new(library, store);

    let report = app.sync(false).context("indexing recipes")?;
    eprintln!(
        "indexed {}: {} recipes ({} new, {} changed, {} removed, {} with errors)",
        recipes.display(),
        report.added + report.updated + report.unchanged,
        report.added,
        report.updated,
        report.removed,
        report.failed
    );
    // Keep the watcher alive for as long as we serve.
    let _watcher = watch(app.clone())?;

    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("binding {}", config.listen))?;
    eprintln!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, api::router(app))
        .with_graceful_shutdown(shutdown())
        .await
        .context("serving")?;
    eprintln!("stopped");
    Ok(())
}

type Watcher = notify_debouncer_mini::Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>;

/// Rescan `Recipes/` after changes settle. Runs on the debouncer's own thread.
///
/// Any event triggers a rescan rather than handling each path: a save is
/// often write-temp-then-rename, and the rescan only re-parses files whose
/// mtime or size changed, so this is both simpler and cheap.
fn watch(app: Arc<App>) -> Result<Watcher> {
    let dir = app.library.recipes_dir();
    let handler_app = app.clone();
    let mut debouncer = notify_debouncer_mini::new_debouncer(
        Duration::from_millis(500),
        move |events: notify_debouncer_mini::DebounceEventResult| {
            if let Err(e) = events {
                eprintln!("watch error: {e}");
            }
            // Rescan on errors too: an overflowed event queue means we
            // missed something.
            match handler_app.sync(false) {
                Ok(r) if r.changed() => eprintln!(
                    "reindexed: {} new, {} changed, {} removed, {} with errors",
                    r.added, r.updated, r.removed, r.failed
                ),
                Ok(_) => {}
                Err(e) => eprintln!("reindex failed: {e}"),
            }
        },
    )
    .context("starting the file watcher")?;
    debouncer
        .watcher()
        .watch(&dir, RecursiveMode::Recursive)
        .with_context(|| format!("watching {}", dir.display()))?;
    Ok(debouncer)
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
}
