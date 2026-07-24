use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::time::Duration;

use skylight_sys::SkyLight;

/// Symbolic hotkeys of the native switcher family: ⌘⇥, ⌘⇧⇥, ⌘-backtick.
const SWITCHER_HOTKEYS: [i32; 3] = [1, 2, 6];

pub const WATCHDOG_FLAG: &str = "--restore-watchdog";

type SetHotKeyEnabled = unsafe extern "C" fn(i32, bool) -> i32;

fn set_switcher_enabled(set: SetHotKeyEnabled, enabled: bool) {
    for key in SWITCHER_HOTKEYS {
        unsafe { set(key, enabled) };
    }
}

/// Native-switcher suppression, alive while this value is.
///
/// The disable persists at the OS level past our death, so restoration is
/// belt-and-braces: `Drop` for clean exits, plus a watchdog process that
/// restores when this process dies any other way (including SIGKILL). The
/// watchdog is spawned *before* the disable so no failure or death window
/// can leak a suppressed switcher.
pub struct Suppression {
    set: SetHotKeyEnabled,
    watchdog: Option<Child>,
}

impl Suppression {
    pub fn engage() -> Option<Self> {
        let set = SkyLight::load()?.CGSSetSymbolicHotKeyEnabled?;
        let exe = std::env::current_exe().ok()?;
        let watchdog = Command::new(exe)
            .arg(WATCHDOG_FLAG)
            .arg(std::process::id().to_string())
            // Own process group: a terminal's SIGINT/SIGHUP goes to the parent's
            // foreground group. Sharing it would kill the watchdog alongside us,
            // and signals don't run `Drop` — the switcher would stay suppressed.
            // Detached, the watchdog outlives a Ctrl+C'd parent and restores on
            // reparent. Restoration is the hard invariant (PRD §4.1, §8.4).
            .process_group(0)
            .spawn()
            .ok()?;
        set_switcher_enabled(set, false);
        Some(Self {
            set,
            watchdog: Some(watchdog),
        })
    }
}

impl Drop for Suppression {
    fn drop(&mut self) {
        set_switcher_enabled(self.set, true);
        if let Some(mut watchdog) = self.watchdog.take() {
            let _ = watchdog.kill();
            let _ = watchdog.wait();
        }
    }
}

/// Entry point for the watchdog process: waits for the parent to die (we get
/// re-parented), restores the native switcher, exits.
pub fn watchdog_main(parent: u32) -> ! {
    loop {
        if std::os::unix::process::parent_id() != parent {
            if let Some(set) = SkyLight::load().and_then(|sl| sl.CGSSetSymbolicHotKeyEnabled) {
                set_switcher_enabled(set, true);
            }
            std::process::exit(0);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
