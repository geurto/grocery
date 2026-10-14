//! NEVO and nutrition commands: import-nevo, nevo search/show/suggest, nutrition.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use grocery_core::library::Library;
use grocery_core::nutrition::{Basis, Options};
use grocery_store::{read_nevo, Hit, Store};

/// `$XDG_DATA_HOME/grocery/grocery.sqlite`, else `~/.local/share/...`.
pub fn default_db_path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("grocery").join("grocery.sqlite")
}

fn open_store(db: &Path) -> Result<Store> {
    Store::open(db).with_context(|| format!("opening {}", db.display()))
}

/// Find the food table and the nutrients file in an unzipped NEVO export,
/// or next to a given food table CSV.
fn locate(path: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        bail!("{} is a zip; unzip it and pass the folder", path.display());
    }
    let (dir, main) = if path.is_dir() {
        (path.to_path_buf(), None)
    } else if path.is_file() {
        (
            path.parent().unwrap_or(Path::new(".")).to_path_buf(),
            Some(path.to_path_buf()),
        )
    } else {
        bail!("{} does not exist", path.display());
    };
    let csvs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("csv")))
        .collect();
    let name = |p: &PathBuf| {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    };

    let main = match main {
        Some(m) => m,
        None => {
            let mut tables: Vec<&PathBuf> = csvs
                .iter()
                .filter(|p| {
                    let n = name(p);
                    n.contains("NEVO")
                        && !["Details", "Nutrient", "Recept", "Referent"]
                            .iter()
                            .any(|x| n.contains(x))
                })
                .collect();
            match tables.len() {
                1 => tables.remove(0).clone(),
                0 => bail!(
                    "no NEVO food table (NEVO<year>_v<x>.csv) in {}",
                    dir.display()
                ),
                _ => bail!(
                    "several candidate food tables in {}; pass the CSV itself: {}",
                    dir.display(),
                    tables
                        .iter()
                        .map(|p| name(p))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        }
    };
    let nutrients = csvs.iter().find(|p| name(p).contains("Nutrient")).cloned();
    Ok((main, nutrients))
}

pub fn import(db: &Path, path: &Path) -> Result<ExitCode> {
    let (main, nutrients) = locate(path)?;
    let data = read_nevo(
        File::open(&main).with_context(|| format!("opening {}", main.display()))?,
        nutrients
            .as_ref()
            .map(|p| File::open(p).with_context(|| format!("opening {}", p.display())))
            .transpose()?,
    )
    .with_context(|| format!("reading {}", main.display()))?;
    if nutrients.is_none() {
        eprintln!("note: no *Nutrienten_Nutrients.csv next to the food table; nutrient codes used as names");
    }
    let mut store = open_store(db)?;
    let info = store.import_nevo(&data)?;
    println!(
        "Imported NEVO-online {}: {} foods, {} nutrients\n→ {}\nOutputs will cite: {}",
        info.version,
        info.foods,
        info.nutrients,
        db.display(),
        info.citation()
    );
    Ok(ExitCode::SUCCESS)
}

fn print_hits(hits: &[Hit], indent: &str) {
    for h in hits {
        let c = &h.candidate;
        let kcal = c
            .kcal
            .map(|k| format!("{k:.0} kcal"))
            .unwrap_or_else(|| "? kcal".into());
        println!(
            "{indent}{:>5}  {:<38} {:<34} {kcal:>9}",
            c.code,
            truncate(&c.name_nl, 38),
            truncate(&c.name_en, 34)
        );
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_owned()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

pub fn search(db: &Path, query: &[String], limit: usize) -> Result<ExitCode> {
    let store = open_store(db)?;
    store.require_nevo()?;
    let q = query.join(" ");
    let hits = store.search(&q, limit)?;
    if hits.is_empty() {
        println!("no NEVO food matches '{q}'");
        return Ok(ExitCode::FAILURE);
    }
    print_hits(&hits, "");
    Ok(ExitCode::SUCCESS)
}

pub fn show(db: &Path, code: u32, all: bool) -> Result<ExitCode> {
    let store = open_store(db)?;
    let info = store.require_nevo()?;
    let Some(food) = store.food(code)? else {
        bail!("NEVO {} has no food with code {code}", info.version);
    };
    let table = store.compositions(&[code].into())?;
    let comp = table.get(code).expect("food exists");
    let per = match food.basis {
        Basis::Per100g => "100 g",
        Basis::Per100ml => "100 ml",
    };
    println!("{} — {} ({code})", food.name_nl, food.name_en);
    println!("group: {}", food.group_en);
    if !food.synonyms.is_empty() {
        println!("synonyms: {}", food.synonyms);
    }
    if !food.note.is_empty() {
        println!("note: {}", food.note);
    }
    println!("\nper {per}");
    for row in store.display_rows(all)? {
        if let Some(s) = &row.section {
            println!("\n{s}");
        }
        let value = if row.code == "SALT" {
            comp.values.get("NA").map(|na| na * 2.5 / 1000.0)
        } else {
            comp.values.get(&row.code).copied()
        };
        let label = if row.indent {
            format!("  {}", row.label)
        } else {
            row.label.clone()
        };
        match value {
            Some(v) => println!("{label:<22}{:>10} {}", format_num(v), row.unit),
            None => println!("{label:<22}{:>10}", "–"),
        }
    }
    println!("\n{}", info.citation());
    Ok(ExitCode::SUCCESS)
}

fn format_num(v: f64) -> String {
    if v >= 100.0 {
        format!("{v:.0}")
    } else if v >= 1.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

/// Candidates for every food in aliases.yaml that has no `nevo:` code yet.
pub fn suggest(library: &Library, db: &Path, limit: usize) -> Result<ExitCode> {
    let store = open_store(db)?;
    store.require_nevo()?;
    let foods = library.food_db()?;
    let missing: Vec<_> = foods
        .foods()
        .filter(|(_, info)| info.nevo.is_none())
        .collect();
    if missing.is_empty() {
        println!("every food in Food/aliases.yaml has a nevo code");
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "{} foods without a nevo code. Add the right one as `nevo: <code>` in Food/aliases.yaml.\n\
         Pick the raw product when recipes weigh it raw (pasta 'rauw' ≈ dry, not 'gekookt').\n",
        missing.len()
    );
    for (name, info) in missing {
        // Search the canonical name and every synonym; keep each code's best score.
        let mut best: BTreeMap<u32, Hit> = BTreeMap::new();
        for q in std::iter::once(name).chain(info.synonyms.iter().map(String::as_str)) {
            for hit in store.search(q, limit)? {
                match best.get(&hit.candidate.code) {
                    Some(b) if b.score >= hit.score => {}
                    _ => {
                        best.insert(hit.candidate.code, hit);
                    }
                }
            }
        }
        let mut hits: Vec<Hit> = best.into_values().collect();
        hits.sort_by(Hit::order);
        hits.truncate(limit);

        let syn = if info.synonyms.is_empty() {
            String::new()
        } else {
            format!("  ({})", info.synonyms.join(", "))
        };
        println!("{name}{syn}");
        if hits.is_empty() {
            println!("       no match; try `grocery nevo search <other words>`");
        } else {
            print_hits(&hits, "  ");
        }
        println!();
    }
    Ok(ExitCode::SUCCESS)
}

pub fn nutrition(
    library: &Library,
    db: &Path,
    recipe: &str,
    all: bool,
    with_optional: bool,
) -> Result<ExitCode> {
    let recipe = crate::load(library, recipe)?;
    let foods = library.food_db()?;
    let store = open_store(db)?;
    let opts = Options {
        include_optional: with_optional,
    };
    print!("{}", store.nutrition_report(&recipe, &foods, opts, all)?);
    for w in &recipe.warnings {
        eprintln!("warning: {w}");
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_finds_files_in_an_export_folder() {
        let dir = std::env::temp_dir().join(format!("grocery-locate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in [
            "NEVO2025_v9.0.csv",
            "NEVO2025_v9.0_Details.csv",
            "NEVO2025_v9.0_Nutrienten_Nutrients.csv",
            "NEVO2025_v9.0_Recepten_Recipes.csv",
            "NEVO2025_v9.0_Referenties_References.csv",
            "notes.txt",
        ] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        let (main, nutrients) = locate(&dir).unwrap();
        assert!(main.ends_with("NEVO2025_v9.0.csv"));
        assert!(nutrients
            .unwrap()
            .ends_with("NEVO2025_v9.0_Nutrienten_Nutrients.csv"));

        let (main, _) = locate(&dir.join("NEVO2025_v9.0.csv")).unwrap();
        assert!(main.ends_with("NEVO2025_v9.0.csv"));
        assert!(locate(&dir.join("x.zip")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
