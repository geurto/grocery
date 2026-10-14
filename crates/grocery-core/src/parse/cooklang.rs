//! `.cook` files, via the official `cooklang` crate.
//!
//! The crate does the parsing; this adapter only maps its model onto ours.
//! Units are left as written (empty converter) because [`crate::units`]
//! handles normalization, including Dutch units.

use std::path::Path;

use cooklang::{quantity::Value, Converter, CooklangParser, Extensions, Item, Modifiers};

use super::title_from_path;
use crate::error::{Error, Result};
use crate::model::{
    Amount, Content, Duration, Ingredient, Recipe, RecipeTime, Section, Step, StepItem,
};
use crate::units::{parse_minutes, time_unit_seconds};

/// Building the inner parser is not free, so create one and reuse it.
#[derive(Debug)]
pub struct Parser {
    parser: CooklangParser,
}

impl Default for Parser {
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

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse `text`, which was read from `path`. The path is only used for
    /// error messages and as a fallback title.
    pub fn parse(&self, path: &Path, text: &str) -> Result<Recipe> {
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
        let tags = parsed
            .metadata
            .tags()
            .unwrap_or_default()
            .into_iter()
            .map(|t| t.into_owned())
            .collect();
        let description = parsed.metadata.description().map(str::to_owned);

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
                ingredients.push(base(amount_of(q)));
            }
        }

        let sections = parsed
            .sections
            .iter()
            .map(|section| Section {
                name: section.name.clone(),
                content: section
                    .content
                    .iter()
                    .map(|c| match c {
                        cooklang::Content::Step(step) => Content::Step(step_items(step, &parsed)),
                        cooklang::Content::Text(t) => Content::Note(t.trim().to_owned()),
                    })
                    .collect(),
            })
            .collect();

        let time = recipe_time(&parsed.metadata, &mut warnings);

        Ok(Recipe {
            path: path.into(),
            title,
            servings,
            tags,
            description,
            time,
            ingredients,
            sections,
            warnings,
        })
    }
}

fn amount_of(q: &cooklang::Quantity) -> Amount {
    match q.value() {
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
    }
}

/// A step with its ingredients, cookware and timers kept apart from the text.
fn step_items(step: &cooklang::Step, recipe: &cooklang::Recipe) -> Step {
    let mut items: Vec<StepItem> = Vec::new();
    for item in &step.items {
        let next = match item {
            Item::Text { value } => StepItem::Text(value.clone()),
            Item::Ingredient { index } => {
                let ing = &recipe.ingredients[*index];
                StepItem::Ingredient {
                    name: ing.display_name().into_owned(),
                    amount: ing.quantity.as_ref().map_or(Amount::Unspecified, amount_of),
                }
            }
            Item::Cookware { index } => {
                StepItem::Cookware(recipe.cookware[*index].display_name().to_owned())
            }
            Item::Timer { index } => {
                let timer = &recipe.timers[*index];
                let text = match (&timer.quantity, &timer.name) {
                    (Some(q), _) => q.to_string(),
                    (None, Some(name)) => name.clone(),
                    (None, None) => String::new(),
                };
                StepItem::Timer {
                    name: timer.name.clone(),
                    text,
                    duration: timer.quantity.as_ref().and_then(timer_duration),
                }
            }
            Item::InlineQuantity { index } => {
                StepItem::Quantity(recipe.inline_quantities[*index].to_string())
            }
        };
        // Merge adjacent text so the viewer gets whole runs.
        match (items.last_mut(), next) {
            (Some(StepItem::Text(prev)), StepItem::Text(t)) => prev.push_str(&t),
            (_, next) => items.push(next),
        }
    }
    // Trim like the old text export did.
    if let Some(StepItem::Text(t)) = items.first_mut() {
        *t = t.trim_start().to_owned();
    }
    if let Some(StepItem::Text(t)) = items.last_mut() {
        *t = t.trim_end().to_owned();
    }
    items.retain(|i| !matches!(i, StepItem::Text(t) if t.is_empty()));
    Step { items }
}

fn timer_duration(q: &cooklang::Quantity) -> Option<Duration> {
    let unit = time_unit_seconds(q.unit()?)?;
    match q.value() {
        Value::Number(n) => Some(Duration::exact(n.value() * unit)),
        Value::Range { start, end } => Some(Duration {
            min: start.value() * unit,
            max: end.value() * unit,
        }),
        Value::Text(t) => crate::units::parse_number(t).map(|v| Duration::exact(v * unit)),
    }
}

/// `time:` (also `duration`, `tijd`, `bereidingstijd`), else `prep time:` and
/// `cook time:`. A value we can't read is a warning, not a guess.
fn recipe_time(meta: &cooklang::Metadata, warnings: &mut Vec<String>) -> Option<RecipeTime> {
    let mut read = |keys: &[&str]| -> Option<f64> {
        let (key, value) = keys.iter().find_map(|k| Some((*k, meta.get(*k)?)))?;
        let minutes = value
            .as_f64()
            .or_else(|| value.as_str().and_then(parse_minutes))
            .filter(|m| *m >= 0.0);
        if minutes.is_none() {
            warnings.push(format!(
                "metadata '{key}': can't read a time from {value:?}; write e.g. '40 min'"
            ));
        }
        minutes
    };
    let time = RecipeTime {
        total: read(&[
            "time",
            "duration",
            "time required",
            "tijd",
            "bereidingstijd",
        ]),
        prep: read(&["prep time", "prep_time", "voorbereidingstijd"]),
        cook: read(&["cook time", "cook_time", "kooktijd"]),
    };
    (time != RecipeTime::default()).then_some(time)
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
        Parser::new()
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
    fn metadata_and_method() {
        let r = parse(
            "---\ntags: [pasta, quick]\ndescription: Weeknight dinner\n---\n\
             Boil @spaghetti{400%g} in a #pot for ~{10%minutes}.\n\n\
             > Use plenty of water.\n\n\
             == Sauce ==\n\
             Fry @olive oil{2%tbsp} gently.\n",
        );
        assert_eq!(r.tags, ["pasta", "quick"]);
        assert_eq!(r.description.as_deref(), Some("Weeknight dinner"));
        assert_eq!(
            r.sections,
            [
                Section {
                    name: None,
                    content: vec![
                        Content::Step(Step {
                            items: vec![
                                StepItem::Text("Boil ".into()),
                                StepItem::Ingredient {
                                    name: "spaghetti".into(),
                                    amount: value(400.0, Some("g"))
                                },
                                StepItem::Text(" in a ".into()),
                                StepItem::Cookware("pot".into()),
                                StepItem::Text(" for ".into()),
                                StepItem::Timer {
                                    name: None,
                                    text: "10 minutes".into(),
                                    duration: Some(Duration::exact(600.0)),
                                },
                                StepItem::Text(".".into()),
                            ]
                        }),
                        Content::Note("Use plenty of water.".into()),
                    ],
                },
                Section {
                    name: Some("Sauce".into()),
                    content: vec![Content::Step(Step {
                        items: vec![
                            StepItem::Text("Fry ".into()),
                            StepItem::Ingredient {
                                name: "olive oil".into(),
                                amount: value(2.0, Some("tbsp"))
                            },
                            StepItem::Text(" gently.".into()),
                        ]
                    })],
                },
            ]
        );
        let Content::Step(first) = &r.sections[0].content[0] else {
            panic!()
        };
        assert_eq!(first.text(), "Boil spaghetti in a pot for 10 minutes.");
    }

    #[test]
    fn times_from_metadata_and_timers() {
        let r = parse(
            "---\ntime: 1 uur 15 min\n---\n\
             Bak ~{30%seconds}, dan ~{2-3%minuten}.\n\n\
             Laat ~{1%uur} rusten.\n\n\
             Snijd de ui.\n",
        );
        assert_eq!(r.time.unwrap().minutes(), Some(75.0));
        assert_eq!(
            r.timers_total(),
            Some(Duration {
                min: 30.0 + 120.0 + 3600.0,
                max: 30.0 + 180.0 + 3600.0
            })
        );
        let steps: Vec<_> = r.sections[0]
            .content
            .iter()
            .map(|c| match c {
                Content::Step(s) => s.duration(),
                Content::Note(_) => None,
            })
            .collect();
        assert_eq!(steps[2], None);

        let r = parse("---\nprep time: 10\ncook time: 20 min\n---\nCook.");
        assert_eq!(r.time.unwrap().minutes(), Some(30.0));

        let r = parse("---\ntime: a while\n---\nCook.");
        assert_eq!(r.time, None);
        assert!(r.warnings[0].contains("'time'"), "{:?}", r.warnings);
    }

    #[test]
    fn syntax_errors_carry_the_path() {
        let err = Parser::new()
            // a reference to an ingredient that was never defined
            .parse(Path::new("Recipes/Broken.cook"), "Add @&flour{100%g}.")
            .unwrap_err();
        assert!(err.to_string().contains("Broken.cook"), "{err}");
    }
}
