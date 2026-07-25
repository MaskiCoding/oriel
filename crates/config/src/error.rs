use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum ConfigError {
    Toml(toml::de::Error),
    Io { path: PathBuf, source: io::Error },
    EmptyTrigger,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(e) => write!(f, "invalid config TOML: {e}"),
            Self::Io { path, source } => {
                write!(f, "failed to read config {}: {source}", path.display())
            }
            Self::EmptyTrigger => write!(f, "lens trigger must not be empty"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Toml(e) => Some(e),
            Self::Io { source, .. } => Some(source),
            Self::EmptyTrigger => None,
        }
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(value: toml::de::Error) -> Self {
        Self::Toml(value)
    }
}
