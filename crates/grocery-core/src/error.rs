use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{}: invalid YAML: {source}", path.display())]
    Yaml {
        path: PathBuf,
        #[source]
        source: serde_norway::Error,
    },

    #[error("{}: {message}", path.display())]
    Parse { path: PathBuf, message: String },

    #[error("{}: not a recipe (expected a .cook file)", .0.display())]
    Unsupported(PathBuf),

    #[error("{}: {message}", path.display())]
    Config { path: PathBuf, message: String },
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }
}
