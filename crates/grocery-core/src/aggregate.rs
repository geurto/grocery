//! Turn a set of recipes into one shopping list.
//!
//! Per ingredient: scale to the requested servings → resolve the name through
//! the food db → normalize the unit → accumulate per food and dimension.
//! When a food shows up in more than one dimension (2 onions + 100 g onion),
//! the buckets are merged *only* if every bucket can be converted to grams
//! with data from `aliases.yaml`. Otherwise they stay separate lines: an
//! honest "2 + 100 g" beats a guess.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use crate::food::{normalize_name, FoodDb, FoodInfo, Pantry};
use crate::model::{Amount, Recipe};
use crate::units::{self, Dim};

/// One recipe to cook, optionally at a different number of servings.
#[derive(Debug, Clone, Copy)]
pub struct Selection<'a> {
    pub recipe: &'a Recipe,
    /// `None` = cook as written.
    pub servings: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShoppingList {
    pub sources: Vec<Source>,
    pub items: Vec<Item>,
    /// Foods left off because they're in the pantry.
    pub pantry_skipped: Vec<String>,
    /// Ingredient names with no entry in `aliases.yaml` (sorted, unique).
    pub unmapped: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub title: String,
    pub path: PathBuf,
    /// Servings cooked; `None` if neither the recipe nor the request says.
    pub servings: Option<f64>,
    pub factor: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Canonical food name if resolved, otherwise the name as written.
    pub name: String,
    /// The names recipes use for it ("krieltjes" for the food "kruimige
    /// aardappelen"), one per distinct spelling. Empty for hand-added items
    /// and lines saved before this existed.
    pub written: Vec<String>,
    pub aisle: Option<String>,
    /// `None` for "to taste" / text-only amounts.
    pub quantity: Option<Qty>,
    /// Non-numeric amounts ("a handful"), de-duplicated.
    pub notes: Vec<String>,
    /// True only if every recipe marks it optional.
    pub optional: bool,
    /// Titles of the recipes that need this item.
    pub recipes: Vec<String>,
    /// Whether the name resolved to a food in `aliases.yaml`.
    pub known: bool,
    /// Ticked off while shopping. Always `false` from [`build`]; the web
    /// service stores it.
    pub checked: bool,
    /// Who needs how much of `quantity`: each recipe, and what was added by
    /// hand. Empty for lines saved before parts were recorded.
    pub parts: Vec<Part>,
}

/// One recipe's share of a list line, or an amount added by hand.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub source: PartSource,
    /// In its own unit; `None` for "to taste", a text amount, or an item
    /// typed without an amount.
    pub quantity: Option<Qty>,
    /// Text amounts ("a handful") from this source.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartSource {
    /// Path as in [`Source::path`].
    Recipe {
        path: String,
        title: String,
    },
    Hand,
    /// What the line held before parts were recorded, by the recipe titles
    /// it had then (empty: added by hand).
    Earlier {
        titles: Vec<String>,
    },
}

impl Part {
    pub fn hand(quantity: Option<Qty>) -> Self {
        Part {
            source: PartSource::Hand,
            quantity,
            notes: vec![],
        }
    }
}

/// Add `part` to `parts`: to a part from the same source in the same unit
/// when there is one.
fn add_part(parts: &mut Vec<Part>, part: Part) {
    let same = parts.iter_mut().find(|p| {
        p.source == part.source
            && match (&p.quantity, &part.quantity) {
                (Some(a), Some(b)) => a.dim == b.dim,
                (None, None) => true,
                _ => false,
            }
    });
    match same {
        Some(p) => {
            if let (Some(a), Some(b)) = (&mut p.quantity, &part.quantity) {
                a.value += b.value;
            }
            for n in part.notes {
                if !p.notes.contains(&n) {
                    p.notes.push(n);
                }
            }
        }
        None => parts.push(part),
    }
}

/// `q` in the unit of `like`: the same dimension, or by way of grams.
pub fn convert(q: &Qty, like: &Qty, info: Option<&FoodInfo>) -> Option<f64> {
    if q.dim == like.dim {
        return Some(q.value);
    }
    let grams = |dim: &Dim, value: f64| {
        FoodDb::grams(
            info?,
            &units::Normalized {
                dim: dim.clone(),
                value,
            },
        )
    };
    let per_unit = grams(&like.dim, 1.0).filter(|g| *g > 0.0)?;
    Some(grams(&q.dim, q.value)? / per_unit)
}

/// A line's parts added up per source, each in the line's own unit where
/// it converts: what the list shows under a line ("Curry 6 · added by
/// hand 8").
pub fn breakdown(item: &Item, db: &FoodDb) -> Vec<Part> {
    let info = db.resolve(&item.name).map(|r| r.info);
    let mut out: Vec<Part> = Vec::new();
    for p in &item.parts {
        let quantity = match (&p.quantity, &item.quantity) {
            (Some(q), Some(line)) => Some(match convert(q, line, info) {
                Some(value) => Qty {
                    value,
                    ..line.clone()
                },
                None => q.clone(),
            }),
            (q, _) => q.clone(),
        };
        add_part(
            &mut out,
            Part {
                source: p.source.clone(),
                quantity,
                notes: p.notes.clone(),
            },
        );
    }
    out
}

impl Item {
    /// What to print on the list: the name the recipe uses, not the food's
    /// own name, so "krieltjes" stays "krieltjes". Two recipes that name the
    /// same food differently get both.
    pub fn shown(&self) -> String {
        if self.written.is_empty() {
            self.name.clone()
        } else {
            self.written.join(" / ")
        }
    }
}

/// Remember a spelling, unless the item has it already (or a plural of it).
fn add_written(written: &mut Vec<String>, name: &str, db: &FoodDb) {
    let name = normalize_name(name);
    let key = |n: &str| {
        db.resolve(n)
            .map_or_else(|| n.to_owned(), |r| r.matched.to_owned())
    };
    let new = key(&name);
    if !name.is_empty() && !written.iter().any(|w| key(w) == new) {
        written.push(name);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Qty {
    pub dim: Dim,
    /// In g (Mass), ml (Volume), pieces (Count) or the named unit.
    pub value: f64,
    /// For named units: the unit as first written ("cloves", "blik").
    pub label: Option<String>,
}

impl Qty {
    /// An amount as written ("500 g", "2", "1 blik") in its dimension.
    pub fn of(qty: f64, unit: Option<&str>) -> Self {
        let n = units::normalize(qty, unit);
        let label = matches!(n.dim, Dim::Named(_)).then(|| unit.unwrap_or_default().to_owned());
        Qty {
            dim: n.dim,
            value: n.value,
            label,
        }
    }
}

#[derive(Default)]
struct Acc<'a> {
    name: String,
    aisle: Option<String>,
    info: Option<&'a FoodInfo>,
    buckets: Vec<Qty>,
    notes: Vec<String>,
    all_optional: bool,
    recipes: Vec<String>,
    written: Vec<String>,
    parts: Vec<Part>,
}

impl Acc<'_> {
    fn add(&mut self, dim: Dim, value: f64, label: Option<String>) {
        match self.buckets.iter_mut().find(|b| b.dim == dim) {
            Some(b) => b.value += value,
            None => self.buckets.push(Qty { dim, value, label }),
        }
    }

    /// Merge all buckets if (and only if) every one converts to grams.
    ///
    /// The merged amount is expressed in the first *shop-able* unit seen
    /// (pieces, cloves, cans) when there is one, because that's how you buy
    /// it: 2 onions + 100 g onion → 3 onions, not 320 g. Otherwise grams.
    fn merge_if_possible(&mut self) {
        if self.buckets.len() < 2 {
            return;
        }
        let Some(info) = self.info else { return };
        let to_grams = |dim: &Dim, value: f64| {
            FoodDb::grams(
                info,
                &units::Normalized {
                    dim: dim.clone(),
                    value,
                },
            )
        };
        let Some(total_g) = self
            .buckets
            .iter()
            .map(|b| to_grams(&b.dim, b.value))
            .sum::<Option<f64>>()
        else {
            return;
        };

        let unit_target = self
            .buckets
            .iter()
            .filter(|b| matches!(b.dim, Dim::Count | Dim::Named(_)))
            .find_map(|b| Some((b, to_grams(&b.dim, 1.0).filter(|g| *g > 0.0)?)));
        self.buckets = vec![match unit_target {
            Some((b, grams_per_unit)) => Qty {
                value: total_g / grams_per_unit,
                ..b.clone()
            },
            None => Qty {
                dim: Dim::Mass,
                value: total_g,
                label: None,
            },
        }];
    }
}

pub fn build(selections: &[Selection<'_>], db: &FoodDb, pantry: &Pantry) -> ShoppingList {
    let mut order: Vec<String> = Vec::new();
    let mut accs: HashMap<String, Acc<'_>> = HashMap::new();
    let mut unmapped = BTreeSet::new();
    let mut sources = Vec::new();
    let mut warnings = Vec::new();

    for sel in selections {
        let recipe = sel.recipe;
        let (factor, servings) = scale_factor(recipe, sel.servings, &mut warnings);
        sources.push(Source {
            title: recipe.title.clone(),
            path: recipe.path.clone(),
            servings,
            factor,
        });
        warnings.extend(
            recipe
                .warnings
                .iter()
                .map(|w| format!("{}: {w}", recipe.title)),
        );

        for ing in &recipe.ingredients {
            let resolved = db.resolve(&ing.name);
            let key = match resolved {
                Some(r) => r.canonical.to_owned(),
                None => normalize_name(&ing.name),
            };
            if key.is_empty() {
                continue;
            }

            let acc = accs.entry(key.clone()).or_insert_with(|| {
                order.push(key.clone());
                Acc {
                    name: resolved
                        .map_or_else(|| ing.name.trim().to_owned(), |r| r.canonical.to_owned()),
                    aisle: resolved.and_then(|r| r.info.aisle.clone()),
                    info: resolved.map(|r| r.info),
                    all_optional: true,
                    ..Default::default()
                }
            });
            acc.all_optional &= ing.optional;
            if resolved.is_some() {
                add_written(&mut acc.written, &ing.name, db);
            }
            if !acc.recipes.contains(&recipe.title) {
                acc.recipes.push(recipe.title.clone());
            }

            let mut part = Part {
                source: PartSource::Recipe {
                    path: recipe.path.to_string_lossy().into_owned(),
                    title: recipe.title.clone(),
                },
                quantity: None,
                notes: vec![],
            };
            match ing.amount.scaled(factor) {
                Amount::Unspecified => {}
                Amount::Text(t) => {
                    if !acc.notes.contains(&t) {
                        acc.notes.push(t.clone());
                    }
                    part.notes.push(t);
                }
                Amount::Value { qty, unit, .. } => {
                    let q = Qty::of(qty, unit.as_deref());
                    acc.add(q.dim.clone(), q.value, q.label.clone());
                    part.quantity = Some(q);
                }
            }
            add_part(&mut acc.parts, part);

            if resolved.is_none() {
                unmapped.insert(key);
            }
        }
    }

    let mut items = Vec::new();
    let mut pantry_skipped = Vec::new();
    for key in order {
        let mut acc = accs
            .remove(&key)
            .expect("every key in `order` has an accumulator");
        if pantry.contains(&key, db) {
            unmapped.remove(&key);
            pantry_skipped.push(acc.name);
            continue;
        }
        acc.merge_if_possible();
        let known = acc.info.is_some();
        let make = |quantity: Option<Qty>, notes: Vec<String>, parts: Vec<Part>| Item {
            name: acc.name.clone(),
            written: acc.written.clone(),
            aisle: acc.aisle.clone(),
            quantity,
            notes,
            optional: acc.all_optional,
            recipes: acc.recipes.clone(),
            known,
            checked: false,
            parts,
        };
        if acc.buckets.len() < 2 {
            let quantity = acc.buckets.first().cloned();
            items.push(make(quantity, acc.notes.clone(), acc.parts.clone()));
        } else {
            // Text notes (and parts without an amount) ride along on the
            // first quantity line; amounts go with the line in their unit.
            let mut notes = Some(acc.notes.clone());
            for (i, b) in acc.buckets.iter().enumerate() {
                let parts = acc
                    .parts
                    .iter()
                    .filter(|p| match &p.quantity {
                        Some(q) => q.dim == b.dim,
                        None => i == 0,
                    })
                    .cloned()
                    .collect();
                items.push(make(
                    Some(b.clone()),
                    notes.take().unwrap_or_default(),
                    parts,
                ));
            }
        }
    }

    sort_items(&mut items, db.aisle_order());

    ShoppingList {
        sources,
        items,
        pantry_skipped,
        unmapped: unmapped.into_iter().collect(),
        warnings,
    }
}

fn scale_factor(
    recipe: &Recipe,
    target: Option<f64>,
    warnings: &mut Vec<String>,
) -> (f64, Option<f64>) {
    match (target, recipe.servings) {
        (None, base) => (1.0, base),
        (Some(t), _) if !(t > 0.0 && t.is_finite()) => {
            warnings.push(format!("{}: ignoring invalid servings {t}", recipe.title));
            (1.0, recipe.servings)
        }
        (Some(t), Some(base)) if base > 0.0 => (t / base, Some(t)),
        (Some(t), _) => {
            warnings.push(format!(
                "{}: recipe has no `servings`, so it can't be scaled to {t}; using it as written",
                recipe.title
            ));
            (1.0, None)
        }
    }
}

/// Configured aisles first (in walking order), then other aisles
/// alphabetically, then items without an aisle. Alphabetical within an aisle.
pub fn sort_items(items: &mut [Item], aisle_order: &[String]) {
    items.sort_by_cached_key(|i| sort_key(i, aisle_order));
}

/// Where an item goes on a list: aisles in walking order, then aisles not
/// in the walking order (alphabetically), then items without an aisle; by
/// name within an aisle.
pub fn sort_key(item: &Item, aisle_order: &[String]) -> (usize, String, String) {
    let (rank, unlisted) = match &item.aisle {
        Some(a) => {
            let norm = normalize_name(a);
            match aisle_order.iter().position(|o| normalize_name(o) == norm) {
                Some(i) => (i, String::new()),
                None => (aisle_order.len(), norm),
            }
        }
        None => (usize::MAX, String::new()),
    };
    (rank, unlisted, item.name.to_lowercase())
}

/// An item someone typed in ("melk", "toilet paper"): the food and aisle
/// when `aliases.yaml` knows it, else as typed. A known food keeps the word
/// that was typed ("krieltjes"), so the list says what was meant.
pub fn hand_item(name: &str, db: &FoodDb) -> Item {
    let name = name.trim();
    let resolved = db.resolve(name);
    Item {
        name: resolved.map_or_else(|| name.to_owned(), |r| r.canonical.to_owned()),
        written: resolved.map(|_| normalize_name(name)).into_iter().collect(),
        aisle: resolved.and_then(|r| r.info.aisle.clone()),
        quantity: None,
        notes: vec![],
        optional: false,
        recipes: vec![],
        known: resolved.is_some(),
        checked: false,
        parts: vec![Part::hand(None)],
    }
}

/// [`hand_item`] with an amount ("500 g kaas").
pub fn hand_amount(name: &str, quantity: Option<Qty>, db: &FoodDb) -> Item {
    Item {
        quantity: quantity.clone(),
        parts: vec![Part::hand(quantity)],
        ..hand_item(name, db)
    }
}

/// Add `incoming` (from [`build`]) to a list's `items`. An item for the same
/// food joins the existing line when the amounts add up: same unit, or both
/// convertible to grams. Otherwise it gets its own line. A ticked line that
/// now needs more is unticked, so it isn't forgotten in the shop.
///
/// Existing items keep their index (so callers can keep their ids); new
/// ones are appended. Returns the indexes of existing items that changed.
pub fn merge_into(items: &mut Vec<Item>, incoming: Vec<Item>, db: &FoodDb) -> Vec<usize> {
    let existing = items.len();
    let mut changed = Vec::new();
    for new in incoming {
        let info = db.resolve(&new.name).map(|r| r.info);
        let grams = |q: &Qty| {
            info.and_then(|i| {
                FoodDb::grams(
                    i,
                    &units::Normalized {
                        dim: q.dim.clone(),
                        value: q.value,
                    },
                )
            })
        };
        // By food, not by spelling: a line saved as "onion" before the food
        // was renamed "ui" is still the same food.
        let food_of = |name: &str| {
            db.resolve(name)
                .map_or_else(|| normalize_name(name), |r| r.canonical.to_owned())
        };
        let new_food = food_of(&new.name);
        let same_food = |i: &Item| food_of(&i.name) == new_food;
        // Best match first: the same unit, then convertible, then a line
        // without an amount (a hand-added "milk").
        let target = (0..items.len())
            .filter(|&i| same_food(&items[i]))
            .min_by_key(|&i| match (&items[i].quantity, &new.quantity) {
                (Some(a), Some(b)) if a.dim == b.dim => 0,
                (Some(a), Some(b)) if grams(a).is_some() && grams(b).is_some() => 1,
                (None, _) | (_, None) => 2,
                _ => 3,
            })
            .filter(|&i| match (&items[i].quantity, &new.quantity) {
                (Some(a), Some(b)) => a.dim == b.dim || (grams(a).is_some() && grams(b).is_some()),
                _ => true,
            });

        let Some(i) = target else {
            items.push(new);
            continue;
        };
        let item = &mut items[i];
        if item.parts.is_empty() {
            // Saved before parts were recorded: keep what it held as one.
            item.parts.push(Part {
                source: PartSource::Earlier {
                    titles: item.recipes.clone(),
                },
                quantity: item.quantity.clone(),
                notes: item.notes.clone(),
            });
        }
        for p in new.parts {
            add_part(&mut item.parts, p);
        }
        let more = new.quantity.is_some();
        // Typing it in doesn't rename a line that's there ("ui" stays
        // "onion" when that's what the recipe says).
        let typed = new.recipes.is_empty();
        item.quantity = match (item.quantity.take(), new.quantity) {
            (Some(a), Some(b)) if a.dim == b.dim => Some(Qty {
                value: a.value + b.value,
                ..a
            }),
            (Some(a), Some(b)) => match (grams(&a), grams(&b)) {
                (Some(x), Some(y)) => Some(Qty {
                    dim: Dim::Mass,
                    value: x + y,
                    label: None,
                }),
                _ => Some(a), // unreachable: filtered above
            },
            (a, b) => a.or(b),
        };
        for n in new.notes {
            if !item.notes.contains(&n) {
                item.notes.push(n);
            }
        }
        for r in new.recipes {
            if !item.recipes.contains(&r) {
                item.recipes.push(r);
            }
        }
        if !new.written.is_empty() && !typed {
            // A line without spellings (hand-added, or saved earlier) was
            // the food's own name.
            if item.written.is_empty() {
                item.written.push(item.name.clone());
            }
            for w in &new.written {
                add_written(&mut item.written, w, db);
            }
        }
        item.known |= new.known;
        item.optional &= new.optional;
        if item.aisle.is_none() {
            item.aisle = new.aisle;
        }
        if more {
            item.checked = false;
        }
        if i < existing && !changed.contains(&i) {
            changed.push(i);
        }
    }
    changed.sort_unstable();
    changed
}

/// What [`remove_recipe`] did to a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removal {
    Same,
    Changed,
    /// Nothing needs it any more: take it off the list.
    Gone,
}

/// Take a recipe off a list's `items`: its parts are taken out of every
/// line, a line nothing else needs is [`Removal::Gone`], and the others are
/// added up again from what's left. Lines saved before parts were recorded
/// don't know the recipe's share; for those, `legacy` (the recipe's own
/// items at the amounts it was put on the list with) is subtracted instead.
///
/// Returns one [`Removal`] per item, in order; `items` keeps its length.
pub fn remove_recipe(
    items: &mut [Item],
    path: &str,
    title: &str,
    legacy: &[Item],
    db: &FoodDb,
) -> Vec<Removal> {
    let food_of = |name: &str| {
        db.resolve(name)
            .map_or_else(|| normalize_name(name), |r| r.canonical.to_owned())
    };
    let is_it = |p: &Part| matches!(&p.source, PartSource::Recipe { path: pp, .. } if pp == path);
    items
        .iter_mut()
        .map(|item| {
            let had_part = item.parts.iter().any(is_it);
            let named = item.recipes.iter().any(|t| t == title);
            if !had_part && !named {
                return Removal::Same;
            }
            let info = db.resolve(&item.name).map(|r| r.info);
            let food = food_of(&item.name);
            let theirs: Vec<&Qty> = legacy
                .iter()
                .filter(|l| food_of(&l.name) == food)
                .filter_map(|l| l.quantity.as_ref())
                .collect();
            // Its share of an amount it isn't a part of on its own.
            let subtract = |q: &mut Option<Qty>| {
                if let Some(line) = q {
                    for t in &theirs {
                        if let Some(v) = convert(t, line, info) {
                            line.value -= v;
                        }
                    }
                    if line.value <= 1e-6 {
                        *q = None;
                    }
                }
            };

            item.recipes.retain(|t| t != title);
            if item.parts.is_empty() {
                // Saved before parts: all we know is the total.
                if item.recipes.is_empty() {
                    return Removal::Gone;
                }
                subtract(&mut item.quantity);
                return Removal::Changed;
            }
            item.parts.retain(|p| !is_it(p));
            item.parts.retain_mut(|p| {
                let PartSource::Earlier { titles } = &mut p.source else {
                    return true;
                };
                if !titles.iter().any(|t| t == title) {
                    return true;
                }
                titles.retain(|t| t != title);
                subtract(&mut p.quantity);
                // Only its recipes held it, and they're all gone.
                !(titles.is_empty() && p.quantity.is_none())
            });
            if item.parts.is_empty() {
                return Removal::Gone;
            }
            if let Some(line) = &item.quantity {
                let left: Vec<f64> = item
                    .parts
                    .iter()
                    .filter_map(|p| p.quantity.as_ref())
                    .filter_map(|q| convert(q, line, info))
                    .collect();
                item.quantity = (!left.is_empty()).then(|| Qty {
                    value: left.iter().sum(),
                    ..line.clone()
                });
            }
            item.notes = Vec::new();
            for n in item.parts.iter().flat_map(|p| &p.notes) {
                if !item.notes.contains(n) {
                    item.notes.push(n.clone());
                }
            }
            Removal::Changed
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::model::Ingredient;

    fn recipe(title: &str, servings: Option<f64>, ings: &[(&str, Amount)]) -> Recipe {
        Recipe {
            path: PathBuf::from(format!("Recipes/{title}.cook")),
            title: title.into(),
            servings,
            ingredients: ings
                .iter()
                .map(|(n, a)| Ingredient {
                    name: (*n).into(),
                    amount: a.clone(),
                    optional: false,
                    note: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    fn v(qty: f64, unit: Option<&str>) -> Amount {
        Amount::value(qty, unit)
    }

    fn db() -> FoodDb {
        FoodDb::from_yaml(
            "aisles: [groente, pasta]\n\
             foods:\n\
             \x20 onion: { piece_g: 110, aisle: groente, synonyms: [ui] }\n\
             \x20 spaghetti: { aisle: pasta }\n\
             \x20 garlic: { units: { clove: 5 }, aisle: groente }\n\
             \x20 milk: {}\n\
             \x20 salt: {}\n",
            Path::new("aliases.yaml"),
        )
        .unwrap()
    }

    fn qty(dim: Dim, value: f64) -> Option<Qty> {
        Some(Qty {
            dim,
            value,
            label: None,
        })
    }

    #[test]
    fn hand_items_get_the_foods_identity_and_aisle() {
        let ui = hand_item(" Ui ", &db());
        assert_eq!(
            (ui.name.as_str(), ui.aisle.as_deref(), ui.known),
            ("onion", Some("groente"), true)
        );
        // Shown as typed, not as the food is called.
        assert_eq!(ui.shown(), "ui");
        let tp = hand_item("toilet paper", &db());
        assert_eq!(
            (tp.name.as_str(), tp.aisle, tp.known),
            ("toilet paper", None, false)
        );
    }

    #[test]
    fn merging_a_recipe_into_a_list() {
        let db = db();
        let mut milk = hand_item("milk", &db);
        milk.checked = true;
        let mut onions = hand_item("onion", &db);
        onions.quantity = qty(Dim::Count, 2.0);
        onions.checked = true;
        onions.recipes = vec!["Soup".into()];
        let mut salt = hand_item("salt", &db);
        salt.checked = true;
        let mut items = vec![milk, onions, salt];

        let recipe = recipe(
            "Curry",
            Some(2.0),
            &[
                ("ui", Amount::value(220.0, Some("g"))),
                ("milk", Amount::value(500.0, Some("ml"))),
                ("salt", Amount::Unspecified),
                ("garlic", Amount::value(2.0, Some("cloves"))),
            ],
        );
        let built = build(
            &[Selection {
                recipe: &recipe,
                servings: None,
            }],
            &db,
            &Pantry::default(),
        );
        let changed = merge_into(&mut items, built.items, &db);
        assert_eq!(changed, [0, 1, 2]);

        // 2 onions (220 g) + 220 g = 440 g: converted, since the food has a piece weight.
        assert_eq!(items[1].quantity, qty(Dim::Mass, 440.0));
        assert_eq!(items[1].recipes, ["Soup", "Curry"]);
        assert!(!items[1].checked, "more is needed now: unticked");
        // A hand-added line without an amount takes the recipe's amount.
        assert_eq!(items[0].quantity, qty(Dim::Volume, 500.0));
        assert!(!items[0].checked);
        // "to taste" adds nothing to buy: stays ticked.
        assert!(items[2].checked);
        assert_eq!(items[2].recipes, ["Curry"]);
        // New food: a new line at the end.
        assert_eq!(items.len(), 4);
        assert_eq!(items[3].name, "garlic");
    }

    #[test]
    fn amounts_that_dont_add_up_stay_separate() {
        let db = db();
        let mut items = vec![Item {
            quantity: qty(Dim::Volume, 500.0),
            ..hand_item("milk", &db)
        }];
        // milk has no density: ml and g can't be added.
        let incoming = vec![Item {
            quantity: qty(Dim::Mass, 100.0),
            ..hand_item("milk", &db)
        }];
        assert!(merge_into(&mut items, incoming, &db).is_empty());
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn lists_show_the_name_the_recipe_uses() {
        let db = db();
        let sel = |r| Selection {
            recipe: r,
            servings: None,
        };
        let a = recipe(
            "A",
            None,
            &[("ui", v(1.0, None)), ("salt", Amount::Unspecified)],
        );
        let b = recipe("B", None, &[("Onions", v(1.0, None)), ("ui", v(1.0, None))]);

        let list = build(&[sel(&a)], &db, &Pantry::default());
        let onion = &find(&list, "onion")[0];
        assert_eq!(
            (onion.name.as_str(), onion.shown().as_str()),
            ("onion", "ui")
        );
        assert_eq!(find(&list, "salt")[0].shown(), "salt");

        // Another spelling of the same food joins the line and is named too;
        // plurals and repeats are not.
        let list = build(&[sel(&a), sel(&b)], &db, &Pantry::default());
        assert_eq!(find(&list, "onion")[0].shown(), "ui / onions");

        // Into a list where the line is a hand-added "onion".
        let mut items = vec![hand_item("onion", &db)];
        let first = build(&[sel(&a)], &db, &Pantry::default());
        merge_into(&mut items, first.items, &db);
        assert_eq!(items[0].shown(), "onion / ui");
    }

    #[test]
    fn lines_saved_under_an_old_name_still_merge() {
        let db = db();
        // A list saved when the food was still called "ui"; now it's "onion"
        // with "ui" as a synonym.
        let mut items = vec![Item {
            name: "ui".into(),
            quantity: qty(Dim::Count, 1.0),
            ..hand_item("onion", &db)
        }];
        let incoming = vec![Item {
            quantity: qty(Dim::Count, 2.0),
            ..hand_item("onion", &db)
        }];
        assert_eq!(merge_into(&mut items, incoming, &db), [0]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].quantity, qty(Dim::Count, 3.0));
    }

    fn find<'a>(list: &'a ShoppingList, name: &str) -> Vec<&'a Item> {
        list.items.iter().filter(|i| i.name == name).collect()
    }

    #[test]
    fn scales_and_sums_across_recipes() {
        let a = recipe("A", Some(2.0), &[("spaghetti", v(200.0, Some("g")))]);
        let b = recipe("B", Some(4.0), &[("Spaghetti", v(0.5, Some("kg")))]);
        let sel = [
            Selection {
                recipe: &a,
                servings: Some(4.0),
            },
            Selection {
                recipe: &b,
                servings: None,
            },
        ];
        let list = build(&sel, &db(), &Pantry::default());
        let s = find(&list, "spaghetti");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].quantity.as_ref().unwrap().value, 900.0);
        assert_eq!(s[0].recipes, ["A", "B"]);
        assert_eq!(list.sources[0].factor, 2.0);
    }

    #[test]
    fn mixed_dimensions_merge_only_when_convertible() {
        let a = recipe(
            "A",
            None,
            &[
                ("onion", v(2.0, None)),
                ("ui", v(100.0, Some("g"))),
                ("milk", v(200.0, Some("ml"))),
                ("milk", v(100.0, Some("g"))),
            ],
        );
        let list = build(
            &[Selection {
                recipe: &a,
                servings: None,
            }],
            &db(),
            &Pantry::default(),
        );
        let onion = find(&list, "onion");
        assert_eq!(onion.len(), 1, "count + mass merge via piece_g");
        let q = onion[0].quantity.clone().unwrap();
        assert_eq!(
            q.dim,
            Dim::Count,
            "expressed in pieces, the unit you buy in"
        );
        assert!((q.value - 320.0 / 110.0).abs() < 1e-9);
        assert_eq!(find(&list, "milk").len(), 2, "no density → stays two lines");
    }

    #[test]
    fn mass_and_volume_merge_into_grams() {
        let db =
            FoodDb::from_yaml("foods:\n  oil: { density: 0.9 }\n", Path::new("a.yaml")).unwrap();
        let a = recipe(
            "A",
            None,
            &[("oil", v(100.0, Some("ml"))), ("oil", v(10.0, Some("g")))],
        );
        let list = build(
            &[Selection {
                recipe: &a,
                servings: None,
            }],
            &db,
            &Pantry::default(),
        );
        assert_eq!(
            find(&list, "oil")[0].quantity,
            Some(Qty {
                dim: Dim::Mass,
                value: 100.0,
                label: None
            })
        );
    }

    #[test]
    fn different_named_units_merge_into_the_first() {
        let db = FoodDb::from_yaml(
            "foods:\n  garlic: { units: { clove: 5, teen: 5 } }\n",
            Path::new("a.yaml"),
        )
        .unwrap();
        let a = recipe(
            "A",
            None,
            &[
                ("garlic", v(3.0, Some("cloves"))),
                ("garlic", v(2.0, Some("teen"))),
            ],
        );
        let list = build(
            &[Selection {
                recipe: &a,
                servings: None,
            }],
            &db,
            &Pantry::default(),
        );
        let q = find(&list, "garlic")[0].quantity.clone().unwrap();
        assert_eq!(
            (q.dim, q.value, q.label.as_deref()),
            (Dim::Named("clove".into()), 5.0, Some("cloves"))
        );
    }

    #[test]
    fn single_count_bucket_stays_a_count() {
        let a = recipe(
            "A",
            None,
            &[("onion", v(2.0, None)), ("garlic", v(3.0, Some("cloves")))],
        );
        let list = build(
            &[Selection {
                recipe: &a,
                servings: None,
            }],
            &db(),
            &Pantry::default(),
        );
        assert_eq!(
            find(&list, "onion")[0].quantity.as_ref().unwrap().dim,
            Dim::Count
        );
        let garlic = find(&list, "garlic")[0].quantity.clone().unwrap();
        assert_eq!(garlic.dim, Dim::Named("clove".into()));
        assert_eq!(garlic.label.as_deref(), Some("cloves"));
    }

    #[test]
    fn fixed_amounts_do_not_scale() {
        let mut a = recipe("A", Some(2.0), &[]);
        a.ingredients.push(Ingredient {
            name: "salt".into(),
            amount: Amount::Value {
                qty: 1.0,
                unit: Some("tsp".into()),
                scalable: false,
            },
            optional: false,
            note: None,
        });
        let list = build(
            &[Selection {
                recipe: &a,
                servings: Some(6.0),
            }],
            &db(),
            &Pantry::default(),
        );
        assert_eq!(find(&list, "salt")[0].quantity.as_ref().unwrap().value, 5.0);
    }

    #[test]
    fn pantry_unmapped_optional_and_notes() {
        let mut a = recipe(
            "A",
            None,
            &[
                ("salt", Amount::Unspecified),
                ("Basil", Amount::Text("a handful".into())),
                ("passata", v(500.0, Some("g"))),
            ],
        );
        a.ingredients.push(Ingredient {
            name: "parmesan".into(),
            amount: v(50.0, Some("g")),
            optional: true,
            note: None,
        });
        let b = recipe("B", None, &[("parmesan", v(20.0, Some("g")))]);
        let pantry = Pantry::from_yaml("- salt\n", Path::new("p.yaml")).unwrap();
        let sel = [
            Selection {
                recipe: &a,
                servings: None,
            },
            Selection {
                recipe: &b,
                servings: None,
            },
        ];
        let list = build(&sel, &db(), &pantry);

        assert!(find(&list, "salt").is_empty());
        assert_eq!(list.pantry_skipped, ["salt"]);
        assert_eq!(list.unmapped, ["basil", "parmesan", "passata"]);
        let basil = find(&list, "Basil")[0];
        assert_eq!(
            (basil.quantity.clone(), basil.notes.clone()),
            (None, vec!["a handful".to_string()])
        );
        assert!(!find(&list, "parmesan")[0].optional, "required by B");
    }

    #[test]
    fn scaling_without_base_servings_warns() {
        let a = recipe("A", None, &[("onion", v(1.0, None))]);
        let list = build(
            &[Selection {
                recipe: &a,
                servings: Some(4.0),
            }],
            &db(),
            &Pantry::default(),
        );
        assert_eq!(
            find(&list, "onion")[0].quantity.as_ref().unwrap().value,
            1.0
        );
        assert!(list.warnings[0].contains("no `servings`"));
    }

    #[test]
    fn sorted_by_aisle_walking_order() {
        let a = recipe(
            "A",
            None,
            &[
                ("zucchini", v(1.0, None)),
                ("spaghetti", v(1.0, Some("g"))),
                ("onion", v(1.0, None)),
            ],
        );
        let list = build(
            &[Selection {
                recipe: &a,
                servings: None,
            }],
            &db(),
            &Pantry::default(),
        );
        let names: Vec<_> = list.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["onion", "spaghetti", "zucchini"]);
    }

    fn pantry() -> Pantry {
        Pantry::default()
    }

    fn built(r: &Recipe) -> Vec<Item> {
        build(
            &[Selection {
                recipe: r,
                servings: None,
            }],
            &db(),
            &pantry(),
        )
        .items
    }

    fn line<'a>(items: &'a [Item], name: &str) -> &'a Item {
        items.iter().find(|i| i.name == name).unwrap()
    }

    /// (source, amount) per part, as shown under a line.
    fn shown_parts(item: &Item) -> Vec<(String, Option<f64>)> {
        breakdown(item, &db())
            .into_iter()
            .map(|p| {
                let source = match p.source {
                    PartSource::Recipe { title, .. } => title,
                    PartSource::Hand => "hand".into(),
                    PartSource::Earlier { titles } => format!("earlier {titles:?}"),
                };
                (source, p.quantity.map(|q| q.value))
            })
            .collect()
    }

    #[test]
    fn hand_amounts_add_up_with_each_other_and_recipes() {
        let db = db();
        let curry = recipe("Curry", Some(2.0), &[("ui", v(6.0, None))]);
        let mut items = built(&curry);
        let two = hand_amount("Ui", Some(Qty::of(2.0, None)), &db);
        let twelve = hand_amount("ui", Some(Qty::of(6.0, None)), &db);
        merge_into(&mut items, vec![two], &db);
        merge_into(&mut items, vec![twelve], &db);
        assert_eq!(items.len(), 1);
        let onion = line(&items, "onion");
        assert_eq!(onion.quantity, qty(Dim::Count, 14.0));
        assert_eq!(
            shown_parts(onion),
            [("Curry".into(), Some(6.0)), ("hand".into(), Some(8.0))]
        );
    }

    #[test]
    fn parts_in_other_units_show_in_the_lines_unit() {
        let db = db();
        let curry = recipe("Curry", Some(2.0), &[("onion", v(2.0, None))]);
        let mut items = built(&curry);
        merge_into(
            &mut items,
            vec![hand_amount("onion", Some(Qty::of(220.0, Some("g"))), &db)],
            &db,
        );
        let onion = line(&items, "onion");
        assert_eq!(onion.quantity, qty(Dim::Mass, 440.0));
        assert_eq!(
            shown_parts(onion),
            [("Curry".into(), Some(220.0)), ("hand".into(), Some(220.0))]
        );
    }

    #[test]
    fn removing_a_recipe_takes_its_share_off() {
        let db = db();
        let curry = recipe(
            "Curry",
            Some(2.0),
            &[("onion", v(6.0, None)), ("garlic", v(2.0, Some("cloves")))],
        );
        let soup = recipe("Soup", Some(2.0), &[("onion", v(1.0, None))]);
        let mut items = built(&curry);
        merge_into(&mut items, built(&soup), &db);
        merge_into(
            &mut items,
            vec![
                hand_amount("onion", Some(Qty::of(8.0, None)), &db),
                hand_item("milk", &db),
            ],
            &db,
        );
        let path = curry.path.to_string_lossy().into_owned();
        let what = remove_recipe(&mut items, &path, "Curry", &[], &db);
        let names: Vec<_> = items.iter().map(|i| i.name.as_str()).collect();
        let by_name: HashMap<_, _> = names.iter().zip(&what).collect();
        assert_eq!(
            by_name[&"garlic"],
            &Removal::Gone,
            "only the curry needed it"
        );
        assert_eq!(by_name[&"onion"], &Removal::Changed);
        assert_eq!(by_name[&"milk"], &Removal::Same);
        let onion = line(&items, "onion");
        assert_eq!(onion.quantity, qty(Dim::Count, 9.0));
        assert_eq!(onion.recipes, ["Soup"]);
    }

    #[test]
    fn a_hand_line_without_amount_stays_without_one() {
        let db = db();
        let curry = recipe("Curry", Some(2.0), &[("onion", v(6.0, None))]);
        let mut items = vec![hand_item("ui", &db)];
        merge_into(&mut items, built(&curry), &db);
        assert_eq!(items[0].quantity, qty(Dim::Count, 6.0));
        let path = curry.path.to_string_lossy().into_owned();
        assert_eq!(
            remove_recipe(&mut items, &path, "Curry", &[], &db),
            [Removal::Changed]
        );
        assert_eq!(
            (items[0].quantity.as_ref(), items[0].recipes.len()),
            (None, 0)
        );
    }

    #[test]
    fn lines_from_before_parts_subtract_the_recipe() {
        let db = db();
        let curry = recipe("Curry", Some(2.0), &[("onion", v(6.0, None))]);
        let legacy = built(&curry);
        let old = |recipes: &[&str], n: f64| Item {
            quantity: qty(Dim::Count, n),
            recipes: recipes.iter().map(|r| r.to_string()).collect(),
            parts: vec![],
            ..hand_item("onion", &db)
        };
        let path = curry.path.to_string_lossy().into_owned();

        let mut items = vec![old(&["Curry"], 6.0), old(&["Curry", "Soup"], 10.0)];
        let what = remove_recipe(&mut items, &path, "Curry", &legacy, &db);
        assert_eq!(what, [Removal::Gone, Removal::Changed]);
        assert_eq!(items[1].quantity, qty(Dim::Count, 4.0));

        // An old line that later got an amount by hand.
        let mut items = vec![old(&["Curry"], 6.0)];
        merge_into(
            &mut items,
            vec![hand_amount("ui", Some(Qty::of(2.0, None)), &db)],
            &db,
        );
        assert_eq!(items[0].quantity, qty(Dim::Count, 8.0));
        let what = remove_recipe(&mut items, &path, "Curry", &legacy, &db);
        assert_eq!(what, [Removal::Changed]);
        assert_eq!(items[0].quantity, qty(Dim::Count, 2.0));
        assert_eq!(shown_parts(&items[0]), [("hand".into(), Some(2.0))]);
    }
}
