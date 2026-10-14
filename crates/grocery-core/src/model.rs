//! The normalized recipe model the parser produces.
//!
//! Downstream code (aggregation, nutrition, rendering, the web service) only
//! ever sees these types, not the `cooklang` crate's.

use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recipe {
    /// Path the recipe was read from.
    pub path: PathBuf,
    pub title: String,
    /// Servings the quantities are written for, if the recipe says.
    pub servings: Option<f64>,
    pub tags: Vec<String>,
    pub description: Option<String>,
    pub ingredients: Vec<Ingredient>,
    /// The method, for exporting the recipe as text.
    pub sections: Vec<Section>,
    /// Non-fatal problems found while parsing (unsupported syntax etc.).
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ingredient {
    /// Name as written in the recipe (not yet resolved to a food).
    pub name: String,
    pub amount: Amount,
    pub optional: bool,
    pub note: Option<String>,
}

/// A `== Section ==` of a recipe; recipes without one have a single unnamed section.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Section {
    pub name: Option<String>,
    pub content: Vec<Content>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// An instruction, as readable text ("Cook the spaghetti for 10 minutes").
    Step(String),
    /// A `> note` paragraph.
    Note(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Amount {
    /// No quantity given: `@salt`. Usually "to taste".
    Unspecified,
    /// A quantity that is not a number: "a handful", "snufje".
    Text(String),
    /// A numeric quantity with an optional unit as written ("g", "el", "cloves").
    Value {
        qty: f64,
        unit: Option<String>,
        /// `false` for fixed amounts that must not scale with servings
        /// (Cooklang `@salt{=1%tsp}`).
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
