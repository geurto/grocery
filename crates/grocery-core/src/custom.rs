//! Your own nutrition values: `Food/custom.yaml`.
//!
//! For foods NEVO doesn't have (fresh coriander, a branded product), copy
//! the numbers off the package. Kept apart from the NEVO data on purpose: a
//! re-import replaces NEVO wholesale, RIVM's conditions require its values to
//! stay unchanged, and reports should say which numbers are yours.
//!
//! ```yaml
//! foods:
//!   fresh coriander:
//!     per: 100g              # or 100ml; default 100g
//!     source: package, Albert Heijn
//!     energy_kcal: 23
//!     protein: 2.1
//!     fat: 0.5
//!     saturated_fat: 0.03
//!     carbohydrate: 3.7
//!     sugars: 0.9
//!     fibre: 2.8
//!     salt: 0.1              # label salt in g; stored as sodium (× 400 mg)
//!     nutrients: { VITC: 27 }  # any NEVO nutrient code, in NEVO's unit
//! ```
//!
//! Everything is per 100 g (or 100 ml), as on a label. Leave out what the
//! label doesn't list: missing stays *unknown*, never 0 (see
//! [`crate::nutrition`]). A food in `aliases.yaml` uses it with
//! `custom: fresh coriander`.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::food::normalize_name;
use crate::nutrition::{Basis, Composition, FoodKey};

/// kJ per kcal, as used on EU labels.
const KJ_PER_KCAL: f64 = 4.184;
/// Salt (g) → sodium (mg): sodium × 2.5 = salt, the EU labelling rule.
const SODIUM_MG_PER_SALT_G: f64 = 1000.0 / 2.5;

/// One food as written in the file. Also what `grocery custom add` writes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy_kcal: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy_kj: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protein: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saturated_fat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carbohydrate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sugars: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fibre: Option<f64>,
    /// Label salt, g. Give this or `sodium_mg`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub salt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sodium_mg: Option<f64>,
    /// Raw NEVO nutrient codes ("VITC", "CA", ...) in NEVO's units.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub nutrients: BTreeMap<String, f64>,
}

/// A food with its values resolved to NEVO nutrient codes.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomFood {
    /// As written in the file.
    pub name: String,
    pub basis: Basis,
    pub source: Option<String>,
    /// NEVO nutrient code → amount per 100 g/ml, in NEVO's unit.
    pub values: BTreeMap<String, f64>,
}

impl CustomEntry {
    /// Resolve friendly field names to NEVO codes. The error says what to fix.
    pub fn to_food(&self, name: &str) -> std::result::Result<CustomFood, String> {
        let basis = match self
            .per
            .as_deref()
            .map(|p| p.to_lowercase().replace(char::is_whitespace, ""))
            .as_deref()
        {
            None | Some("100g") => Basis::Per100g,
            Some("100ml") => Basis::Per100ml,
            Some(other) => return Err(format!("`per` must be 100g or 100ml, not '{other}'")),
        };

        let mut values: BTreeMap<String, f64> = BTreeMap::new();
        let mut put =
            |field: &str, code: &str, v: Option<f64>| -> std::result::Result<(), String> {
                let Some(v) = v else { return Ok(()) };
                if !v.is_finite() || v < 0.0 {
                    return Err(format!("`{field}` must be a number ≥ 0, not {v}"));
                }
                if values.insert(code.to_owned(), v).is_some() {
                    return Err(format!("{code} is given twice (`{field}`)"));
                }
                Ok(())
            };
        put("energy_kcal", "ENERCC", self.energy_kcal)?;
        put("energy_kj", "ENERCJ", self.energy_kj)?;
        put("protein", "PROT", self.protein)?;
        put("fat", "FAT", self.fat)?;
        put("saturated_fat", "FASAT", self.saturated_fat)?;
        put("carbohydrate", "CHO", self.carbohydrate)?;
        put("sugars", "SUGAR", self.sugars)?;
        put("fibre", "FIBT", self.fibre)?;
        match (self.salt, self.sodium_mg) {
            (Some(_), Some(_)) => return Err("give `salt` or `sodium_mg`, not both".into()),
            (Some(salt), None) => {
                if !salt.is_finite() || salt < 0.0 {
                    return Err(format!("`salt` must be a number ≥ 0, not {salt}"));
                }
                put("salt", "NA", Some(salt * SODIUM_MG_PER_SALT_G))?
            }
            (None, sodium) => put("sodium_mg", "NA", sodium)?,
        }
        // Labels usually give both energies; with only kJ, derive kcal.
        if let (None, Some(kj)) = (self.energy_kcal, self.energy_kj) {
            put("energy_kj", "ENERCC", Some(kj / KJ_PER_KCAL))?;
        }
        for (code, v) in &self.nutrients {
            if code == "SALT" {
                return Err("SALT is derived from sodium; use `salt` or `sodium_mg`".into());
            }
            let valid = !code.is_empty()
                && code
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
            if !valid {
                return Err(format!(
                    "nutrient code '{code}' should look like a NEVO code (VITC, CA, ENERCC)"
                ));
            }
            put(&format!("nutrients.{code}"), code, Some(*v))?;
        }

        Ok(CustomFood {
            name: name.trim().to_owned(),
            basis,
            source: self.source.clone().filter(|s| !s.trim().is_empty()),
            values,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    foods: BTreeMap<String, CustomEntry>,
}

/// All custom foods, by normalized name.
#[derive(Debug, Clone, Default)]
pub struct CustomFoods {
    foods: BTreeMap<String, CustomFood>,
}

impl CustomFoods {
    /// A missing file is an empty set, so nothing needs creating up front.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_yaml(&text, path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    pub fn from_yaml(text: &str, path: &Path) -> Result<Self> {
        let file: File = if text.trim().is_empty() {
            File::default()
        } else {
            serde_norway::from_str(text).map_err(|source| Error::Yaml {
                path: path.into(),
                source,
            })?
        };
        let config = |message: String| Error::Config {
            path: path.into(),
            message,
        };
        let mut foods = BTreeMap::new();
        for (name, entry) in file.foods {
            let food = entry
                .to_food(&name)
                .map_err(|m| config(format!("{name}: {m}")))?;
            let key = normalize_name(&name);
            if foods.insert(key, food).is_some() {
                return Err(config(format!("'{name}' is listed twice")));
            }
        }
        Ok(Self { foods })
    }

    pub fn get(&self, name: &str) -> Option<&CustomFood> {
        self.foods.get(&normalize_name(name))
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = &CustomFood> {
        self.foods.values()
    }

    pub fn len(&self) -> usize {
        self.foods.len()
    }

    pub fn is_empty(&self) -> bool {
        self.foods.is_empty()
    }

    /// For [`crate::nutrition::CompositionTable::extend`].
    pub fn compositions(&self) -> Vec<Composition> {
        self.foods
            .iter()
            .map(|(key, f)| Composition {
                key: FoodKey::Custom(key.clone()),
                name: f.name.clone(),
                source: f.source.clone(),
                basis: f.basis,
                values: f
                    .values
                    .iter()
                    .map(|(k, v)| (k.clone(), *v))
                    .collect::<HashMap<_, _>>(),
            })
            .collect()
    }
}

/// `existing` (the file's current text, possibly empty) with `name` added.
///
/// The new entry is appended as text, so comments and ordering in a
/// hand-edited file survive. The result is parsed before it's returned, so
/// the caller can write it without breaking the file.
pub fn append_entry(
    existing: &str,
    name: &str,
    entry: &CustomEntry,
    path: &Path,
) -> Result<String> {
    let config = |message: String| Error::Config {
        path: path.into(),
        message,
    };
    let name = name.trim();
    if name.is_empty() {
        return Err(config("a food needs a name".into()));
    }
    entry
        .to_food(name)
        .map_err(|m| config(format!("{name}: {m}")))?;
    let current = CustomFoods::from_yaml(existing, path)?;
    if current.contains(name) {
        return Err(config(format!("'{name}' is already in the file")));
    }

    let one: BTreeMap<&str, &CustomEntry> = BTreeMap::from([(name, entry)]);
    let block = serde_norway::to_string(&one).map_err(|source| Error::Yaml {
        path: path.into(),
        source,
    })?;
    let mut out = String::new();
    if existing.trim().is_empty() {
        out.push_str(
            "# Your own nutrition values, per 100 g (or 100 ml), for foods NEVO lacks.\n\
             # Use a food with `custom: <name>` in aliases.yaml.\n\
             foods:\n",
        );
    } else {
        out.push_str(existing);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    for line in block.lines() {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }

    // The append assumes `foods:` is the last thing in the file with block
    // style. Anything else (`foods: {}`) fails here instead of being written.
    let parsed = CustomFoods::from_yaml(&out, path).map_err(|e| {
        config(format!(
            "couldn't add to the file automatically ({e}); add the entry by hand"
        ))
    })?;
    if !parsed.contains(name) {
        return Err(config(
            "couldn't add to the file automatically; add the entry by hand".into(),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(yaml: &str) -> Result<CustomFoods> {
        CustomFoods::from_yaml(yaml, Path::new("custom.yaml"))
    }

    const YAML: &str = "
foods:
  Fresh Coriander:
    source: package
    energy_kcal: 23
    protein: 2.1
    salt: 0.1
    nutrients: { VITC: 27 }
  oat drink:
    per: 100 ml
    energy_kj: 188
    sodium_mg: 40
";

    #[test]
    fn friendly_names_become_nevo_codes() {
        let foods = load(YAML).unwrap();
        let c = foods.get("fresh  coriander").unwrap();
        assert_eq!(c.name, "Fresh Coriander");
        assert_eq!(c.basis, Basis::Per100g);
        assert_eq!(c.source.as_deref(), Some("package"));
        assert_eq!(c.values["ENERCC"], 23.0);
        assert_eq!(c.values["PROT"], 2.1);
        assert_eq!(c.values["VITC"], 27.0);
        // 0.1 g salt = 40 mg sodium
        assert!((c.values["NA"] - 40.0).abs() < 1e-9);
        assert!(!c.values.contains_key("FAT"), "not on the label: unknown");

        let oat = foods.get("oat drink").unwrap();
        assert_eq!(oat.basis, Basis::Per100ml);
        assert_eq!(oat.values["ENERCJ"], 188.0);
        assert!((oat.values["ENERCC"] - 188.0 / 4.184).abs() < 1e-9);
        assert_eq!(oat.values["NA"], 40.0);
    }

    #[test]
    fn compositions_carry_key_name_and_source() {
        let comps = load(YAML).unwrap().compositions();
        let c = comps.iter().find(|c| c.name == "Fresh Coriander").unwrap();
        assert_eq!(c.key, FoodKey::Custom("fresh coriander".into()));
        assert_eq!(c.source.as_deref(), Some("package"));
        assert_eq!(c.values["PROT"], 2.1);
    }

    #[test]
    fn mistakes_are_errors_not_silent_zeros() {
        for (yaml, expect) in [
            ("foods:\n  a: { protien: 1 }\n", "unknown field"),
            ("foods:\n  a: { protein: -1 }\n", "≥ 0"),
            ("foods:\n  a: { per: 100kg }\n", "100g or 100ml"),
            ("foods:\n  a: { salt: 1, sodium_mg: 400 }\n", "not both"),
            (
                "foods:\n  a: { energy_kcal: 5, nutrients: { ENERCC: 6 } }\n",
                "twice",
            ),
            ("foods:\n  a: { nutrients: { vitc: 6 } }\n", "NEVO code"),
            ("foods:\n  a: { nutrients: { SALT: 1 } }\n", "derived"),
            ("foods:\n  A: {}\n  a: {}\n", "twice"),
        ] {
            let err = load(yaml).unwrap_err().to_string();
            assert!(err.contains(expect), "{yaml:?} → {err}");
        }
    }

    #[test]
    fn missing_or_empty_file_is_an_empty_set() {
        assert!(load("").unwrap().is_empty());
        assert!(CustomFoods::load(Path::new("/nonexistent/custom.yaml"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn append_keeps_comments_and_validates() {
        let path = Path::new("custom.yaml");
        let entry = CustomEntry {
            source: Some("label: AH".into()),
            energy_kcal: Some(23.0),
            salt: Some(0.1),
            ..Default::default()
        };

        // new file
        let text = append_entry("", "fresh coriander", &entry, path).unwrap();
        assert!(text.starts_with("# Your own nutrition values"), "{text}");
        let foods = load(&text).unwrap();
        assert_eq!(foods.get("fresh coriander").unwrap().values["ENERCC"], 23.0);
        assert_eq!(
            foods.get("fresh coriander").unwrap().source.as_deref(),
            Some("label: AH"),
            "colon in the value survives quoting"
        );

        // existing hand-edited file: comments and entries stay
        let existing = "# my values\nfoods:\n  # from the jar\n  tahini:\n    fat: 55\n";
        let text = append_entry(existing, "oat drink", &entry, path).unwrap();
        assert!(text.starts_with(existing), "{text}");
        let foods = load(&text).unwrap();
        assert_eq!(foods.len(), 2);
        assert!(foods.contains("tahini") && foods.contains("OAT drink"));

        // duplicates and unusable files are refused, not written
        assert!(append_entry(&text, "Tahini", &entry, path).is_err());
        assert!(append_entry("foods: {}\n", "x", &entry, path).is_err());
        let bad = CustomEntry {
            protein: Some(-1.0),
            ..Default::default()
        };
        assert!(append_entry("", "x", &bad, path).is_err());
    }
}
