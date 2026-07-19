fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some(flag) if flag == input::WATCHDOG_FLAG => {
            match args.next().and_then(|p| p.parse().ok()) {
                Some(parent) => input::watchdog_main(parent),
                None => std::process::exit(2),
            }
        }
        Some("--suppress-and-hang") if cfg!(debug_assertions) => {
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
        Some("--focus") if cfg!(debug_assertions) => {
            let wid: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
            let pid: i32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
            let ws = winsrv::WindowServer::connect().expect("windowserver");
            println!("focus {wid} pid {pid}: {}", ws.focus_window(pid, wid));
            return;
        }
        Some("--tap-log") if cfg!(debug_assertions) => {
            tap_log();
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

    let ws = match winsrv::WindowServer::connect() {
        Ok(ws) => ws,
        Err(missing) => {
            println!("skylight: unavailable — {}", missing.join(", "));
            return;
        }
    };
    let spaces = ws.spaces();
    let current = spaces
        .iter()
        .find(|s| s.current)
        .map(|s| format!(" (current {})", s.id))
        .unwrap_or_default();
    println!("spaces: {}{current}", spaces.len());

    let ids: Vec<u64> = spaces.iter().map(|s| s.id).collect();
    let windows = ws.windows(&ids);
    println!("windows: {}", windows.len());
    for w in &windows {
        println!(
            "  {:>6}  pid {:>6}  lvl {:>5}  tags {:#018x}  {:<24}  {}",
            w.wid,
            w.pid,
            w.level,
            w.tags,
            w.app.as_deref().unwrap_or("?"),
            w.title.as_deref().unwrap_or(""),
        );
    }
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
