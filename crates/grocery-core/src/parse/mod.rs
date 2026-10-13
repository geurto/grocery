//! Recipe parsers. Each format implements [`RecipeSource`] and produces the
//! same [`Recipe`]; [`Parsers`] picks one by file extension.

mod cooklang;
mod frontmatter;

use std::path::{Path, PathBuf};

pub use self::cooklang::CooklangSource;
pub use self::frontmatter::{parse_shorthand, FrontmatterSource};

use crate::error::{Error, Result};
use crate::model::Recipe;

pub trait RecipeSource {
    /// Parse `text`, which was read from `path`. The path is only used for
    /// error messages and as a fallback title.
    fn parse(&self, path: &Path, text: &str) -> Result<Recipe>;
}

/// Holds one instance of each parser. Building the Cooklang parser is not
/// free, so create this once and reuse it (the web service will keep one).
#[derive(Debug, Default)]
pub struct Parsers {
    pub cooklang: CooklangSource,
    pub frontmatter: FrontmatterSource,
}

impl Parsers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn for_path(&self, path: &Path) -> Option<&dyn RecipeSource> {
        match extension(path).as_deref() {
            Some("cook") => Some(&self.cooklang),
            Some("md") => Some(&self.frontmatter),
            _ => None,
        }
    }

    pub fn parse_str(&self, path: &Path, text: &str) -> Result<Recipe> {
        let source = self
            .for_path(path)
            .ok_or_else(|| Error::Unsupported(path.into()))?;
        source.parse(path, text)
    }

    pub fn parse_file(&self, path: &Path) -> Result<Recipe> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        self.parse_str(path, &text)
    }

    /// Parse every recipe under `dir` (recursively), sorted by path.
    /// Markdown files that aren't recipes are skipped silently; every other
    /// failure is returned alongside the successes so one broken file doesn't
    /// hide the rest.
    pub fn parse_dir(&self, dir: &Path) -> Result<(Vec<Recipe>, Vec<Error>)> {
        let mut files = Vec::new();
        collect_files(dir, &mut files)?;
        files.sort();

        let (mut ok, mut failed) = (Vec::new(), Vec::new());
        for path in files.into_iter().filter(|p| self.for_path(p).is_some()) {
            match self.parse_file(&path) {
                Ok(r) => ok.push(r),
                Err(Error::NotARecipe(_)) => {}
                Err(e) => failed.push(e),
            }
        }
        Ok((ok, failed))
    }
}

fn extension(path: &Path) -> Option<String> {
    path.extension().map(|e| e.to_string_lossy().to_lowercase())
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))? {
        let path = entry.map_err(|e| Error::io(dir, e))?.path();
        let hidden = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden {
            continue; // .obsidian/, .trash/, .livesync/
        }
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Fallback title: the file name without extension.
pub(crate) fn title_from_path(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into())
}
