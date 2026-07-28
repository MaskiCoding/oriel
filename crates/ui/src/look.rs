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

/// Which caption text a tile shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TitleShow {
    #[default]
    Title,
    App,
    Both,
}

/// Where the ellipsis lands when a caption overflows its frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TitleTruncate {
    Start,
    Middle,
    #[default]
    End,
}

/// Runtime presentation of the strip — style, scale, screen, theme, and caption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub style: Style,
    pub size: Size,
    pub show_on: ShowOn,
    pub theme: Theme,
    pub title_show: TitleShow,
    pub title_truncate: TitleTruncate,
    pub markers: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            style: Style::Gallery,
            size: Size::Auto,
            show_on: ShowOn::ActiveScreen,
            theme: Theme::System,
            title_show: TitleShow::Title,
            title_truncate: TitleTruncate::End,
            markers: true,
        }
    }
}
