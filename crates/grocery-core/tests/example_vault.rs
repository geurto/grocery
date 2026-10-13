//! End-to-end over `examples/vault`: parse every recipe, build a list,
//! render it. If you change the example vault, update the expectations.

use std::path::PathBuf;

use grocery_core::aggregate::{build, Selection};
use grocery_core::food::{FoodDb, Pantry};
use grocery_core::parse::Parsers;
use grocery_core::render;

fn vault() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/vault")
}

#[test]
fn every_example_recipe_parses_and_non_recipes_are_skipped() {
    let (recipes, errors) = Parsers::new().parse_dir(&vault().join("Recipes")).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    let titles: Vec<_> = recipes.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Pasta arrabbiata", "Shakshuka", "Stamppot boerenkool"]
    );
    for r in &recipes {
        assert!(r.warnings.is_empty(), "{}: {:?}", r.title, r.warnings);
        assert!(r.servings.is_some(), "{} has servings", r.title);
    }
}

#[test]
fn shopping_list_for_a_week() {
    let parsers = Parsers::new();
    let db = FoodDb::load(&vault().join("Food/aliases.yaml")).unwrap();
    let pantry = Pantry::load(&vault().join("Food/pantry.yaml")).unwrap();
    let pasta = parsers
        .parse_file(&vault().join("Recipes/Pasta arrabbiata.cook"))
        .unwrap();
    let shakshuka = parsers
        .parse_file(&vault().join("Recipes/Shakshuka.md"))
        .unwrap();

    let list = build(
        &[
            Selection {
                recipe: &pasta,
                servings: Some(2.0),
            },
            Selection {
                recipe: &shakshuka,
                servings: None,
            },
        ],
        &db,
        &pantry,
    );

    assert!(list.unmapped.is_empty(), "{:?}", list.unmapped);
    assert!(list.warnings.is_empty(), "{:?}", list.warnings);
    assert_eq!(list.pantry_skipped, ["salt", "chili flakes"]);

    let expected = "\
---
type: shopping-list
created: 2026-10-12
status: open
recipes:
  - \"[[Pasta arrabbiata]]\"
  - \"[[Shakshuka]]\"
---

# Shopping list 2026-10-12

For: [[Pasta arrabbiata]] (2 servings), [[Shakshuka]] (2 servings)

## Groente & fruit

- [ ] basil — a handful
- [ ] coriander
- [ ] garlic — 4 cloves
- [ ] onion — 2
- [ ] red bell pepper — 1

## Zuivel & eieren

- [ ] egg — 4

## Kaas

- [ ] feta — 100 g (optional)
- [ ] parmesan — 30 g (optional)

## Pasta, rijst & wereldkeuken

- [ ] spaghetti — 200 g

## Conserven & sauzen

- [ ] passata — 750 g

## Kruiden & specerijen

- [ ] cumin — 5 ml

## Olie & azijn

- [ ] olive oil — 38 ml

> [!note]- From the pantry (not listed)
> salt, chili flakes
";
    assert_eq!(render::markdown(&list, "2026-10-12"), expected);
}
