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
    /// How long the recipe takes, as stated in its metadata.
    pub time: Option<RecipeTime>,
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
    /// An instruction.
    Step(Step),
    /// A `> note` paragraph.
    Note(String),
}

/// Time from the recipe's metadata (`time:`, or `prep time:` / `cook time:`),
/// in minutes. Only what the recipe says; nothing is estimated.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RecipeTime {
    pub total: Option<f64>,
    pub prep: Option<f64>,
    pub cook: Option<f64>,
}

impl RecipeTime {
    /// `total`, or prep + cook when only those are given.
    pub fn minutes(&self) -> Option<f64> {
        self.total.or(match (self.prep, self.cook) {
            (None, None) => None,
            (p, c) => Some(p.unwrap_or(0.0) + c.unwrap_or(0.0)),
        })
    }
}

/// One instruction, keeping what the text refers to so a viewer can show
/// amounts and timers in place.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Step {
    pub items: Vec<StepItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepItem {
    Text(String),
    /// `@onion{2}`: the amount used in this step (not the recipe's total).
    Ingredient {
        name: String,
        amount: Amount,
    },
    Cookware(String),
    /// `~{10%minutes}`. `text` is how it reads ("10 minutes").
    Timer {
        name: Option<String>,
        text: String,
        duration: Option<Duration>,
    },
    /// An inline quantity that isn't an ingredient, as written.
    Quantity(String),
}

/// A length of time in seconds; `min < max` for a range (`~{2-3%minutes}`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Duration {
    pub min: f64,
    pub max: f64,
}

impl Duration {
    pub fn exact(seconds: f64) -> Self {
        Self {
            min: seconds,
            max: seconds,
        }
    }
}

impl std::ops::Add for Duration {
    type Output = Duration;
    fn add(self, other: Duration) -> Duration {
        Duration {
            min: self.min + other.min,
            max: self.max + other.max,
        }
    }
}

impl Step {
    /// Readable text: ingredients and cookware by name, timers as written.
    /// Amounts are in the ingredient list.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for item in &self.items {
            match item {
                StepItem::Text(t) | StepItem::Cookware(t) | StepItem::Quantity(t) => {
                    out.push_str(t)
                }
                StepItem::Ingredient { name, .. } => out.push_str(name),
                StepItem::Timer { text, .. } => out.push_str(text),
            }
        }
        out.trim().to_owned()
    }

    /// Sum of the step's timers; `None` if it has none with a known length.
    pub fn duration(&self) -> Option<Duration> {
        self.items
            .iter()
            .filter_map(|i| match i {
                StepItem::Timer { duration, .. } => *duration,
                _ => None,
            })
            .reduce(|a, b| a + b)
    }
}

impl Recipe {
    /// Sum of all timers in the method. A lower bound on how long it takes:
    /// chopping and frying "until golden" have no timer.
    pub fn timers_total(&self) -> Option<Duration> {
        self.sections
            .iter()
            .flat_map(|s| &s.content)
            .filter_map(|c| match c {
                Content::Step(step) => step.duration(),
                Content::Note(_) => None,
            })
            .reduce(|a, b| a + b)
    }
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
