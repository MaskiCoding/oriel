//! Config lenses → runtime bindings (hot keys, hold mask, look, release action).

use objc2_core_graphics::CGEventFlags;

/// Default `~/.config/oriel/config.toml` written on first launch.
pub const DEFAULT_TOML: &str = r#"summon_delay_ms = 0
theme = "system"
show_on = "active-screen"
background_capture = true
start_at_login = true
menubar_icon = true

[peek]
enabled = false

[titles]
show = "title"
truncate = "end"
markers = true

[controls]
arrow_keys = true
vim_keys = false
hover_select = false
cursor_follows_focus = "never"

[keys]
focus = "return"
cancel = "escape"
close = "w"
minimize = "m"
fullscreen = "f"
quit_app = "q"
hide_app = "h"

[[lens]]
trigger = "cmd+tab"
apps = "all"
spaces = "all"
screens = "all"
minimized = "show"
hidden = "show"
fullscreen_windows = "show"
windowless_apps = "end"
order = "recent"
group_by_app = false
style = "gallery"
size = "auto"
on_release = "jump"

[[lens]]
trigger = "alt+tab"
apps = "active"

[[rule]]
bundle_prefix = "com.apple.finder"
hide_windows = "windowless"

# Capture-everything remote-desktop and VM clients: hand them the trigger while
# they are fullscreen, so the guest OS sees its own switcher shortcut.
[[rule]]
bundle_prefix = "com.apple.ScreenSharing"
pass_triggers = "fullscreen"

[[rule]]
bundle_prefix = "com.microsoft.rdc."
pass_triggers = "fullscreen"

[[rule]]
bundle_prefix = "com.teamviewer."
pass_triggers = "fullscreen"

[[rule]]
bundle_prefix = "org.virtualbox."
pass_triggers = "fullscreen"

[[rule]]
bundle_prefix = "com.parallels."
pass_triggers = "fullscreen"

[[rule]]
bundle_prefix = "com.citrix."
pass_triggers = "fullscreen"

[[rule]]
bundle_prefix = "com.vmware.fusion"
pass_triggers = "fullscreen"

[[rule]]
bundle_prefix = "com.utmapp."
pass_triggers = "fullscreen"
"#;

/// One configured lens ready for the live loop.
#[derive(Clone)]
pub struct Binding {
    pub model: model::Lens,
    pub look: ui::Look,
    pub on_release: config::OnRelease,
    /// Non-shift modifiers that must stay held; empty means Linger-style open.
    pub hold: CGEventFlags,
    pub key: u32,
    pub modifiers: u32,
}

/// Hot-key ids: forward = `index * 2 + 1`, backward = `index * 2 + 2`.
pub fn hotkey_id(index: usize, backward: bool) -> u32 {
    let base = u32::try_from(index)
        .unwrap_or(u32::MAX)
        .saturating_mul(2)
        .saturating_add(1);
    if backward {
        base.saturating_add(1)
    } else {
        base
    }
}

pub fn decode_hotkey_id(id: u32) -> (usize, bool) {
    let index = ((id.saturating_sub(1)) / 2) as usize;
    let backward = id.is_multiple_of(2);
    (index, backward)
}

pub fn hold_flags(carbon_mods: u32) -> CGEventFlags {
    let mut flags = CGEventFlags::empty();
    if carbon_mods & input::CMD != 0 {
        flags.insert(CGEventFlags::MaskCommand);
    }
    if carbon_mods & input::OPTION != 0 {
        flags.insert(CGEventFlags::MaskAlternate);
    }
    if carbon_mods & input::CONTROL != 0 {
        flags.insert(CGEventFlags::MaskControl);
    }
    flags
}

pub fn look_of(lens: &config::ResolvedLens, cfg: &config::Config) -> ui::Look {
    ui::Look {
        style: match lens.style {
            config::Style::Gallery => ui::Style::Gallery,
            config::Style::Icons => ui::Style::Icons,
            config::Style::List => ui::Style::List,
        },
        size: match lens.size {
            config::Size::Small => ui::Size::Small,
            config::Size::Medium => ui::Size::Medium,
            config::Size::Large => ui::Size::Large,
            config::Size::Auto => ui::Size::Auto,
        },
        show_on: match cfg.show_on {
            config::ShowOn::ActiveScreen => ui::ShowOn::ActiveScreen,
            config::ShowOn::PointerScreen => ui::ShowOn::PointerScreen,
            config::ShowOn::MenubarScreen => ui::ShowOn::MenubarScreen,
        },
        theme: match cfg.theme {
            config::Theme::System => ui::Theme::System,
            config::Theme::Light => ui::Theme::Light,
            config::Theme::Dark => ui::Theme::Dark,
        },
        title_show: match cfg.titles.show {
            config::TitleShow::Title => ui::TitleShow::Title,
            config::TitleShow::App => ui::TitleShow::App,
            config::TitleShow::Both => ui::TitleShow::Both,
        },
        title_truncate: match cfg.titles.truncate {
            config::TitleTruncate::Start => ui::TitleTruncate::Start,
            config::TitleTruncate::Middle => ui::TitleTruncate::Middle,
            config::TitleTruncate::End => ui::TitleTruncate::End,
        },
        markers: cfg.titles.markers,
    }
}

/// Build bindings from config; skips lenses whose trigger fails to parse.
pub fn bindings_from_config(cfg: &config::Config) -> Vec<Binding> {
    let mut out = Vec::new();
    for (i, lens) in cfg.lenses.iter().enumerate() {
        let Some(parsed) = input::parse_trigger(&lens.trigger) else {
            println!(
                "config: skipping lens {i} — unparseable trigger {:?}",
                lens.trigger
            );
            continue;
        };
        let hold = hold_flags(parsed.modifiers);
        let on_release = if hold.is_empty() {
            config::OnRelease::Linger
        } else {
            lens.on_release
        };
        out.push(Binding {
            model: lens.to_model_lens(),
            look: look_of(lens, cfg),
            on_release,
            hold,
            key: parsed.key,
            modifiers: parsed.modifiers,
        });
    }
    out
}

pub fn triggers_for(bindings: &[Binding]) -> Vec<input::Trigger> {
    let mut triggers = Vec::with_capacity(bindings.len() * 2);
    let mut claimed: Vec<(u32, u32)> = Vec::new();
    let mut claim = |key: u32, modifiers: u32| {
        let combo = (key, modifiers);
        if claimed.contains(&combo) {
            return false;
        }
        claimed.push(combo);
        true
    };
    for (i, b) in bindings.iter().enumerate() {
        if claim(b.key, b.modifiers) {
            triggers.push(input::Trigger {
                id: hotkey_id(i, false),
                key: b.key,
                modifiers: b.modifiers,
            });
        }
        // Shift is the reverse key; skip when the trigger already includes it,
        // and never claim a combo an earlier lens already took — Carbon
        // rejects a duplicate registration.
        if b.modifiers & input::SHIFT == 0 && claim(b.key, b.modifiers | input::SHIFT) {
            triggers.push(input::Trigger {
                id: hotkey_id(i, true),
                key: b.key,
                modifiers: b.modifiers | input::SHIFT,
            });
        }
    }
    triggers
}

pub fn stays_open(on_release: config::OnRelease) -> bool {
    matches!(
        on_release,
        config::OnRelease::Linger | config::OnRelease::Filter
    )
}

/// Load `~/.config/oriel/config.toml`, writing defaults if missing.
pub fn bootstrap_config() -> config::Config {
    let Some(home) = std::env::var_os("HOME") else {
        println!("config: HOME unset — using defaults");
        return config::Config::default();
    };
    let dir = std::path::PathBuf::from(home).join(".config/oriel");
    let path = dir.join("config.toml");
    if path.exists() {
        match config::load(&path) {
            Ok(cfg) => cfg,
            Err(err) => {
                println!("config: {err} — using defaults");
                config::Config::default()
            }
        }
    } else {
        let cfg = config::Config::default();
        if let Err(err) = std::fs::create_dir_all(&dir) {
            println!("config: could not create {}: {err}", dir.display());
        } else if let Err(err) = std::fs::write(&path, DEFAULT_TOML) {
            println!("config: could not write {}: {err}", path.display());
        }
        cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file written on first run must mean exactly what the built-in
    /// defaults mean, or a fresh install behaves differently from a bare one.
    #[test]
    fn default_toml_parses_to_the_default_config() {
        assert_eq!(
            config::parse(DEFAULT_TOML).unwrap(),
            config::Config::default()
        );
    }

    #[test]
    fn hotkey_ids_round_trip() {
        for index in 0..8 {
            for backward in [false, true] {
                assert_eq!(
                    decode_hotkey_id(hotkey_id(index, backward)),
                    (index, backward)
                );
            }
        }
    }

    #[test]
    fn hold_flags_maps_each_modifier_and_ignores_shift() {
        // Equality, not `contains`: an implementation that set every flag
        // would satisfy `contains` and then hold on any modifier at all.
        assert_eq!(hold_flags(input::CMD), CGEventFlags::MaskCommand);
        assert_eq!(hold_flags(input::OPTION), CGEventFlags::MaskAlternate);
        assert_eq!(hold_flags(input::CONTROL), CGEventFlags::MaskControl);
        assert_eq!(hold_flags(input::SHIFT), CGEventFlags::empty());
        assert_eq!(hold_flags(0), CGEventFlags::empty());
        assert_eq!(
            hold_flags(input::CMD | input::SHIFT),
            CGEventFlags::MaskCommand
        );
        assert_eq!(
            hold_flags(input::CMD | input::OPTION),
            CGEventFlags::MaskCommand | CGEventFlags::MaskAlternate
        );
    }

    fn binding(key: u32, modifiers: u32) -> Binding {
        Binding {
            model: model::Lens::default(),
            look: ui::Look::default(),
            on_release: config::OnRelease::Jump,
            hold: hold_flags(modifiers),
            key,
            modifiers,
        }
    }

    #[test]
    fn duplicate_triggers_are_claimed_once() {
        let bindings = [binding(48, input::CMD), binding(48, input::CMD)];
        let triggers = triggers_for(&bindings);
        let combos: Vec<(u32, u32)> = triggers.iter().map(|t| (t.key, t.modifiers)).collect();
        // The exact set, not just "no duplicates" — an empty result would
        // satisfy a uniqueness-only assertion while registering nothing.
        assert_eq!(
            combos,
            vec![(48, input::CMD), (48, input::CMD | input::SHIFT)]
        );
        // The second lens contributed nothing, so only lens 0's ids appear.
        let ids: Vec<u32> = triggers.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![hotkey_id(0, false), hotkey_id(0, true)]);
    }

    #[test]
    fn distinct_triggers_each_get_a_forward_and_reverse() {
        let bindings = [binding(48, input::CMD), binding(48, input::OPTION)];
        let triggers = triggers_for(&bindings);
        let combos: Vec<(u32, u32)> = triggers.iter().map(|t| (t.key, t.modifiers)).collect();
        assert_eq!(
            combos,
            vec![
                (48, input::CMD),
                (48, input::CMD | input::SHIFT),
                (48, input::OPTION),
                (48, input::OPTION | input::SHIFT),
            ]
        );
    }

    #[test]
    fn a_shift_trigger_does_not_claim_a_reverse_variant() {
        let bindings = [binding(48, input::CMD | input::SHIFT)];
        assert_eq!(triggers_for(&bindings).len(), 1);
    }
}
