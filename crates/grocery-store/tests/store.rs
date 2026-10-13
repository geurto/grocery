use std::collections::BTreeSet;
use std::fs::File;
use std::path::{Path, PathBuf};

use grocery_core::nutrition::Basis;
use grocery_store::{read_nevo, Store};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn imported() -> Store {
    let data = read_nevo(
        File::open(fixture("nevo_synthetic.csv")).unwrap(),
        Some(File::open(fixture("nutrients_synthetic.csv")).unwrap()),
    )
    .unwrap();
    let mut store = Store::open_in_memory().unwrap();
    store.import_nevo(&data).unwrap();
    store
}

#[test]
fn parses_the_nevo_layout() {
    let data = read_nevo(
        File::open(fixture("nevo_synthetic.csv")).unwrap(),
        Some(File::open(fixture("nutrients_synthetic.csv")).unwrap()),
    )
    .unwrap();
    assert_eq!(data.version, "2099/1.0");
    assert_eq!(data.foods.len(), 5);

    let pasta = &data.foods[2];
    assert_eq!(pasta.name_nl, "Pasta witte rauw");
    assert!(pasta.values.contains(&("FAT".into(), 1.5)), "decimal comma");
    assert!(
        !pasta.values.iter().any(|(c, _)| c == "VITC"),
        "empty = unknown, not 0"
    );

    let kale = &data.foods[1];
    assert!(kale.values.contains(&("VITC".into(), 0.0)), "trace stays 0");
    assert_eq!(kale.traces, "VITC");

    assert_eq!(data.foods[4].basis, Basis::Per100ml);

    // Nutrient names from the nutrients file (duplicate PROT row ignored);
    // SE is only in the main file, so its code doubles as the name.
    let codes: Vec<_> = data.nutrients.iter().map(|n| n.code.as_str()).collect();
    assert_eq!(
        codes,
        ["ENERCJ", "ENERCC", "PROT", "FAT", "NA", "VITC", "SE"]
    );
    assert_eq!(data.nutrients[5].name_en, "Ascorbic acid (Vit C)");
    assert_eq!(data.nutrients[6].unit, "µg");
}

#[test]
fn works_without_the_nutrients_file() {
    let data = read_nevo(
        File::open(fixture("nevo_synthetic.csv")).unwrap(),
        None::<File>,
    )
    .unwrap();
    assert_eq!(data.nutrients[1].name_en, "ENERCC");
}

#[test]
fn import_info_and_citation() {
    let store = imported();
    let info = store.require_nevo().unwrap();
    assert_eq!(
        (info.version.as_str(), info.foods, info.nutrients),
        ("2099/1.0", 5, 7)
    );
    assert_eq!(
        info.citation(),
        "Based on data from NEVO online version 2099/1.0, RIVM, Bilthoven"
    );
}

#[test]
fn reimport_replaces_everything() {
    let mut store = imported();
    let data = read_nevo(
        File::open(fixture("nevo_synthetic.csv")).unwrap(),
        None::<File>,
    )
    .unwrap();
    let info = store.import_nevo(&data).unwrap();
    assert_eq!(info.foods, 5);
}

#[test]
fn compositions_keep_unknowns_absent() {
    let store = imported();
    let table = store.compositions(&BTreeSet::from([3, 4, 999])).unwrap();
    let pasta = table.get(3).unwrap();
    assert_eq!(pasta.values["ENERCC"], 340.0);
    assert!(!pasta.values.contains_key("VITC"));
    assert!(!table.get(4).unwrap().values.contains_key("NA"));
    assert!(table.get(999).is_none());
}

#[test]
fn search_finds_compounds_and_synonyms() {
    let store = imported();
    let code = |q: &str| {
        store
            .search(q, 1)
            .unwrap()
            .first()
            .map(|h| h.candidate.code)
    };
    assert_eq!(code("boerenkool"), Some(2));
    assert_eq!(code("olijfolie"), Some(4));
    assert_eq!(code("spaghetti"), Some(3));
    assert_eq!(code("ui"), Some(1));
    let hit = &store.search("pasta", 1).unwrap()[0];
    assert_eq!(hit.candidate.kcal, Some(340.0));
}

#[test]
fn rejects_files_that_are_not_the_food_table() {
    let err = read_nevo(
        File::open(fixture("nutrients_synthetic.csv")).unwrap(),
        None::<File>,
    )
    .unwrap_err();
    assert!(err.to_string().contains("no column starting with"), "{err}");
}

#[test]
fn not_imported_is_a_clear_error() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.nevo_info().unwrap().is_none());
    assert!(store
        .require_nevo()
        .unwrap_err()
        .to_string()
        .contains("import-nevo"));
}

/// Against the real export: `NEVO_DIR=... cargo test -p grocery-store -- --ignored`
#[test]
#[ignore]
fn real_nevo_export() {
    let dir = PathBuf::from(std::env::var("NEVO_DIR").expect("set NEVO_DIR"));
    let find = |needle: &str, exclude: &[&str]| -> PathBuf {
        std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                let n = p.file_name().unwrap().to_string_lossy().to_string();
                n.ends_with(".csv") && n.contains(needle) && !exclude.iter().any(|x| n.contains(x))
            })
            .unwrap()
    };
    let main = find("NEVO", &["Details", "Nutrient", "Recept", "Referent"]);
    let data = read_nevo(
        File::open(&main).unwrap(),
        Some(File::open(find("Nutrient", &[])).unwrap()),
    )
    .unwrap();
    assert!(data.foods.len() > 2000);
    assert!(data.nutrients.len() > 100);

    let mut store = Store::open_in_memory().unwrap();
    let info = store.import_nevo(&data).unwrap();
    println!(
        "{} foods, {} nutrients, version {}",
        info.foods, info.nutrients, info.version
    );
    for q in [
        "ui",
        "boerenkool",
        "olijfolie",
        "spaghetti",
        "knoflook",
        "rookworst",
        "ei",
    ] {
        let hits = store.search(q, 3).unwrap();
        println!(
            "{q:>12}: {:?}",
            hits.iter()
                .map(|h| (h.candidate.code, &h.candidate.name_nl))
                .collect::<Vec<_>>()
        );
        assert!(!hits.is_empty(), "{q}");
    }
    let _ = Path::new(&main);
}

/// The whole chain: fixture import → aliases → recipe → per-serving report.
#[test]
fn end_to_end_nutrition() {
    use grocery_core::food::FoodDb;
    use grocery_core::nutrition::{self, Options};
    use grocery_core::parse::Parsers;

    let store = imported();
    let foods = FoodDb::from_yaml(
        "foods:\n\
         \x20 onion: { nevo: 1, piece_g: 100, synonyms: [ui] }\n\
         \x20 kale: { nevo: 2, synonyms: [boerenkool] }\n\
         \x20 olive oil: { nevo: 4, density: 0.9, synonyms: [olijfolie] }\n",
        Path::new("aliases.yaml"),
    )
    .unwrap();
    let recipe = Parsers::new()
        .parse_str(
            Path::new("Test.md"),
            "---\nservings: 2\ningredients:\n  - 2 ui\n  - 400 g boerenkool\n  - 2 el olijfolie\n  - zout\n---\n",
        )
        .unwrap();

    let table = store
        .compositions(&nutrition::needed_codes(&recipe, &foods))
        .unwrap();
    let n = nutrition::compute(&recipe, &foods, &table, Options::default());
    // (200 g × 25 + 400 g × 30 + 27 g × 880) / 100 / 2 servings
    let (kcal, _) = n.per_serving("ENERCC").unwrap();
    assert!((kcal - (50.0 + 120.0 + 237.6) / 2.0).abs() < 1e-9, "{kcal}");
    // olive oil has no sodium value in the fixture → salt is a lower bound
    assert_eq!(n.per_serving("SALT").unwrap().1.missing_in, ["Olie olijf-"]);

    let citation = store.require_nevo().unwrap().citation();
    let text = nutrition::render_text(&n, &table, &nutrition::default_rows(), &citation);
    assert!(text.contains("No amount given: zout"), "{text}");
    assert!(text
        .trim_end()
        .ends_with("NEVO online version 2099/1.0, RIVM, Bilthoven"));
}
