//! `grocery custom add|list`: your own nutrition values (Food/custom.yaml).

use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use grocery_core::custom::{append_entry, CustomEntry, CustomFoods};
use grocery_core::library::Library;
use grocery_core::nutrition::Basis;
use grocery_core::units::parse_number;

// Parsed once per run; boxing the args would only add noise.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
pub enum CustomCommand {
    /// Add a food. Give values as flags, or run without any in a terminal
    /// to be asked for each line of the package label.
    Add(AddArgs),
    /// List your own foods.
    List,
}

#[derive(Args)]
pub struct AddArgs {
    /// Name of the food, e.g. "fresh coriander".
    name: String,
    /// What the values are per: 100g (default) or 100ml.
    #[arg(long)]
    per: Option<String>,
    /// Where the numbers came from ("package, Albert Heijn").
    #[arg(long)]
    source: Option<String>,
    /// Energy, kcal.
    #[arg(long, value_name = "KCAL")]
    kcal: Option<f64>,
    /// Energy, kJ (kcal is derived from it if you don't give kcal).
    #[arg(long, value_name = "KJ")]
    kj: Option<f64>,
    #[arg(long, value_name = "G")]
    protein: Option<f64>,
    #[arg(long, value_name = "G")]
    fat: Option<f64>,
    #[arg(long, value_name = "G")]
    saturated: Option<f64>,
    #[arg(long, value_name = "G")]
    carbs: Option<f64>,
    #[arg(long, value_name = "G")]
    sugars: Option<f64>,
    #[arg(long, value_name = "G")]
    fibre: Option<f64>,
    /// Salt as on the label, g.
    #[arg(long, value_name = "G", conflicts_with = "sodium_mg")]
    salt: Option<f64>,
    #[arg(long, value_name = "MG")]
    sodium_mg: Option<f64>,
    /// Any other NEVO nutrient, in NEVO's unit: --nutrient VITC=27
    #[arg(long = "nutrient", value_name = "CODE=VALUE")]
    nutrients: Vec<String>,
}

impl AddArgs {
    fn has_values(&self) -> bool {
        [
            self.kcal,
            self.kj,
            self.protein,
            self.fat,
            self.saturated,
            self.carbs,
            self.sugars,
            self.fibre,
            self.salt,
            self.sodium_mg,
        ]
        .iter()
        .any(Option::is_some)
            || !self.nutrients.is_empty()
    }

    fn entry(&self) -> Result<CustomEntry> {
        let mut nutrients = std::collections::BTreeMap::new();
        for pair in &self.nutrients {
            let (code, value) = pair
                .split_once('=')
                .and_then(|(c, v)| Some((c.trim(), parse_number(v)?)))
                .with_context(|| format!("--nutrient wants CODE=VALUE, not '{pair}'"))?;
            nutrients.insert(code.to_owned(), value);
        }
        Ok(CustomEntry {
            per: self.per.clone(),
            source: self.source.clone(),
            energy_kcal: self.kcal,
            energy_kj: self.kj,
            protein: self.protein,
            fat: self.fat,
            saturated_fat: self.saturated,
            carbohydrate: self.carbs,
            sugars: self.sugars,
            fibre: self.fibre,
            salt: self.salt,
            sodium_mg: self.sodium_mg,
            nutrients,
        })
    }
}

pub fn run(library: &Library, command: CustomCommand) -> Result<ExitCode> {
    match command {
        CustomCommand::Add(args) => add(library, args),
        CustomCommand::List => list(library),
    }
}

fn list(library: &Library) -> Result<ExitCode> {
    let foods = library.custom_foods()?;
    if foods.is_empty() {
        println!(
            "no own foods yet ({} doesn't exist or is empty)\nAdd one with `grocery custom add \"name\"`.",
            library.custom_path().display()
        );
        return Ok(ExitCode::SUCCESS);
    }
    let aliases = library.food_db()?;
    for food in foods.iter() {
        let per = match food.basis {
            Basis::Per100g => "100 g",
            Basis::Per100ml => "100 ml",
        };
        let get = |code: &str, unit: &str| {
            food.values
                .get(code)
                .map_or_else(|| "–".to_owned(), |v| format!("{v:.1} {unit}"))
        };
        let used = aliases
            .foods()
            .filter(|(_, i)| {
                i.custom
                    .as_deref()
                    .is_some_and(|c| c.eq_ignore_ascii_case(&food.name))
            })
            .map(|(n, _)| n)
            .collect::<Vec<_>>();
        println!(
            "{} (per {per}): {}, protein {}, fat {}, carbs {}; {} nutrients",
            food.name,
            get("ENERCC", "kcal"),
            get("PROT", "g"),
            get("FAT", "g"),
            get("CHO", "g"),
            food.values.len()
        );
        if let Some(source) = &food.source {
            println!("    source: {source}");
        }
        if used.is_empty() {
            println!("    not used by any food in aliases.yaml yet");
        } else {
            println!("    used by: {}", used.join(", "));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn add(library: &Library, mut args: AddArgs) -> Result<ExitCode> {
    if !args.has_values() {
        if !std::io::stdin().is_terminal() {
            bail!(
                "no values given. Pass flags (--kcal 23 --protein 2.1 ...), or run this in a terminal to be asked."
            );
        }
        prompt(&mut args)?;
        if !args.has_values() {
            bail!("nothing entered, so nothing was added");
        }
    }
    let entry = args.entry()?;
    let path = library.custom_path();
    let existing = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let updated = append_entry(&existing, &args.name, &entry, &path)?;
    write_atomically(&path, &updated)?;

    let per = entry.per.as_deref().unwrap_or("100g");
    println!(
        "Added '{}' (per {per}) to {}",
        args.name.trim(),
        path.display()
    );
    let missing: Vec<&str> = [
        (
            "energy",
            entry.energy_kcal.is_some() || entry.energy_kj.is_some(),
        ),
        ("protein", entry.protein.is_some()),
        ("fat", entry.fat.is_some()),
        ("carbohydrate", entry.carbohydrate.is_some()),
        ("salt", entry.salt.is_some() || entry.sodium_mg.is_some()),
    ]
    .into_iter()
    .filter(|(_, given)| !given)
    .map(|(name, _)| name)
    .collect();
    if !missing.is_empty() {
        println!(
            "Not given (stays unknown, totals using it are marked *): {}",
            missing.join(", ")
        );
    }
    point_to_aliases(library, args.name.trim());
    Ok(ExitCode::SUCCESS)
}

/// Tell the user the one line that makes the food count in nutrition.
fn point_to_aliases(library: &Library, name: &str) {
    println!("\nTo use it, point a food in Food/aliases.yaml at it:");
    let resolved = library.food_db().ok().and_then(|db| {
        db.resolve(name)
            .map(|r| (r.canonical.to_owned(), r.info.nevo, r.info.custom.clone()))
    });
    match resolved {
        Some((canonical, None, None)) => {
            println!("  {canonical}:\n    custom: {name}");
        }
        Some((canonical, None, Some(c))) if c.eq_ignore_ascii_case(name) => {
            println!("  already done: '{canonical}' in aliases.yaml uses it.");
        }
        Some((canonical, nevo, custom)) => println!(
            "  '{name}' already resolves to '{canonical}', which has {}; change that entry if you meant to use the new values.",
            match (nevo, custom) {
                (Some(code), _) => format!("nevo: {code}"),
                (_, Some(c)) => format!("custom: {c}"),
                _ => "no source".into(),
            }
        ),
        None => println!(
            "  an existing food: add `custom: {name}` to it, or a new one:\n  \
             {name}:\n    custom: {name}\n    aisle: ...\n    synonyms: [...]"
        ),
    }
}

struct Prompter<R: BufRead> {
    lines: std::io::Lines<R>,
}

impl<R: BufRead> Prompter<R> {
    /// One line of input; `None` for an empty answer (or end of input).
    fn ask(&mut self, question: &str) -> Result<Option<String>> {
        print!("{question}: ");
        std::io::stdout().flush()?;
        match self.lines.next() {
            Some(line) => {
                let line = line?;
                let t = line.trim();
                Ok((!t.is_empty()).then(|| t.to_owned()))
            }
            None => Ok(None),
        }
    }

    /// A number ≥ 0, asked again until it is one; `None` to skip.
    fn number(&mut self, question: &str) -> Result<Option<f64>> {
        loop {
            let Some(text) = self.ask(question)? else {
                return Ok(None);
            };
            match parse_number(&text) {
                Some(v) if v >= 0.0 => return Ok(Some(v)),
                _ => println!("  not a number, try again (or press Enter to skip)"),
            }
        }
    }
}

/// Ask for each label line; empty answer = not on the label.
fn prompt(args: &mut AddArgs) -> Result<()> {
    let mut p = Prompter {
        lines: std::io::stdin().lock().lines(),
    };
    println!(
        "Adding '{}'. Enter the values from the package, as printed per 100 g (or ml).\n\
         Press Enter to skip a line the package doesn't have.\n",
        args.name.trim()
    );
    if args.per.is_none() {
        args.per = p.ask("Values are per [100g]/100ml")?;
    }
    if args.source.is_none() {
        args.source = p.ask("Source, e.g. \"package, Albert Heijn\"")?;
    }
    args.kcal = args.kcal.or(p.number("Energy, kcal")?);
    if args.kcal.is_none() {
        args.kj = args.kj.or(p.number("Energy, kJ")?);
    }
    args.fat = args.fat.or(p.number("Fat, g")?);
    args.saturated = args.saturated.or(p.number("  of which saturated, g")?);
    args.carbs = args.carbs.or(p.number("Carbohydrate, g")?);
    args.sugars = args.sugars.or(p.number("  of which sugars, g")?);
    args.fibre = args.fibre.or(p.number("Fibre, g")?);
    args.protein = args.protein.or(p.number("Protein, g")?);
    if args.salt.is_none() && args.sodium_mg.is_none() {
        args.salt = p.number("Salt, g")?;
    }
    println!();
    Ok(())
}

/// Write via a temp file in the same directory, so a crash can't leave a
/// half-written custom.yaml.
fn write_atomically(path: &Path, text: &str) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let tmp = path.with_extension("yaml.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// For `grocery check`: names that `custom:` points at but custom.yaml lacks.
pub fn missing_references(
    foods: &grocery_core::food::FoodDb,
    custom: &CustomFoods,
) -> Vec<(String, String)> {
    foods
        .foods()
        .filter_map(|(name, info)| {
            let c = info.custom.as_deref()?;
            (!custom.contains(c)).then(|| (name.to_owned(), c.to_owned()))
        })
        .collect()
}
