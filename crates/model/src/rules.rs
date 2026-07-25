#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HideWindows {
    Never,
    Always,
    Windowless,
    TitleContains(Vec<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassTriggers {
    Never,
    Always,
    Fullscreen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub bundle_prefix: String,
    pub hide: HideWindows,
    pub pass: PassTriggers,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Rules {
    rules: Vec<Rule>,
}

impl Rules {
    pub fn new(rules: Vec<Rule>) -> Self {
        Self { rules }
    }

    /// Shipped §4.7 defaults: Finder windowless-hide; fullscreen trigger
    /// pass-through for capture-everything remote/VM apps.
    pub fn defaults() -> Self {
        Self {
            rules: vec![
                Rule {
                    bundle_prefix: "com.apple.finder".into(),
                    hide: HideWindows::Windowless,
                    pass: PassTriggers::Never,
                },
                Rule {
                    bundle_prefix: "com.apple.ScreenSharing".into(),
                    hide: HideWindows::Never,
                    pass: PassTriggers::Fullscreen,
                },
                Rule {
                    // verify on device
                    bundle_prefix: "com.microsoft.rdc.".into(),
                    hide: HideWindows::Never,
                    pass: PassTriggers::Fullscreen,
                },
                Rule {
                    // verify on device
                    bundle_prefix: "com.teamviewer.".into(),
                    hide: HideWindows::Never,
                    pass: PassTriggers::Fullscreen,
                },
                Rule {
                    // verify on device
                    bundle_prefix: "org.virtualbox.".into(),
                    hide: HideWindows::Never,
                    pass: PassTriggers::Fullscreen,
                },
                Rule {
                    bundle_prefix: "com.parallels.".into(),
                    hide: HideWindows::Never,
                    pass: PassTriggers::Fullscreen,
                },
                Rule {
                    // verify on device
                    bundle_prefix: "com.citrix.".into(),
                    hide: HideWindows::Never,
                    pass: PassTriggers::Fullscreen,
                },
                Rule {
                    bundle_prefix: "com.vmware.fusion".into(),
                    hide: HideWindows::Never,
                    pass: PassTriggers::Fullscreen,
                },
                Rule {
                    // verify on device
                    bundle_prefix: "com.utmapp.".into(),
                    hide: HideWindows::Never,
                    pass: PassTriggers::Fullscreen,
                },
            ],
        }
    }

    fn best(&self, bundle_id: &str) -> Option<&Rule> {
        self.rules
            .iter()
            .filter(|r| bundle_id.starts_with(&r.bundle_prefix))
            .max_by_key(|r| r.bundle_prefix.len())
    }

    pub fn should_hide(&self, bundle_id: &str, title: &str, has_open_window: bool) -> bool {
        match self.best(bundle_id).map(|r| &r.hide) {
            None | Some(HideWindows::Never) => false,
            Some(HideWindows::Always) => true,
            Some(HideWindows::Windowless) => !has_open_window,
            Some(HideWindows::TitleContains(subs)) => subs.iter().any(|s| title.contains(s)),
        }
    }

    pub fn passes_trigger(&self, bundle_id: &str, app_fullscreen: bool) -> bool {
        match self.best(bundle_id).map(|r| r.pass) {
            None | Some(PassTriggers::Never) => false,
            Some(PassTriggers::Always) => true,
            Some(PassTriggers::Fullscreen) => app_fullscreen,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(prefix: &str, hide: HideWindows, pass: PassTriggers) -> Rule {
        Rule {
            bundle_prefix: prefix.into(),
            hide,
            pass,
        }
    }

    #[test]
    fn prefix_match_parallels() {
        let rules = Rules::new(vec![rule(
            "com.parallels.",
            HideWindows::Never,
            PassTriggers::Always,
        )]);
        assert!(rules.passes_trigger("com.parallels.desktop", false));
    }

    #[test]
    fn longest_prefix_wins() {
        let rules = Rules::new(vec![
            rule("com.example.", HideWindows::Always, PassTriggers::Never),
            rule("com.example.app", HideWindows::Never, PassTriggers::Always),
        ]);
        assert!(!rules.should_hide("com.example.app.helper", "", true));
        assert!(rules.passes_trigger("com.example.app.helper", false));
        assert!(rules.should_hide("com.example.other", "", true));
        assert!(!rules.passes_trigger("com.example.other", false));
    }

    #[test]
    fn no_match_defaults() {
        let rules = Rules::new(vec![rule(
            "com.known.",
            HideWindows::Always,
            PassTriggers::Always,
        )]);
        assert!(!rules.should_hide("com.unknown.app", "title", true));
        assert!(!rules.passes_trigger("com.unknown.app", true));
    }

    #[test]
    fn hide_never() {
        let rules = Rules::new(vec![rule(
            "com.app.",
            HideWindows::Never,
            PassTriggers::Never,
        )]);
        assert!(!rules.should_hide("com.app.foo", "x", false));
    }

    #[test]
    fn hide_always() {
        let rules = Rules::new(vec![rule(
            "com.app.",
            HideWindows::Always,
            PassTriggers::Never,
        )]);
        assert!(rules.should_hide("com.app.foo", "x", true));
    }

    #[test]
    fn hide_windowless() {
        let rules = Rules::new(vec![rule(
            "com.app.",
            HideWindows::Windowless,
            PassTriggers::Never,
        )]);
        assert!(rules.should_hide("com.app.foo", "", false));
        assert!(!rules.should_hide("com.app.foo", "", true));
    }

    #[test]
    fn hide_title_contains_any() {
        let rules = Rules::new(vec![rule(
            "com.app.",
            HideWindows::TitleContains(vec!["secret".into(), "draft".into()]),
            PassTriggers::Never,
        )]);
        assert!(rules.should_hide("com.app.foo", "my secret note", true));
        assert!(rules.should_hide("com.app.foo", "a draft copy", true));
        assert!(!rules.should_hide("com.app.foo", "public note", true));
    }

    #[test]
    fn pass_never() {
        let rules = Rules::new(vec![rule(
            "com.app.",
            HideWindows::Never,
            PassTriggers::Never,
        )]);
        assert!(!rules.passes_trigger("com.app.foo", true));
    }

    #[test]
    fn pass_always() {
        let rules = Rules::new(vec![rule(
            "com.app.",
            HideWindows::Never,
            PassTriggers::Always,
        )]);
        assert!(rules.passes_trigger("com.app.foo", false));
    }

    #[test]
    fn pass_fullscreen() {
        let rules = Rules::new(vec![rule(
            "com.app.",
            HideWindows::Never,
            PassTriggers::Fullscreen,
        )]);
        assert!(rules.passes_trigger("com.app.foo", true));
        assert!(!rules.passes_trigger("com.app.foo", false));
    }

    #[test]
    fn defaults_finder_windowless() {
        let rules = Rules::defaults();
        assert!(rules.should_hide("com.apple.finder", "", false));
        assert!(!rules.should_hide("com.apple.finder", "Downloads", true));
    }

    #[test]
    fn defaults_vm_passes_iff_fullscreen() {
        let rules = Rules::defaults();
        assert!(rules.passes_trigger("com.parallels.desktop", true));
        assert!(!rules.passes_trigger("com.parallels.desktop", false));
        assert!(rules.passes_trigger("com.apple.ScreenSharing", true));
        assert!(!rules.passes_trigger("com.apple.ScreenSharing", false));
    }

    #[test]
    fn defaults_unknown_neither_hides_nor_passes() {
        let rules = Rules::defaults();
        assert!(!rules.should_hide("com.unknown.app", "title", false));
        assert!(!rules.passes_trigger("com.unknown.app", true));
    }
}
