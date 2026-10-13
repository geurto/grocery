//! Output formats for a [`ShoppingList`].
//!
//! - [`markdown`]: the file written to `Shopping/` in the vault. Checkbox
//!   lines, one `##` section per aisle, frontmatter Obsidian understands.
//! - [`plain`]: for pasting into Signal (no Markdown syntax).

use std::fmt::Write as _;

use crate::aggregate::{Item, Qty, ShoppingList};
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
    let mut s = item.name.clone();
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

fn yaml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The vault file. `date` is ISO (`2026-10-12`); passed in so this stays pure.
pub fn markdown(list: &ShoppingList, date: &str) -> String {
    let mut out = String::new();
    out.push_str("---\ntype: shopping-list\n");
    let _ = writeln!(out, "created: {date}");
    out.push_str("status: open\nrecipes:\n");
    for s in &list.sources {
        let _ = writeln!(out, "  - {}", yaml_quote(&format!("[[{}]]", s.link)));
    }
    out.push_str("---\n\n");
    let _ = writeln!(out, "# Shopping list {date}\n");

    if !list.sources.is_empty() {
        let used: Vec<String> = list
            .sources
            .iter()
            .map(|s| match s.servings {
                Some(n) => format!("[[{}]] ({} servings)", s.link, fmt_num(n)),
                None => format!("[[{}]]", s.link),
            })
            .collect();
        let _ = writeln!(out, "For: {}\n", used.join(", "));
    }

    for (aisle, items) in by_aisle(&list.items) {
        let _ = writeln!(out, "## {aisle}\n");
        for item in items {
            let _ = writeln!(out, "- [ ] {}", item_text(item));
        }
        out.push('\n');
    }

    if !list.pantry_skipped.is_empty() {
        let _ = writeln!(
            out,
            "> [!note]- From the pantry (not listed)\n> {}\n",
            list.pantry_skipped.join(", ")
        );
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

/// For Signal: aisle names as plain lines, bullets, nothing Markdown-specific.
pub fn plain(list: &ShoppingList) -> String {
    let mut out = String::new();
    for (aisle, items) in by_aisle(&list.items) {
        let _ = writeln!(out, "{aisle}");
        for item in items {
            let _ = writeln!(out, "• {}", item_text(item));
        }
        out.push('\n');
    }
    out.trim_end().to_owned()
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
            aisle: aisle.map(Into::into),
            quantity,
            notes: vec![],
            optional: false,
            recipes: vec!["Pasta".into()],
            known: true,
        }
    }

    fn list() -> ShoppingList {
        let mut basil = item("basil", Some("groente"), None);
        basil.notes = vec!["a handful".into()];
        let mut parm = item("parmesan", None, Some(q(Dim::Mass, 50.0)));
        parm.optional = true;
        ShoppingList {
            sources: vec![Source {
                title: "Pasta \"X\"".into(),
                link: "Pasta".into(),
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
        let md = markdown(&list(), "2026-10-12");
        let expected = "\
---
type: shopping-list
created: 2026-10-12
status: open
recipes:
  - \"[[Pasta]]\"
---

# Shopping list 2026-10-12

For: [[Pasta]] (4 servings)

## Groente

- [ ] onion — 2
- [ ] basil — a handful

## Other

- [ ] parmesan — 50 g (optional)

> [!note]- From the pantry (not listed)
> salt
";
        assert_eq!(md, expected);
    }

    #[test]
    fn plain_output() {
        assert_eq!(
            plain(&list()),
            "Groente\n• onion — 2\n• basil — a handful\n\nOther\n• parmesan — 50 g (optional)"
        );
    }
}
