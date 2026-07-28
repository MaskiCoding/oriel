use std::path::Path;

use serde::Serialize;

use crate::Config;
use crate::error::ConfigError;
use crate::types::{Controls, Keys, Peek, ResolvedLens, Rule, ShowOn, Theme};

#[derive(Serialize)]
struct OutConfig<'a> {
    summon_delay_ms: u32,
    theme: Theme,
    show_on: ShowOn,
    background_capture: bool,
    start_at_login: bool,
    menubar_icon: bool,
    peek: &'a Peek,
    controls: &'a Controls,
    keys: &'a Keys,
    #[serde(rename = "lens")]
    lenses: &'a [ResolvedLens],
    #[serde(rename = "rule")]
    rules: &'a [Rule],
}

/// Renders `config` as TOML, round-trippable through `parse`.
pub fn to_toml(config: &Config) -> String {
    let out = OutConfig {
        summon_delay_ms: config.summon_delay_ms,
        theme: config.theme,
        show_on: config.show_on,
        background_capture: config.background_capture,
        start_at_login: config.start_at_login,
        menubar_icon: config.menubar_icon,
        peek: &config.peek,
        controls: &config.controls,
        keys: &config.keys,
        lenses: &config.lenses,
        rules: &config.rules,
    };
    toml::to_string_pretty(&out).expect("config serialization is infallible")
}

/// Writes `config` to `path` atomically (temp file in the same directory, then rename).
pub fn save(config: &Config, path: &Path) -> Result<(), ConfigError> {
    let text = to_toml(config);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
        path: parent.to_path_buf(),
        source,
    })?;

    let tmp_name = {
        let mut name = path.file_name().map_or_else(
            || std::ffi::OsString::from("config.toml"),
            std::ffi::OsStr::to_owned,
        );
        name.push(".tmp");
        name
    };
    let tmp_path = parent.join(tmp_name);

    let write_err = |source| ConfigError::Io {
        path: tmp_path.clone(),
        source,
    };
    std::fs::write(&tmp_path, text.as_bytes()).map_err(write_err)?;
    std::fs::rename(&tmp_path, path).map_err(|source| {
        let _ = std::fs::remove_file(&tmp_path);
        ConfigError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use crate::types::{
        Apps, CursorFollowsFocus, Disposition, HideWindows, OnRelease, Order, PassTriggers,
        Screens, Size, Spaces, Style, lens_field_defaults,
    };

    fn assert_round_trip(config: &Config) {
        let text = to_toml(config);
        let parsed = parse(&text).unwrap();
        assert_eq!(&parsed, config, "round-trip mismatch for:\n{text}");
    }

    #[test]
    fn round_trip_default() {
        assert_round_trip(&Config::default());
    }

    #[test]
    fn round_trip_empty_rules() {
        let mut config = Config::default();
        config.rules.clear();
        assert_round_trip(&config);
    }

    #[test]
    fn round_trip_multiple_lenses() {
        let base = lens_field_defaults();
        let config = Config {
            summon_delay_ms: 150,
            theme: Theme::Dark,
            show_on: ShowOn::PointerScreen,
            background_capture: false,
            start_at_login: false,
            menubar_icon: false,
            peek: Peek { enabled: true },
            controls: Controls {
                arrow_keys: false,
                vim_keys: true,
                hover_select: true,
                cursor_follows_focus: CursorFollowsFocus::Always,
            },
            keys: Keys::default(),
            lenses: vec![
                ResolvedLens {
                    trigger: "cmd+tab".into(),
                    apps: Apps::All,
                    spaces: Spaces::Visible,
                    screens: Screens::StripScreen,
                    minimized: Disposition::End,
                    hidden: Disposition::Hide,
                    fullscreen_windows: Disposition::Show,
                    windowless_apps: Disposition::End,
                    order: Order::Alphabetical,
                    group_by_app: true,
                    style: Style::List,
                    size: Size::Small,
                    on_release: OnRelease::Linger,
                },
                ResolvedLens {
                    trigger: "alt+tab".into(),
                    apps: Apps::Active,
                    spaces: Spaces::Hidden,
                    screens: Screens::All,
                    minimized: Disposition::Show,
                    hidden: Disposition::End,
                    fullscreen_windows: Disposition::Hide,
                    windowless_apps: Disposition::Hide,
                    order: Order::Created,
                    group_by_app: false,
                    style: Style::Icons,
                    size: Size::Large,
                    on_release: OnRelease::Filter,
                },
                ResolvedLens {
                    trigger: "ctrl+tab".into(),
                    apps: Apps::Inactive,
                    ..base
                },
            ],
            rules: vec![Rule {
                bundle_prefix: "com.example.".into(),
                pass_triggers: Some(PassTriggers::Fullscreen),
                hide_windows: Some(HideWindows::TitleContains),
                hide_title_substrings: vec!["secret".into()],
            }],
        };
        assert_round_trip(&config);
    }

    #[test]
    fn round_trip_every_enum_variant() {
        for theme in [Theme::System, Theme::Light, Theme::Dark] {
            for show_on in [
                ShowOn::ActiveScreen,
                ShowOn::PointerScreen,
                ShowOn::MenubarScreen,
            ] {
                for cursor in [
                    CursorFollowsFocus::Never,
                    CursorFollowsFocus::Always,
                    CursorFollowsFocus::OtherScreen,
                ] {
                    assert_round_trip(&exhaustive_config(theme, show_on, cursor));
                }
            }
        }
    }

    fn exhaustive_config(theme: Theme, show_on: ShowOn, cursor: CursorFollowsFocus) -> Config {
        Config {
            summon_delay_ms: 900,
            theme,
            show_on,
            background_capture: true,
            start_at_login: true,
            menubar_icon: true,
            peek: Peek { enabled: false },
            controls: Controls {
                arrow_keys: true,
                vim_keys: false,
                hover_select: false,
                cursor_follows_focus: cursor,
            },
            keys: Keys::default(),
            lenses: exhaustive_lenses(),
            rules: exhaustive_rules(),
        }
    }

    fn exhaustive_lenses() -> Vec<ResolvedLens> {
        vec![
            ResolvedLens {
                trigger: "cmd+1".into(),
                apps: Apps::All,
                spaces: Spaces::All,
                screens: Screens::All,
                minimized: Disposition::Show,
                hidden: Disposition::Show,
                fullscreen_windows: Disposition::Show,
                windowless_apps: Disposition::Show,
                order: Order::Recent,
                group_by_app: false,
                style: Style::Gallery,
                size: Size::Auto,
                on_release: OnRelease::Jump,
            },
            ResolvedLens {
                trigger: "cmd+2".into(),
                apps: Apps::Active,
                spaces: Spaces::Visible,
                screens: Screens::StripScreen,
                minimized: Disposition::End,
                hidden: Disposition::End,
                fullscreen_windows: Disposition::End,
                windowless_apps: Disposition::End,
                order: Order::Created,
                group_by_app: true,
                style: Style::Icons,
                size: Size::Medium,
                on_release: OnRelease::Linger,
            },
            ResolvedLens {
                trigger: "cmd+3".into(),
                apps: Apps::Inactive,
                spaces: Spaces::Hidden,
                screens: Screens::All,
                minimized: Disposition::Hide,
                hidden: Disposition::Hide,
                fullscreen_windows: Disposition::Hide,
                windowless_apps: Disposition::Hide,
                order: Order::Space,
                group_by_app: false,
                style: Style::List,
                size: Size::Large,
                on_release: OnRelease::Filter,
            },
        ]
    }

    fn exhaustive_rules() -> Vec<Rule> {
        vec![
            Rule {
                bundle_prefix: "com.a.".into(),
                pass_triggers: Some(PassTriggers::Never),
                hide_windows: Some(HideWindows::Never),
                hide_title_substrings: Vec::new(),
            },
            Rule {
                bundle_prefix: "com.b.".into(),
                pass_triggers: Some(PassTriggers::Always),
                hide_windows: Some(HideWindows::Always),
                hide_title_substrings: Vec::new(),
            },
            Rule {
                bundle_prefix: "com.c.".into(),
                pass_triggers: Some(PassTriggers::Fullscreen),
                hide_windows: Some(HideWindows::Windowless),
                hide_title_substrings: Vec::new(),
            },
            Rule {
                bundle_prefix: "com.d.".into(),
                pass_triggers: None,
                hide_windows: Some(HideWindows::TitleContains),
                hide_title_substrings: vec!["x".into(), "y".into()],
            },
        ]
    }

    #[test]
    fn save_round_trips_through_load() {
        let dir = std::env::temp_dir().join(format!("oriel-config-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("config.toml");
        let config = Config::default();
        save(&config, &path).unwrap();
        let loaded = crate::load(&path).unwrap();
        assert_eq!(loaded, config);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
