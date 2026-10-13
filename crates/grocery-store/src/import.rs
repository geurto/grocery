//! Reading the NEVO-online CSV export.
//!
//! The main file (`NEVO2025_v9.0.csv`) is pipe-separated, quoted, UTF-8,
//! one row per food: 11 descriptive columns, then one column per nutrient
//! headed `CODE (unit)`. Values use a decimal comma; an empty cell means
//! "not known" and is kept as absent, never 0. Trace amounts are given as 0
//! by NEVO itself.
//!
//! The optional nutrients file (`..._Nutrienten_Nutrients.csv`) supplies
//! Dutch/English nutrient names. Without it the codes are used as names.
//!
//! The NEVO conditions of use don't allow changing the data, so values are
//! stored exactly as published.

use std::collections::HashSet;
use std::io::Read;

use grocery_core::nutrition::Basis;

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct NevoFood {
    pub code: u32,
    pub name_nl: String,
    pub name_en: String,
    pub synonyms: String,
    pub group_nl: String,
    pub group_en: String,
    pub basis: Basis,
    pub note: String,
    /// Nutrient codes present only in trace amounts (stored as 0).
    pub traces: String,
    /// Nutrient codes the product is fortified with.
    pub fortified: String,
    /// (nutrient code, value per 100 g/ml); unknown values are omitted.
    pub values: Vec<(String, f64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NevoNutrient {
    pub code: String,
    pub name_nl: String,
    pub name_en: String,
    pub unit: String,
    pub group_en: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NevoData {
    /// As in the file: "NEVO-Online 2025 9.0".
    pub version_raw: String,
    /// As the conditions of use cite it: "2025/9.0".
    pub version: String,
    pub foods: Vec<NevoFood>,
    /// In display order (nutrients file order, then any extra columns).
    pub nutrients: Vec<NevoNutrient>,
}

const META_COLUMNS: [(&str, &str); 11] = [
    ("version", "NEVO-versie"),
    ("group_nl", "Voedingsmiddelgroep"),
    ("group_en", "Food group"),
    ("code", "NEVO-code"),
    ("name_nl", "Voedingsmiddelnaam"),
    ("name_en", "Engelse naam"),
    ("synonyms", "Synoniem"),
    ("basis", "Hoeveelheid"),
    ("note", "Opmerking"),
    ("traces", "Bevat sporen"),
    ("fortified", "Is verrijkt"),
];

fn reader(input: impl Read) -> csv::Reader<impl Read> {
    csv::ReaderBuilder::new()
        .delimiter(b'|')
        .has_headers(true)
        .flexible(false)
        .from_reader(input)
}

/// "CODE (unit)" → ("CODE", "unit").
fn nutrient_header(h: &str) -> Option<(String, String)> {
    let (code, rest) = h.trim().split_once(" (")?;
    let unit = rest.strip_suffix(')')?;
    let valid = !code.is_empty()
        && code
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, '_' | ':'));
    valid.then(|| (code.to_owned(), unit.to_owned()))
}

/// "NEVO-Online 2025 9.0" → "2025/9.0"; falls back to the raw text.
pub fn format_version(raw: &str) -> String {
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    tokens
        .windows(2)
        .find(|w| {
            w[0].len() == 4
                && w[0].chars().all(|c| c.is_ascii_digit())
                && w[1].contains('.')
                && w[1].chars().all(|c| c.is_ascii_digit() || c == '.')
        })
        .map(|w| format!("{}/{}", w[0], w[1]))
        .unwrap_or_else(|| raw.trim().to_owned())
}

fn parse_value(cell: &str) -> Option<std::result::Result<f64, ()>> {
    let t = cell.trim();
    if t.is_empty() {
        return None;
    }
    Some(
        t.replace(',', ".")
            .parse::<f64>()
            .map_err(|_| ())
            .and_then(|v| if v.is_finite() { Ok(v) } else { Err(()) }),
    )
}

pub fn read_nevo(main: impl Read, nutrients: Option<impl Read>) -> Result<NevoData> {
    let mut rdr = reader(main);
    let headers = rdr
        .headers()
        .map_err(|e| Error::csv("main file", e))?
        .clone();

    let find = |prefix: &str| {
        headers
            .iter()
            .position(|h| h.trim().starts_with(prefix))
            .ok_or_else(|| Error::Format(format!("main file: no column starting with '{prefix}'")))
    };
    let mut meta = std::collections::HashMap::new();
    for (key, prefix) in META_COLUMNS {
        meta.insert(key, find(prefix)?);
    }

    let nutrient_cols: Vec<(usize, String, String)> = headers
        .iter()
        .enumerate()
        .filter_map(|(i, h)| nutrient_header(h).map(|(c, u)| (i, c, u)))
        .collect();
    if !nutrient_cols.iter().any(|(_, c, _)| c == "ENERCC") {
        return Err(Error::Format(
            "main file: no 'ENERCC (kcal)' column; is this the NEVO food table?".into(),
        ));
    }

    let mut foods = Vec::new();
    let mut seen = HashSet::new();
    let mut version_raw: Option<String> = None;
    for (i, record) in rdr.records().enumerate() {
        let line = i + 2; // 1-based, after the header
        let record = record.map_err(|e| Error::csv("main file", e))?;
        let get = |key: &str| record.get(meta[key]).unwrap_or("").trim().to_owned();

        let v = get("version");
        match &version_raw {
            None => version_raw = Some(v),
            Some(first) if *first != v => {
                return Err(Error::Format(format!(
                    "main file line {line}: mixed versions ('{first}' and '{v}')"
                )))
            }
            Some(_) => {}
        }

        let code: u32 = get("code").parse().map_err(|_| {
            Error::Format(format!(
                "main file line {line}: bad NEVO-code '{}'",
                get("code")
            ))
        })?;
        if !seen.insert(code) {
            return Err(Error::Format(format!(
                "main file line {line}: duplicate NEVO-code {code}"
            )));
        }
        let basis = match get("basis").replace(' ', "").to_lowercase().as_str() {
            "per100g" => Basis::Per100g,
            "per100ml" => Basis::Per100ml,
            other => {
                return Err(Error::Format(format!(
                "main file line {line}: unknown quantity '{other}' (expected per 100g / per 100ml)"
            )))
            }
        };

        let mut values = Vec::new();
        for (col, ncode, _) in &nutrient_cols {
            match parse_value(record.get(*col).unwrap_or("")) {
                None => {}
                Some(Ok(v)) => values.push((ncode.clone(), v)),
                Some(Err(())) => {
                    return Err(Error::Format(format!(
                        "main file line {line}: {ncode} value '{}' is not a number",
                        record.get(*col).unwrap_or("")
                    )))
                }
            }
        }

        foods.push(NevoFood {
            code,
            name_nl: get("name_nl"),
            name_en: get("name_en"),
            synonyms: get("synonyms"),
            group_nl: get("group_nl"),
            group_en: get("group_en"),
            basis,
            note: get("note"),
            traces: get("traces"),
            fortified: get("fortified"),
            values,
        });
    }
    let version_raw =
        version_raw.ok_or_else(|| Error::Format("main file has no food rows".into()))?;

    // Nutrient metadata: names from the nutrients file if given, in its order;
    // then any column codes it didn't list.
    let mut list: Vec<NevoNutrient> = Vec::new();
    let units: std::collections::HashMap<&str, &str> = nutrient_cols
        .iter()
        .map(|(_, c, u)| (c.as_str(), u.as_str()))
        .collect();
    if let Some(input) = nutrients {
        let mut rdr = reader(input);
        for record in rdr.records() {
            let r = record.map_err(|e| Error::csv("nutrients file", e))?;
            let field = |i: usize| r.get(i).unwrap_or("").trim().to_owned();
            let code = field(2);
            // The file lists some nutrients twice (under two groups).
            if code.is_empty()
                || list.iter().any(|n| n.code == code)
                || !units.contains_key(code.as_str())
            {
                continue;
            }
            list.push(NevoNutrient {
                unit: units[code.as_str()].to_owned(),
                code,
                name_nl: field(3),
                name_en: field(4),
                group_en: field(1),
            });
        }
    }
    for (_, code, unit) in &nutrient_cols {
        if !list.iter().any(|n| &n.code == code) {
            list.push(NevoNutrient {
                code: code.clone(),
                name_nl: code.clone(),
                name_en: code.clone(),
                unit: unit.clone(),
                group_en: String::new(),
            });
        }
    }

    Ok(NevoData {
        version: format_version(&version_raw),
        version_raw,
        foods,
        nutrients: list,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_formatting() {
        assert_eq!(format_version("NEVO-Online 2025 9.0"), "2025/9.0");
        assert_eq!(format_version("NEVO-Online 2027 10.1"), "2027/10.1");
        assert_eq!(format_version("weird"), "weird");
    }

    #[test]
    fn nutrient_headers() {
        assert_eq!(
            nutrient_header("ENERCC (kcal)"),
            Some(("ENERCC".into(), "kcal".into()))
        );
        assert_eq!(
            nutrient_header("F18:2CN6 (g)"),
            Some(("F18:2CN6".into(), "g".into()))
        );
        assert_eq!(nutrient_header("SE (µg)"), Some(("SE".into(), "µg".into())));
        assert_eq!(nutrient_header("Hoeveelheid/Quantity"), None);
        assert_eq!(nutrient_header("Engelse naam/Food name"), None);
    }

    #[test]
    fn values() {
        assert_eq!(parse_value(""), None);
        assert_eq!(parse_value("0"), Some(Ok(0.0)));
        assert_eq!(parse_value("63,3"), Some(Ok(63.3)));
        assert_eq!(parse_value("x"), Some(Err(())));
    }
}
