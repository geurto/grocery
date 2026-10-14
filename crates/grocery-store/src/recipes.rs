//! The recipe index: one row per `.cook` file, for listing and searching
//! without parsing every file on each request.
//!
//! The files stay the source of truth. [`Store::sync_recipes`] re-parses
//! only files whose modification time or size changed, and drops rows for
//! files that are gone, so it's cheap to call after every change.

use std::collections::{HashMap, HashSet};
use std::time::UNIX_EPOCH;

use grocery_core::library::Library;
use rusqlite::{params, OptionalExtension, Row};

use crate::{join_lines, split_lines, Result, Store};

#[derive(Debug, Clone, PartialEq)]
pub struct IndexedRecipe {
    /// Relative to `Recipes/`, `/`-separated: "Pasta arrabbiata.cook".
    pub path: String,
    pub title: String,
    pub servings: Option<f64>,
    pub tags: Vec<String>,
    pub description: Option<String>,
    /// Ingredient names as written, unique, in recipe order.
    pub ingredients: Vec<String>,
    pub warnings: Vec<String>,
    /// Set when the file doesn't parse; the title is then the file name.
    pub error: Option<String>,
}

impl IndexedRecipe {
    /// Case-insensitive match on title, tags or ingredient names.
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty()
            || self.title.to_lowercase().contains(&q)
            || self.tags.iter().any(|t| t.to_lowercase().contains(&q))
            || self
                .ingredients
                .iter()
                .any(|i| i.to_lowercase().contains(&q))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
    /// Files that were (re)parsed and failed; they're indexed with `error` set.
    pub failed: usize,
}

impl SyncReport {
    pub fn changed(&self) -> bool {
        self.added + self.updated + self.removed > 0
    }
}

const COLUMNS: &str = "path, title, servings, tags, description, ingredients, warnings, error";

fn from_row(r: &Row<'_>) -> rusqlite::Result<IndexedRecipe> {
    Ok(IndexedRecipe {
        path: r.get(0)?,
        title: r.get(1)?,
        servings: r.get(2)?,
        tags: split_lines(&r.get::<_, String>(3)?),
        description: r.get(4)?,
        ingredients: split_lines(&r.get::<_, String>(5)?),
        warnings: split_lines(&r.get::<_, String>(6)?),
        error: r.get(7)?,
    })
}

impl Store {
    /// Bring the index in line with the files under `Recipes/`. With
    /// `force`, every file is re-parsed even if it looks unchanged.
    pub fn sync_recipes(&mut self, library: &Library, force: bool) -> Result<SyncReport> {
        let files = library.recipe_files()?;
        let tx = self.conn.transaction()?;
        let known: HashMap<String, (i64, i64)> = tx
            .prepare("SELECT path, mtime, size FROM recipes")?
            .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
            .collect::<Result<_, _>>()?;

        let mut report = SyncReport::default();
        let mut seen = HashSet::new();
        {
            let mut upsert = tx.prepare(
                "INSERT OR REPLACE INTO recipes
                 (path, title, servings, tags, description, ingredients, warnings, error, mtime, size)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for file in files {
                // Non-UTF-8 paths can't be addressed through the API; skip them.
                let Some(rel) = library.relative(&file) else {
                    continue;
                };
                let Ok(meta) = std::fs::metadata(&file) else {
                    continue; // deleted while scanning; the next sync drops it
                };
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos() as i64);
                let size = meta.len() as i64;
                seen.insert(rel.clone());

                let previous = known.get(&rel);
                if !force && previous == Some(&(mtime, size)) {
                    report.unchanged += 1;
                    continue;
                }
                let entry = match library.parser.parse_file(&file) {
                    Ok(recipe) => {
                        let mut names: Vec<&str> = Vec::new();
                        for ing in &recipe.ingredients {
                            if !names.contains(&ing.name.as_str()) {
                                names.push(&ing.name);
                            }
                        }
                        IndexedRecipe {
                            path: rel,
                            title: recipe.title.clone(),
                            servings: recipe.servings,
                            tags: recipe.tags.clone(),
                            description: recipe.description.clone(),
                            ingredients: names.into_iter().map(str::to_owned).collect(),
                            warnings: recipe.warnings.clone(),
                            error: None,
                        }
                    }
                    Err(e) => {
                        report.failed += 1;
                        IndexedRecipe {
                            title: file
                                .file_stem()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            path: rel,
                            servings: None,
                            tags: vec![],
                            description: None,
                            ingredients: vec![],
                            warnings: vec![],
                            error: Some(e.to_string()),
                        }
                    }
                };
                upsert.execute(params![
                    entry.path,
                    entry.title,
                    entry.servings,
                    join_lines(&entry.tags),
                    entry.description,
                    join_lines(&entry.ingredients),
                    join_lines(&entry.warnings),
                    entry.error,
                    mtime,
                    size
                ])?;
                if previous.is_some() {
                    report.updated += 1;
                } else {
                    report.added += 1;
                }
            }
            let mut delete = tx.prepare("DELETE FROM recipes WHERE path = ?1")?;
            for path in known.keys().filter(|p| !seen.contains(*p)) {
                delete.execute([path])?;
                report.removed += 1;
            }
        }
        tx.commit()?;
        Ok(report)
    }

    /// Indexed recipes sorted by title, filtered by [`IndexedRecipe::matches`].
    pub fn recipes(&self, query: Option<&str>) -> Result<Vec<IndexedRecipe>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM recipes ORDER BY title COLLATE NOCASE, path"
        ))?;
        let all = stmt
            .query_map([], from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(match query {
            Some(q) => all.into_iter().filter(|r| r.matches(q)).collect(),
            None => all,
        })
    }

    /// One recipe by its path relative to `Recipes/`.
    pub fn recipe(&self, path: &str) -> Result<Option<IndexedRecipe>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM recipes WHERE path = ?1"),
                [path],
                from_row,
            )
            .optional()?)
    }
}
