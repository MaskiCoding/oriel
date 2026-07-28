//! TOML config: schema, defaults, parse + validate + write.

mod error;
mod types;
mod write;

pub use error::ConfigError;
pub use types::{
    Animation, Apps, Controls, CursorFollowsFocus, Disposition, HideWindows, Keys, OnRelease,
    Order, PassTriggers, Peek, ResolvedLens, Rule, Screens, ShowOn, Size, Spaces, Style, Theme,
    TitleShow, TitleTruncate, Titles, to_model_rules,
};
pub use write::{save, to_toml};

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
    pub animation: Animation,
    pub titles: Titles,
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
            animation: Animation::default(),
            titles: Titles::default(),
            controls: Controls::default(),
            keys: Keys::default(),
            lenses: default_lenses(),
            rules: default_rules(),
        }
    }
}

pub fn parse(toml: &str) -> Result<Config, ConfigError> {
    let mut raw: RawConfig = toml::from_str(toml)?;
    let lenses = resolve_lenses(&raw.lenses)?;
    types::validate_rules(&mut raw.rules)?;
    Ok(Config {
        summon_delay_ms: raw.summon_delay_ms.min(types::MAX_SUMMON_DELAY_MS),
        theme: raw.theme,
        show_on: raw.show_on,
        background_capture: raw.background_capture,
        start_at_login: raw.start_at_login,
        menubar_icon: raw.menubar_icon,
        peek: raw.peek,
        animation: raw.animation,
        titles: raw.titles,
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
        assert_eq!(cfg.titles, Titles::default());
        assert_eq!(cfg.titles.show, TitleShow::Title);
        assert_eq!(cfg.titles.truncate, TitleTruncate::End);
        assert!(cfg.titles.markers);
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
        assert_eq!(cfg.titles, Titles::default());
        assert_eq!(cfg.controls, Controls::default());
        assert_eq!(cfg.keys, Keys::default());

        assert_eq!(cfg.lenses.len(), 2);
        assert_eq!(cfg.lenses[0].trigger, "cmd+tab");
        assert_eq!(cfg.lenses[0].apps, Apps::All);
        assert_eq!(cfg.lenses[1].trigger, "alt+tab");
        assert_eq!(cfg.lenses[1].apps, Apps::Active);
        assert_eq!(cfg.lenses[1].spaces, Spaces::All);
        assert_eq!(cfg.lenses[1].windowless_apps, Disposition::End);

        // PRD §4.7 shipped rules: Finder hidden when windowless, plus
        // pass-through-when-fullscreen for every remote/VM client named there.
        assert_eq!(cfg.rules, default_rules());
        assert_eq!(cfg.rules[0].bundle_prefix, "com.apple.finder");
        assert_eq!(cfg.rules[0].hide_windows, Some(HideWindows::Windowless));
        for prefix in [
            "com.apple.ScreenSharing",
            "com.microsoft.rdc.",
            "com.teamviewer.",
            "org.virtualbox.",
            "com.parallels.",
            "com.citrix.",
            "com.vmware.fusion",
            "com.utmapp.",
        ] {
            let rule = cfg
                .rules
                .iter()
                .find(|r| r.bundle_prefix == prefix)
                .unwrap_or_else(|| panic!("no shipped rule for {prefix}"));
            assert_eq!(rule.pass_triggers, Some(PassTriggers::Fullscreen));
        }

        // The PRD §10 example is an illustration with a cut-down rule list, so
        // it parses to the same globals and lenses but not the same rules.
        let example = parse(PRD_EXAMPLE).unwrap();
        assert_eq!(cfg.lenses, example.lenses);
        assert_eq!(cfg.summon_delay_ms, example.summon_delay_ms);
        assert_eq!(cfg.theme, example.theme);
        assert_eq!(cfg.titles, example.titles);
        assert_eq!(cfg.controls, example.controls);
        assert_eq!(cfg.keys, example.keys);
    }

    #[test]
    fn absent_titles_table_uses_builtin_defaults() {
        let cfg = parse(
            r#"
summon_delay_ms = 0
theme = "system"
"#,
        )
        .unwrap();
        assert_eq!(cfg.titles, Titles::default());
    }

    #[test]
    fn parse_titles_variants() {
        let cfg = parse(
            r#"
[titles]
show = "both"
truncate = "middle"
markers = false
"#,
        )
        .unwrap();
        assert_eq!(cfg.titles.show, TitleShow::Both);
        assert_eq!(cfg.titles.truncate, TitleTruncate::Middle);
        assert!(!cfg.titles.markers);

        let cfg = parse(
            r#"
[titles]
show = "app"
truncate = "start"
markers = true
"#,
        )
        .unwrap();
        assert_eq!(cfg.titles.show, TitleShow::App);
        assert_eq!(cfg.titles.truncate, TitleTruncate::Start);
        assert!(cfg.titles.markers);
    }

    /// Inheritance is "from the first lens", not "from the previous one" — a
    /// third lens must still take lens 1's values, not lens 2's.
    #[test]
    fn every_later_lens_inherits_from_the_first() {
        let toml = r#"
[[lens]]
trigger = "cmd+tab"
order = "alphabetical"
style = "list"

[[lens]]
trigger = "alt+tab"
style = "icons"

[[lens]]
trigger = "ctrl+tab"

[[lens]]
trigger = "cmd+grave"
order = "created"
"#;
        let cfg = parse(toml).unwrap();
        assert_eq!(cfg.lenses.len(), 4);
        // Third lens sets nothing: everything comes from lens 1, not lens 2.
        assert_eq!(cfg.lenses[2].style, Style::List);
        assert_eq!(cfg.lenses[2].order, Order::Alphabetical);
        // Second lens's own override does not leak forward.
        assert_eq!(cfg.lenses[1].style, Style::Icons);
        assert_eq!(cfg.lenses[3].style, Style::List);
        assert_eq!(cfg.lenses[3].order, Order::Created);
    }

    #[test]
    fn an_empty_trigger_on_a_later_lens_still_errors() {
        let err = parse(
            r#"
[[lens]]
trigger = "cmd+tab"

[[lens]]
trigger = ""
"#,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::EmptyTrigger));
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

    /// TOML array-of-tables cannot distinguish "absent" from "empty", so an
    /// omitted `[[rule]]` section means *no rules* rather than the shipped
    /// defaults. That is deliberate: it keeps deleting the section a usable way
    /// to say "none", and it keeps `parse(to_toml(c)) == c` true for a config
    /// with no rules. A fresh install still gets the §4.7 defaults — they are
    /// written into the first-run file, and `Config::default()` carries them
    /// whenever no file exists.
    #[test]
    fn an_omitted_rule_section_means_no_rules_and_round_trips() {
        let cfg = parse("theme = \"dark\"\n").unwrap();
        assert!(cfg.rules.is_empty());
        assert_eq!(parse(&to_toml(&cfg)).unwrap(), cfg);
        assert_eq!(Config::default().rules, default_rules());
    }

    #[test]
    fn empty_bundle_prefix_errors() {
        // starts_with("") is true for every app — this must never parse.
        let err = parse(
            r#"
[[rule]]
bundle_prefix = ""
hide_windows = "always"
"#,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::EmptyBundlePrefix));

        let err = parse(
            r#"
[[rule]]
bundle_prefix = "   "
"#,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::EmptyBundlePrefix));
    }

    #[test]
    fn blank_title_substrings_are_dropped_not_matched() {
        // contains("") is true for every title.
        let cfg = parse(
            r#"
[[rule]]
bundle_prefix = "com.example."
hide_windows = "title-contains"
hide_title_substrings = ["", "  ", "draft"]
"#,
        )
        .unwrap();
        assert_eq!(
            cfg.rules[0].hide_title_substrings,
            vec!["draft".to_string()]
        );
    }

    #[test]
    fn title_contains_with_no_usable_substrings_errors() {
        let err = parse(
            r#"
[[rule]]
bundle_prefix = "com.example."
hide_windows = "title-contains"
hide_title_substrings = ["", "   "]
"#,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            ConfigError::TitleContainsWithoutSubstrings(ref p) if p == "com.example."
        ));
    }

    #[test]
    fn summon_delay_is_clamped_to_the_documented_range() {
        assert_eq!(
            parse("summon_delay_ms = 5000\n").unwrap().summon_delay_ms,
            900
        );
        assert_eq!(
            parse("summon_delay_ms = 250\n").unwrap().summon_delay_ms,
            250
        );
    }

    #[test]
    fn whitespace_only_trigger_errors() {
        let err = parse(
            r#"
[[lens]]
trigger = "   "
"#,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::EmptyTrigger));
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
