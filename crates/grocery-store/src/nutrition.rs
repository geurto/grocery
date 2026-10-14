//! Nutrition reports, shared by the CLI and the web service.

use std::collections::BTreeMap;

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
    /// conditions of use require.
    pub fn nutrition_report(
        &self,
        recipe: &Recipe,
        foods: &FoodDb,
        opts: Options,
        all: bool,
    ) -> Result<String> {
        let info = self.require_nevo()?;
        let table = self.compositions(&nutrition::needed_codes(recipe, foods))?;
        let n = nutrition::compute(recipe, foods, &table, opts);
        let rows = self.display_rows(all)?;
        Ok(nutrition::render_text(&n, &table, &rows, &info.citation()))
    }
}

pub(crate) fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_owned()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}
