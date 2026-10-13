//! `.cook` files, via the official `cooklang` crate.
//!
//! The crate does the parsing; this adapter only maps its model onto ours.
//! Units are left as written (empty converter) because [`crate::units`]
//! handles normalization for both formats, including Dutch units.

use std::path::Path;

use cooklang::{quantity::Value, Converter, CooklangParser, Extensions, Modifiers};

use super::{title_from_path, RecipeSource};
use crate::error::{Error, Result};
use crate::model::{Amount, Ingredient, Recipe};

#[derive(Debug)]
pub struct CooklangSource {
    parser: CooklangParser,
}

impl Default for CooklangSource {
    fn default() -> Self {
        // ADVANCED_UNITS validates timer units against the converter, and with
        // an empty converter every `~{5%minutes}` becomes an error. We don't
        // need the crate's unit handling, so leave that extension off.
        // (Its other feature, `@flour{100 g}` without `%`, is covered by
        // `text_quantity` below.)
        let extensions = Extensions::COMPAT - Extensions::ADVANCED_UNITS;
        Self {
            parser: CooklangParser::new(extensions, Converter::empty()),
        }
    }
}

/// cooklang 0.19 warns about *every* scaling lock (`@salt{=1%tsp}`), even on
/// ingredients where it works. Filter that one so real warnings stand out.
fn is_spurious_warning(message: &str) -> bool {
    message.starts_with("Unnecessary scaling lock modifier")
}

/// `@flour{100 g}` (no `%`) arrives as text; recover "<number> [unit]".
fn text_quantity(text: &str, scalable: bool) -> Amount {
    let mut parts = text.split_whitespace();
    let qty = parts.next().and_then(crate::units::parse_number);
    let unit: Vec<&str> = parts.collect();
    match qty {
        Some(qty) if unit.len() <= 1 => Amount::Value {
            qty,
            unit: unit.first().map(|u| (*u).to_owned()),
            scalable,
        },
        _ => Amount::Text(text.to_owned()),
    }
}

impl RecipeSource for CooklangSource {
    fn parse(&self, path: &Path, text: &str) -> Result<Recipe> {
        let (parsed, report) =
            self.parser
                .parse(text)
                .into_result()
                .map_err(|report| Error::Parse {
                    path: path.into(),
                    message: render_report(&report, path, text),
                })?;

        let mut warnings: Vec<String> = report
            .warnings()
            .map(|w| w.to_string())
            .filter(|w| !is_spurious_warning(w))
            .map(|w| format!("cooklang: {w}"))
            .collect();

        let title = parsed
            .metadata
            .title()
            .map(str::to_owned)
            .unwrap_or_else(|| title_from_path(path));
        let servings = parsed
            .metadata
            .servings()
            .and_then(|s| s.as_number())
            .map(f64::from);

        let mut ingredients = Vec::new();
        // Walk definitions only; `@&onion` references contribute their
        // quantities through `all_quantities`. We deliberately don't use the
        // crate's `group_ingredients`: its summing drops the "fixed amount"
        // (`=`) flag, and the aggregator sums anyway.
        for ing in parsed
            .ingredients
            .iter()
            .filter(|i| i.relation.is_definition())
        {
            let mods = ing.modifiers();
            if mods.contains(Modifiers::RECIPE) {
                warnings.push(format!(
                    "'{}' references another recipe; add that recipe to the list separately",
                    ing.name
                ));
                continue;
            }
            if mods.contains(Modifiers::HIDDEN) {
                continue; // `@-water`: shown in the text, not something to buy
            }

            let optional = mods.contains(Modifiers::OPT);
            let base = |amount| Ingredient {
                name: ing.name.clone(),
                amount,
                optional,
                note: ing.note.clone(),
            };

            let quantities: Vec<_> = ing.all_quantities(&parsed.ingredients).collect();
            if quantities.is_empty() {
                ingredients.push(base(Amount::Unspecified));
                continue;
            }
            for q in quantities {
                let amount = match q.value() {
                    Value::Number(n) => Amount::Value {
                        qty: n.value(),
                        unit: q.unit().map(str::to_owned),
                        scalable: q.scalable(),
                    },
                    // Buy for the upper end of a range.
                    Value::Range { end, .. } => Amount::Value {
                        qty: end.value(),
                        unit: q.unit().map(str::to_owned),
                        scalable: q.scalable(),
                    },
                    // Text is never scalable upstream; a recovered number
                    // should scale like any other ingredient amount.
                    Value::Text(t) => text_quantity(t, true),
                };
                ingredients.push(base(amount));
            }
        }

        Ok(Recipe {
            path: path.into(),
            title,
            servings,
            ingredients,
            warnings,
        })
    }
}

fn render_report(report: &cooklang::error::SourceReport, path: &Path, text: &str) -> String {
    let mut buf = Vec::new();
    let name = path.display().to_string();
    match report.write(&name, text, false, &mut buf) {
        Ok(()) if !buf.is_empty() => String::from_utf8_lossy(&buf).trim().to_owned(),
        _ => report.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Recipe {
        CooklangSource::default()
            .parse(Path::new("Recipes/Test.cook"), text)
            .unwrap()
    }

    fn value(qty: f64, unit: Option<&str>) -> Amount {
        Amount::value(qty, unit)
    }

    #[test]
    fn metadata_and_basic_ingredients() {
        let r = parse(
            "---\ntitle: Pasta\nservings: 4\n---\n\
             Boil @spaghetti{400%g} with @salt.\n\
             Fry @onion{1} and @garlic{2%cloves} in @olive oil{2%tbsp}.\n",
        );
        assert_eq!(r.title, "Pasta");
        assert_eq!(r.servings, Some(4.0));
        let got: Vec<_> = r
            .ingredients
            .iter()
            .map(|i| (i.name.as_str(), &i.amount))
            .collect();
        assert_eq!(
            got,
            [
                ("spaghetti", &value(400.0, Some("g"))),
                ("salt", &Amount::Unspecified),
                ("onion", &value(1.0, None)),
                ("garlic", &value(2.0, Some("cloves"))),
                ("olive oil", &value(2.0, Some("tbsp"))),
            ]
        );
    }

    #[test]
    fn title_falls_back_to_file_name() {
        assert_eq!(parse("Add @water{1%l}.").title, "Test");
    }

    #[test]
    fn references_contribute_to_the_definition() {
        let r = parse("Use @flour{300%g}. Later add @&flour{100%g}. Finish with @&flour.");
        let amounts: Vec<_> = r
            .ingredients
            .iter()
            .map(|i| (i.name.as_str(), &i.amount))
            .collect();
        // one food, two quantities: the aggregator sums them
        assert_eq!(
            amounts,
            [
                ("flour", &value(300.0, Some("g"))),
                ("flour", &value(100.0, Some("g")))
            ]
        );
    }

    #[test]
    fn modifiers_ranges_text_and_fixed_amounts() {
        let r = parse(
            "Top with @?parmesan{50%g}. Add @-water{1%l}. \
             Use @basil{a handful} and @chili{1-2}. Season with @salt{=1%tsp}.",
        );
        let by_name = |n: &str| r.ingredients.iter().find(|i| i.name == n).unwrap();
        assert!(by_name("parmesan").optional);
        assert!(
            r.ingredients.iter().all(|i| i.name != "water"),
            "hidden is skipped"
        );
        assert_eq!(by_name("basil").amount, Amount::Text("a handful".into()));
        assert_eq!(by_name("chili").amount, value(2.0, None));
        assert_eq!(
            by_name("salt").amount,
            Amount::Value {
                qty: 1.0,
                unit: Some("tsp".into()),
                scalable: false
            }
        );
    }

    #[test]
    fn timers_and_unitless_shorthand() {
        let r = parse("Simmer @lentils{200 g} for ~{15%minutes}. Rest ~{1%hour}.");
        assert_eq!(r.ingredients[0].amount, value(200.0, Some("g")));
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    #[test]
    fn scaling_lock_does_not_warn() {
        let r = parse("Season with @salt{=1%tsp}.");
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    #[test]
    fn syntax_errors_carry_the_path() {
        let err = CooklangSource::default()
            // a reference to an ingredient that was never defined
            .parse(Path::new("Recipes/Broken.cook"), "Add @&flour{100%g}.")
            .unwrap_err();
        assert!(err.to_string().contains("Broken.cook"), "{err}");
    }
}
