//! Recipe parsing: `.cook` files into [`Recipe`]s.
//!
//! [`Parser`] wraps the official `cooklang` crate (see [`cooklang`] for the
//! mapping); this module adds the file and directory handling.

mod cooklang;

use std::path::{Path, PathBuf};

pub use self::cooklang::Parser;

use crate::error::{Error, Result};
use crate::model::Recipe;

/// True for `.cook` files (case-insensitive).
pub fn is_recipe_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cook"))
}

impl Parser {
    pub fn parse_file(&self, path: &Path) -> Result<Recipe> {
        if !is_recipe_file(path) {
            return Err(Error::Unsupported(path.into()));
        }
        let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        self.parse(path, &text)
    }

    /// Parse every recipe under `dir` (recursively), sorted by path. Failures
    /// are returned alongside the successes so one broken file doesn't hide
    /// the rest.
    pub fn parse_dir(&self, dir: &Path) -> Result<(Vec<Recipe>, Vec<Error>)> {
        let (mut ok, mut failed) = (Vec::new(), Vec::new());
        for path in recipe_files(dir)? {
            match self.parse_file(&path) {
                Ok(r) => ok.push(r),
                Err(e) => failed.push(e),
            }
        }
        Ok((ok, failed))
    }
}

/// Every `.cook` file under `dir`, recursively, sorted. Hidden files and
/// directories (`.git/`, editor swap files) are skipped.
pub fn recipe_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_files(dir, &mut files)?;
    files.retain(|p| is_recipe_file(p));
    files.sort();
    Ok(files)
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))? {
        let path = entry.map_err(|e| Error::io(dir, e))?.path();
        let hidden = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden {
            continue;
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
