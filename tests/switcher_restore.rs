use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use skylight_sys::SkyLight;

fn hotkey_enabled() -> Option<bool> {
    Some(unsafe { SkyLight::load()?.CGSIsSymbolicHotKeyEnabled?(1) })
}

fn set_hotkeys_enabled(enabled: bool) {
    if let Some(set) = SkyLight::load().and_then(|sl| sl.CGSSetSymbolicHotKeyEnabled) {
        for key in [1, 2, 6] {
            unsafe { set(key, enabled) };
        }
    }
}

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Puts the hotkeys back the way the machine had them (another switcher may
/// hold the suppression), waiting out the watchdog's async restore first so
/// it can't trample us. Runs on panic paths too.
struct RestoreAsFound(bool);

impl Drop for RestoreAsFound {
    fn drop(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while hotkey_enabled() != Some(true) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        set_hotkeys_enabled(self.0);
    }
}

fn wait_for(state: bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while hotkey_enabled() != Some(state) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn native_switcher_restored_after_kill_9() {
    let Some(initially_enabled) = hotkey_enabled() else {
        eprintln!("skipping: no WindowServer");
        return;
    };
    let _restore = RestoreAsFound(initially_enabled);
    let mut child = KillOnDrop(
        Command::new(env!("CARGO_BIN_EXE_oriel"))
            .arg("--suppress-and-hang")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut ready = String::new();
    BufReader::new(child.0.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready.trim(), "engaged", "suppression failed to engage");
    if initially_enabled {
        wait_for(false, "suppression to take effect");
    }

    child.0.kill().unwrap();
    child.0.wait().unwrap();
    wait_for(true, "the watchdog to restore the native switcher");
}
