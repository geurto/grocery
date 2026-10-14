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
    let extra = store.add_item(id, "  toilet paper ").unwrap().unwrap();
    assert!(store.add_item(id, "").is_err());
    assert!(store.add_item(id + 100, "x").unwrap().is_none());
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
