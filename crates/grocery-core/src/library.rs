//! The recipe library on disk:
//!
//! ```text
//! <root>/
//!   Recipes/            .cook files, any depth
//!   Food/aliases.yaml   ingredient names → foods, aisles, gram data, NEVO codes
//!   Food/pantry.yaml    staples never put on a list
//!   Food/custom.yaml    your own nutrition values, for foods NEVO lacks
//! ```

use std::path::{Path, PathBuf};

use crate::custom::CustomFoods;
use crate::error::{Error, Result};
use crate::food::{FoodDb, Pantry};
use crate::parse::{self, Parser};

#[derive(Debug)]
pub struct Library {
    root: PathBuf,
    pub parser: Parser,
}

impl Library {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            parser: Parser::new(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn recipes_dir(&self) -> PathBuf {
        self.root.join("Recipes")
    }

    pub fn aliases_path(&self) -> PathBuf {
        self.root.join("Food/aliases.yaml")
    }

    pub fn food_db(&self) -> Result<FoodDb> {
        FoodDb::load(&self.aliases_path())
    }

    pub fn custom_path(&self) -> PathBuf {
        self.root.join("Food/custom.yaml")
    }

    /// The user's own nutrition values; a missing file is an empty set.
    pub fn custom_foods(&self) -> Result<CustomFoods> {
        CustomFoods::load(&self.custom_path())
    }

    pub fn pantry(&self) -> Result<Pantry> {
        Pantry::load(&self.root.join("Food/pantry.yaml"))
    }

    /// Every `.cook` file under `Recipes/`, sorted. A missing directory is empty.
    pub fn recipe_files(&self) -> Result<Vec<PathBuf>> {
        let dir = self.recipes_dir();
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        parse::recipe_files(&dir)
    }

    /// `path` relative to `Recipes/`, with `/` separators: the key the
    /// recipe index and the web API use.
    pub fn relative(&self, path: &Path) -> Option<String> {
        let rel = path.strip_prefix(self.recipes_dir()).ok()?;
        let parts: Option<Vec<&str>> = rel.components().map(|c| c.as_os_str().to_str()).collect();
        Some(parts?.join("/"))
    }

    /// Find a recipe by path (as given, or relative to the library root or
    /// `Recipes/`) or by name: a file stem in `Recipes/`, case-insensitive.
    pub fn find_recipe(&self, arg: &str) -> Result<PathBuf, FindError> {
        let candidates = [
            PathBuf::from(arg),
            self.root.join(arg),
            self.recipes_dir().join(arg),
        ];
        if let Some(found) = candidates
            .into_iter()
            .find(|p| p.is_file() && parse::is_recipe_file(p))
        {
            return Ok(found);
        }
        let wanted = arg.to_lowercase();
        let mut matches: Vec<PathBuf> = self
            .recipe_files()
            .map_err(FindError::Io)?
            .into_iter()
            .filter(|p| {
                p.file_stem()
                    .is_some_and(|s| s.to_string_lossy().to_lowercase() == wanted)
            })
            .collect();
        match matches.len() {
            1 => Ok(matches.remove(0)),
            0 => Err(FindError::NotFound(arg.to_owned())),
            _ => Err(FindError::Ambiguous(arg.to_owned(), matches)),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FindError {
    #[error("no recipe named '{0}' in Recipes/")]
    NotFound(String),
    #[error("'{0}' is ambiguous: {list}", list = .1.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "))]
    Ambiguous(String, Vec<PathBuf>),
    #[error(transparent)]
    Io(Error),
}
