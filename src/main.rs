mod app;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some(flag) if flag == input::WATCHDOG_FLAG => {
            match args.next().and_then(|p| p.parse().ok()) {
                Some(parent) => input::watchdog_main(parent),
                None => std::process::exit(2),
            }
        }
        #[cfg(debug_assertions)]
        Some("--suppress-and-hang") => {
            let suppression = input::Suppression::engage();
            println!(
                "{}",
                if suppression.is_some() {
                    "engaged"
                } else {
                    "engage-failed"
                }
            );
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
        #[cfg(debug_assertions)]
        Some("--focus") => {
            let wid: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
            let pid: i32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
            let ws = winsrv::WindowServer::connect().expect("windowserver");
            let fronted = ws.focus_window(pid, wid);
            let raised = ax::raise_window(pid, wid);
            println!("focus {wid} pid {pid}: fronted={fronted} raised={raised}");
            return;
        }
        #[cfg(debug_assertions)]
        Some("--focused-wid") => {
            let pid: i32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
            println!("{}", ax::focused_window(pid).unwrap_or(0));
            return;
        }
        #[cfg(debug_assertions)]
        Some("--tap-log") => {
            tap_log();
            return;
        }
        #[cfg(debug_assertions)]
        Some("--strip-demo") => {
            strip_demo();
            return;
        }
        #[cfg(debug_assertions)]
        Some("--hotkey-log") => {
            hotkey_log();
            return;
        }
        _ => {}
    }

    if !ax::trusted() {
        println!("accessibility: required — approve the system prompt, then relaunch");
        ax::request_trust();
        return;
    }
    if !capture::permitted() && !capture::request_permission() {
        println!("screen recording: not granted — previews and titles disabled");
    }

    let Some(mtm) = objc2::MainThreadMarker::new() else {
        println!("must run on the main thread");
        return;
    };
    app::run(mtm);
}

#[cfg(debug_assertions)]
fn hotkey_log() {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let mtm = objc2::MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let triggers = [
        (1, input::CMD),
        (2, input::CMD | input::SHIFT),
        (3, input::OPTION),
        (4, input::OPTION | input::SHIFT),
    ]
    .map(|(id, modifiers)| input::Trigger {
        id,
        key: input::KEY_TAB,
        modifiers,
    });

    let hotkeys = input::Hotkeys::register(&triggers, |id| println!("hotkey {id}"));
    if hotkeys.is_none() {
        println!("hotkey-log: registration failed");
        return;
    }
    println!("hotkey-log: registered — ctrl-c to stop");
    app.run();
}

#[cfg(debug_assertions)]
fn strip_demo() {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSWorkspace};

    let mtm = objc2::MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // Real running apps, so the demo shows real icons.
    let tiles: Vec<ui::Tile> = NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .filter(|a| a.activationPolicy() == NSApplicationActivationPolicy::Regular)
        .take(9)
        .map(|a| ui::Tile {
            app: a.localizedName().map(|n| n.to_string()).unwrap_or_default(),
            title: "window".to_string(),
            pid: a.processIdentifier(),
        })
        .collect();

    let strip = ui::Strip::new(mtm);
    strip.show(&tiles, 1);
    app.run();
}

#[cfg(debug_assertions)]
fn tap_log() {
    use objc2_core_foundation::CFRunLoop;
    use objc2_core_graphics::CGEventType;

    let mask = input::event_mask(&[CGEventType::KeyDown, CGEventType::FlagsChanged]);
    let tap = input::EventTap::install(
        objc2_core_graphics::CGEventTapOptions::ListenOnly,
        mask,
        |ty, event| {
            match ty {
                CGEventType::KeyDown => println!("keydown {}", input::keycode(event)),
                CGEventType::FlagsChanged => println!("flags {:#x}", input::flags(event).0),
                _ => {}
            }
            input::Disposition::Keep
        },
    );
    if tap.is_none() {
        println!("tap-log: install failed (accessibility not granted to this binary)");
        return;
    }
    println!("tap-log: listening — ctrl-c to stop");
    CFRunLoop::run();
}
