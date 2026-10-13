//! `.md` recipes with the ingredient list in YAML frontmatter.
//!
//! ```markdown
//! ---
//! type: recipe
//! title: Shakshuka          # optional, defaults to the file name
//! servings: 2
//! ingredients:
//!   - { item: egg, qty: 4 }
//!   - { item: passata, qty: 500, unit: g }
//!   - { item: feta, qty: 100, unit: g, optional: true }
//!   - { item: salt, qty: 1, unit: tsp, scale: false }
//!   - 1 tl komijn              # shorthand: "<qty> [unit] name[, note] [(optional)]"
//!   - 2 cloves garlic, crushed
//!   - peterselie               # no quantity = to taste
//! ---
//! ## Steps
//! ...
//! ```
//!
//! The body is ignored; it's for humans.

use std::path::Path;

use serde::Deserialize;

use super::{title_from_path, RecipeSource};
use crate::error::{Error, Result};
use crate::model::{Amount, Ingredient, Recipe};
use crate::units::{is_standard_unit, parse_number};

#[derive(Debug, Default)]
pub struct FrontmatterSource;

#[derive(Deserialize)]
struct Frontmatter {
    title: Option<String>,
    servings: Option<NumberOrText>,
    ingredients: Option<Vec<IngredientEntry>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum NumberOrText {
    Number(f64),
    Text(String),
}

impl NumberOrText {
    /// `4`, `"4"`, `"4 personen"`, `"1/2"` → a number; `"veel"` → None.
    fn as_number(&self) -> Option<f64> {
        match self {
            NumberOrText::Number(n) => Some(*n),
            NumberOrText::Text(t) => t.split_whitespace().next().and_then(parse_number),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IngredientEntry {
    Full(FullEntry),
    Short(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FullEntry {
    item: String,
    qty: Option<NumberOrText>,
    unit: Option<String>,
    #[serde(default)]
    optional: bool,
    note: Option<String>,
    /// `scale: false` keeps the amount fixed when changing servings.
    #[serde(default = "default_true")]
    scale: bool,
}

fn default_true() -> bool {
    true
}

impl RecipeSource for FrontmatterSource {
    fn parse(&self, path: &Path, text: &str) -> Result<Recipe> {
        let yaml = split_frontmatter(text).ok_or_else(|| Error::NotARecipe(path.into()))?;
        let fm: Frontmatter = serde_norway::from_str(yaml).map_err(|source| Error::Yaml {
            path: path.into(),
            source,
        })?;
        let entries = fm
            .ingredients
            .ok_or_else(|| Error::NotARecipe(path.into()))?;

        let mut warnings = Vec::new();
        let servings = match &fm.servings {
            None => None,
            Some(s) => {
                let n = s.as_number();
                if n.is_none() {
                    warnings.push("could not read `servings` as a number".to_owned());
                }
                n
            }
        };

        let ingredients = entries
            .into_iter()
            .map(|entry| match entry {
                IngredientEntry::Short(s) => parse_shorthand(&s),
                IngredientEntry::Full(f) => full_entry(f),
            })
            .filter(|i| !i.name.is_empty())
            .collect();

        Ok(Recipe {
            path: path.into(),
            title: fm.title.unwrap_or_else(|| title_from_path(path)),
            servings,
            ingredients,
            warnings,
        })
    }
}

fn full_entry(f: FullEntry) -> Ingredient {
    let unit = f.unit.filter(|u| !u.trim().is_empty());
    let amount = match f.qty {
        None => Amount::Unspecified,
        Some(NumberOrText::Number(qty)) => Amount::Value {
            qty,
            unit,
            scalable: f.scale,
        },
        Some(NumberOrText::Text(t)) => match parse_number(&t) {
            Some(qty) => Amount::Value {
                qty,
                unit,
                scalable: f.scale,
            },
            None => Amount::Text(t),
        },
    };
    Ingredient {
        name: f.item.trim().to_owned(),
        amount,
        optional: f.optional,
        note: f.note,
    }
}

/// Return the YAML between a leading `---` line and the next `---`/`...`.
fn split_frontmatter(text: &str) -> Option<&str> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split_inclusive('\n');
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let start = text.find('\n')? + 1;
    let mut offset = start;
    for line in lines {
        if matches!(line.trim_end(), "---" | "...") {
            return Some(&text[start..offset]);
        }
        offset += line.len();
    }
    None
}

/// Parse a one-line ingredient: `"<qty> [unit] name[, note] [(optional)]"`.
///
/// - `400 g spaghetti`, `1 1/2 cups flour`, `0,5 l melk`, `2-3 tomatoes`
/// - `2 onions` (no unit → pieces), `3 cloves garlic` (named unit: the
///   word after the number is a unit when the name continues after it)
/// - `feta (optional)`, `2 tomatoes, diced`, `salt` (no quantity)
pub fn parse_shorthand(s: &str) -> Ingredient {
    let mut rest = s.trim();
    let mut optional = false;
    for marker in ["(optional)", "(optioneel)"] {
        if let Some(stripped) = strip_suffix_ci(rest, marker) {
            rest = stripped.trim_end();
            optional = true;
        }
    }
    // ", " (with a space) so decimal commas like "0,5 l" survive.
    let (main, note) = match rest.split_once(", ") {
        Some((m, n)) if !n.trim().is_empty() => (m.trim(), Some(n.trim().to_owned())),
        Some((m, _)) => (m.trim(), None),
        None => (rest, None),
    };

    let tokens: Vec<&str> = main.split_whitespace().collect();
    let mut i = 0;
    let mut qty = None;
    // leading number, optionally followed by a fraction: "1 1/2"
    if let Some(n) = tokens.first().and_then(|t| parse_number(t)) {
        qty = Some(n);
        i = 1;
        if let Some(frac) = tokens
            .get(1)
            .filter(|t| t.contains('/'))
            .and_then(|t| parse_number(t))
        {
            qty = Some(n + frac);
            i = 2;
        }
    }

    let mut unit = None;
    if qty.is_some() && tokens.len() > i + 1 {
        // Only known words count as units, so "2 sweet potatoes" stays a
        // name. For anything unusual, use the `{ item, qty, unit }` form.
        let candidate = tokens[i];
        if is_standard_unit(candidate) || is_named_unit(candidate) {
            unit = Some(candidate.to_owned());
            i += 1;
        }
    }

    let name = tokens[i..].join(" ");
    let amount = match qty {
        Some(qty) => Amount::Value {
            qty,
            unit,
            scalable: true,
        },
        None => Amount::Unspecified,
    };
    Ingredient {
        name,
        amount,
        optional,
        note,
    }
}

/// Food-specific units the shorthand recognizes (English and Dutch).
/// Their gram weights live per food in `aliases.yaml` under `units:`.
fn is_named_unit(word: &str) -> bool {
    matches!(
        crate::units::named_key(word).as_str(),
        "clove"
            | "can"
            | "tin"
            | "jar"
            | "bunch"
            | "pinch"
            | "handful"
            | "slice"
            | "sprig"
            | "stalk"
            | "head"
            | "packet"
            | "pack"
            | "bag"
            | "sheet"
            | "cube"
            | "dash"
            | "knob"
            | "teen"
            | "tenen"
            | "blik"
            | "blikje"
            | "pot"
            | "potje"
            | "bos"
            | "bosje"
            | "snuf"
            | "snufje"
            | "handje"
            | "plak"
            | "plakje"
            | "takje"
            | "stengel"
            | "krop"
            | "pak"
            | "pakje"
            | "zak"
            | "zakje"
            | "vel"
            | "velletje"
            | "blok"
            | "blokje"
            | "scheut"
            | "scheutje"
            | "klont"
            | "klontje"
            | "partje"
    )
}

fn strip_suffix_ci<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    let split = s.len().checked_sub(suffix.len())?;
    (s.is_char_boundary(split) && s[split..].eq_ignore_ascii_case(suffix)).then(|| &s[..split])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn short(s: &str) -> (String, Amount, bool, Option<String>) {
        let i = parse_shorthand(s);
        (i.name, i.amount, i.optional, i.note)
    }

    fn v(qty: f64, unit: Option<&str>) -> Amount {
        Amount::value(qty, unit)
    }

    #[test]
    fn shorthand_forms() {
        assert_eq!(short("400 g spaghetti").1, v(400.0, Some("g")));
        assert_eq!(short("400 g spaghetti").0, "spaghetti");
        assert_eq!(short("1 1/2 cups flour").1, v(1.5, Some("cups")));
        assert_eq!(
            short("0,5 l halfvolle melk"),
            ("halfvolle melk".into(), v(0.5, Some("l")), false, None)
        );
        assert_eq!(
            short("2 onions"),
            ("onions".into(), v(2.0, None), false, None)
        );
        assert_eq!(
            short("2 large onions"),
            ("large onions".into(), v(2.0, None), false, None)
        );
        assert_eq!(short("3 cloves garlic").1, v(3.0, Some("cloves")));
        assert_eq!(
            short("2-3 tomatoes, diced"),
            ("tomatoes".into(), v(3.0, None), false, Some("diced".into()))
        );
        assert_eq!(
            short("feta (Optional)"),
            ("feta".into(), Amount::Unspecified, true, None)
        );
        assert_eq!(
            short("salt"),
            ("salt".into(), Amount::Unspecified, false, None)
        );
        assert_eq!(short("1 blik tomatenblokjes").1, v(1.0, Some("blik")));
        assert_eq!(short("1 bos lente-ui").1, v(1.0, Some("bos")));
        assert_eq!(
            short("2 sweet potatoes"),
            ("sweet potatoes".into(), v(2.0, None), false, None)
        );
    }

    #[test]
    fn frontmatter_recipe() {
        let text = "---\ntype: recipe\nservings: \"2 personen\"\ningredients:\n\
                    \x20 - { item: egg, qty: 4 }\n\
                    \x20 - { item: salt, qty: 1, unit: tsp, scale: false }\n\
                    \x20 - { item: basil, qty: a handful }\n\
                    \x20 - { item: feta, qty: \"1/2\", unit: blok, optional: true }\n\
                    \x20 - 500 g passata\n\
                    ---\n# Shakshuka\nBody is ignored.\n";
        let r = FrontmatterSource
            .parse(Path::new("Recipes/Shakshuka.md"), text)
            .unwrap();
        assert_eq!(r.title, "Shakshuka");
        assert_eq!(r.servings, Some(2.0));
        assert_eq!(r.ingredients.len(), 5);
        assert_eq!(r.ingredients[0].amount, v(4.0, None));
        assert_eq!(
            r.ingredients[1].amount,
            Amount::Value {
                qty: 1.0,
                unit: Some("tsp".into()),
                scalable: false
            }
        );
        assert_eq!(r.ingredients[2].amount, Amount::Text("a handful".into()));
        assert!(r.ingredients[3].optional);
        assert_eq!(r.ingredients[3].amount, v(0.5, Some("blok")));
        assert_eq!(r.ingredients[4].name, "passata");
    }

    #[test]
    fn non_recipes_are_reported_as_such() {
        let note = "---\ntags: [idea]\n---\nJust a note\n";
        assert!(matches!(
            FrontmatterSource.parse(Path::new("Notes/x.md"), note),
            Err(Error::NotARecipe(_))
        ));
        assert!(matches!(
            FrontmatterSource.parse(Path::new("Notes/y.md"), "# no frontmatter"),
            Err(Error::NotARecipe(_))
        ));
    }

    #[test]
    fn typo_in_a_field_is_not_silently_turned_into_a_string() {
        // `qyt` is a typo; deny_unknown_fields makes the Full variant fail and
        // the untagged enum then fails too (a map is not a string).
        let text = "---\ningredients:\n  - { item: egg, qyt: 4 }\n---\n";
        assert!(FrontmatterSource.parse(Path::new("x.md"), text).is_err());
    }

    #[test]
    fn frontmatter_split_handles_crlf_and_bom() {
        let text = "\u{feff}---\r\ningredients:\r\n  - salt\r\n---\r\nbody";
        let r = FrontmatterSource.parse(Path::new("x.md"), text).unwrap();
        assert_eq!(r.ingredients[0].name, "salt");
    }
}
