use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum ConfigError {
    Toml(toml::de::Error),
    Io { path: PathBuf, source: io::Error },
    EmptyTrigger,
    EmptyBundlePrefix,
    TitleContainsWithoutSubstrings(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(e) => write!(f, "invalid config TOML: {e}"),
            Self::Io { path, source } => {
                // `save` reports through this variant too, so do not claim "read".
                write!(f, "config {}: {source}", path.display())
            }
            Self::EmptyTrigger => write!(f, "lens trigger must not be empty"),
            Self::EmptyBundlePrefix => write!(
                f,
                "rule bundle_prefix must not be empty — an empty prefix matches every app"
            ),
            Self::TitleContainsWithoutSubstrings(prefix) => write!(
                f,
                "rule {prefix}: hide_windows = \"title-contains\" needs at least one non-empty hide_title_substrings entry"
            ),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Toml(e) => Some(e),
            Self::Io { source, .. } => Some(source),
            Self::EmptyTrigger
            | Self::EmptyBundlePrefix
            | Self::TitleContainsWithoutSubstrings(_) => None,
        }
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(value: toml::de::Error) -> Self {
        Self::Toml(value)
    }
}
