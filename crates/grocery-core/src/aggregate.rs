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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Qty {
    pub dim: Dim,
    /// In g (Mass), ml (Volume), pieces (Count) or the named unit.
    pub value: f64,
    /// For named units: the unit as first written ("cloves", "blik").
    pub label: Option<String>,
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
            if !acc.recipes.contains(&recipe.title) {
                acc.recipes.push(recipe.title.clone());
            }

            match ing.amount.scaled(factor) {
                Amount::Unspecified => {}
                Amount::Text(t) => {
                    if !acc.notes.contains(&t) {
                        acc.notes.push(t);
                    }
                }
                Amount::Value { qty, unit, .. } => {
                    let n = units::normalize(qty, unit.as_deref());
                    let label = matches!(n.dim, Dim::Named(_)).then(|| unit.unwrap_or_default());
                    acc.add(n.dim, n.value, label);
                }
            }

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
        let make = |quantity: Option<Qty>, notes: Vec<String>| Item {
            name: acc.name.clone(),
            aisle: acc.aisle.clone(),
            quantity,
            notes,
            optional: acc.all_optional,
            recipes: acc.recipes.clone(),
            known,
            checked: false,
        };
        if acc.buckets.is_empty() {
            items.push(make(None, acc.notes.clone()));
        } else {
            // Text notes ride along on the first quantity line.
            let mut notes = Some(acc.notes.clone());
            for b in &acc.buckets {
                items.push(make(Some(b.clone()), notes.take().unwrap_or_default()));
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
fn sort_items(items: &mut [Item], aisle_order: &[String]) {
    let rank = |aisle: &Option<String>| -> (usize, String) {
        match aisle {
            Some(a) => {
                let norm = normalize_name(a);
                match aisle_order.iter().position(|o| normalize_name(o) == norm) {
                    Some(i) => (i, String::new()),
                    None => (aisle_order.len(), norm),
                }
            }
            None => (usize::MAX, String::new()),
        }
    };
    items.sort_by_cached_key(|i| (rank(&i.aisle), i.name.to_lowercase()));
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
}
