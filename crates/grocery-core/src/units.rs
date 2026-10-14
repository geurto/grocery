//! Unit normalization.
//!
//! Every quantity ends up in one of four dimensions. Mass and volume have a
//! fixed base unit (g, ml); count is "pieces"; anything else ("clove", "can",
//! "bunch") is a named unit whose weight is food-specific and lives in
//! `aliases.yaml`. English and Dutch spellings are both recognized.

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Dim {
    /// Grams.
    Mass,
    /// Millilitres.
    Volume,
    /// Pieces (no unit, "pc", "stuk", ...).
    Count,
    /// A food-specific unit, normalized (lowercase, singular-ish).
    Named(String),
}

/// A quantity expressed in its dimension's base unit.
#[derive(Debug, Clone, PartialEq)]
pub struct Normalized {
    pub dim: Dim,
    pub value: f64,
}

fn table(unit: &str) -> Option<(Dim, f64)> {
    use Dim::*;
    let entry = match unit {
        // mass → g
        "g" | "gr" | "gram" | "grams" | "gramm" => (Mass, 1.0),
        "kg" | "kilo" | "kilogram" | "kilograms" => (Mass, 1000.0),
        "mg" => (Mass, 0.001),
        "ons" => (Mass, 100.0), // Dutch ons
        "oz" | "ounce" | "ounces" => (Mass, 28.3495),
        "lb" | "lbs" | "pound" | "pounds" => (Mass, 453.592),
        // volume → ml
        "ml" | "milliliter" | "millilitre" | "milliliters" | "millilitres" => (Volume, 1.0),
        "cl" => (Volume, 10.0),
        "dl" => (Volume, 100.0),
        "l" | "liter" | "litre" | "liters" | "litres" => (Volume, 1000.0),
        "tsp" | "teaspoon" | "teaspoons" | "tl" | "theelepel" | "theelepels" => (Volume, 5.0),
        "tbsp" | "tablespoon" | "tablespoons" | "el" | "eetlepel" | "eetlepels" => (Volume, 15.0),
        "cup" | "cups" | "kop" | "kopje" | "kopjes" => (Volume, 240.0),
        "fl oz" => (Volume, 29.5735),
        // count → pieces
        "pc" | "pcs" | "piece" | "pieces" | "st" | "stuk" | "stuks" | "x" => (Count, 1.0),
        _ => return None,
    };
    Some(entry)
}

/// Lowercase, trim, drop a trailing period ("el." → "el").
pub fn clean_unit(unit: &str) -> String {
    unit.trim().trim_end_matches('.').to_lowercase()
}

/// Normalize a named unit so "cloves"/"clove" and "Cans"/"can" match.
/// Deliberately naive: strips one trailing "s" (not "ss", and not from
/// short words like "bos"). Dutch plurals
/// ("tenen") don't follow this; list both spellings under `units:` instead.
pub fn named_key(unit: &str) -> String {
    let u = clean_unit(unit);
    if u.len() > 3 && u.ends_with('s') && !u.ends_with("ss") {
        u[..u.len() - 1].to_owned()
    } else {
        u
    }
}

pub fn normalize(qty: f64, unit: Option<&str>) -> Normalized {
    let Some(unit) = unit.map(clean_unit).filter(|u| !u.is_empty()) else {
        return Normalized {
            dim: Dim::Count,
            value: qty,
        };
    };
    match table(&unit) {
        Some((dim, factor)) => Normalized {
            dim,
            value: qty * factor,
        },
        None => Normalized {
            dim: Dim::Named(named_key(&unit)),
            value: qty,
        },
    }
}

/// Parse a single numeric token: `2`, `2.5`, `2,5`, `1/2`, `½`, or a range
/// `2-3` (returns the upper bound: when shopping you buy for the maximum).
pub fn parse_number(token: &str) -> Option<f64> {
    let t = token.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(v) = vulgar_fraction(t) {
        return Some(v);
    }
    if let Some((a, b)) = t.split_once(['-', '–']) {
        // range: both sides must be numbers
        let (a, b) = (parse_number(a)?, parse_number(b)?);
        return Some(a.max(b));
    }
    if let Some((n, d)) = t.split_once('/') {
        let (n, d): (f64, f64) = (n.trim().parse().ok()?, d.trim().parse().ok()?);
        return (d != 0.0).then_some(n / d);
    }
    t.replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

fn vulgar_fraction(t: &str) -> Option<f64> {
    Some(match t {
        "½" => 0.5,
        "⅓" => 1.0 / 3.0,
        "⅔" => 2.0 / 3.0,
        "¼" => 0.25,
        "¾" => 0.75,
        "⅛" => 0.125,
        _ => return None,
    })
}

/// Seconds per time unit, English and Dutch: `min`, `minuten`, `uur`, ...
pub fn time_unit_seconds(unit: &str) -> Option<f64> {
    Some(match clean_unit(unit).as_str() {
        "s" | "sec" | "secs" | "second" | "seconds" | "seconde" | "seconden" => 1.0,
        "m" | "min" | "mins" | "minute" | "minutes" | "minuut" | "minuten" => 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" | "u" | "uur" | "uren" => 3600.0,
        _ => return None,
    })
}

/// A duration as written in metadata, in minutes: `40 min`, `1 uur 15 min`,
/// `1h30m`, `1,5 uur`, or a bare `45` (minutes).
pub fn parse_minutes(text: &str) -> Option<f64> {
    let t = text.trim().to_lowercase();
    if let Some(v) = parse_number(&t) {
        return Some(v);
    }
    // Split "1h30m" / "1 uur 30 min" into number and unit tokens.
    let mut tokens: Vec<String> = Vec::new();
    for ch in t.chars() {
        let digit = ch.is_ascii_digit() || ch == '.' || ch == ',';
        match tokens.last_mut() {
            Some(last)
                if !ch.is_whitespace()
                    && last.chars().next().is_some_and(|c| c.is_ascii_digit()) == digit =>
            {
                last.push(ch)
            }
            _ if ch.is_whitespace() => tokens.push(String::new()),
            _ => tokens.push(ch.to_string()),
        }
    }
    tokens.retain(|t| !t.is_empty() && t != "en" && t != "and");
    if tokens.len() % 2 != 0 {
        return None;
    }
    let mut seconds = 0.0;
    for pair in tokens.chunks(2) {
        seconds += parse_number(&pair[0])? * time_unit_seconds(&pair[1])?;
    }
    Some(seconds / 60.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mass_and_volume_convert_to_base_units() {
        assert_eq!(
            normalize(1.5, Some("kg")),
            Normalized {
                dim: Dim::Mass,
                value: 1500.0
            }
        );
        assert_eq!(
            normalize(2.0, Some("EL")),
            Normalized {
                dim: Dim::Volume,
                value: 30.0
            }
        );
        assert_eq!(
            normalize(2.0, Some("tsp.")),
            Normalized {
                dim: Dim::Volume,
                value: 10.0
            }
        );
        assert_eq!(normalize(1.0, Some("ons")).value, 100.0);
    }

    #[test]
    fn no_unit_is_count_and_unknown_is_named() {
        assert_eq!(normalize(2.0, None).dim, Dim::Count);
        assert_eq!(normalize(2.0, Some("")).dim, Dim::Count);
        assert_eq!(
            normalize(3.0, Some("Cloves")).dim,
            Dim::Named("clove".into())
        );
        assert_eq!(normalize(1.0, Some("teen")).dim, Dim::Named("teen".into()));
        assert_eq!(
            normalize(1.0, Some("glass")).dim,
            Dim::Named("glass".into())
        );
    }

    #[test]
    fn durations() {
        assert_eq!(parse_minutes("40 min"), Some(40.0));
        assert_eq!(parse_minutes("45"), Some(45.0));
        assert_eq!(parse_minutes("1h30m"), Some(90.0));
        assert_eq!(parse_minutes("1 uur en 15 minuten"), Some(75.0));
        assert_eq!(parse_minutes("1,5 uur"), Some(90.0));
        assert_eq!(parse_minutes("90 seconden"), Some(1.5));
        assert_eq!(parse_minutes("a while"), None);
        assert_eq!(parse_minutes("30 parsecs"), None);
        assert_eq!(time_unit_seconds("Minuten"), Some(60.0));
    }

    #[test]
    fn numbers() {
        assert_eq!(parse_number("2"), Some(2.0));
        assert_eq!(parse_number("2,5"), Some(2.5));
        assert_eq!(parse_number("1/2"), Some(0.5));
        assert_eq!(parse_number("½"), Some(0.5));
        assert_eq!(parse_number("2-3"), Some(3.0));
        assert_eq!(parse_number("1/0"), None);
        assert_eq!(parse_number("handful"), None);
        assert_eq!(parse_number("inf"), None);
    }
}
