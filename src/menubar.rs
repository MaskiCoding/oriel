//! Menu-bar status item (tray icon + menu).

use std::cell::RefCell;
use std::io::Cursor;
use std::sync::Once;

use dispatch2::DispatchQueue;
use objc2_foundation::{NSBundle, NSString};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCommand {
    Settings,
    TogglePause,
    Restart,
    Quit,
}

type CommandCb = Box<dyn FnMut(MenuCommand)>;

thread_local! {
    static ON_COMMAND: RefCell<Option<CommandCb>> = const { RefCell::new(None) };
}

fn install_event_bridge() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        MenuEvent::set_event_handler(Some(|event: MenuEvent| {
            let Some(cmd) = command_for(event.id().as_ref()) else {
                return;
            };
            DispatchQueue::main().exec_async(move || {
                ON_COMMAND.with(|slot| {
                    if let Some(cb) = slot.borrow_mut().as_mut() {
                        cb(cmd);
                    }
                });
            });
        }));
    });
}

fn command_for(id: &str) -> Option<MenuCommand> {
    match id {
        "settings" => Some(MenuCommand::Settings),
        "pause" => Some(MenuCommand::TogglePause),
        "restart" => Some(MenuCommand::Restart),
        "quit" => Some(MenuCommand::Quit),
        _ => None,
    }
}

pub struct MenuBar {
    _tray: TrayIcon,
    pause: CheckMenuItem,
}

impl MenuBar {
    /// Creates the status item. `on_command` fires on the main thread when an
    /// item is chosen. `None` if the item could not be created.
    pub fn new(on_command: impl FnMut(MenuCommand) + 'static) -> Option<Self> {
        install_event_bridge();
        ON_COMMAND.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(on_command));
        });

        let settings = MenuItem::with_id("settings", "Settings…", true, None);
        let pause = CheckMenuItem::with_id("pause", "Pause Oriel", true, false, None);
        let restart = MenuItem::with_id("restart", "Restart", true, None);
        let quit = MenuItem::with_id("quit", "Quit", true, None);

        let menu = Menu::new();
        menu.append(&settings).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&pause).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&restart).ok()?;
        menu.append(&quit).ok()?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Oriel")
            .with_icon(load_icon())
            .with_icon_as_template(true)
            .build()
            .ok()?;

        Some(Self { _tray: tray, pause })
    }

    /// Reflects paused state in the menu (checkmark).
    pub fn set_paused(&self, paused: bool) {
        self.pause.set_checked(paused);
    }
}

impl Drop for MenuBar {
    fn drop(&mut self) {
        ON_COMMAND.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
}

fn load_icon() -> Icon {
    if let Some(icon) = icon_from_bundle() {
        return icon;
    }
    fallback_icon()
}

fn icon_from_bundle() -> Option<Icon> {
    let bundle = NSBundle::mainBundle();
    for name in ["MenubarTemplate@2x", "MenubarTemplate"] {
        let Some(path) = bundle.pathForResource_ofType(
            Some(&NSString::from_str(name)),
            Some(&NSString::from_str("png")),
        ) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(path.to_string()) else {
            continue;
        };
        if let Some(icon) = decode_png(&bytes) {
            return Some(icon);
        }
    }
    None
}

fn decode_png(bytes: &[u8]) -> Option<Icon> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let width = info.width;
    let height = info.height;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => {
            let rgb = &buf[..info.buffer_size()];
            let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
            for chunk in rgb.chunks_exact(3) {
                rgba.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
            }
            rgba
        }
        png::ColorType::GrayscaleAlpha => {
            let ga = &buf[..info.buffer_size()];
            let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
            for chunk in ga.chunks_exact(2) {
                rgba.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
            }
            rgba
        }
        png::ColorType::Grayscale => {
            let g = &buf[..info.buffer_size()];
            let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
            for &v in g {
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
            rgba
        }
        png::ColorType::Indexed => return None,
    };
    Icon::from_rgba(rgba, width, height).ok()
}

fn fallback_icon() -> Icon {
    const SIZE: u32 = 18;
    let mut rgba = vec![0_u8; (SIZE * SIZE * 4) as usize];
    for px in rgba.chunks_exact_mut(4) {
        // Opaque black template glyph — harmless when the bundle asset is missing.
        px[3] = 255;
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("fallback icon")
}
