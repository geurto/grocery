//! The normalized recipe model every parser produces.
//!
//! Downstream code (aggregation, nutrition, the web service) only ever sees
//! these types, so the on-disk format can change without touching it.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    /// Path the recipe was read from (used for wikilinks and the index).
    pub path: PathBuf,
    pub title: String,
    /// Servings the quantities are written for, if the recipe says.
    pub servings: Option<f64>,
    pub ingredients: Vec<Ingredient>,
    /// Non-fatal problems found while parsing (unsupported syntax etc.).
    pub warnings: Vec<String>,
}

impl Recipe {
    /// File name without extension; what Obsidian uses as the link target.
    pub fn link_target(&self) -> String {
        self.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.title.clone())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ingredient {
    /// Name as written in the recipe (not yet resolved to a food).
    pub name: String,
    pub amount: Amount,
    pub optional: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Amount {
    /// No quantity given: `@salt`, `- salt`. Usually "to taste".
    Unspecified,
    /// A quantity that is not a number: "a handful", "snufje".
    Text(String),
    /// A numeric quantity with an optional unit as written ("g", "el", "cloves").
    Value {
        qty: f64,
        unit: Option<String>,
        /// `false` for fixed amounts that must not scale with servings
        /// (Cooklang `@salt{=1%tsp}`, frontmatter `scale: false`).
        scalable: bool,
    },
}

impl Amount {
    pub fn value(qty: f64, unit: Option<&str>) -> Self {
        Amount::Value {
            qty,
            unit: unit.map(str::to_owned),
            scalable: true,
        }
    }

    /// Multiply by `factor` if the amount is scalable.
    pub fn scaled(&self, factor: f64) -> Amount {
        match self {
            Amount::Value {
                qty,
                unit,
                scalable: true,
            } => Amount::Value {
                qty: qty * factor,
                unit: unit.clone(),
                scalable: true,
            },
            other => other.clone(),
        }
    }
}
