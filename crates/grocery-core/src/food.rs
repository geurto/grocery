//! Food knowledge from the library: `Food/aliases.yaml` and `Food/pantry.yaml`.
//!
//! `aliases.yaml` maps the many ways an ingredient gets written to one
//! canonical food, and carries what's needed to turn "2 cloves" into grams.
//! It's plain YAML on purpose so you can fix a bad mapping in any editor.
//!
//! ```yaml
//! aisles: [groente & fruit, brood, zuivel, pasta & rijst, blik & pot]
//! foods:
//!   onion:
//!     nevo: 63             # NEVO code ("Ui rauw"); `grocery nevo suggest` helps
//!                          # (or `custom: <name>` for a food in custom.yaml)
//!   dried thyme:
//!     negligible: true     # left out of nutrition, not flagged
//!     piece_g: 110         # weight of one piece (shorthand for units: {pc: 110})
//!     aisle: groente & fruit
//!     synonyms: [onions, ui, uien]
//!   garlic:
//!     units: { clove: 5, teen: 5, tenen: 5 }
//!     synonyms: [knoflook]
//!   olive oil:
//!     density: 0.91        # g/ml, for el/tbsp/ml → g
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::units::{named_key, Dim, Normalized};

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FoodInfo {
    /// NEVO food code (Dutch food composition database).
    pub nevo: Option<u32>,
    /// Name of a food in `Food/custom.yaml`, for nutrition values NEVO
    /// doesn't have. Not together with `nevo`.
    pub custom: Option<String>,
    /// Too small to matter nutritionally (dried herbs, a pinch of spice):
    /// left out of nutrition without being flagged. Not together with
    /// `nevo` or `custom`.
    #[serde(default)]
    pub negligible: bool,
    /// Grams per piece.
    pub piece_g: Option<f64>,
    /// Grams per millilitre.
    pub density: Option<f64>,
    pub aisle: Option<String>,
    #[serde(default)]
    pub synonyms: Vec<String>,
    /// Grams per named unit: `{ clove: 5, can: 400 }`.
    #[serde(default)]
    pub units: BTreeMap<String, f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AliasesFile {
    #[serde(default)]
    aisles: Vec<String>,
    #[serde(default)]
    foods: BTreeMap<String, Option<FoodInfo>>,
}

/// A resolved ingredient name.
#[derive(Debug, Clone, Copy)]
pub struct Resolved<'a> {
    pub canonical: &'a str,
    pub info: &'a FoodInfo,
}

#[derive(Debug, Default)]
pub struct FoodDb {
    aisles: Vec<String>,
    foods: BTreeMap<String, FoodInfo>,
    /// normalized name or synonym → canonical key in `foods`
    index: HashMap<String, String>,
}

/// Lowercase, trim and collapse internal whitespace.
pub fn normalize_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

impl FoodDb {
    /// Load from a file; a missing file is an empty database, not an error,
    /// so a fresh library works before any aliases exist.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_yaml(&text, path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    pub fn from_yaml(text: &str, path: &Path) -> Result<Self> {
        let file: AliasesFile = if text.trim().is_empty() {
            AliasesFile::default()
        } else {
            serde_norway::from_str(text).map_err(|source| Error::Yaml {
                path: path.into(),
                source,
            })?
        };

        let mut db = FoodDb {
            aisles: file.aisles,
            ..Default::default()
        };
        for (name, info) in file.foods {
            // `onion:` with no body is allowed: a known food without details yet.
            let mut info = info.unwrap_or_default();
            let canonical = normalize_name(&name);
            if info.nevo.is_some() && info.custom.is_some() {
                return Err(Error::Config {
                    path: path.into(),
                    message: format!("'{name}' has both `nevo` and `custom`; use one"),
                });
            }
            if info.negligible && (info.nevo.is_some() || info.custom.is_some()) {
                return Err(Error::Config {
                    path: path.into(),
                    message: format!(
                        "'{name}' is `negligible` but also has `nevo` or `custom`; use one"
                    ),
                });
            }
            // Named units are matched on their normalized key.
            info.units = info
                .units
                .into_iter()
                .map(|(u, g)| (named_key(&u), g))
                .collect();

            for alias in std::iter::once(&name).chain(info.synonyms.iter()) {
                let key = normalize_name(alias);
                if let Some(existing) = db.index.get(&key) {
                    if *existing != canonical {
                        return Err(Error::Config {
                            path: path.into(),
                            message: format!(
                                "'{alias}' is listed for both '{existing}' and '{canonical}'"
                            ),
                        });
                    }
                }
                db.index.insert(key, canonical.clone());
            }
            db.foods.insert(canonical, info);
        }
        Ok(db)
    }

    /// Aisle walking order as configured; unknown aisles sort after these.
    pub fn aisle_order(&self) -> &[String] {
        &self.aisles
    }

    pub fn len(&self) -> usize {
        self.foods.len()
    }

    pub fn is_empty(&self) -> bool {
        self.foods.is_empty()
    }

    /// All foods by canonical name, sorted.
    pub fn foods(&self) -> impl Iterator<Item = (&str, &FoodInfo)> {
        self.foods.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Convert a normalized quantity to millilitres (for foods whose
    /// composition is given per 100 ml). Mass and pieces need a density.
    pub fn millilitres(info: &FoodInfo, q: &Normalized) -> Option<f64> {
        match &q.dim {
            Dim::Volume => Some(q.value),
            _ => {
                let g = Self::grams(info, q)?;
                info.density.filter(|d| *d > 0.0).map(|d| g / d)
            }
        }
    }

    /// Resolve a name as written in a recipe. Tries the exact (normalized)
    /// name, then a naive singular ("tomatoes" → "tomato", "onions" → "onion").
    pub fn resolve(&self, raw: &str) -> Option<Resolved<'_>> {
        let key = normalize_name(raw);
        let candidates = [
            Some(key.clone()),
            key.strip_suffix("es").map(str::to_owned),
            key.strip_suffix('s').map(str::to_owned),
        ];
        candidates.into_iter().flatten().find_map(|k| {
            let canonical = self.index.get(&k)?;
            let (canonical, info) = self.foods.get_key_value(canonical)?;
            Some(Resolved { canonical, info })
        })
    }

    /// Convert a normalized quantity to grams, if this food has the data.
    /// Never guesses: volume without a density stays volume.
    pub fn grams(info: &FoodInfo, q: &Normalized) -> Option<f64> {
        match &q.dim {
            Dim::Mass => Some(q.value),
            Dim::Volume => info.density.map(|d| q.value * d),
            Dim::Count => info
                .piece_g
                .or_else(|| info.units.get("pc").copied())
                .map(|g| q.value * g),
            Dim::Named(u) => info.units.get(u).map(|g| q.value * g),
        }
    }
}

/// Things you always have at home and never want on the list.
#[derive(Debug, Default)]
pub struct Pantry(HashSet<String>);

impl Pantry {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_yaml(&text, path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    pub fn from_yaml(text: &str, path: &Path) -> Result<Self> {
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        let items: Vec<String> = serde_norway::from_str(text).map_err(|source| Error::Yaml {
            path: path.into(),
            source,
        })?;
        Ok(Pantry(items.iter().map(|s| normalize_name(s)).collect()))
    }

    /// `key` is either the canonical food name or the normalized raw name.
    /// Pantry entries are resolved through the food db too, so listing
    /// "zout" in the pantry also covers recipes that say "salt".
    pub fn contains(&self, key: &str, db: &FoodDb) -> bool {
        self.0.contains(key)
            || self
                .0
                .iter()
                .any(|p| db.resolve(p).is_some_and(|r| r.canonical == key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::normalize;

    const YAML: &str = r#"
aisles: [groente & fruit, zuivel]
foods:
  Onion:
    piece_g: 110
    aisle: groente & fruit
    synonyms: [ui, uien]
  garlic:
    units: { cloves: 5, teen: 5 }
    synonyms: [knoflook]
  olive oil:
    density: 0.91
  tomato:
  salt:
    synonyms: [zout]
"#;

    fn db() -> FoodDb {
        FoodDb::from_yaml(YAML, Path::new("aliases.yaml")).unwrap()
    }

    #[test]
    fn resolves_names_synonyms_and_plurals() {
        let db = db();
        assert_eq!(db.resolve("onion").unwrap().canonical, "onion");
        assert_eq!(db.resolve("  Uien ").unwrap().canonical, "onion");
        assert_eq!(db.resolve("onions").unwrap().canonical, "onion");
        assert_eq!(db.resolve("tomatoes").unwrap().canonical, "tomato");
        assert!(db.resolve("passata").is_none());
        assert_eq!(db.aisle_order(), ["groente & fruit", "zuivel"]);
    }

    #[test]
    fn grams_uses_food_data_and_never_guesses() {
        let db = db();
        let onion = db.resolve("onion").unwrap().info;
        assert_eq!(FoodDb::grams(onion, &normalize(2.0, None)), Some(220.0));
        let garlic = db.resolve("knoflook").unwrap().info;
        assert_eq!(
            FoodDb::grams(garlic, &normalize(3.0, Some("clove"))),
            Some(15.0)
        );
        assert_eq!(
            FoodDb::grams(garlic, &normalize(3.0, Some("teen"))),
            Some(15.0)
        );
        let oil = db.resolve("olive oil").unwrap().info;
        assert!((FoodDb::grams(oil, &normalize(2.0, Some("el"))).unwrap() - 27.3).abs() < 1e-9);
        let tomato = db.resolve("tomato").unwrap().info;
        assert_eq!(FoodDb::grams(tomato, &normalize(500.0, Some("ml"))), None);
    }

    #[test]
    fn duplicate_synonym_is_an_error() {
        let yaml = "foods:\n  a: { synonyms: [x] }\n  b: { synonyms: [x] }\n";
        let err = FoodDb::from_yaml(yaml, Path::new("aliases.yaml")).unwrap_err();
        assert!(err.to_string().contains("'x' is listed for both"), "{err}");
    }

    #[test]
    fn unknown_field_is_an_error() {
        let yaml = "foods:\n  a: { pice_g: 10 }\n";
        assert!(FoodDb::from_yaml(yaml, Path::new("aliases.yaml")).is_err());
    }

    #[test]
    fn pantry_matches_through_synonyms() {
        let db = db();
        let pantry = Pantry::from_yaml("- zout\n- black pepper\n", Path::new("p.yaml")).unwrap();
        assert!(pantry.contains("salt", &db));
        assert!(pantry.contains("black pepper", &db));
        assert!(!pantry.contains("onion", &db));
    }
}
