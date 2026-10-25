//! Text exports of shopping lists and recipes.
//!
//! - [`markdown`]: checkbox lines, one `##` section per aisle.
//! - [`plain`]: what's left to buy, for pasting into Signal (no Markdown syntax).
//! - [`recipe_markdown`], [`recipe_plain`]: a recipe with its method, optionally
//!   scaled to a number of servings.

use std::fmt::Write as _;

use crate::aggregate::{Item, Qty, ShoppingList};
use crate::model::{Amount, Content, Recipe, RecipeTime};
use crate::units::Dim;

/// Heading used for items without an aisle.
pub const OTHER_AISLE: &str = "Other";

/// Format a quantity for humans: `320 g`, `1.25 kg`, `30 ml`, `2`, `3 cloves`.
/// Counts and named units round *up*: you can't buy 0.75 onion.
pub fn format_qty(q: &Qty) -> String {
    match &q.dim {
        Dim::Mass => metric(q.value, "g", "kg"),
        Dim::Volume => metric(q.value, "ml", "l"),
        Dim::Count => fmt_num(round_up(q.value)),
        Dim::Named(key) => {
            let label = q.label.as_deref().filter(|l| !l.is_empty()).unwrap_or(key);
            format!("{} {label}", fmt_num(round_up(q.value)))
        }
    }
}

fn metric(value: f64, small: &str, large: &str) -> String {
    if value >= 1000.0 {
        format!("{} {large}", fmt_num(round_to(value / 1000.0, 2)))
    } else if value >= 10.0 {
        format!("{} {small}", fmt_num(value.round()))
    } else {
        format!("{} {small}", fmt_num(round_to(value, 1)))
    }
}

fn round_up(v: f64) -> f64 {
    // tolerate float noise: 3 × (1/3) must stay 1, not become 2
    (v - 1e-6).ceil().max(if v > 0.0 { 1.0 } else { 0.0 })
}

fn round_to(v: f64, decimals: i32) -> f64 {
    let p = 10f64.powi(decimals);
    (v * p).round() / p
}

/// `2.0` → "2", `2.50` → "2.5".
fn fmt_num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn item_text(item: &Item) -> String {
    let mut s = item.shown();
    if let Some(q) = &item.quantity {
        let _ = write!(s, " — {}", format_qty(q));
    }
    if !item.notes.is_empty() {
        let sep = if item.quantity.is_some() {
            ", "
        } else {
            " — "
        };
        let _ = write!(s, "{sep}{}", item.notes.join(", "));
    }
    if item.optional {
        s.push_str(" (optional)");
    }
    s
}

/// Items grouped by aisle, keeping the order `aggregate::build` sorted them in.
fn by_aisle(items: &[Item]) -> Vec<(String, Vec<&Item>)> {
    let mut groups: Vec<(String, Vec<&Item>)> = Vec::new();
    for item in items {
        let heading = item
            .aisle
            .as_deref()
            .map(capitalize)
            .unwrap_or_else(|| OTHER_AISLE.to_owned());
        match groups.last_mut() {
            Some((h, list)) if *h == heading => list.push(item),
            _ => groups.push((heading, vec![item])),
        }
    }
    groups
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The list as Markdown, ticked items as `- [x]`.
pub fn markdown(list: &ShoppingList, heading: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {heading}\n");
    if !list.sources.is_empty() {
        let _ = writeln!(out, "For: {}\n", sources_text(list));
    }
    for (aisle, items) in by_aisle(&list.items) {
        let _ = writeln!(out, "## {aisle}\n");
        for item in items {
            let mark = if item.checked { 'x' } else { ' ' };
            let _ = writeln!(out, "- [{mark}] {}", item_text(item));
        }
        out.push('\n');
    }
    if !list.pantry_skipped.is_empty() {
        let _ = writeln!(
            out,
            "From the pantry (not listed): {}",
            list.pantry_skipped.join(", ")
        );
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

/// For pasting into a chat: what's still to buy, without Markdown syntax.
/// Ticked items are left out, and so are aisles with nothing left.
pub fn plain(list: &ShoppingList) -> String {
    let open: Vec<Item> = list.items.iter().filter(|i| !i.checked).cloned().collect();
    let mut out = String::new();
    for (aisle, items) in by_aisle(&open) {
        let _ = writeln!(out, "{aisle}");
        for item in items {
            let _ = writeln!(out, "• {}", item_text(item));
        }
        out.push('\n');
    }
    out.trim_end().to_owned()
}

fn sources_text(list: &ShoppingList) -> String {
    list.sources
        .iter()
        .map(|s| match s.servings {
            Some(n) => format!("{} ({} servings)", s.title, fmt_num(n)),
            None => s.title.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A recipe prepared for display: header line, ingredient lines, method.
/// The text exports and the web app's recipe screen both use this.
pub struct RecipeView<'a> {
    pub recipe: &'a Recipe,
    /// "Serves 2 (recipe: 4) · pasta, quick"
    pub info: String,
    /// "spaghetti — 200 g", one per ingredient name, scaled.
    pub ingredients: Vec<String>,
    /// What amounts are multiplied by.
    pub factor: f64,
}

impl<'a> RecipeView<'a> {
    /// `servings` rescales the ingredient amounts; it's ignored for recipes
    /// that don't say how many they serve.
    pub fn new(recipe: &'a Recipe, servings: Option<f64>) -> Self {
        let (factor, serves) = match (servings, recipe.servings) {
            (Some(want), Some(base)) if want > 0.0 && base > 0.0 && want != base => (
                want / base,
                Some(format!(
                    "Serves {} (recipe: {})",
                    fmt_num(want),
                    fmt_num(base)
                )),
            ),
            (_, Some(base)) => (1.0, Some(format!("Serves {}", fmt_num(base)))),
            (_, None) => (1.0, None),
        };
        let tags = (!recipe.tags.is_empty()).then(|| recipe.tags.join(", "));
        let time = recipe.time.as_ref().map(time_text);
        let info = [serves, time, tags]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        Self {
            recipe,
            info,
            ingredients: ingredient_lines(recipe, factor),
            factor,
        }
    }

    fn write(&self, out: &mut String, markdown: bool) {
        let r = self.recipe;
        let h2 = if markdown { "## " } else { "" };
        let bullet = if markdown { "- " } else { "• " };

        if markdown {
            let _ = writeln!(out, "# {}\n", r.title);
        } else {
            let _ = writeln!(out, "{}\n", r.title);
        }
        if !self.info.is_empty() {
            let _ = writeln!(out, "{}\n", self.info);
        }
        if let Some(d) = &r.description {
            let _ = writeln!(out, "{}\n", d.trim());
        }
        if !self.ingredients.is_empty() {
            let _ = writeln!(out, "{h2}Ingredients\n");
            for line in &self.ingredients {
                let _ = writeln!(out, "{bullet}{line}");
            }
            out.push('\n');
        }
        if r.sections.iter().any(|s| !s.content.is_empty()) {
            let _ = writeln!(out, "{h2}Method\n");
        }
        for section in &r.sections {
            if let Some(name) = &section.name {
                if markdown {
                    let _ = writeln!(out, "### {name}\n");
                } else {
                    let _ = writeln!(out, "{name}\n");
                }
            }
            let mut n = 0;
            for content in &section.content {
                match content {
                    Content::Step(step) => {
                        n += 1;
                        let _ = writeln!(out, "{n}. {}\n", step.text());
                    }
                    Content::Note(text) if markdown => {
                        let _ = writeln!(out, "> {text}\n");
                    }
                    Content::Note(text) => {
                        let _ = writeln!(out, "{text}\n");
                    }
                }
            }
        }
        out.truncate(out.trim_end().len());
        out.push('\n');
    }
}

/// One line per ingredient name, in recipe order. Amounts of the same name
/// and unit are summed (`@flour{300%g}` … `@&flour{100%g}` → 400 g).
fn ingredient_lines(recipe: &Recipe, factor: f64) -> Vec<String> {
    struct Line<'a> {
        name: &'a str,
        optional: bool,
        notes: Vec<&'a str>,
        values: Vec<(f64, Option<String>)>,
        texts: Vec<String>,
    }
    let mut lines: Vec<Line> = Vec::new();
    for ing in &recipe.ingredients {
        let idx = match lines.iter().position(|l| l.name == ing.name) {
            Some(i) => i,
            None => {
                lines.push(Line {
                    name: &ing.name,
                    optional: ing.optional,
                    notes: vec![],
                    values: vec![],
                    texts: vec![],
                });
                lines.len() - 1
            }
        };
        let line = &mut lines[idx];
        if let Some(note) = ing.note.as_deref() {
            if !line.notes.contains(&note) {
                line.notes.push(note);
            }
        }
        match ing.amount.scaled(factor) {
            Amount::Unspecified => {}
            Amount::Text(t) => line.texts.push(t),
            Amount::Value { qty, unit, .. } => {
                match line.values.iter_mut().find(|(_, u)| *u == unit) {
                    Some((sum, _)) => *sum += qty,
                    None => line.values.push((qty, unit)),
                }
            }
        }
    }
    lines
        .into_iter()
        .map(|l| {
            let mut amounts: Vec<String> = l
                .values
                .iter()
                .map(|(qty, unit)| match unit {
                    Some(u) => format!("{} {u}", fmt_amount(*qty)),
                    None => fmt_amount(*qty),
                })
                .collect();
            amounts.extend(l.texts);
            let mut s = l.name.to_owned();
            if !amounts.is_empty() {
                let _ = write!(s, " — {}", amounts.join(" + "));
            }
            if !l.notes.is_empty() {
                let _ = write!(s, " ({})", l.notes.join("; "));
            }
            if l.optional {
                s.push_str(" (optional)");
            }
            s
        })
        .collect()
}

/// An ingredient amount for reading, scaled by `factor`: "200 g", "2",
/// "a handful". `None` when the recipe gives no amount.
pub fn amount_text(amount: &Amount, factor: f64) -> Option<String> {
    match amount.scaled(factor) {
        Amount::Unspecified => None,
        Amount::Text(t) => Some(t),
        Amount::Value {
            qty, unit: Some(u), ..
        } => Some(format!("{} {u}", fmt_amount(qty))),
        Amount::Value {
            qty, unit: None, ..
        } => Some(fmt_amount(qty)),
    }
}

/// "40 min", "1 h 15 min".
pub fn minutes_text(minutes: f64) -> String {
    let m = minutes.round() as u64;
    match (m / 60, m % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// "40 min", or "prep 10 min, cook 30 min" when that's what the recipe says.
pub fn time_text(time: &RecipeTime) -> String {
    match (time.total, time.prep, time.cook) {
        (Some(t), _, _) => minutes_text(t),
        (None, p, c) => [p.map(|p| ("prep", p)), c.map(|c| ("cook", c))]
            .into_iter()
            .flatten()
            .map(|(what, m)| format!("{what} {}", minutes_text(m)))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Recipe amounts as written, rounded for reading: 266.67 g → 267, 1.333 → 1.33.
fn fmt_amount(v: f64) -> String {
    if v >= 10.0 {
        fmt_num(v.round())
    } else {
        fmt_num(round_to(v, 2))
    }
}

/// The recipe as Markdown: ingredients (scaled to `servings` if given) and method.
pub fn recipe_markdown(recipe: &Recipe, servings: Option<f64>) -> String {
    let mut out = String::new();
    RecipeView::new(recipe, servings).write(&mut out, true);
    out
}

/// The recipe as plain text, for a chat or a note.
pub fn recipe_plain(recipe: &Recipe, servings: Option<f64>) -> String {
    let mut out = String::new();
    RecipeView::new(recipe, servings).write(&mut out, false);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::Source;

    fn q(dim: Dim, value: f64) -> Qty {
        Qty {
            dim,
            value,
            label: None,
        }
    }

    #[test]
    fn quantities() {
        assert_eq!(format_qty(&q(Dim::Mass, 320.4)), "320 g");
        assert_eq!(format_qty(&q(Dim::Mass, 2.54)), "2.5 g");
        assert_eq!(format_qty(&q(Dim::Mass, 1250.0)), "1.25 kg");
        assert_eq!(format_qty(&q(Dim::Volume, 1000.0)), "1 l");
        assert_eq!(format_qty(&q(Dim::Volume, 30.0)), "30 ml");
        assert_eq!(format_qty(&q(Dim::Count, 0.75)), "1");
        assert_eq!(format_qty(&q(Dim::Count, 3.0 * (1.0 / 3.0))), "1");
        assert_eq!(format_qty(&q(Dim::Count, 2.2)), "3");
        let cloves = Qty {
            dim: Dim::Named("clove".into()),
            value: 4.5,
            label: Some("cloves".into()),
        };
        assert_eq!(format_qty(&cloves), "5 cloves");
    }

    fn item(name: &str, aisle: Option<&str>, quantity: Option<Qty>) -> Item {
        Item {
            name: name.into(),
            written: vec![],
            aisle: aisle.map(Into::into),
            quantity,
            notes: vec![],
            optional: false,
            recipes: vec!["Pasta".into()],
            known: true,
            checked: false,
            parts: vec![],
        }
    }

    fn list() -> ShoppingList {
        let mut basil = item("basil", Some("groente"), None);
        basil.notes = vec!["a handful".into()];
        basil.checked = true;
        let mut parm = item("parmesan", None, Some(q(Dim::Mass, 50.0)));
        parm.optional = true;
        ShoppingList {
            sources: vec![Source {
                title: "Pasta".into(),
                path: "Recipes/Pasta.cook".into(),
                servings: Some(4.0),
                factor: 2.0,
            }],
            items: vec![
                item("onion", Some("groente"), Some(q(Dim::Count, 2.0))),
                basil,
                parm,
            ],
            pantry_skipped: vec!["salt".into()],
            unmapped: vec![],
            warnings: vec![],
        }
    }

    #[test]
    fn markdown_output() {
        let md = markdown(&list(), "Shopping list 2026-10-12");
        let expected = "\
# Shopping list 2026-10-12

For: Pasta (4 servings)

## Groente

- [ ] onion — 2
- [x] basil — a handful

## Other

- [ ] parmesan — 50 g (optional)

From the pantry (not listed): salt
";
        assert_eq!(md, expected);
    }

    #[test]
    fn plain_output_leaves_out_ticked_items() {
        assert_eq!(
            plain(&list()),
            "Groente\n• onion — 2\n\nOther\n• parmesan — 50 g (optional)"
        );
        let mut all_done = list();
        all_done.items.iter_mut().for_each(|i| i.checked = true);
        assert_eq!(plain(&all_done), "");
    }

    fn recipe() -> Recipe {
        crate::parse::Parser::new()
            .parse(
                std::path::Path::new("Recipes/Pasta.cook"),
                "---\nservings: 4\ntime: 25 min\ntags: [pasta]\n---\n\
                 Boil @spaghetti{400%g} with @salt.\n\n\
                 > Save some pasta water.\n\n\
                 == Sauce ==\n\
                 Fry @garlic{2%cloves}(sliced) in @olive oil{3%tbsp}, add @&garlic{1%clove}.\n\n\
                 Finish with @?parmesan{60%g} and @basil{a handful}.\n",
            )
            .unwrap()
    }

    #[test]
    fn recipe_markdown_output() {
        let expected = "\
# Pasta

Serves 4 · 25 min · pasta

## Ingredients

- spaghetti — 400 g
- salt
- garlic — 2 cloves + 1 clove (sliced)
- olive oil — 3 tbsp
- parmesan — 60 g (optional)
- basil — a handful

## Method

1. Boil spaghetti with salt.

> Save some pasta water.

### Sauce

1. Fry garlic in olive oil, add garlic.

2. Finish with parmesan and basil.
";
        assert_eq!(recipe_markdown(&recipe(), None), expected);
    }

    #[test]
    fn times_and_amounts_for_reading() {
        assert_eq!(minutes_text(75.0), "1 h 15 min");
        assert_eq!(minutes_text(120.0), "2 h");
        let t = RecipeTime {
            prep: Some(10.0),
            cook: Some(30.0),
            ..Default::default()
        };
        assert_eq!(time_text(&t), "prep 10 min, cook 30 min");
        assert_eq!(
            amount_text(&Amount::value(400.0, Some("g")), 0.5).as_deref(),
            Some("200 g")
        );
        assert_eq!(amount_text(&Amount::Unspecified, 2.0), None);
    }

    #[test]
    fn recipe_plain_scaled() {
        let text = recipe_plain(&recipe(), Some(3.0));
        assert!(
            text.starts_with(
                "Pasta\n\nServes 3 (recipe: 4) · 25 min · pasta\n\nIngredients\n\n• spaghetti — 300 g\n"
            ),
            "{text}"
        );
        assert!(text.contains("• olive oil — 2.25 tbsp\n"), "{text}");
        assert!(text.contains("\nSauce\n\n1. Fry garlic"), "{text}");
        assert!(!text.contains('#') && !text.contains("> "), "{text}");
    }
}
