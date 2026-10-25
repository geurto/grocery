//! The recipe index and saved lists, against a copy of `tests/fixtures/library`.

use std::path::Path;

use grocery_core::aggregate::{build, Selection};
use grocery_core::library::Library;
use grocery_core::render;
use grocery_store::{ListStatus, Store, SyncReport};

/// A scratch copy of the example library, so tests can edit files.
fn scratch_library() -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/library");
    copy_dir(&src, dir.path());
    let lib = Library::new(dir.path());
    (dir, lib)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_dir(&path, &target);
        } else {
            std::fs::copy(&path, &target).unwrap();
        }
    }
}

#[test]
fn index_follows_the_files() {
    let (_dir, lib) = scratch_library();
    let mut store = Store::open_in_memory().unwrap();

    let first = store.sync_recipes(&lib, false).unwrap();
    assert_eq!(
        first,
        SyncReport {
            added: 3,
            ..Default::default()
        }
    );
    let all = store.recipes(None).unwrap();
    let paths: Vec<_> = all.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "Pasta arrabbiata.cook",
            "Shakshuka.cook",
            "Stamppot boerenkool.cook"
        ]
    );
    let shakshuka = store.recipe("Shakshuka.cook").unwrap().unwrap();
    assert_eq!(shakshuka.servings, Some(2.0));
    assert_eq!(shakshuka.tags, ["eggs", "vegetarian"]);
    assert!(shakshuka.ingredients.contains(&"passata".to_owned()));

    // search: title, tag, ingredient
    let titles = |q| -> Vec<String> {
        store
            .recipes(Some(q))
            .unwrap()
            .into_iter()
            .map(|r| r.title)
            .collect()
    };
    assert_eq!(titles("STAMP"), ["Stamppot boerenkool"]);
    assert_eq!(titles("vegetarian"), ["Pasta arrabbiata", "Shakshuka"]);
    assert_eq!(titles("passata"), ["Pasta arrabbiata", "Shakshuka"]);

    // nothing changed → nothing re-parsed
    let again = store.sync_recipes(&lib, false).unwrap();
    assert_eq!(again.unchanged, 3);
    assert!(!again.changed());

    // add one (in a subfolder), break one, delete one
    let recipes = lib.recipes_dir();
    std::fs::create_dir(recipes.join("soups")).unwrap();
    std::fs::write(
        recipes.join("soups/Tomato soup.cook"),
        "---\nservings: 2\n---\nSimmer @passata{500%g}.",
    )
    .unwrap();
    std::fs::write(recipes.join("Shakshuka.cook"), "Add @&eggs{2}.").unwrap();
    std::fs::remove_file(recipes.join("Stamppot boerenkool.cook")).unwrap();
    std::fs::write(recipes.join("notes.txt"), "not a recipe").unwrap();

    let report = store.sync_recipes(&lib, false).unwrap();
    assert_eq!(
        report,
        SyncReport {
            added: 1,
            updated: 1,
            removed: 1,
            unchanged: 1,
            failed: 1,
        }
    );
    let broken = store.recipe("Shakshuka.cook").unwrap().unwrap();
    assert_eq!(broken.title, "Shakshuka");
    assert!(broken.error.is_some());
    assert!(store.recipe("soups/Tomato soup.cook").unwrap().is_some());
    assert!(store.recipe("Stamppot boerenkool.cook").unwrap().is_none());

    assert_eq!(
        store.sync_recipes(&lib, true).unwrap().updated,
        3,
        "force re-parses"
    );
}

/// A list can start empty and grow: typed items land in their food's
/// aisle, and recipes merge into what's there.
#[test]
fn lists_grow_by_hand_and_by_recipe() {
    let (_dir, lib) = scratch_library();
    let mut store = Store::open_in_memory().unwrap();
    let db = lib.food_db().unwrap();
    let empty = build(&[], &db, &lib.pantry().unwrap());
    let id = store
        .create_list("Empty", "2026-10-16T10:00:00+02:00", &empty)
        .unwrap();
    assert!(store.list(id).unwrap().unwrap().items.is_empty());

    let tp = store
        .add_item(id, "toilet paper", None, &db)
        .unwrap()
        .unwrap();
    let melk = store.add_item(id, "Melk", None, &db).unwrap().unwrap();
    let list = store.list(id).unwrap().unwrap();
    let names: Vec<_> = list
        .items
        .iter()
        .map(|i| (i.id, i.item.name.as_str(), i.item.aisle.as_deref()))
        .collect();
    // "Melk" is the food "milk" with its aisle, sorted before aisle-less items.
    assert_eq!(names[0].0, melk);
    assert_eq!(names[0].1, "milk");
    assert!(names[0].2.is_some());
    assert_eq!(names[1], (tp, "toilet paper", None));
    store.set_checked(id, melk, true).unwrap();

    let pasta = lib
        .parser
        .parse_file(&lib.find_recipe("Pasta arrabbiata").unwrap())
        .unwrap();
    let built = build(
        &[Selection {
            recipe: &pasta,
            servings: Some(2.0),
        }],
        &db,
        &lib.pantry().unwrap(),
    );
    assert!(store.add_recipes(id, &built, &db).unwrap());
    assert!(!store.add_recipes(id + 100, &built, &db).unwrap());
    let list = store.list(id).unwrap().unwrap();
    assert_eq!(list.sources.len(), 1);
    assert_eq!(list.sources[0].title, "Pasta arrabbiata");
    assert_eq!(list.items.len(), 2 + built.items.len());
    // The names the recipe uses are kept with the line.
    let ui = list.items.iter().find(|i| i.item.name == "onion").unwrap();
    assert_eq!(ui.item.written, ["onion"]);
    assert!(list.items.iter().any(|i| i.item.shown() == "spaghetti"));
    // Still in walking order, hand-added items without an aisle last.
    assert_eq!(list.items.last().unwrap().item.name, "toilet paper");
    let aisles: Vec<_> = list.items.iter().map(|i| i.item.aisle.clone()).collect();
    let order = db.aisle_order();
    let rank = |a: &Option<String>| {
        a.as_ref().map_or(usize::MAX, |a| {
            order.iter().position(|o| o == a).unwrap_or(order.len())
        })
    };
    assert!(
        aisles.windows(2).all(|w| rank(&w[0]) <= rank(&w[1])),
        "{aisles:?}"
    );
    // The ticked milk wasn't touched (pasta has no milk).
    assert!(
        list.items
            .iter()
            .find(|i| i.id == melk)
            .unwrap()
            .item
            .checked
    );

    // The same recipe again doubles the amounts on the same lines.
    let before = list.items.len();
    store.add_recipes(id, &built, &db).unwrap();
    let again = store.list(id).unwrap().unwrap();
    assert_eq!(again.items.len(), before);
    // Still one source, now cooked twice as much.
    assert_eq!(again.sources.len(), 1);
    assert_eq!(again.sources[0].servings, Some(4.0));
    let spaghetti = |l: &grocery_store::StoredList| {
        l.items
            .iter()
            .find(|i| i.item.name == "spaghetti")
            .unwrap()
            .item
            .quantity
            .clone()
            .unwrap()
            .value
    };
    assert_eq!(spaghetti(&again), 2.0 * spaghetti(&list));
}

#[test]
fn lists_round_trip_and_tick_off() {
    let (_dir, lib) = scratch_library();
    let mut store = Store::open_in_memory().unwrap();
    let db = lib.food_db().unwrap();
    let pantry = lib.pantry().unwrap();
    let pasta = lib
        .parser
        .parse_file(&lib.find_recipe("Pasta arrabbiata").unwrap())
        .unwrap();
    let stamppot = lib
        .parser
        .parse_file(&lib.find_recipe("Stamppot boerenkool").unwrap())
        .unwrap();
    let list = build(
        &[
            Selection {
                recipe: &pasta,
                servings: Some(2.0),
            },
            Selection {
                recipe: &stamppot,
                servings: None,
            },
        ],
        &db,
        &pantry,
    );

    let id = store
        .create_list("Week 42", "2026-10-14T10:00:00+02:00", &list)
        .unwrap();
    let stored = store.list(id).unwrap().unwrap();
    assert_eq!(stored.summary.title, "Week 42");
    assert_eq!(stored.summary.status, ListStatus::Open);
    assert_eq!(stored.summary.items, list.items.len());
    assert_eq!(stored.summary.checked, 0);

    // What comes back renders exactly like what went in.
    let heading = "Week 42";
    assert_eq!(
        render::markdown(&stored.shopping_list(), heading),
        render::markdown(&list, heading)
    );

    // tick off garlic, add something by hand
    let garlic = stored
        .items
        .iter()
        .find(|i| i.item.name == "garlic")
        .unwrap()
        .id;
    assert!(store.set_checked(id, garlic, true).unwrap());
    let extra = store
        .add_item(id, "  toilet paper ", None, &db)
        .unwrap()
        .unwrap();
    assert!(store.add_item(id, "", None, &db).is_err());
    assert!(store.add_item(id + 100, "x", None, &db).unwrap().is_none());
    assert!(
        !store.set_checked(id + 100, garlic, true).unwrap(),
        "wrong list"
    );

    let stored = store.list(id).unwrap().unwrap();
    assert_eq!(stored.summary.checked, 1);
    let md = render::markdown(&stored.shopping_list(), heading);
    assert!(md.contains("- [x] garlic — 2 cloves\n"), "{md}");
    // hand-added items go last, under Other
    assert!(
        md.ends_with("- [ ] mosterd (optional)\n- [ ] toilet paper\n\nFrom the pantry (not listed): salt, chili flakes, black pepper\n"),
        "{md}"
    );
    let text = render::plain(&stored.shopping_list());
    assert!(
        !text.contains("garlic") && text.contains("• toilet paper"),
        "{text}"
    );

    assert!(store.delete_item(id, extra).unwrap());
    assert!(store
        .update_list(id, Some("Groceries"), Some(ListStatus::Done))
        .unwrap());
    let summaries = store.lists().unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].title, "Groceries");
    assert_eq!(summaries[0].status, ListStatus::Done);

    assert!(store.delete_list(id).unwrap());
    assert!(store.list(id).unwrap().is_none());
    assert!(store.lists().unwrap().is_empty());
}

#[test]
fn databases_from_before_recipe_names_still_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.sqlite");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE lists (id INTEGER PRIMARY KEY, title TEXT NOT NULL, created TEXT NOT NULL);
             CREATE TABLE list_items (
                id INTEGER PRIMARY KEY, list INTEGER NOT NULL, pos INTEGER NOT NULL,
                name TEXT NOT NULL, aisle TEXT, dim TEXT, value REAL, label TEXT,
                notes TEXT NOT NULL, optional INTEGER NOT NULL, recipes TEXT NOT NULL,
                known INTEGER NOT NULL, checked INTEGER NOT NULL DEFAULT 0);
             PRAGMA user_version = 3;",
        )
        .unwrap();
    }
    // Opening upgrades the table; the add-column must not fail or repeat.
    Store::open(&path).unwrap();
    Store::open(&path).unwrap();
}

#[test]
fn plans_from_before_single_foods_still_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.sqlite");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE);
             CREATE TABLE plan_entries (
                id INTEGER PRIMARY KEY, person INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
                day TEXT NOT NULL, meal TEXT, meal_order INTEGER NOT NULL, path TEXT NOT NULL,
                title TEXT NOT NULL, portions REAL NOT NULL CHECK (portions > 0));
             INSERT INTO people (name) VALUES ('Peter');
             INSERT INTO plan_entries (person, day, meal, meal_order, path, title, portions)
             VALUES (1, '2026-10-14', 'dinner', 2, 'Curry.cook', 'Curry', 2);
             PRAGMA user_version = 4;",
        )
        .unwrap();
    }
    Store::open(&path).unwrap();
    let store = Store::open(&path).unwrap();
    let day = grocery_store::plans::parse_day("2026-10-14").unwrap();
    let entries = store.plan(1, day, day).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!((entries[0].portions, &entries[0].food), (2.0, &None));
}
