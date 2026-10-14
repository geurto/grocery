//! Nutrition estimates for a recipe.
//!
//! Pure: compositions come in as a [`CompositionTable`] (NEVO rows built by
//! `grocery-store`, plus the user's own foods from [`crate::custom`]), so this
//! is testable without a database.
//!
//! Three rules keep the numbers honest:
//! - **Unknown is not zero.** A nutrient NEVO has no value for is absent from
//!   [`Composition::values`]. Totals record which foods lacked it, and the
//!   output marks those totals as lower bounds.
//! - **No guessed weights.** An ingredient is only counted if its amount
//!   converts to grams (or ml) with data from `aliases.yaml`.
//! - **Coverage is reported.** The share of known ingredient weight that was
//!   actually counted, plus lists of what wasn't and why.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

use crate::food::{normalize_name, FoodDb, FoodInfo};
use crate::model::{Amount, Recipe};
use crate::units::{self, Dim};

/// What a composition's values refer to. NEVO uses per 100 g for everything
/// except a few liquids (infant formula) that are per 100 ml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    Per100g,
    Per100ml,
}

/// Where a composition comes from. NEVO rows are RIVM's data, kept exactly
/// as published; custom rows are the user's own (`Food/custom.yaml`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FoodKey {
    Nevo(u32),
    /// Normalized name, as in [`normalize_name`].
    Custom(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Composition {
    pub key: FoodKey,
    pub name: String,
    /// For custom foods: where the numbers came from ("package label").
    pub source: Option<String>,
    pub basis: Basis,
    /// Nutrient code (e.g. "ENERCC", "PROT") → amount per 100 g/ml in the
    /// nutrient's unit. Missing key = not known.
    pub values: HashMap<String, f64>,
}

#[derive(Debug, Clone, Default)]
pub struct CompositionTable {
    foods: HashMap<FoodKey, Composition>,
}

impl CompositionTable {
    pub fn new(foods: impl IntoIterator<Item = Composition>) -> Self {
        Self {
            foods: foods.into_iter().map(|c| (c.key.clone(), c)).collect(),
        }
    }

    /// Add more compositions (the user's own foods); same key replaces.
    pub fn extend(&mut self, foods: impl IntoIterator<Item = Composition>) {
        self.foods
            .extend(foods.into_iter().map(|c| (c.key.clone(), c)));
    }

    pub fn get(&self, key: &FoodKey) -> Option<&Composition> {
        self.foods.get(key)
    }

    /// Shorthand for a NEVO code.
    pub fn nevo(&self, code: u32) -> Option<&Composition> {
        self.foods.get(&FoodKey::Nevo(code))
    }
}

/// NEVO codes referenced by the foods a recipe uses. Lets the caller load
/// only what's needed.
pub fn needed_codes(recipe: &Recipe, db: &FoodDb) -> BTreeSet<u32> {
    recipe
        .ingredients
        .iter()
        .filter_map(|i| db.resolve(&i.name)?.info.nevo)
        .collect()
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Count ingredients marked optional (`@?feta`).
    pub include_optional: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Counted {
        key: FoodKey,
        food: String,
        basis: Basis,
        /// In the composition's basis unit (g or ml).
        amount: f64,
        /// Weight for the coverage figure (ml is used as a proxy when the
        /// density is unknown).
        weight_g: f64,
    },
    /// Name doesn't resolve through `aliases.yaml`.
    NotInAliases { grams: Option<f64> },
    /// Food is known but has neither a `nevo:` code nor a `custom:` entry.
    NoNevoCode { grams: Option<f64> },
    /// `nevo:` points at a code that isn't in the imported dataset.
    NevoNotFound { code: u32, grams: Option<f64> },
    /// `custom:` names a food that isn't in `Food/custom.yaml`.
    CustomNotFound { name: String, grams: Option<f64> },
    /// Mapped, but the amount can't be converted ("1 blik" with no weight).
    NoWeight { unit: String },
    /// "to taste" or a text amount.
    Unquantified,
    /// Optional and not included.
    Optional,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// As written in the recipe.
    pub name: String,
    pub amount: Amount,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Total {
    /// Sum over counted ingredients that have a value.
    pub value: f64,
    /// Counted foods (NEVO names) with no value for this nutrient. Non-empty
    /// means `value` is a lower bound.
    pub missing_in: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Nutrition {
    pub title: String,
    /// Recipe servings; `None` means totals are for the whole recipe.
    pub servings: Option<f64>,
    pub lines: Vec<Line>,
    /// Whole-recipe totals by nutrient code. Includes derived `SALT` (g).
    pub totals: BTreeMap<String, Total>,
}

impl Nutrition {
    fn divisor(&self) -> f64 {
        self.servings.filter(|s| *s > 0.0).unwrap_or(1.0)
    }

    /// Per serving (or whole recipe if no servings). `None` if no counted
    /// ingredient has a value for this nutrient.
    pub fn per_serving(&self, code: &str) -> Option<(f64, &Total)> {
        self.totals.get(code).map(|t| (t.value / self.divisor(), t))
    }

    /// Weight counted / weight known, in grams.
    pub fn weights(&self) -> (f64, f64) {
        let mut counted = 0.0;
        let mut known = 0.0;
        for line in &self.lines {
            match &line.status {
                Status::Counted { weight_g, .. } => {
                    counted += weight_g;
                    known += weight_g;
                }
                Status::NotInAliases { grams: Some(g) }
                | Status::NoNevoCode { grams: Some(g) }
                | Status::CustomNotFound { grams: Some(g), .. }
                | Status::NevoNotFound { grams: Some(g), .. } => known += g,
                _ => {}
            }
        }
        (counted, known)
    }

    /// Fraction (0..=1) of known ingredient weight that was counted.
    pub fn coverage(&self) -> Option<f64> {
        let (counted, known) = self.weights();
        (known > 0.0).then(|| counted / known)
    }
}

pub fn compute(recipe: &Recipe, db: &FoodDb, table: &CompositionTable, opts: Options) -> Nutrition {
    let lines: Vec<Line> = recipe
        .ingredients
        .iter()
        .map(|ing| Line {
            name: ing.name.clone(),
            amount: ing.amount.clone(),
            status: if ing.optional && !opts.include_optional {
                Status::Optional
            } else {
                classify(&ing.name, &ing.amount, db, table)
            },
        })
        .collect();

    // Every nutrient that any counted food has a value for.
    let counted: Vec<(&Composition, f64)> = lines
        .iter()
        .filter_map(|l| match &l.status {
            Status::Counted { key, amount, .. } => Some((table.get(key)?, *amount)),
            _ => None,
        })
        .collect();
    let codes: BTreeSet<&String> = counted.iter().flat_map(|(c, _)| c.values.keys()).collect();

    let mut totals = BTreeMap::new();
    for code in codes {
        let mut total = Total::default();
        for (comp, amount) in &counted {
            match comp.values.get(code) {
                Some(v) => total.value += v * amount / 100.0,
                None if !total.missing_in.contains(&comp.name) => {
                    total.missing_in.push(comp.name.clone())
                }
                None => {}
            }
        }
        totals.insert(code.clone(), total);
    }
    // EU labelling: salt = sodium × 2.5 (mg → g).
    if let Some(na) = totals.get("NA").cloned() {
        totals.insert(
            "SALT".into(),
            Total {
                value: na.value * 2.5 / 1000.0,
                missing_in: na.missing_in,
            },
        );
    }

    Nutrition {
        title: recipe.title.clone(),
        servings: recipe.servings,
        lines,
        totals,
    }
}

fn classify(name: &str, amount: &Amount, db: &FoodDb, table: &CompositionTable) -> Status {
    let Amount::Value { qty, unit, .. } = amount else {
        return Status::Unquantified;
    };
    let q = units::normalize(*qty, unit.as_deref());
    let Some(resolved) = db.resolve(name) else {
        let grams = (q.dim == Dim::Mass).then_some(q.value);
        return Status::NotInAliases { grams };
    };
    let info: &FoodInfo = resolved.info;
    let grams = FoodDb::grams(info, &q);
    // `FoodDb` rejects a food with both, so at most one of these is set.
    let key = match (info.nevo, &info.custom) {
        (Some(code), _) => FoodKey::Nevo(code),
        (None, Some(name)) => FoodKey::Custom(normalize_name(name)),
        (None, None) => return Status::NoNevoCode { grams },
    };
    let Some(comp) = table.get(&key) else {
        return match key {
            FoodKey::Nevo(code) => Status::NevoNotFound { code, grams },
            FoodKey::Custom(name) => Status::CustomNotFound { name, grams },
        };
    };
    let amount = match comp.basis {
        Basis::Per100g => grams,
        Basis::Per100ml => FoodDb::millilitres(info, &q),
    };
    match amount {
        Some(amount) => Status::Counted {
            key,
            food: comp.name.clone(),
            basis: comp.basis,
            amount,
            weight_g: grams.unwrap_or(amount),
        },
        None => Status::NoWeight {
            unit: unit.clone().unwrap_or_else(|| "pc".into()),
        },
    }
}

// ---------------------------------------------------------------------------
// Display

/// One row in the nutrition table.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayRow {
    pub code: String,
    pub label: String,
    pub unit: String,
    /// EU reference intake (Regulation (EU) 1169/2011, Annex XIII), same unit.
    pub ri: Option<f64>,
    /// Sub-item of the row above ("of which saturated").
    pub indent: bool,
    /// Starts a new section with this heading.
    pub section: Option<String>,
}

/// The default table: label-style macros, then vitamins and minerals that
/// have an EU reference intake.
pub fn default_rows() -> Vec<DisplayRow> {
    /// (code, label, unit, EU RI, indented, starts section)
    type Row = (
        &'static str,
        &'static str,
        &'static str,
        Option<f64>,
        bool,
        Option<&'static str>,
    );
    #[rustfmt::skip]
    const ROWS: &[Row] = &[
        ("ENERCC",   "Energy",        "kcal", Some(2000.0), false, None),
        ("PROT",     "Protein",       "g",    Some(50.0),   false, None),
        ("FAT",      "Fat",           "g",    Some(70.0),   false, None),
        ("FASAT",    "saturated",     "g",    Some(20.0),   true,  None),
        ("CHO",      "Carbohydrate",  "g",    Some(260.0),  false, None),
        ("SUGAR",    "sugars",        "g",    Some(90.0),   true,  None),
        ("FIBT",     "Fibre",         "g",    None,         false, None),
        ("SALT",     "Salt",          "g",    Some(6.0),    false, None),
        ("VITA_RAE", "Vitamin A",     "µg",   Some(800.0),  false, Some("Vitamins")),
        ("THIA",     "B1 thiamin",    "mg",   Some(1.1),    false, None),
        ("RIBF",     "B2 riboflavin", "mg",   Some(1.4),    false, None),
        ("NIAEQ",    "B3 niacin",     "mg",   Some(16.0),   false, None),
        ("VITB6",    "B6",            "mg",   Some(1.4),    false, None),
        ("FOL",      "Folate",        "µg",   Some(200.0),  false, None),
        ("VITB12",   "B12",           "µg",   Some(2.5),    false, None),
        ("VITC",     "Vitamin C",     "mg",   Some(80.0),   false, None),
        ("VITD",     "Vitamin D",     "µg",   Some(5.0),    false, None),
        ("VITE",     "Vitamin E",     "mg",   Some(12.0),   false, None),
        ("VITK",     "Vitamin K",     "µg",   Some(75.0),   false, None),
        ("K",        "Potassium",     "mg",   Some(2000.0), false, Some("Minerals")),
        ("CA",       "Calcium",       "mg",   Some(800.0),  false, None),
        ("P",        "Phosphorus",    "mg",   Some(700.0),  false, None),
        ("MG",       "Magnesium",     "mg",   Some(375.0),  false, None),
        ("FE",       "Iron",          "mg",   Some(14.0),   false, None),
        ("ZN",       "Zinc",          "mg",   Some(10.0),   false, None),
        ("CU",       "Copper",        "mg",   Some(1.0),    false, None),
        ("SE",       "Selenium",      "µg",   Some(55.0),   false, None),
        ("ID",       "Iodine",        "µg",   Some(150.0),  false, None),
    ];
    ROWS.iter()
        .map(|&(code, label, unit, ri, indent, section)| DisplayRow {
            code: code.into(),
            label: label.into(),
            unit: unit.into(),
            ri,
            indent,
            section: section.map(Into::into),
        })
        .collect()
}

fn fmt_value(v: f64, unit: &str) -> String {
    if unit == "kcal" || unit == "kJ" || v >= 100.0 {
        format!("{v:.0}")
    } else if v >= 1.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

/// 2.0 → "2", 2.50 → "2.5".
fn fmt_trim(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn fmt_amount(a: &Amount) -> String {
    match a {
        Amount::Unspecified => "to taste".into(),
        Amount::Text(t) => t.clone(),
        Amount::Value { qty, unit, .. } => {
            let q = fmt_trim(*qty);
            match unit {
                Some(u) => format!("{q} {u}"),
                None => q.to_owned(),
            }
        }
    }
}

/// ", 110 g" when the amount wasn't already written in a mass unit.
fn fmt_grams(amount: &Amount, g: Option<f64>) -> String {
    let is_mass = matches!(amount, Amount::Value { qty, unit, .. }
        if units::normalize(*qty, unit.as_deref()).dim == Dim::Mass);
    match g {
        Some(g) if !is_mass => format!(", {g:.0} g"),
        _ => String::new(),
    }
}

/// Plain-text report. `citation` is required by the NEVO conditions of use
/// on every output; the caller builds it from the imported version.
pub fn render_text(
    n: &Nutrition,
    table: &CompositionTable,
    rows: &[DisplayRow],
    citation: &str,
) -> String {
    let mut out = String::new();
    match n.servings {
        Some(s) => {
            let _ = writeln!(
                out,
                "{} — per serving (recipe serves {})\n",
                n.title,
                fmt_trim(s)
            );
        }
        None => {
            let _ = writeln!(out, "{} — whole recipe (no `servings` set)\n", n.title);
        }
    }

    // Footnotes: food → nutrient labels it has no value for.
    let mut gaps: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let any_ri = rows.iter().any(|r| r.ri.is_some());
    let _ = writeln!(
        out,
        "{:<22}{:>12}{}",
        "",
        "amount",
        if any_ri { "    %RI" } else { "" }
    );
    for row in rows {
        if let Some(section) = &row.section {
            let _ = writeln!(out, "\n{section}");
        }
        let label = if row.indent {
            format!("  {}", row.label)
        } else {
            row.label.clone()
        };
        let Some((v, total)) = n.per_serving(&row.code) else {
            let _ = writeln!(out, "{label:<22}{:>12}", "–");
            continue;
        };
        let mark = if total.missing_in.is_empty() {
            " "
        } else {
            "*"
        };
        for food in &total.missing_in {
            gaps.entry(food).or_default().push(&row.label);
        }
        let value = format!("{} {}", fmt_value(v, &row.unit), row.unit);
        let ri = row
            .ri
            .map(|ri| format!("{:>6.0}%", 100.0 * v / ri))
            .unwrap_or_default();
        let _ = writeln!(out, "{label:<22}{value:>12}{mark}{ri}");
    }

    // Coverage.
    let (counted, known) = n.weights();
    let _ = writeln!(out);
    match n.coverage() {
        Some(c) => {
            let _ = writeln!(
                out,
                "Coverage: {:.0}% of known ingredient weight ({counted:.0} of {known:.0} g)",
                c * 100.0
            );
        }
        None => {
            let _ = writeln!(out, "Coverage: no ingredient weights known");
        }
    }
    let mut not_counted = Vec::new();
    let mut no_weight = Vec::new();
    let mut unquantified = Vec::new();
    let mut optional = Vec::new();
    for l in &n.lines {
        let amount = fmt_amount(&l.amount);
        match &l.status {
            Status::Counted { .. } => {}
            Status::NotInAliases { grams } => not_counted.push(format!(
                "{} ({amount}{}): not in aliases.yaml",
                l.name,
                fmt_grams(&l.amount, *grams)
            )),
            Status::NoNevoCode { grams } => not_counted.push(format!(
                "{} ({amount}{}): no nevo or custom entry",
                l.name,
                fmt_grams(&l.amount, *grams)
            )),
            Status::NevoNotFound { code, grams } => not_counted.push(format!(
                "{} ({amount}{}): nevo {code} not in dataset",
                l.name,
                fmt_grams(&l.amount, *grams)
            )),
            Status::CustomNotFound { name, grams } => not_counted.push(format!(
                "{} ({amount}{}): '{name}' not in custom.yaml",
                l.name,
                fmt_grams(&l.amount, *grams)
            )),
            Status::NoWeight { unit } => no_weight.push(format!(
                "{} ({amount}): no gram weight for '{unit}'",
                l.name
            )),
            Status::Unquantified => unquantified.push(l.name.clone()),
            Status::Optional => optional.push(l.name.clone()),
        }
    }
    for (label, items) in [
        ("Not counted", &not_counted),
        ("Weight unknown", &no_weight),
    ] {
        if !items.is_empty() {
            let _ = writeln!(out, "{label}:");
            for i in items {
                let _ = writeln!(out, "  - {i}");
            }
        }
    }
    if !unquantified.is_empty() {
        let _ = writeln!(out, "No amount given: {}", unquantified.join(", "));
    }
    if !optional.is_empty() {
        let _ = writeln!(
            out,
            "Optional, left out: {} (use --with-optional)",
            optional.join(", ")
        );
    }
    if !gaps.is_empty() {
        let _ = writeln!(out, "* lower bound; no value for:");
        for (food, labels) in &gaps {
            let _ = writeln!(out, "  - {food}: {}", labels.join(", "));
        }
    }

    // Per-ingredient breakdown, for checking mappings.
    let _ = writeln!(out, "\nIngredients (whole recipe)");
    for l in &n.lines {
        if let Status::Counted {
            key,
            food,
            basis,
            amount,
            ..
        } = &l.status
        {
            let comp = table.get(key);
            let origin = match key {
                FoodKey::Nevo(code) => code.to_string(),
                FoodKey::Custom(_) => "own value".to_owned(),
            };
            let get = |code: &str| {
                comp.and_then(|c| c.values.get(code))
                    .map(|v| v * amount / 100.0)
            };
            let unit = if *basis == Basis::Per100ml { "ml" } else { "g" };
            let kcal = get("ENERCC")
                .map(|v| format!("{v:.0} kcal"))
                .unwrap_or_else(|| "? kcal".into());
            let prot = get("PROT")
                .map(|v| format!("{v:.1} g prot"))
                .unwrap_or_else(|| "? prot".into());
            let _ = writeln!(
                out,
                "  {:<20} {:>12} → {:>5.0} {unit:<2} {:>9} {:>11}   {food} ({origin})",
                l.name,
                fmt_amount(&l.amount),
                amount,
                kcal,
                prot
            );
        }
    }

    // Own values aren't NEVO data: say so, and where they came from.
    let mut own: Vec<String> = Vec::new();
    for l in &n.lines {
        if let Status::Counted {
            key: key @ FoodKey::Custom(_),
            food,
            ..
        } = &l.status
        {
            let source = table.get(key).and_then(|c| c.source.as_deref());
            let text = match source {
                Some(src) => format!("{food} ({src})"),
                None => food.clone(),
            };
            if !own.contains(&text) {
                own.push(text);
            }
        }
    }
    if !own.is_empty() {
        let _ = writeln!(
            out,
            "\nOwn values from Food/custom.yaml, not NEVO data: {}",
            own.join("; ")
        );
    }

    let _ = writeln!(out, "\n{citation}");
    out
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::model::Ingredient;

    fn comp(code: u32, name: &str, basis: Basis, values: &[(&str, f64)]) -> Composition {
        Composition {
            key: FoodKey::Nevo(code),
            name: name.into(),
            source: None,
            basis,
            values: values.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect(),
        }
    }

    fn table() -> CompositionTable {
        CompositionTable::new([
            comp(
                1,
                "Pasta raw",
                Basis::Per100g,
                &[("ENERCC", 350.0), ("PROT", 12.0), ("NA", 10.0)],
            ),
            comp(
                2,
                "Onion raw",
                Basis::Per100g,
                &[("ENERCC", 40.0), ("PROT", 1.0), ("NA", 4.0), ("VITC", 8.0)],
            ),
            comp(
                3,
                "Oil",
                Basis::Per100g,
                &[("ENERCC", 900.0), ("PROT", 0.0)],
            ),
            comp(4, "Formula", Basis::Per100ml, &[("ENERCC", 65.0)]),
        ])
    }

    fn db() -> FoodDb {
        FoodDb::from_yaml(
            "foods:\n\
             \x20 pasta: { nevo: 1 }\n\
             \x20 onion: { nevo: 2, piece_g: 100, synonyms: [ui] }\n\
             \x20 oil: { nevo: 3, density: 0.9 }\n\
             \x20 formula: { nevo: 4 }\n\
             \x20 tomato: { nevo: 99 }\n\
             \x20 herbs: {}\n\
             \x20 garlic: { nevo: 2 }\n",
            Path::new("aliases.yaml"),
        )
        .unwrap()
    }

    fn recipe(servings: Option<f64>, ings: &[(&str, Amount, bool)]) -> Recipe {
        Recipe {
            path: PathBuf::from("R.cook"),
            title: "R".into(),
            servings,
            ingredients: ings
                .iter()
                .map(|(n, a, opt)| Ingredient {
                    name: (*n).into(),
                    amount: a.clone(),
                    optional: *opt,
                    note: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    fn v(q: f64, u: Option<&str>) -> Amount {
        Amount::value(q, u)
    }

    #[test]
    fn sums_per_serving_with_unit_conversion() {
        let r = recipe(
            Some(2.0),
            &[
                ("pasta", v(200.0, Some("g")), false),
                ("ui", v(1.0, None), false),          // 100 g via piece_g
                ("oil", v(2.0, Some("tbsp")), false), // 30 ml × 0.9 = 27 g
            ],
        );
        let n = compute(&r, &db(), &table(), Options::default());
        let (kcal, t) = n.per_serving("ENERCC").unwrap();
        // (700 + 40 + 243) / 2
        assert!((kcal - 491.5).abs() < 1e-9, "{kcal}");
        assert!(t.missing_in.is_empty());
        assert_eq!(n.coverage(), Some(1.0));
    }

    #[test]
    fn unknown_is_not_zero() {
        let r = recipe(
            None,
            &[
                ("pasta", v(100.0, Some("g")), false),
                ("onion", v(100.0, Some("g")), false),
            ],
        );
        let n = compute(&r, &db(), &table(), Options::default());
        let (vitc, t) = n.per_serving("VITC").unwrap();
        assert_eq!(vitc, 8.0);
        assert_eq!(
            t.missing_in,
            ["Pasta raw"],
            "pasta has no VITC value: lower bound"
        );
        assert!(
            n.per_serving("VITD").is_none(),
            "no food has it: unknown, not 0"
        );
        // salt derived from sodium: (10 + 4) mg × 2.5 / 1000
        assert!((n.per_serving("SALT").unwrap().0 - 0.035).abs() < 1e-12);
    }

    #[test]
    fn statuses_and_coverage() {
        let r = recipe(
            Some(1.0),
            &[
                ("pasta", v(100.0, Some("g")), false),
                ("passata", v(300.0, Some("g")), false), // not in aliases, weight known
                ("herbs", v(10.0, Some("g")), false),    // no nevo code
                ("tomato", v(100.0, Some("g")), false),  // code missing from dataset
                ("garlic", v(2.0, Some("cloves")), false), // mapped but no clove weight
                ("salt", Amount::Unspecified, false),
                ("onion", v(1.0, None), true), // optional
            ],
        );
        let n = compute(&r, &db(), &table(), Options::default());
        let statuses: Vec<_> = n.lines.iter().map(|l| &l.status).collect();
        assert!(matches!(statuses[0], Status::Counted { amount, .. } if *amount == 100.0));
        assert_eq!(statuses[1], &Status::NotInAliases { grams: Some(300.0) });
        assert_eq!(statuses[2], &Status::NoNevoCode { grams: Some(10.0) });
        assert_eq!(
            statuses[3],
            &Status::NevoNotFound {
                code: 99,
                grams: Some(100.0)
            }
        );
        assert_eq!(
            statuses[4],
            &Status::NoWeight {
                unit: "cloves".into()
            }
        );
        assert_eq!(statuses[5], &Status::Unquantified);
        assert_eq!(statuses[6], &Status::Optional);
        assert_eq!(n.weights(), (100.0, 510.0));
        assert_eq!(n.per_serving("ENERCC").unwrap().0, 350.0);

        let with_opt = compute(
            &r,
            &db(),
            &table(),
            Options {
                include_optional: true,
            },
        );
        assert_eq!(with_opt.per_serving("ENERCC").unwrap().0, 390.0);
    }

    #[test]
    fn per_100ml_foods_need_millilitres() {
        let r = recipe(None, &[("formula", v(200.0, Some("ml")), false)]);
        let n = compute(&r, &db(), &table(), Options::default());
        assert_eq!(n.per_serving("ENERCC").unwrap().0, 130.0);
        // grams without a density can't become ml
        let r = recipe(None, &[("formula", v(200.0, Some("g")), false)]);
        let n = compute(&r, &db(), &table(), Options::default());
        assert!(matches!(n.lines[0].status, Status::NoWeight { .. }));
    }

    #[test]
    fn needed_codes_only_lists_mapped_foods() {
        let r = recipe(
            None,
            &[
                ("pasta", Amount::Unspecified, false),
                ("ui", Amount::Unspecified, false),
                ("x", Amount::Unspecified, false),
            ],
        );
        assert_eq!(
            needed_codes(&r, &db()).into_iter().collect::<Vec<_>>(),
            [1, 2]
        );
    }

    #[test]
    fn custom_foods_are_counted_and_labelled_as_own_values() {
        use crate::custom::CustomFoods;

        let custom = CustomFoods::from_yaml(
            "foods:\n  Fresh Coriander:\n    source: AH label\n    energy_kcal: 23\n    protein: 2.1\n",
            Path::new("custom.yaml"),
        )
        .unwrap();
        let db = FoodDb::from_yaml(
            "foods:\n\
             \x20 pasta: { nevo: 1 }\n\
             \x20 coriander: { custom: fresh coriander }\n\
             \x20 cumin: { custom: nope }\n",
            Path::new("aliases.yaml"),
        )
        .unwrap();
        let mut table = table();
        table.extend(custom.compositions());
        let r = recipe(
            Some(1.0),
            &[
                ("pasta", v(100.0, Some("g")), false),
                ("coriander", v(50.0, Some("g")), false),
                ("cumin", v(5.0, Some("g")), false),
            ],
        );
        let n = compute(&r, &db, &table, Options::default());
        assert!(matches!(
            &n.lines[1].status,
            Status::Counted { key: FoodKey::Custom(k), .. } if k == "fresh coriander"
        ));
        assert_eq!(
            n.lines[2].status,
            Status::CustomNotFound {
                name: "nope".into(),
                grams: Some(5.0)
            }
        );
        // 350 + 50 × 23 / 100
        assert!((n.per_serving("ENERCC").unwrap().0 - 361.5).abs() < 1e-9);
        // the label gave no sodium: unknown, so salt is a lower bound naming the food
        assert_eq!(
            n.per_serving("SALT").unwrap().1.missing_in,
            ["Fresh Coriander"]
        );

        let text = render_text(&n, &table, &default_rows(), "CITATION");
        assert!(
            text.contains(
                "Own values from Food/custom.yaml, not NEVO data: Fresh Coriander (AH label)"
            ),
            "{text}"
        );
        assert!(text.contains("Fresh Coriander (own value)"), "{text}");
        assert!(text.contains("'nope' not in custom.yaml"), "{text}");
        assert!(text.trim_end().ends_with("CITATION"));
    }

    #[test]
    fn a_food_cant_have_both_nevo_and_custom() {
        let err = FoodDb::from_yaml(
            "foods:\n  x: { nevo: 1, custom: y }\n",
            Path::new("aliases.yaml"),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("both `nevo` and `custom`"),
            "{err}"
        );
    }

    #[test]
    fn report_mentions_gaps_coverage_and_citation() {
        let r = recipe(
            Some(2.0),
            &[
                ("pasta", v(200.0, Some("g")), false),
                ("passata", v(200.0, Some("g")), false),
                ("onion", v(1.0, None), false),
            ],
        );
        let n = compute(&r, &db(), &table(), Options::default());
        let text = render_text(&n, &table(), &default_rows(), "Based on data from TEST");
        assert!(
            text.starts_with("R — per serving (recipe serves 2)"),
            "{text}"
        );
        assert!(
            text.contains("Coverage: 60% of known ingredient weight (300 of 500 g)"),
            "{text}"
        );
        assert!(
            text.contains("passata (200 g): not in aliases.yaml"),
            "{text}"
        );
        assert!(text.contains("- Pasta raw: Vitamin C"), "{text}");
        assert!(text.trim_end().ends_with("Based on data from TEST"));
    }
}
