//! SQLite storage for the grocery app.
//!
//! - `nevo_*`: the NEVO food composition table, rebuilt by `import-nevo`.
//! - `recipes`: an index of the `.cook` files, rebuilt from them at will.
//! - `lists*`: saved shopping lists with their ticked items.
//! - `people`, `plan_entries`: meal plans.
//!
//! Lists and plans exist nowhere else, so they're what a backup is for.

pub mod import;
pub mod lists;
mod nutrition;
pub mod plans;
pub mod recipes;
pub mod search;

use std::cell::OnceCell;
use std::collections::BTreeSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use grocery_core::nutrition::{Basis, Composition, CompositionTable, FoodKey};
use rusqlite::{params, Connection, OptionalExtension};

pub use import::{read_nevo, NevoData};
pub use lists::{ListStatus, ListSummary, StoredItem, StoredList};
pub use plans::{EntryChange, Meal, NewEntry, Person, PlanEntry};
pub use recipes::{IndexedRecipe, SyncReport};
pub use search::{Candidate, Hit};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Format(String),
    #[error("NEVO data has not been imported yet (run `grocery import-nevo <dir>`)")]
    NotImported,
    #[error(transparent)]
    Library(#[from] grocery_core::Error),
    #[error("{0}")]
    Invalid(String),
}

impl Error {
    pub(crate) fn csv(what: &str, e: csv::Error) -> Self {
        let pos = e
            .position()
            .map(|p| format!(" line {}", p.line()))
            .unwrap_or_default();
        Error::Format(format!("{what}{pos}: {e}"))
    }
}

/// v2 added the recipe index and shopping lists, v3 meal plans. Every table
/// is `CREATE IF NOT EXISTS`, so opening an older database upgrades it in
/// place.
const SCHEMA_VERSION: i32 = 3;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS nevo_foods (
    code      INTEGER PRIMARY KEY,
    name_nl   TEXT NOT NULL,
    name_en   TEXT NOT NULL,
    synonyms  TEXT NOT NULL,
    group_nl  TEXT NOT NULL,
    group_en  TEXT NOT NULL,
    basis     TEXT NOT NULL CHECK (basis IN ('100g', '100ml')),
    note      TEXT NOT NULL,
    traces    TEXT NOT NULL,
    fortified TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS nevo_nutrients (
    code     TEXT PRIMARY KEY,
    name_nl  TEXT NOT NULL,
    name_en  TEXT NOT NULL,
    unit     TEXT NOT NULL,
    group_en TEXT NOT NULL,
    sort     INTEGER NOT NULL
);
-- Only known values are stored: a missing row means NEVO has no value.
CREATE TABLE IF NOT EXISTS nevo_values (
    food     INTEGER NOT NULL REFERENCES nevo_foods(code),
    nutrient TEXT NOT NULL REFERENCES nevo_nutrients(code),
    value    REAL NOT NULL,
    PRIMARY KEY (food, nutrient)
) WITHOUT ROWID;

-- Lists of short strings (tags, names, warnings) are stored newline-joined.
CREATE TABLE IF NOT EXISTS recipes (
    path        TEXT PRIMARY KEY,  -- relative to Recipes/, '/'-separated
    title       TEXT NOT NULL,
    servings    REAL,
    tags        TEXT NOT NULL,
    description TEXT,
    ingredients TEXT NOT NULL,
    warnings    TEXT NOT NULL,
    error       TEXT,
    mtime       INTEGER NOT NULL,  -- ns since the epoch
    size        INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS lists (
    id      INTEGER PRIMARY KEY,
    title   TEXT NOT NULL,
    created TEXT NOT NULL,
    status  TEXT NOT NULL CHECK (status IN ('open', 'done')),
    pantry  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS list_recipes (
    list     INTEGER NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    pos      INTEGER NOT NULL,
    path     TEXT NOT NULL,
    title    TEXT NOT NULL,
    servings REAL,
    factor   REAL NOT NULL,
    PRIMARY KEY (list, pos)
);
CREATE TABLE IF NOT EXISTS list_items (
    id       INTEGER PRIMARY KEY,
    list     INTEGER NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    pos      INTEGER NOT NULL,
    name     TEXT NOT NULL,
    aisle    TEXT,
    dim      TEXT,                 -- g | ml | pc | unit:<name>; NULL = no quantity
    value    REAL,
    label    TEXT,
    notes    TEXT NOT NULL,
    optional INTEGER NOT NULL,
    recipes  TEXT NOT NULL,
    known    INTEGER NOT NULL,
    checked  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS list_items_by_list ON list_items (list, pos);
CREATE TABLE IF NOT EXISTS people (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
);
CREATE TABLE IF NOT EXISTS plan_entries (
    id         INTEGER PRIMARY KEY,
    person     INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
    day        TEXT NOT NULL,      -- YYYY-MM-DD
    meal       TEXT CHECK (meal IN ('breakfast', 'lunch', 'dinner', 'snack')),
    meal_order INTEGER NOT NULL,   -- for sorting; no meal sorts last
    path       TEXT NOT NULL,      -- relative to Recipes/
    title      TEXT NOT NULL,
    portions   REAL NOT NULL CHECK (portions > 0)
);
CREATE INDEX IF NOT EXISTS plan_entries_by_day ON plan_entries (person, day);
";

pub(crate) fn join_lines(items: &[String]) -> String {
    items.join("\n")
}

pub(crate) fn split_lines(s: &str) -> Vec<String> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.split('\n').map(str::to_owned).collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NevoInfo {
    pub version: String,
    pub foods: u32,
    pub nutrients: u32,
    /// Unix seconds.
    pub imported_at: u64,
}

impl NevoInfo {
    /// The reference the NEVO conditions of use require on any output of
    /// nutritional calculations.
    pub fn citation(&self) -> String {
        format!(
            "Based on data from NEVO online version {}, RIVM, Bilthoven",
            self.version
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NutrientMeta {
    pub code: String,
    pub name_nl: String,
    pub name_en: String,
    pub unit: String,
    pub group_en: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FoodRecord {
    pub code: u32,
    pub name_nl: String,
    pub name_en: String,
    pub synonyms: String,
    pub group_en: String,
    pub basis: Basis,
    pub note: String,
}

pub struct Store {
    conn: Connection,
    /// Food names for search, loaded on first use.
    candidates: OnceCell<Vec<Candidate>>,
}

fn basis_from_sql(s: &str) -> Basis {
    if s == "100ml" {
        Basis::Per100ml
    } else {
        Basis::Per100g
    }
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)
                .map_err(|e| Error::Format(format!("creating {}: {e}", dir.display())))?;
        }
        let conn = Connection::open(path)?;
        // The service and the CLI may use the same file at once.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(Error::Format(format!(
                "database schema v{version} is newer than this build (v{SCHEMA_VERSION})"
            )));
        }
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self {
            conn,
            candidates: OnceCell::new(),
        })
    }

    /// Replace all NEVO data in one transaction.
    pub fn import_nevo(&mut self, data: &NevoData) -> Result<NevoInfo> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "DELETE FROM nevo_values; DELETE FROM nevo_foods; DELETE FROM nevo_nutrients;",
        )?;
        {
            let mut ins = tx.prepare(
                "INSERT INTO nevo_nutrients (code, name_nl, name_en, unit, group_en, sort) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (i, n) in data.nutrients.iter().enumerate() {
                ins.execute(params![
                    n.code, n.name_nl, n.name_en, n.unit, n.group_en, i as i64
                ])?;
            }
            let mut food = tx.prepare(
                "INSERT INTO nevo_foods (code, name_nl, name_en, synonyms, group_nl, group_en, basis, note, traces, fortified)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            let mut value =
                tx.prepare("INSERT INTO nevo_values (food, nutrient, value) VALUES (?1, ?2, ?3)")?;
            for f in &data.foods {
                let basis = match f.basis {
                    Basis::Per100g => "100g",
                    Basis::Per100ml => "100ml",
                };
                food.execute(params![
                    f.code,
                    f.name_nl,
                    f.name_en,
                    f.synonyms,
                    f.group_nl,
                    f.group_en,
                    basis,
                    f.note,
                    f.traces,
                    f.fortified
                ])?;
                for (code, v) in &f.values {
                    value.execute(params![f.code, code, v])?;
                }
            }
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let mut meta =
                tx.prepare("INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)")?;
            meta.execute(params!["nevo_version", data.version])?;
            meta.execute(params!["nevo_version_raw", data.version_raw])?;
            meta.execute(params!["nevo_imported_at", now.to_string()])?;
        }
        tx.commit()?;
        self.candidates = OnceCell::new();
        self.nevo_info()?.ok_or(Error::NotImported)
    }

    fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn nevo_info(&self) -> Result<Option<NevoInfo>> {
        let Some(version) = self.meta("nevo_version")? else {
            return Ok(None);
        };
        let count = |table: &str| -> Result<u32> {
            Ok(self
                .conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?)
        };
        Ok(Some(NevoInfo {
            version,
            foods: count("nevo_foods")?,
            nutrients: count("nevo_nutrients")?,
            imported_at: self
                .meta("nevo_imported_at")?
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
        }))
    }

    /// Like [`Self::nevo_info`] but an error if nothing is imported.
    pub fn require_nevo(&self) -> Result<NevoInfo> {
        self.nevo_info()?.ok_or(Error::NotImported)
    }

    pub fn nutrients(&self) -> Result<Vec<NutrientMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT code, name_nl, name_en, unit, group_en FROM nevo_nutrients ORDER BY sort",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(NutrientMeta {
                code: r.get(0)?,
                name_nl: r.get(1)?,
                name_en: r.get(2)?,
                unit: r.get(3)?,
                group_en: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn food(&self, code: u32) -> Result<Option<FoodRecord>> {
        Ok(self
            .conn
            .query_row(
                "SELECT code, name_nl, name_en, synonyms, group_en, basis, note FROM nevo_foods WHERE code = ?1",
                [code],
                |r| {
                    Ok(FoodRecord {
                        code: r.get(0)?,
                        name_nl: r.get(1)?,
                        name_en: r.get(2)?,
                        synonyms: r.get(3)?,
                        group_en: r.get(4)?,
                        basis: basis_from_sql(&r.get::<_, String>(5)?),
                        note: r.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    /// Compositions for the given codes. Codes not in the dataset are simply
    /// absent; the nutrition report flags them.
    pub fn compositions(&self, codes: &BTreeSet<u32>) -> Result<CompositionTable> {
        let mut foods = Vec::new();
        let mut values = self
            .conn
            .prepare("SELECT nutrient, value FROM nevo_values WHERE food = ?1")?;
        for &code in codes {
            let Some(rec) = self.food(code)? else {
                continue;
            };
            let vals = values
                .query_map([code], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
                })?
                .collect::<Result<_, _>>()?;
            foods.push(Composition {
                key: FoodKey::Nevo(code),
                name: rec.name_nl,
                source: None,
                basis: rec.basis,
                values: vals,
            });
        }
        Ok(CompositionTable::new(foods))
    }

    fn candidates(&self) -> Result<&Vec<Candidate>> {
        if let Some(c) = self.candidates.get() {
            return Ok(c);
        }
        let mut stmt = self.conn.prepare(
            "SELECT f.code, f.name_nl, f.name_en, f.synonyms, f.group_en, v.value
             FROM nevo_foods f
             LEFT JOIN nevo_values v ON v.food = f.code AND v.nutrient = 'ENERCC'
             ORDER BY f.code",
        )?;
        let list = stmt
            .query_map([], |r| {
                Ok(Candidate {
                    code: r.get(0)?,
                    name_nl: r.get(1)?,
                    name_en: r.get(2)?,
                    synonyms: r.get(3)?,
                    group_en: r.get(4)?,
                    kcal: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self.candidates.get_or_init(|| list))
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        Ok(search::rank(query, self.candidates()?, limit))
    }
}
