//! Nutrition reports, shared by the CLI and the web service.

use std::collections::{BTreeMap, BTreeSet};

use grocery_core::custom::CustomFoods;
use grocery_core::food::FoodDb;
use grocery_core::model::Recipe;
use grocery_core::nutrition::{self, DisplayRow, Options};

use crate::{Result, Store};

impl Store {
    /// The label-style selection, or with `all` every NEVO nutrient in file
    /// order (salt after sodium), with EU reference intakes where known.
    pub fn display_rows(&self, all: bool) -> Result<Vec<DisplayRow>> {
        if !all {
            return Ok(nutrition::default_rows());
        }
        let ri: BTreeMap<String, Option<f64>> = nutrition::default_rows()
            .into_iter()
            .map(|r| (r.code, r.ri))
            .collect();
        let mut rows = Vec::new();
        for n in self.nutrients()? {
            let is_na = n.code == "NA";
            rows.push(DisplayRow {
                ri: ri.get(&n.code).copied().flatten(),
                label: truncate(n.name_en.trim(), 21),
                unit: n.unit,
                code: n.code,
                indent: false,
                section: None,
            });
            if is_na {
                rows.push(DisplayRow {
                    code: "SALT".into(),
                    label: "Salt (from sodium)".into(),
                    unit: "g".into(),
                    ri: Some(6.0),
                    indent: false,
                    section: None,
                });
            }
        }
        Ok(rows)
    }

    /// Per-serving nutrition as text, ending with the NEVO citation the
    /// conditions of use require. `custom` supplies foods NEVO doesn't have;
    /// the report says which numbers are yours.
    pub fn nutrition_report(
        &self,
        recipe: &Recipe,
        foods: &FoodDb,
        custom: &CustomFoods,
        opts: Options,
        all: bool,
    ) -> Result<String> {
        let info = self.require_nevo()?;
        let mut table = self.compositions(&nutrition::needed_codes(recipe, foods))?;
        table.extend(custom.compositions());
        let n = nutrition::compute(recipe, foods, &table, opts);
        let rows = self.display_rows(all)?;
        let mut text = nutrition::render_text(&n, &table, &rows, &info.citation());

        // A typo'd nutrient code in custom.yaml would otherwise just never show.
        let known: BTreeSet<String> = self.nutrients()?.into_iter().map(|n| n.code).collect();
        let mut unknown: Vec<String> = Vec::new();
        for line in &n.lines {
            let nutrition::Status::Counted {
                key: key @ nutrition::FoodKey::Custom(_),
                food,
                ..
            } = &line.status
            else {
                continue;
            };
            for code in table.get(key).into_iter().flat_map(|c| c.values.keys()) {
                if !known.contains(code) && !unknown.contains(&format!("{food}: {code}")) {
                    unknown.push(format!("{food}: {code}"));
                }
            }
        }
        if !unknown.is_empty() {
            // render_text ends with "\n<citation>\n"; keep the citation last.
            let tail = format!("\n{}\n", info.citation());
            if let Some(at) = text
                .len()
                .checked_sub(tail.len())
                .filter(|_| text.ends_with(&tail))
            {
                text.insert_str(
                    at,
                    &format!(
                        "\nNutrient codes in custom.yaml that NEVO doesn't have (typo?): {}\n",
                        unknown.join(", ")
                    ),
                );
            }
        }
        Ok(text)
    }
}

pub(crate) fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_owned()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}
