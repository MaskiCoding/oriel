/// How tiles are presented in the strip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    #[default]
    Gallery,
    Icons,
    List,
}

/// Tile scale. `Auto` picks from the window count and target screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Size {
    Small,
    Medium,
    Large,
    #[default]
    Auto,
}

/// Which screen the strip centers on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShowOn {
    #[default]
    ActiveScreen,
    PointerScreen,
    MenubarScreen,
}

/// Panel and caption colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// Runtime presentation of the strip — style, scale, screen, and theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub style: Style,
    pub size: Size,
    pub show_on: ShowOn,
    pub theme: Theme,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            style: Style::Gallery,
            size: Size::Auto,
            show_on: ShowOn::ActiveScreen,
            theme: Theme::System,
        }
    }
}
