mod app;
mod lantern;
mod lens;
#[allow(dead_code)] // wired into the loop by a later task
mod login;
#[allow(dead_code)]
mod menubar;
mod snapshot;

fn main() {
    let mut args = std::env::args().skip(1);
    let first = args.next();
    if first.as_deref() == Some(input::WATCHDOG_FLAG) {
        match args.next().and_then(|p| p.parse().ok()) {
            Some(parent) => input::watchdog_main(parent),
            None => std::process::exit(2),
        }
    }
    #[cfg(debug_assertions)]
    if let Some(flag) = first.as_deref()
        && debug_command(flag, &mut args)
    {
        return;
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
    let cfg = lens::bootstrap_config();
    app::run(mtm, &cfg);
}

/// Development entry points. `true` when `flag` was one of them and handled.
#[cfg(debug_assertions)]
fn debug_command(flag: &str, args: &mut impl Iterator<Item = String>) -> bool {
    match flag {
        "--menubar-demo" => menubar_demo(),
        "--suppress-and-hang" => {
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
        "--focus" => {
            let wid: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
            let pid: i32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
            let ws = winsrv::WindowServer::connect().expect("windowserver");
            let fronted = ws.focus_window(pid, wid);
            let raised = ax::raise_window(pid, wid);
            println!("focus {wid} pid {pid}: fronted={fronted} raised={raised}");
        }
        "--activate" => {
            let pid: i32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
            let ok =
                objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
                    .map(|app| {
                        app.activateWithOptions(
                            objc2_app_kit::NSApplicationActivationOptions::ActivateAllWindows,
                        )
                    });
            println!("activate {pid}: {ok:?}");
        }
        "--focused-wid" => {
            let pid: i32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
            println!("{}", ax::focused_window(pid).unwrap_or(0));
        }
        "--capture-window" => {
            let wid: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
            let path = args.next().unwrap_or_else(|| "capture.png".to_string());
            capture_window(wid, &path);
        }
        "--lantern" => lantern_probe(),
        "--window-bits" => window_bits(),
        "--snapshot" => dump_snapshot(),
        "--tap-log" => tap_log(),
        "--strip-demo" => {
            let style = match args.next().as_deref() {
                Some("icons") => ui::Style::Icons,
                Some("list") => ui::Style::List,
                _ => ui::Style::Gallery,
            };
            let query = args.next();
            strip_demo(style, query.as_deref());
        }
        "--settings-demo" => settings_demo(&lens::bootstrap_config()),
        "--hotkey-log" => hotkey_log(),
        _ => return false,
    }
    true
}

/// Captures a single window to a PNG on disk — proof the private capture path
/// works, including for minimized and off-Space windows.
#[cfg(debug_assertions)]
fn capture_window(wid: u32, path: &str) {
    let Some(capturer) = capture::Capturer::new() else {
        println!("capture: unavailable (SkyLight or the capture symbol is missing)");
        return;
    };
    if !capture::permitted() {
        println!("capture: screen recording not granted — the frame will be black");
    }
    let Some(image) = capturer.window_image(wid) else {
        println!("capture: no image for window {wid} (zero size or gone?)");
        return;
    };
    let w = objc2_core_graphics::CGImage::width(Some(&image));
    let h = objc2_core_graphics::CGImage::height(Some(&image));
    if write_png(&image, path) {
        println!("capture: wrote {w}x{h} to {path}");
    } else {
        println!("capture: PNG encoding failed");
    }
}

#[cfg(debug_assertions)]
fn write_png(image: &objc2_core_graphics::CGImage, path: &str) -> bool {
    use objc2::AllocAnyThread;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep};
    use objc2_foundation::{NSDictionary, NSString};

    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), image);
    let properties = NSDictionary::new();
    let Some(data) = (unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
    }) else {
        return false;
    };
    data.writeToFile_atomically(&NSString::from_str(path), true)
}

/// Dumps every switchable window's raw tags/attributes and Space — the way the
/// state-marker bits were established (minimize a window, diff the output).
/// Reports what Lantern sees, through the exact path the app uses, so a window
/// that is not lighting up can be told apart from an agent that is not working.
#[cfg(debug_assertions)]
fn lantern_probe() {
    let binaries: Vec<String> = lantern::DEFAULT_BINARIES
        .iter()
        .map(|b| (*b).to_string())
        .collect();
    let roots = snapshot::app_pids();

    let sample = || {
        let mut table = proctable::table();
        let under = model::descendants(&table, &roots);
        proctable::detail(&mut table, &under);
        table
    };
    // Both samples must be detailed the same way, or the "before" CPU reads as
    // zero and every agent looks like it burned its whole lifetime in one tick.
    let before = sample();
    std::thread::sleep(lantern::WINDOW);
    let after = sample();

    println!("apps with windows : {}", roots.len());
    println!("window            : {:?}", lantern::WINDOW);
    println!("threshold         : {:?}", lantern::THRESHOLD);
    for proc in after.iter().filter(|p| binaries.contains(&p.name)) {
        let burn = model::burn(&before, &after, proc.pid, &binaries);
        let share = burn.as_secs_f64() / lantern::WINDOW.as_secs_f64() * 100.0;
        println!(
            "  pid {:>6} {:<14} {:>7.1} ms  ({share:>5.1}% of a core)  {}",
            proc.pid,
            proc.name,
            burn.as_secs_f64() * 1000.0,
            if burn > lantern::THRESHOLD {
                "WORKING"
            } else {
                "idle"
            }
        );
    }
}

#[cfg(debug_assertions)]
fn window_bits() {
    let ws = winsrv::WindowServer::connect().expect("windowserver");
    let spaces = ws.spaces();
    for s in &spaces {
        println!(
            "space {} current={} fullscreen={}",
            s.id, s.current, s.fullscreen
        );
    }
    let ids: Vec<u64> = spaces.iter().map(|s| s.id).collect();
    for w in ws.windows(&ids) {
        println!(
            "wid {:>6} pid {:>6} level {:>4} {:>5.0}x{:<5.0} space {:?} tags {:#018x} attrs {:#018x} {}",
            w.wid,
            w.pid,
            w.level,
            w.width,
            w.height,
            ws.window_space(w.wid),
            w.tags,
            w.attributes,
            w.app.unwrap_or_default()
        );
    }
}

/// Builds a full lens-resolver snapshot and prints one line per window.
#[cfg(debug_assertions)]
fn dump_snapshot() {
    let ws = winsrv::WindowServer::connect().expect("windowserver");
    let mut mru = model::Mru::default();
    let snap = snapshot::snapshot(&ws, &mut mru);
    let mut real = 0_usize;
    let mut windowless = 0_usize;
    for w in &snap.windows {
        if w.windowless {
            windowless += 1;
        } else {
            real += 1;
        }
        let flags = [
            ("visible", w.space_visible),
            ("fullscreen", w.fullscreen),
            ("minimized", w.state.minimized),
            ("hidden", w.state.hidden),
            ("main", w.is_main),
            ("windowless", w.windowless),
        ]
        .into_iter()
        .filter_map(|(name, on)| on.then_some(name))
        .collect::<Vec<_>>()
        .join(",");
        let meta = snap.meta.get(&w.id);
        let pid = meta.map_or(w.app, |m| m.pid);
        let aspect = meta.map_or(0.0, |m| m.aspect);
        let badge = meta.map_or("", |m| m.badge.as_str());
        println!(
            "id={} pid={pid} app={} screen={} space={} ordinal={} aspect={aspect:.2} flags={} badge={} title={}",
            w.id.0,
            w.app_name,
            w.screen,
            w.space.map_or_else(|| "-".into(), |s| s.to_string()),
            w.space_ordinal,
            if flags.is_empty() { "-" } else { &flags },
            if badge.is_empty() { "-" } else { badge },
            w.title,
        );
    }
    println!("real={real} windowless={windowless}");
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
fn strip_demo(style: ui::Style, query: Option<&str>) {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let mtm = objc2::MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // Real windows with live screenshots, so the demo shows the Gallery style.
    let ws = winsrv::WindowServer::connect().expect("windowserver");
    let capturer = capture::Capturer::new();
    let space_ids: Vec<u64> = ws.spaces().iter().map(|s| s.id).collect();
    let mut shown = ws.windows(&space_ids);
    shown.retain(|w| w.level == 0 && model::WindowState::decode(w.tags, w.attributes).switchable());
    shown.truncate(9);
    ws.fill_titles(&mut shown);
    // ORIEL_LANTERN=1 lights the first tile so the working-window look can be
    // judged without an agent actually running.
    let demo_lantern = std::env::var("ORIEL_LANTERN").is_ok();
    let tiles: Vec<ui::Tile> = shown
        .into_iter()
        .map(|w| {
            let state = model::WindowState::decode(w.tags, w.attributes);
            ui::Tile {
                preview: capturer.as_ref().and_then(|c| c.window_image(w.wid)),
                pid: w.pid,
                aspect: if w.height > 0.0 {
                    w.width / w.height
                } else {
                    1.6
                },
                badge: model::SpaceMap::new([]).badge(state.minimized, None),
                app: w.app.unwrap_or_default(),
                title: w.title.unwrap_or_default(),
                ..ui::Tile::default()
            }
        })
        .collect();
    let tiles: Vec<ui::Tile> = tiles
        .into_iter()
        .enumerate()
        .map(|(i, t)| ui::Tile {
            lantern: demo_lantern && i % 2 == 0,
            ..t
        })
        .collect();

    let strip = ui::Strip::new(mtm);
    strip.set_look(ui::Look {
        style,
        ..ui::Look::default()
    });
    strip.set_query(query);
    strip.show(&tiles, 1);
    app.run();
}

/// Opens the settings window over the live config, printing each edit it emits.
#[cfg(debug_assertions)]
fn settings_demo(cfg: &config::Config) {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let mtm = objc2::MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let settings = ui::Settings::new(mtm, cfg, |edited| {
        println!(
            "settings: delay={} theme={:?} lenses={}",
            edited.summon_delay_ms,
            edited.theme,
            edited.lenses.len()
        );
    });
    settings.show(mtm);
    app.run();
}

#[cfg(debug_assertions)]
fn menubar_demo() {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let mtm = objc2::MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    println!("login::enabled() = {}", login::enabled());

    let Some(menubar) = menubar::MenuBar::new(|cmd| {
        println!("{cmd:?}");
        if cmd == menubar::MenuCommand::Quit {
            std::process::exit(0);
        }
    }) else {
        println!("menubar-demo: failed to create status item");
        return;
    };
    menubar.set_paused(false);
    println!("menubar-demo: status item ready — Quit from the menu to exit");
    // Keep the status item alive for the run loop.
    std::mem::forget(menubar);
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
