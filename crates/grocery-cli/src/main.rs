//! `grocery`: shopping lists and nutrition from a library of Cooklang recipes.
//!
//! ```text
//! grocery list "Pasta arrabbiata=4" Shakshuka        # Markdown to stdout
//! grocery list --plain Pasta Shakshuka               # text for Signal
//! grocery export "Pasta arrabbiata=2"                # recipe as Markdown
//! grocery show Shakshuka                             # parsed recipe
//! grocery check                                      # parse errors + unmapped names
//! grocery serve                                      # web service (see grocery-server)
//!
//! grocery import-nevo ~/Downloads/NEVO2025           # load NEVO into SQLite
//! grocery nevo search boerenkool                     # find NEVO codes
//! grocery nevo suggest                               # candidates for unmapped foods
//! grocery nutrition Shakshuka                        # per-serving estimate
//! grocery custom add "fresh coriander"               # your own values, from a label
//! ```

mod custom_cmd;
mod nevo_cmd;

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use grocery_core::aggregate::{self, Selection};
use grocery_core::library::Library;
use grocery_core::model::{Amount, Recipe};
use grocery_core::render;

#[derive(Parser)]
#[command(version, about = "Shopping lists and nutrition from Cooklang recipes")]
struct Cli {
    /// Library root (contains Recipes/ and Food/).
    #[arg(long, env = "GROCERY_LIBRARY", default_value = ".", global = true)]
    library: PathBuf,

    /// SQLite database: NEVO data, recipe index, saved lists.
    /// [default: $XDG_DATA_HOME/grocery/grocery.sqlite]
    #[arg(long, env = "GROCERY_DB", global = true)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Command {
    /// Build a shopping list. RECIPE is a path or a name from Recipes/,
    /// optionally with servings: "Pasta=4".
    List {
        #[arg(required = true, value_name = "RECIPE[=SERVINGS]")]
        recipes: Vec<String>,
        /// Plain text for pasting into Signal instead of Markdown.
        #[arg(long)]
        plain: bool,
    },
    /// Print a recipe as Markdown (or plain text), scaled with "=SERVINGS".
    Export {
        #[arg(value_name = "RECIPE[=SERVINGS]")]
        recipe: String,
        #[arg(long)]
        plain: bool,
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
    /// Your own nutrition values for foods NEVO lacks (Food/custom.yaml).
    Custom {
        #[command(subcommand)]
        command: custom_cmd::CustomCommand,
    },
    /// Run the web service: recipe index, shopping lists, exports.
    Serve {
        /// No authentication: keep this on localhost or a private network.
        #[arg(long, env = "GROCERY_LISTEN", default_value = "127.0.0.1:8080")]
        listen: SocketAddr,
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
    let db = cli.db.unwrap_or_else(nevo_cmd::default_db_path);
    if let Command::Serve { listen } = cli.command {
        grocery_server::run(grocery_server::Config {
            library: cli.library,
            db,
            listen,
        })?;
        return Ok(ExitCode::SUCCESS);
    }
    let library = &Library::new(cli.library);
    match cli.command {
        Command::List { recipes, plain } => list(library, &recipes, plain),
        Command::Export { recipe, plain } => export(library, &recipe, plain),
        Command::Show { recipe } => show(library, &recipe),
        Command::Check => check(library),
        Command::ImportNevo { path } => nevo_cmd::import(&db, &path),
        Command::Nevo { command } => match command {
            NevoCommand::Search { query, limit } => nevo_cmd::search(&db, &query, limit),
            NevoCommand::Show { code, all } => nevo_cmd::show(&db, code, all),
            NevoCommand::Suggest { limit } => nevo_cmd::suggest(library, &db, limit),
        },
        Command::Nutrition {
            recipe,
            all,
            with_optional,
        } => nevo_cmd::nutrition(library, &db, &recipe, all, with_optional),
        Command::Custom { command } => custom_cmd::run(library, command),
        Command::Serve { .. } => unreachable!("handled above"),
    }
}

/// Find and parse a recipe given as a path or name.
pub(crate) fn load(library: &Library, arg: &str) -> Result<Recipe> {
    let path = library.find_recipe(arg)?;
    Ok(library.parser.parse_file(&path)?)
}

fn list(library: &Library, args: &[String], plain: bool) -> Result<ExitCode> {
    let db = library.food_db()?;
    let pantry = library.pantry()?;

    let mut parsed = Vec::new();
    for arg in args {
        let (name, servings) = split_servings(arg);
        parsed.push((load(library, name)?, servings));
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
    } else {
        let date = chrono::Local::now().format("%Y-%m-%d");
        print!(
            "{}",
            render::markdown(&list, &format!("Shopping list {date}"))
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn export(library: &Library, arg: &str, plain: bool) -> Result<ExitCode> {
    let (name, servings) = split_servings(arg);
    let recipe = load(library, name)?;
    if servings.is_some() && recipe.servings.is_none() {
        eprintln!(
            "warning: {} has no `servings`, so it can't be scaled",
            recipe.title
        );
    }
    if plain {
        print!("{}", render::recipe_plain(&recipe, servings));
    } else {
        print!("{}", render::recipe_markdown(&recipe, servings));
    }
    Ok(ExitCode::SUCCESS)
}

fn show(library: &Library, arg: &str) -> Result<ExitCode> {
    let recipe = load(library, arg)?;
    let db = library.food_db()?;

    println!("{} ({})", recipe.title, recipe.path.display());
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

fn check(library: &Library) -> Result<ExitCode> {
    let dir = library.recipes_dir();
    if !dir.is_dir() {
        bail!("{} does not exist", dir.display());
    }
    let db = library.food_db()?;
    let custom = library.custom_foods()?;
    let (recipes, errors) = library.parser.parse_dir(&dir)?;

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
                Some(res)
                    if res.info.nevo.is_none()
                        && res.info.custom.is_none()
                        && !res.info.negligible =>
                {
                    no_nevo.entry(res.canonical).or_default()
                }
                Some(_) => continue,
            };
            if !users.contains(&r.title.as_str()) {
                users.push(&r.title);
            }
        }
    }

    println!(
        "{} recipes parsed, {} failed, {} foods in aliases.yaml, {} own foods in custom.yaml",
        recipes.len(),
        errors.len(),
        db.len(),
        custom.len()
    );
    let dangling = custom_cmd::missing_references(&db, &custom);
    for (food, name) in &dangling {
        eprintln!(
            "error: aliases.yaml: '{food}' uses custom '{name}', which isn't in Food/custom.yaml"
        );
    }
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
            "\nno nevo or custom entry, so not counted in nutrition ({}; see `grocery nevo suggest`, or `grocery custom add`):",
            no_nevo.len()
        );
        for (name, users) in &no_nevo {
            println!("  {name:<24} {}", users.join(", "));
        }
    }
    Ok(if errors.is_empty() && dangling.is_empty() {
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
}
