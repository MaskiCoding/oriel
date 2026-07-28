//! Hot-reload `~/.config/oriel/config.toml` via a directory watch.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::event::{EventKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

const DEBOUNCE: Duration = Duration::from_millis(250);

/// Watches the config file's parent directory and delivers successfully parsed
/// configs to `on_config`. Parse failures are printed and ignored so a bad
/// mid-edit save cannot take down the running app. Spawns a background thread;
/// the watch lives for the process lifetime.
pub fn spawn(path: PathBuf, on_config: impl Fn(config::Config) + Send + 'static) {
    std::thread::Builder::new()
        .name("oriel-config".into())
        .spawn(move || watch_loop(&path, on_config))
        .expect("spawn config watcher");
}

fn watch_loop(path: &Path, on_config: impl Fn(config::Config)) {
    let Some(dir) = path.parent() else {
        println!(
            "config: reload watch skipped — no parent for {}",
            path.display()
        );
        return;
    };
    let (tx, rx) = mpsc::channel();
    let mut watcher = match RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        notify::Config::default(),
    ) {
        Ok(w) => w,
        Err(err) => {
            println!("config: could not start reload watch: {err}");
            return;
        }
    };
    if let Err(err) = watcher.watch(dir, RecursiveMode::NonRecursive) {
        println!("config: could not watch {}: {err}", dir.display());
        return;
    }

    let mut deadline: Option<Instant> = None;
    loop {
        let timeout = deadline.map_or(Duration::from_secs(60), |d| {
            d.saturating_duration_since(Instant::now())
        });
        match rx.recv_timeout(timeout) {
            Ok(Ok(event)) => {
                if touches_config(&event, path) {
                    deadline = Some(Instant::now() + DEBOUNCE);
                }
            }
            Ok(Err(err)) => println!("config: watch error: {err}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if deadline.is_some_and(|d| Instant::now() >= d) {
                    deadline = None;
                    reload_once(path, &on_config);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // Keep the watcher alive until the channel closes.
    drop(watcher);
}

fn touches_config(event: &notify::Event, target: &Path) -> bool {
    let name = target.file_name();
    let hit = event.paths.iter().any(|p| {
        p == target
            || (name.is_some() && p.file_name() == name)
            || p.file_name().is_some_and(|n| n == "config.toml")
    });
    if !hit {
        return false;
    }
    matches!(
        event.kind,
        EventKind::Create(_)
            | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Name(_) | ModifyKind::Any)
            | EventKind::Remove(_)
            | EventKind::Any
    )
}

fn reload_once(path: &Path, on_config: &impl Fn(config::Config)) {
    match config::load(path) {
        Ok(cfg) => on_config(cfg),
        Err(err) => println!("config: reload ignored — {err}"),
    }
}
