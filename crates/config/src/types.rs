use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShowOn {
    #[default]
    ActiveScreen,
    PointerScreen,
    MenubarScreen,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CursorFollowsFocus {
    #[default]
    Never,
    Always,
    OtherScreen,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Apps {
    #[default]
    All,
    Active,
    Inactive,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Spaces {
    #[default]
    All,
    Visible,
    Hidden,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Screens {
    #[default]
    All,
    StripScreen,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Disposition {
    #[default]
    Show,
    #[serde(rename = "end")]
    End,
    Hide,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Order {
    #[default]
    Recent,
    Created,
    Alphabetical,
    Space,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Style {
    #[default]
    Gallery,
    Icons,
    List,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Size {
    Small,
    Medium,
    Large,
    #[default]
    Auto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OnRelease {
    #[default]
    Jump,
    Linger,
    Filter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PassTriggers {
    Never,
    Always,
    Fullscreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HideWindows {
    Never,
    Always,
    Windowless,
    TitleContains,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Peek {
    pub enabled: bool,
}

/// PRD §4.6: nothing ever animates in; dismissal may optionally fade.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Animation {
    pub fade_out: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TitleShow {
    #[default]
    Title,
    App,
    Both,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TitleTruncate {
    Start,
    Middle,
    #[default]
    End,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Titles {
    pub show: TitleShow,
    pub truncate: TitleTruncate,
    pub markers: bool,
}

impl Default for Titles {
    fn default() -> Self {
        Self {
            show: TitleShow::Title,
            truncate: TitleTruncate::End,
            markers: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Controls {
    pub arrow_keys: bool,
    pub vim_keys: bool,
    pub hover_select: bool,
    pub cursor_follows_focus: CursorFollowsFocus,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            arrow_keys: true,
            vim_keys: false,
            hover_select: false,
            cursor_follows_focus: CursorFollowsFocus::Never,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Keys {
    pub focus: String,
    pub cancel: String,
    pub close: String,
    pub minimize: String,
    pub fullscreen: String,
    pub quit_app: String,
    pub hide_app: String,
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            focus: "return".into(),
            cancel: "escape".into(),
            close: "w".into(),
            minimize: "m".into(),
            fullscreen: "f".into(),
            quit_app: "q".into(),
            hide_app: "h".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub bundle_prefix: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass_triggers: Option<PassTriggers>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_windows: Option<HideWindows>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hide_title_substrings: Vec<String>,
}

impl Rule {
    pub fn to_model_rule(&self) -> model::Rule {
        model::Rule {
            bundle_prefix: self.bundle_prefix.clone(),
            hide: match self.hide_windows {
                None | Some(HideWindows::Never) => model::HideWindows::Never,
                Some(HideWindows::Always) => model::HideWindows::Always,
                Some(HideWindows::Windowless) => model::HideWindows::Windowless,
                Some(HideWindows::TitleContains) => {
                    model::HideWindows::TitleContains(self.hide_title_substrings.clone())
                }
            },
            pass: match self.pass_triggers {
                None | Some(PassTriggers::Never) => model::PassTriggers::Never,
                Some(PassTriggers::Always) => model::PassTriggers::Always,
                Some(PassTriggers::Fullscreen) => model::PassTriggers::Fullscreen,
            },
        }
    }
}

pub fn to_model_rules(rules: &[Rule]) -> model::Rules {
    model::Rules::new(rules.iter().map(Rule::to_model_rule).collect())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResolvedLens {
    pub trigger: String,
    pub apps: Apps,
    pub spaces: Spaces,
    pub screens: Screens,
    pub minimized: Disposition,
    pub hidden: Disposition,
    pub fullscreen_windows: Disposition,
    pub windowless_apps: Disposition,
    pub order: Order,
    pub group_by_app: bool,
    pub style: Style,
    pub size: Size,
    pub on_release: OnRelease,
}

impl ResolvedLens {
    pub fn to_model_lens(&self) -> model::Lens {
        model::Lens {
            apps: match self.apps {
                Apps::All => model::AppScope::All,
                Apps::Active => model::AppScope::ActiveOnly,
                Apps::Inactive => model::AppScope::ExceptActive,
            },
            spaces: match self.spaces {
                Spaces::All => model::SpaceScope::All,
                Spaces::Visible => model::SpaceScope::VisibleOnly,
                Spaces::Hidden => model::SpaceScope::NonVisibleOnly,
            },
            screens: match self.screens {
                Screens::All => model::ScreenScope::All,
                Screens::StripScreen => model::ScreenScope::StripScreen,
            },
            minimized: to_model_disposition(self.minimized),
            hidden: to_model_disposition(self.hidden),
            fullscreen: to_model_disposition(self.fullscreen_windows),
            windowless: to_model_disposition(self.windowless_apps),
            order: match self.order {
                Order::Recent => model::Order::RecentFocus,
                Order::Created => model::Order::Creation,
                Order::Alphabetical => model::Order::Alphabetical,
                Order::Space => model::Order::SpaceOrder,
            },
            grouping: if self.group_by_app {
                model::Grouping::PerApp
            } else {
                model::Grouping::Windows
            },
        }
    }
}

fn to_model_disposition(d: Disposition) -> model::Disposition {
    match d {
        Disposition::Show => model::Disposition::Show,
        Disposition::End => model::Disposition::ShowAtEnd,
        Disposition::Hide => model::Disposition::Hide,
    }
}

pub(crate) fn lens_field_defaults() -> ResolvedLens {
    ResolvedLens {
        trigger: "cmd+tab".into(),
        apps: Apps::All,
        spaces: Spaces::All,
        screens: Screens::All,
        minimized: Disposition::Show,
        hidden: Disposition::Show,
        fullscreen_windows: Disposition::Show,
        windowless_apps: Disposition::End,
        order: Order::Recent,
        group_by_app: false,
        style: Style::Gallery,
        size: Size::Auto,
        on_release: OnRelease::Jump,
    }
}

pub(crate) fn default_lenses() -> Vec<ResolvedLens> {
    let first = lens_field_defaults();
    let second = ResolvedLens {
        trigger: "alt+tab".into(),
        apps: Apps::Active,
        ..first.clone()
    };
    vec![first, second]
}

/// PRD §4.7 shipped defaults: Finder hidden when it has no window, and
/// pass-through-when-fullscreen for the capture-everything remote/VM clients.
pub(crate) fn default_rules() -> Vec<Rule> {
    let pass_fullscreen = |prefix: &str| Rule {
        bundle_prefix: prefix.into(),
        pass_triggers: Some(PassTriggers::Fullscreen),
        hide_windows: None,
        hide_title_substrings: Vec::new(),
    };
    let mut rules = vec![Rule {
        bundle_prefix: "com.apple.finder".into(),
        pass_triggers: None,
        hide_windows: Some(HideWindows::Windowless),
        hide_title_substrings: Vec::new(),
    }];
    rules.extend(
        [
            "com.apple.ScreenSharing",
            "com.microsoft.rdc.",
            "com.teamviewer.",
            "org.virtualbox.",
            "com.parallels.",
            "com.citrix.",
            "com.vmware.fusion",
            "com.utmapp.",
        ]
        .map(pass_fullscreen),
    );
    rules
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawLens {
    pub trigger: String,
    #[serde(default)]
    pub apps: Option<Apps>,
    #[serde(default)]
    pub spaces: Option<Spaces>,
    #[serde(default)]
    pub screens: Option<Screens>,
    #[serde(default)]
    pub minimized: Option<Disposition>,
    #[serde(default)]
    pub hidden: Option<Disposition>,
    #[serde(default)]
    pub fullscreen_windows: Option<Disposition>,
    #[serde(default)]
    pub windowless_apps: Option<Disposition>,
    #[serde(default)]
    pub order: Option<Order>,
    #[serde(default)]
    pub group_by_app: Option<bool>,
    #[serde(default)]
    pub style: Option<Style>,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default)]
    pub on_release: Option<OnRelease>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct RawConfig {
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
    #[serde(default, rename = "lens")]
    pub lenses: Vec<RawLens>,
    #[serde(default, rename = "rule")]
    pub rules: Vec<Rule>,
}

impl Default for RawConfig {
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
            lenses: Vec::new(),
            rules: Vec::new(),
        }
    }
}

/// PRD §4.6 documents the apparition delay as 0–900 ms. The loop clamps too,
/// but clamping here means the value the settings window reads back and writes
/// is the value actually in force.
pub(crate) const MAX_SUMMON_DELAY_MS: u32 = 900;

/// Rules come from a hand-edited file, and the two empty-string cases are
/// silent disasters: `starts_with("")` matches every app, and `contains("")`
/// matches every title — one stray blank would hide the whole strip.
pub(crate) fn validate_rules(rules: &mut [Rule]) -> Result<(), ConfigError> {
    for rule in rules.iter_mut() {
        if rule.bundle_prefix.trim().is_empty() {
            return Err(ConfigError::EmptyBundlePrefix);
        }
        rule.hide_title_substrings
            .retain(|sub| !sub.trim().is_empty());
        if rule.hide_windows == Some(HideWindows::TitleContains)
            && rule.hide_title_substrings.is_empty()
        {
            return Err(ConfigError::TitleContainsWithoutSubstrings(
                rule.bundle_prefix.clone(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn resolve_lenses(raw: &[RawLens]) -> Result<Vec<ResolvedLens>, ConfigError> {
    if raw.is_empty() {
        return Ok(default_lenses());
    }

    for lens in raw {
        // Whitespace-only is just as unusable as empty, and would otherwise
        // round-trip into a lens that can never register.
        if lens.trigger.trim().is_empty() {
            return Err(ConfigError::EmptyTrigger);
        }
    }

    let base = lens_field_defaults();
    let first = fill_lens(&raw[0], &base);
    let mut out = Vec::with_capacity(raw.len());
    out.push(first.clone());
    for lens in &raw[1..] {
        out.push(fill_lens(lens, &first));
    }
    Ok(out)
}

fn fill_lens(raw: &RawLens, base: &ResolvedLens) -> ResolvedLens {
    ResolvedLens {
        trigger: raw.trigger.clone(),
        apps: raw.apps.unwrap_or(base.apps),
        spaces: raw.spaces.unwrap_or(base.spaces),
        screens: raw.screens.unwrap_or(base.screens),
        minimized: raw.minimized.unwrap_or(base.minimized),
        hidden: raw.hidden.unwrap_or(base.hidden),
        fullscreen_windows: raw.fullscreen_windows.unwrap_or(base.fullscreen_windows),
        windowless_apps: raw.windowless_apps.unwrap_or(base.windowless_apps),
        order: raw.order.unwrap_or(base.order),
        group_by_app: raw.group_by_app.unwrap_or(base.group_by_app),
        style: raw.style.unwrap_or(base.style),
        size: raw.size.unwrap_or(base.size),
        on_release: raw.on_release.unwrap_or(base.on_release),
    }
}
