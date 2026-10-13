//! `grocery`: build shopping lists from the recipes in an Obsidian vault.
//!
//! ```text
//! grocery list "Pasta arrabbiata=4" Shakshuka        # Markdown to stdout
//! grocery list --plain Pasta Shakshuka               # text for Signal
//! grocery list --write Pasta Shakshuka               # → Shopping/2026-10-12.md
//! grocery show Shakshuka                             # parsed recipe
//! grocery check                                      # parse errors + unmapped names
//!
//! grocery import-nevo ~/Downloads/NEVO2025           # load NEVO into SQLite
//! grocery nevo search boerenkool                     # find NEVO codes
//! grocery nevo suggest                               # candidates for unmapped foods
//! grocery nutrition Shakshuka                        # per-serving estimate
//! ```

mod nevo_cmd;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use grocery_core::aggregate::{self, Selection};
use grocery_core::food::{FoodDb, Pantry};
use grocery_core::model::Amount;
use grocery_core::parse::Parsers;
use grocery_core::render;

#[derive(Parser)]
#[command(version, about = "Shopping lists from Markdown/Cooklang recipes")]
struct Cli {
    /// Vault root (contains Recipes/, Food/, Shopping/).
    #[arg(long, env = "GROCERY_VAULT", default_value = ".", global = true)]
    vault: PathBuf,

    /// SQLite database for NEVO data. Keep it outside the vault.
    /// [default: $XDG_DATA_HOME/grocery/grocery.sqlite]
    #[arg(long, env = "GROCERY_DB", global = true)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build a shopping list. RECIPE is a path or a name from Recipes/,
    /// optionally with servings: "Pasta=4".
    List {
        #[arg(required = true, value_name = "RECIPE[=SERVINGS]")]
        recipes: Vec<String>,
        /// Plain text for pasting into Signal instead of Markdown.
        #[arg(long, conflicts_with = "write")]
        plain: bool,
        /// Write to Shopping/<date>.md in the vault (never overwrites).
        #[arg(long)]
        write: bool,
    },
    /// Show how a recipe was parsed.
    Show { recipe: String },
    /// Parse every recipe; report errors and ingredients missing from aliases.yaml.
    Check,
    /// Import the NEVO-online export (the unzipped folder, or its main CSV).
    ImportNevo { path: PathBuf },
    /// Look up NEVO foods.
    Nevo {
        #[command(subcommand)]
        command: NevoCommand,
    },
    /// Estimate nutrition per serving from NEVO data.
    Nutrition {
        recipe: String,
        /// Show every NEVO nutrient, not just the label-style selection.
        #[arg(long)]
        all: bool,
        /// Count optional ingredients too.
        #[arg(long)]
        with_optional: bool,
    },
}

#[derive(Subcommand)]
enum NevoCommand {
    /// Search by Dutch or English name ("boerenkool", "olive oil").
    Search {
        #[arg(required = true)]
        query: Vec<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Show one food's composition per 100 g.
    Show {
        code: u32,
        #[arg(long)]
        all: bool,
    },
    /// Suggest codes for foods in aliases.yaml that don't have one.
    Suggest {
        #[arg(long, default_value_t = 3)]
        limit: usize,
    },
}

pub(crate) struct Vault {
    root: PathBuf,
    parsers: Parsers,
}

impl Vault {
    fn recipes_dir(&self) -> PathBuf {
        self.root.join("Recipes")
    }

    fn food_db(&self) -> Result<FoodDb> {
        Ok(FoodDb::load(&self.root.join("Food/aliases.yaml"))?)
    }

    fn pantry(&self) -> Result<Pantry> {
        Ok(Pantry::load(&self.root.join("Food/pantry.yaml"))?)
    }

    /// Accept a path (as given, or relative to the vault) or a recipe name
    /// matching a file stem in Recipes/ (case-insensitive).
    fn find_recipe(&self, arg: &str) -> Result<PathBuf> {
        for candidate in [PathBuf::from(arg), self.root.join(arg)] {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        let wanted = arg.to_lowercase();
        let mut matches = Vec::new();
        walk(&self.recipes_dir(), &mut |p: &Path| {
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_lowercase());
            if stem.as_deref() == Some(wanted.as_str()) && self.parsers.for_path(p).is_some() {
                matches.push(p.to_path_buf());
            }
        })?;
        match matches.len() {
            1 => Ok(matches.remove(0)),
            0 => bail!(
                "no recipe named '{arg}' in {}",
                self.recipes_dir().display()
            ),
            _ => bail!(
                "'{arg}' is ambiguous: {}",
                matches
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

fn walk(dir: &Path, f: &mut dyn FnMut(&Path)) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        if path.is_dir() {
            walk(&path, f)?;
        } else {
            f(&path);
        }
    }
    Ok(())
}

/// "Pasta=4" → ("Pasta", Some(4.0)). Only splits when the part after the
/// last '=' is a number, so file names containing '=' still work.
fn split_servings(arg: &str) -> (&str, Option<f64>) {
    if let Some((name, n)) = arg.rsplit_once('=') {
        if let Some(n) = grocery_core::units::parse_number(n) {
            return (name, Some(n));
        }
    }
    (arg, None)
}

fn main() -> ExitCode {
    // Behave like other Unix tools when piped into `head`: exit quietly on
    // a closed pipe instead of panicking in println!.
    #[cfg(unix)]
    // SAFETY: called before any threads exist; restores the default handler.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let vault = Vault {
        root: cli.vault,
        parsers: Parsers::new(),
    };
    let db = cli.db.unwrap_or_else(nevo_cmd::default_db_path);
    match cli.command {
        Command::List {
            recipes,
            plain,
            write,
        } => list(&vault, &recipes, plain, write),
        Command::Show { recipe } => show(&vault, &recipe),
        Command::Check => check(&vault),
        Command::ImportNevo { path } => nevo_cmd::import(&db, &path),
        Command::Nevo { command } => match command {
            NevoCommand::Search { query, limit } => nevo_cmd::search(&db, &query, limit),
            NevoCommand::Show { code, all } => nevo_cmd::show(&db, code, all),
            NevoCommand::Suggest { limit } => nevo_cmd::suggest(&vault, &db, limit),
        },
        Command::Nutrition {
            recipe,
            all,
            with_optional,
        } => nevo_cmd::nutrition(&vault, &db, &recipe, all, with_optional),
    }
}

fn list(vault: &Vault, args: &[String], plain: bool, write: bool) -> Result<ExitCode> {
    let db = vault.food_db()?;
    let pantry = vault.pantry()?;

    let mut parsed = Vec::new();
    for arg in args {
        let (name, servings) = split_servings(arg);
        let path = vault.find_recipe(name)?;
        parsed.push((vault.parsers.parse_file(&path)?, servings));
    }
    let selections: Vec<_> = parsed
        .iter()
        .map(|(recipe, servings)| Selection {
            recipe,
            servings: *servings,
        })
        .collect();
    let list = aggregate::build(&selections, &db, &pantry);

    for w in &list.warnings {
        eprintln!("warning: {w}");
    }
    if !list.unmapped.is_empty() {
        eprintln!(
            "note: not in Food/aliases.yaml (no aisle or gram data): {}",
            list.unmapped.join(", ")
        );
    }

    if plain {
        println!("{}", render::plain(&list));
    } else if write {
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let path = write_new(
            &vault.root.join("Shopping"),
            &date,
            &render::markdown(&list, &date),
        )?;
        println!("{}", path.display());
    } else {
        print!(
            "{}",
            render::markdown(&list, &chrono::Local::now().format("%Y-%m-%d").to_string())
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Write `<dir>/<date>.md`, or `<date>-2.md`, `-3`, ... if it exists.
/// Never overwrites: the app must not clobber a list someone is ticking off.
fn write_new(dir: &Path, date: &str, contents: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    for n in 1.. {
        let name = if n == 1 {
            format!("{date}.md")
        } else {
            format!("{date}-{n}.md")
        };
        let path = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(contents.as_bytes())
                    .with_context(|| format!("writing {}", path.display()))?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("creating {}", path.display())),
        }
    }
    unreachable!()
}

fn show(vault: &Vault, arg: &str) -> Result<ExitCode> {
    let path = vault.find_recipe(arg)?;
    let recipe = vault.parsers.parse_file(&path)?;
    let db = vault.food_db()?;

    println!("{} ({})", recipe.title, path.display());
    match recipe.servings {
        Some(s) => println!("servings: {s}"),
        None => println!("servings: (not set — can't be scaled)"),
    }
    for ing in &recipe.ingredients {
        let amount = match &ing.amount {
            Amount::Unspecified => "to taste".to_owned(),
            Amount::Text(t) => t.clone(),
            Amount::Value {
                qty,
                unit,
                scalable,
            } => format!(
                "{qty} {}{}",
                unit.as_deref().unwrap_or("pc"),
                if *scalable { "" } else { " (fixed)" }
            ),
        };
        let resolved = match db.resolve(&ing.name) {
            Some(r) if r.canonical == ing.name.to_lowercase() => "✓".to_owned(),
            Some(r) => format!("→ {}", r.canonical),
            None => "? not in aliases.yaml".to_owned(),
        };
        let opt = if ing.optional { " (optional)" } else { "" };
        println!("  {:<24} {:<18} {resolved}{opt}", ing.name, amount);
    }
    for w in &recipe.warnings {
        eprintln!("warning: {w}");
    }
    Ok(ExitCode::SUCCESS)
}

fn check(vault: &Vault) -> Result<ExitCode> {
    let dir = vault.recipes_dir();
    if !dir.is_dir() {
        bail!("{} does not exist", dir.display());
    }
    let db = vault.food_db()?;
    let (recipes, errors) = vault.parsers.parse_dir(&dir)?;

    for e in &errors {
        eprintln!("error: {e}");
    }
    for r in &recipes {
        for w in &r.warnings {
            eprintln!("warning: {}: {w}", r.title);
        }
        if r.servings.is_none() {
            eprintln!("warning: {}: no `servings`, can't be scaled", r.title);
        }
    }

    // unmapped name → recipes using it; foods used without a nevo code
    let mut unmapped: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut no_nevo: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for r in &recipes {
        for ing in &r.ingredients {
            let users = match db.resolve(&ing.name) {
                None => unmapped.entry(ing.name.to_lowercase()).or_default(),
                Some(res) if res.info.nevo.is_none() => no_nevo.entry(res.canonical).or_default(),
                Some(_) => continue,
            };
            if !users.contains(&r.title.as_str()) {
                users.push(&r.title);
            }
        }
    }

    println!(
        "{} recipes parsed, {} failed, {} foods in aliases.yaml",
        recipes.len(),
        errors.len(),
        db.len()
    );
    if unmapped.is_empty() {
        println!("every ingredient resolves");
    } else {
        println!("\nnot in Food/aliases.yaml ({}):", unmapped.len());
        for (name, users) in &unmapped {
            println!("  {name:<24} {}", users.join(", "));
        }
    }
    if !no_nevo.is_empty() {
        println!(
            "\nno nevo code, so not counted in nutrition ({}; see `grocery nevo suggest`):",
            no_nevo.len()
        );
        for (name, users) in &no_nevo {
            println!("  {name:<24} {}", users.join(", "));
        }
    }
    Ok(if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servings_suffix() {
        assert_eq!(split_servings("Pasta=4"), ("Pasta", Some(4.0)));
        assert_eq!(split_servings("Pasta"), ("Pasta", None));
        assert_eq!(split_servings("a=b.cook"), ("a=b.cook", None));
        assert_eq!(
            split_servings("Recipes/Pasta.cook=2,5"),
            ("Recipes/Pasta.cook", Some(2.5))
        );
    }

    #[test]
    fn write_new_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("grocery-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = write_new(&dir, "2026-10-12", "one").unwrap();
        let b = write_new(&dir, "2026-10-12", "two").unwrap();
        assert!(a.ends_with("2026-10-12.md"));
        assert!(b.ends_with("2026-10-12-2.md"));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "one");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
