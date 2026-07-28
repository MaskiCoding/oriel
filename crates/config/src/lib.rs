//! TOML config: schema, defaults, parse + validate.

mod error;
mod types;

pub use error::ConfigError;
pub use types::{
    Apps, Controls, CursorFollowsFocus, Disposition, HideWindows, Keys, OnRelease, Order,
    PassTriggers, Peek, ResolvedLens, Rule, Screens, ShowOn, Size, Spaces, Style, Theme,
    to_model_rules,
};

use std::path::Path;

use types::{RawConfig, default_lenses, default_rules, resolve_lenses};

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Config {
    pub summon_delay_ms: u32,
    pub theme: Theme,
    pub show_on: ShowOn,
    pub background_capture: bool,
    pub start_at_login: bool,
    pub menubar_icon: bool,
    pub peek: Peek,
    pub controls: Controls,
    pub keys: Keys,
    pub lenses: Vec<ResolvedLens>,
    pub rules: Vec<Rule>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            summon_delay_ms: 0,
            theme: Theme::System,
            show_on: ShowOn::ActiveScreen,
            background_capture: true,
            start_at_login: true,
            menubar_icon: true,
            peek: Peek::default(),
            controls: Controls::default(),
            keys: Keys::default(),
            lenses: default_lenses(),
            rules: default_rules(),
        }
    }
}

pub fn parse(toml: &str) -> Result<Config, ConfigError> {
    let raw: RawConfig = toml::from_str(toml)?;
    let lenses = resolve_lenses(&raw.lenses)?;
    Ok(Config {
        summon_delay_ms: raw.summon_delay_ms,
        theme: raw.theme,
        show_on: raw.show_on,
        background_capture: raw.background_capture,
        start_at_login: raw.start_at_login,
        menubar_icon: raw.menubar_icon,
        peek: raw.peek,
        controls: raw.controls,
        keys: raw.keys,
        lenses,
        rules: raw.rules,
    })
}

pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::{default_lenses, default_rules, lens_field_defaults};

    const PRD_EXAMPLE: &str = r#"
summon_delay_ms = 0
theme = "system"
show_on = "active-screen"
background_capture = true
start_at_login = true
menubar_icon = true

[peek]
enabled = false

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
bundle_prefix = "com.parallels."
pass_triggers = "always"

[[rule]]
bundle_prefix = "com.apple.finder"
hide_windows = "windowless"
"#;

    #[test]
    fn parse_prd_example() {
        let cfg = parse(PRD_EXAMPLE).unwrap();
        assert_eq!(cfg.summon_delay_ms, 0);
        assert_eq!(cfg.theme, Theme::System);
        assert_eq!(cfg.show_on, ShowOn::ActiveScreen);
        assert!(cfg.background_capture);
        assert!(cfg.start_at_login);
        assert!(cfg.menubar_icon);
        assert!(!cfg.peek.enabled);
        assert!(cfg.controls.arrow_keys);
        assert!(!cfg.controls.vim_keys);
        assert_eq!(cfg.controls.cursor_follows_focus, CursorFollowsFocus::Never);
        assert_eq!(cfg.keys.focus, "return");
        assert_eq!(cfg.keys.hide_app, "h");

        assert_eq!(cfg.lenses.len(), 2);
        assert_eq!(cfg.lenses[0].trigger, "cmd+tab");
        assert_eq!(cfg.lenses[0].apps, Apps::All);
        assert_eq!(cfg.lenses[0].windowless_apps, Disposition::End);
        assert_eq!(cfg.lenses[0].style, Style::Gallery);
        assert_eq!(cfg.lenses[1].trigger, "alt+tab");
        assert_eq!(cfg.lenses[1].apps, Apps::Active);

        assert_eq!(cfg.rules.len(), 2);
        assert_eq!(cfg.rules[0].bundle_prefix, "com.parallels.");
        assert_eq!(cfg.rules[0].pass_triggers, Some(PassTriggers::Always));
        assert_eq!(cfg.rules[1].hide_windows, Some(HideWindows::Windowless));
    }

    #[test]
    fn default_matches_documented_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.summon_delay_ms, 0);
        assert_eq!(cfg.theme, Theme::System);
        assert_eq!(cfg.show_on, ShowOn::ActiveScreen);
        assert!(cfg.background_capture);
        assert!(cfg.start_at_login);
        assert!(cfg.menubar_icon);
        assert!(!cfg.peek.enabled);
        assert_eq!(cfg.controls, Controls::default());
        assert_eq!(cfg.keys, Keys::default());

        assert_eq!(cfg.lenses.len(), 2);
        assert_eq!(cfg.lenses[0].trigger, "cmd+tab");
        assert_eq!(cfg.lenses[0].apps, Apps::All);
        assert_eq!(cfg.lenses[1].trigger, "alt+tab");
        assert_eq!(cfg.lenses[1].apps, Apps::Active);
        assert_eq!(cfg.lenses[1].spaces, Spaces::All);
        assert_eq!(cfg.lenses[1].windowless_apps, Disposition::End);

        assert_eq!(cfg.rules, default_rules());
        assert_eq!(cfg, parse(PRD_EXAMPLE).unwrap());
    }

    #[test]
    fn second_lens_inherits_from_first() {
        let toml = r#"
[[lens]]
trigger = "cmd+tab"
apps = "all"
spaces = "visible"
screens = "strip-screen"
minimized = "end"
hidden = "hide"
fullscreen_windows = "end"
windowless_apps = "hide"
order = "alphabetical"
group_by_app = true
style = "list"
size = "small"
on_release = "linger"

[[lens]]
trigger = "alt+tab"
apps = "active"
"#;
        let cfg = parse(toml).unwrap();
        let second = &cfg.lenses[1];
        assert_eq!(second.trigger, "alt+tab");
        assert_eq!(second.apps, Apps::Active);
        assert_eq!(second.spaces, Spaces::Visible);
        assert_eq!(second.screens, Screens::StripScreen);
        assert_eq!(second.minimized, Disposition::End);
        assert_eq!(second.hidden, Disposition::Hide);
        assert_eq!(second.fullscreen_windows, Disposition::End);
        assert_eq!(second.windowless_apps, Disposition::Hide);
        assert_eq!(second.order, Order::Alphabetical);
        assert!(second.group_by_app);
        assert_eq!(second.style, Style::List);
        assert_eq!(second.size, Size::Small);
        assert_eq!(second.on_release, OnRelease::Linger);
    }

    #[test]
    fn first_lens_inherits_global_field_defaults() {
        let toml = r#"
[[lens]]
trigger = "cmd+tab"
"#;
        let cfg = parse(toml).unwrap();
        let expected = ResolvedLens {
            trigger: "cmd+tab".into(),
            ..lens_field_defaults()
        };
        assert_eq!(cfg.lenses, vec![expected]);
    }

    #[test]
    fn deny_unknown_fields_errors() {
        let err = parse("summon_delay_mss = 0\n").unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)));

        let err = parse(
            r#"
[[lens]]
trigger = "cmd+tab"
appz = "all"
"#,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)));
    }

    #[test]
    fn unknown_enum_value_errors() {
        let err = parse(
            r#"
[[lens]]
trigger = "cmd+tab"
order = "banana"
"#,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)));
        assert!(!format!("{err}").is_empty());
    }

    #[test]
    fn absent_lens_uses_builtin_defaults() {
        let cfg = parse(
            r#"
summon_delay_ms = 10
theme = "dark"
"#,
        )
        .unwrap();
        assert_eq!(cfg.summon_delay_ms, 10);
        assert_eq!(cfg.theme, Theme::Dark);
        assert_eq!(cfg.lenses, default_lenses());
    }

    #[test]
    fn empty_trigger_errors() {
        let err = parse(
            r#"
[[lens]]
trigger = ""
"#,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::EmptyTrigger));
    }

    #[test]
    fn broken_toml_errors() {
        let err = parse("summon_delay_ms = [").unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)));
    }

    #[test]
    fn to_model_lens_maps_every_variant() {
        let base = lens_field_defaults();

        let all = ResolvedLens {
            apps: Apps::All,
            spaces: Spaces::All,
            screens: Screens::All,
            minimized: Disposition::Show,
            hidden: Disposition::Show,
            fullscreen_windows: Disposition::Show,
            windowless_apps: Disposition::Show,
            order: Order::Recent,
            group_by_app: false,
            ..base.clone()
        };
        assert_eq!(
            all.to_model_lens(),
            model::Lens {
                apps: model::AppScope::All,
                spaces: model::SpaceScope::All,
                screens: model::ScreenScope::All,
                minimized: model::Disposition::Show,
                hidden: model::Disposition::Show,
                fullscreen: model::Disposition::Show,
                windowless: model::Disposition::Show,
                order: model::Order::RecentFocus,
                grouping: model::Grouping::Windows,
            }
        );

        let active = ResolvedLens {
            apps: Apps::Active,
            spaces: Spaces::Visible,
            screens: Screens::StripScreen,
            minimized: Disposition::End,
            hidden: Disposition::End,
            fullscreen_windows: Disposition::End,
            windowless_apps: Disposition::End,
            order: Order::Created,
            group_by_app: true,
            ..base.clone()
        };
        assert_eq!(
            active.to_model_lens(),
            model::Lens {
                apps: model::AppScope::ActiveOnly,
                spaces: model::SpaceScope::VisibleOnly,
                screens: model::ScreenScope::StripScreen,
                minimized: model::Disposition::ShowAtEnd,
                hidden: model::Disposition::ShowAtEnd,
                fullscreen: model::Disposition::ShowAtEnd,
                windowless: model::Disposition::ShowAtEnd,
                order: model::Order::Creation,
                grouping: model::Grouping::PerApp,
            }
        );

        let inactive = ResolvedLens {
            apps: Apps::Inactive,
            spaces: Spaces::Hidden,
            minimized: Disposition::Hide,
            hidden: Disposition::Hide,
            fullscreen_windows: Disposition::Hide,
            windowless_apps: Disposition::Hide,
            order: Order::Alphabetical,
            ..base.clone()
        };
        let ml = inactive.to_model_lens();
        assert_eq!(ml.apps, model::AppScope::ExceptActive);
        assert_eq!(ml.spaces, model::SpaceScope::NonVisibleOnly);
        assert_eq!(ml.minimized, model::Disposition::Hide);
        assert_eq!(ml.hidden, model::Disposition::Hide);
        assert_eq!(ml.fullscreen, model::Disposition::Hide);
        assert_eq!(ml.windowless, model::Disposition::Hide);
        assert_eq!(ml.order, model::Order::Alphabetical);

        let space = ResolvedLens {
            order: Order::Space,
            ..base
        };
        assert_eq!(space.to_model_lens().order, model::Order::SpaceOrder);
    }

    #[test]
    fn default_lens_one_matches_model_default() {
        let lens = lens_field_defaults().to_model_lens();
        assert_eq!(lens, model::Lens::default());
    }

    #[test]
    fn parse_title_contains_with_substrings() {
        let cfg = parse(
            r#"
[[rule]]
bundle_prefix = "com.notes."
hide_windows = "title-contains"
hide_title_substrings = ["secret", "draft"]
"#,
        )
        .unwrap();
        assert_eq!(cfg.rules.len(), 1);
        assert_eq!(cfg.rules[0].hide_windows, Some(HideWindows::TitleContains));
        assert_eq!(
            cfg.rules[0].hide_title_substrings,
            vec!["secret".to_string(), "draft".to_string()]
        );
    }

    #[test]
    fn to_model_rule_maps_every_variant() {
        let never = Rule {
            bundle_prefix: "com.a.".into(),
            pass_triggers: Some(PassTriggers::Never),
            hide_windows: Some(HideWindows::Never),
            hide_title_substrings: Vec::new(),
        };
        assert_eq!(
            never.to_model_rule(),
            model::Rule {
                bundle_prefix: "com.a.".into(),
                hide: model::HideWindows::Never,
                pass: model::PassTriggers::Never,
            }
        );

        let always = Rule {
            bundle_prefix: "com.b.".into(),
            pass_triggers: Some(PassTriggers::Always),
            hide_windows: Some(HideWindows::Always),
            hide_title_substrings: Vec::new(),
        };
        assert_eq!(
            always.to_model_rule(),
            model::Rule {
                bundle_prefix: "com.b.".into(),
                hide: model::HideWindows::Always,
                pass: model::PassTriggers::Always,
            }
        );

        let windowless = Rule {
            bundle_prefix: "com.c.".into(),
            pass_triggers: Some(PassTriggers::Fullscreen),
            hide_windows: Some(HideWindows::Windowless),
            hide_title_substrings: Vec::new(),
        };
        assert_eq!(
            windowless.to_model_rule(),
            model::Rule {
                bundle_prefix: "com.c.".into(),
                hide: model::HideWindows::Windowless,
                pass: model::PassTriggers::Fullscreen,
            }
        );

        let title = Rule {
            bundle_prefix: "com.d.".into(),
            pass_triggers: Some(PassTriggers::Never),
            hide_windows: Some(HideWindows::TitleContains),
            hide_title_substrings: vec!["secret".into(), "draft".into()],
        };
        assert_eq!(
            title.to_model_rule(),
            model::Rule {
                bundle_prefix: "com.d.".into(),
                hide: model::HideWindows::TitleContains(vec!["secret".into(), "draft".into()]),
                pass: model::PassTriggers::Never,
            }
        );
    }

    #[test]
    fn to_model_rule_none_defaults_to_never() {
        let rule = Rule {
            bundle_prefix: "com.app.".into(),
            pass_triggers: None,
            hide_windows: None,
            hide_title_substrings: Vec::new(),
        };
        assert_eq!(
            rule.to_model_rule(),
            model::Rule {
                bundle_prefix: "com.app.".into(),
                hide: model::HideWindows::Never,
                pass: model::PassTriggers::Never,
            }
        );
    }

    #[test]
    fn to_model_rules_builds_model_rules() {
        let rules = vec![Rule {
            bundle_prefix: "com.app.".into(),
            pass_triggers: Some(PassTriggers::Always),
            hide_windows: Some(HideWindows::Always),
            hide_title_substrings: Vec::new(),
        }];
        let model_rules = to_model_rules(&rules);
        assert!(model_rules.should_hide("com.app.foo", "", true));
        assert!(model_rules.passes_trigger("com.app.foo", false));
    }
}
