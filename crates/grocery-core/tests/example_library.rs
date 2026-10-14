//! End-to-end over `tests/fixtures/library`: parse every recipe, build a list,
//! render it. If you change the example library, update the expectations.

use grocery_core::aggregate::{build, Selection};
use grocery_core::library::Library;
use grocery_core::render;

fn library() -> Library {
    Library::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/library"
    ))
}

#[test]
fn every_example_recipe_parses() {
    let lib = library();
    let (recipes, errors) = lib.parser.parse_dir(&lib.recipes_dir()).unwrap();
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

/// `library/` holds real recipes that change; they must parse, but
/// no expectations beyond that.
#[test]
fn every_recipe_in_the_library_parses() {
    let lib = Library::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../library"
    ));
    let (_, errors) = lib.parser.parse_dir(&lib.recipes_dir()).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    lib.food_db().unwrap();
    lib.custom_foods().unwrap();
}

#[test]
fn shopping_list_for_a_week() {
    let lib = library();
    let db = lib.food_db().unwrap();
    let pantry = lib.pantry().unwrap();
    let parse = |name| {
        lib.parser
            .parse_file(&lib.find_recipe(name).unwrap())
            .unwrap()
    };
    let pasta = parse("pasta arrabbiata");
    let shakshuka = parse("Shakshuka");

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
# Shopping list 2026-10-12

For: Pasta arrabbiata (2 servings), Shakshuka (2 servings)

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

From the pantry (not listed): salt, chili flakes
";
    assert_eq!(
        render::markdown(&list, "Shopping list 2026-10-12"),
        expected
    );
}
