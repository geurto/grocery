//! Writing to the library: new and edited recipes, new foods.
//!
//! Everything here works on text, so the files stay the plain, hand-editable
//! files they are. Recipes are split into a few form fields plus the method
//! (`RecipeSource`) and put back together; new foods are appended to
//! `aliases.yaml` without touching the rest of the file (comments and order
//! survive). What's produced is parsed again before the caller writes it.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::food::{normalize_name, FoodDb};
use crate::units::{self, parse_number};

// ---------------------------------------------------------------- recipes

/// A recipe as an editor shows it: the usual metadata as fields, everything
/// else in the front matter kept as YAML text, and the method.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeSource {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub servings: Option<f64>,
    /// As written: "40 min", "1 uur 15 min", "45".
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Other front matter (author, source, ...) as YAML lines.
    #[serde(default)]
    pub extra: String,
    /// The method, in Cooklang.
    #[serde(default)]
    pub body: String,
}

const TITLE_KEYS: &[&str] = &["title"];
const SERVINGS_KEYS: &[&str] = &["servings", "serves", "yield"];
const TIME_KEYS: &[&str] = &["time", "duration", "time required"];
const TAG_KEYS: &[&str] = &["tags", "tag"];

fn split_front_matter(text: &str) -> (Option<&str>, &str) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return (None, text);
    };
    if let Some(end) = rest.find("\n---\n") {
        return (Some(&rest[..end]), &rest[end + 5..]);
    }
    match rest.strip_suffix("\n---") {
        Some(front) => (Some(front), ""),
        None => (None, text),
    }
}

/// A YAML scalar for `s`, quoted only when it has to be.
fn scalar(s: &str) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    serde_norway::to_string(&one_line)
        .map(|y| y.trim_end().to_owned())
        .unwrap_or_else(|_| format!("{one_line:?}"))
}

fn fmt_number(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Look up the first of `keys` whose value `accept`s, and say which key
/// it was so its lines can be dropped from the leftover text.
fn take(
    map: &serde_norway::Mapping,
    keys: &[&str],
    consumed: &mut Vec<String>,
    accept: impl Fn(&serde_norway::Value) -> bool,
) -> Option<serde_norway::Value> {
    for key in keys {
        let k = serde_norway::Value::String((*key).to_owned());
        if let Some(v) = map.get(&k).filter(|v| accept(v)) {
            consumed.push((*key).to_owned());
            return Some(v.clone());
        }
    }
    None
}

/// The front matter's top-level entries as text: (key, lines). Comments
/// above a key belong to it; indented lines and `- item` lines belong to the
/// key before them.
fn entries(front: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut pending = String::new();
    for line in front.lines() {
        let top = !line.starts_with([' ', '\t', '-', '#']) && !line.trim().is_empty();
        if top && line.contains(':') {
            let key = line[..line.find(':').unwrap_or(0)]
                .trim()
                .trim_matches(['"', '\''])
                .to_owned();
            out.push((key, format!("{pending}{line}\n")));
            pending.clear();
        } else if line.starts_with('#') {
            pending.push_str(line);
            pending.push('\n');
        } else if let Some((_, text)) = out.last_mut() {
            text.push_str(line);
            text.push('\n');
        } else {
            pending.push_str(line);
            pending.push('\n');
        }
    }
    if !pending.trim().is_empty() {
        // Comments at the end: keep them with whatever is left.
        out.push((String::new(), pending));
    }
    out
}

impl RecipeSource {
    /// Split recipe text into fields. An error means the front matter isn't
    /// valid YAML (the recipe itself can still be shown as text).
    pub fn from_text(text: &str) -> std::result::Result<Self, String> {
        let text = text.replace("\r\n", "\n");
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let (front, body) = split_front_matter(text);
        let mut out = RecipeSource {
            body: body.trim().to_owned(),
            ..Default::default()
        };
        let Some(front) = front else {
            return Ok(out);
        };
        let map: serde_norway::Mapping = if front.trim().is_empty() {
            Default::default()
        } else {
            serde_norway::from_str(front)
                .map_err(|e| format!("the front matter (between the --- lines) isn't valid: {e}"))?
        };
        let mut consumed: Vec<String> = Vec::new();

        if let Some(v) = take(&map, TITLE_KEYS, &mut consumed, |v| v.is_string()) {
            out.title = v.as_str().unwrap_or_default().trim().to_owned();
        }
        if let Some(v) = take(&map, SERVINGS_KEYS, &mut consumed, |v| {
            v.as_f64().is_some() || v.as_str().and_then(parse_number).is_some()
        }) {
            out.servings = v
                .as_f64()
                .or_else(|| v.as_str().and_then(parse_number))
                .filter(|s| *s > 0.0);
        }
        if let Some(v) = take(&map, TIME_KEYS, &mut consumed, |v| {
            v.is_string() || v.is_number()
        }) {
            out.time = match v {
                serde_norway::Value::String(s) => Some(s.trim().to_owned()),
                serde_norway::Value::Number(n) => Some(n.to_string()),
                _ => None,
            }
            .filter(|t| !t.is_empty());
        }
        if let Some(v) = take(&map, TAG_KEYS, &mut consumed, |v| {
            v.is_string() || v.is_sequence()
        }) {
            let items: Vec<String> = match v {
                serde_norway::Value::String(s) => s.split(',').map(str::to_owned).collect(),
                serde_norway::Value::Sequence(seq) => seq
                    .into_iter()
                    .filter_map(|t| match t {
                        serde_norway::Value::String(s) => Some(s),
                        serde_norway::Value::Number(n) => Some(n.to_string()),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            out.tags = items
                .into_iter()
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty())
                .collect();
        }
        out.extra = entries(front)
            .into_iter()
            .filter(|(key, _)| !consumed.contains(key))
            .map(|(_, text)| text)
            .collect::<String>()
            .trim_end()
            .to_owned();
        Ok(out)
    }

    /// The recipe as a `.cook` file.
    pub fn to_text(&self) -> String {
        let mut out = String::from("---\n");
        let title = self.title.trim();
        if !title.is_empty() {
            out.push_str(&format!("title: {}\n", scalar(title)));
        }
        if let Some(s) = self.servings.filter(|s| *s > 0.0) {
            out.push_str(&format!("servings: {}\n", fmt_number(s)));
        }
        if let Some(t) = self
            .time
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            // A bare number is minutes; keep it a number.
            if t.parse::<f64>().is_ok() {
                out.push_str(&format!("time: {t}\n"));
            } else {
                out.push_str(&format!("time: {}\n", scalar(t)));
            }
        }
        let tags: Vec<String> = self
            .tags
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .map(scalar)
            .collect();
        if !tags.is_empty() {
            out.push_str(&format!("tags: [{}]\n", tags.join(", ")));
        }
        let extra = self.extra.trim();
        if !extra.is_empty() {
            out.push_str(extra);
            out.push('\n');
        }
        if out == "---\n" {
            out.clear(); // no metadata: no front matter
        } else {
            out.push_str("---\n\n");
        }
        out.push_str(self.body.replace("\r\n", "\n").trim());
        out.push('\n');
        out
    }
}

/// Stable, cheap fingerprint of a file's text, so an editor can tell the
/// file changed under it. Not for security.
pub fn version(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn clean_segment(raw: &str, what: &str) -> std::result::Result<String, String> {
    const BAD: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || BAD.contains(&c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_end_matches(['.', ' ']).to_owned();
    if cleaned.is_empty() {
        return Err(format!("{what} can't be empty"));
    }
    if raw.trim() == "." || raw.trim() == ".." || cleaned.starts_with('.') {
        return Err(format!("{what} can't start with a dot"));
    }
    if cleaned.chars().count() > 120 {
        return Err(format!("{what} is too long (120 characters at most)"));
    }
    Ok(cleaned)
}

/// Path relative to `Recipes/` for a new recipe: `folder` may be nested
/// ("Diner/Soep"); the file is named after `title`.
pub fn recipe_rel_path(folder: &str, title: &str) -> std::result::Result<String, String> {
    let mut parts = Vec::new();
    for seg in folder.split('/').filter(|s| !s.trim().is_empty()) {
        parts.push(clean_segment(seg, "a folder name")?);
    }
    parts.push(format!("{}.cook", clean_segment(title, "the title")?));
    Ok(parts.join("/"))
}

#[derive(Debug)]
pub enum CreateError {
    Exists,
    Other(Error),
}

fn temp_beside(path: &Path) -> std::path::PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.tmp{}", std::process::id()))
}

fn write_temp(path: &Path, text: &str) -> Result<std::path::PathBuf> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    }
    let tmp = temp_beside(path);
    let mut f = std::fs::File::create(&tmp).map_err(|e| Error::io(&tmp, e))?;
    f.write_all(text.as_bytes())
        .and_then(|()| f.sync_all())
        .map_err(|e| Error::io(&tmp, e))?;
    Ok(tmp)
}

/// Replace a file's content; a crash leaves the old or the new, never half.
pub fn replace_file(path: &Path, text: &str) -> Result<()> {
    let tmp = write_temp(path, text)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::io(path, e)
    })
}

/// Write a new file, failing (and leaving the existing one alone) if it exists.
pub fn create_file(path: &Path, text: &str) -> std::result::Result<(), CreateError> {
    let tmp = write_temp(path, text).map_err(CreateError::Other)?;
    // A hard link fails if the target exists, unlike a rename.
    let linked = std::fs::hard_link(&tmp, path);
    let _ = std::fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(CreateError::Exists),
        Err(e) => Err(CreateError::Other(Error::io(path, e))),
    }
}

// ---------------------------------------------------------------- foods

/// A food to add to `aliases.yaml`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewFood {
    pub name: String,
    #[serde(default)]
    pub aisle: Option<String>,
    #[serde(default)]
    pub synonyms: Vec<String>,
    #[serde(default)]
    pub nevo: Option<u32>,
    /// Shown as a comment next to the code: the NEVO name.
    #[serde(default)]
    pub nevo_name: Option<String>,
    #[serde(default)]
    pub custom: Option<String>,
    #[serde(default)]
    pub negligible: bool,
    #[serde(default)]
    pub piece_g: Option<f64>,
    #[serde(default)]
    pub density: Option<f64>,
    #[serde(default)]
    pub units: BTreeMap<String, f64>,
}

fn positive(what: &str, v: Option<f64>) -> std::result::Result<(), String> {
    match v {
        Some(v) if !(v.is_finite() && v > 0.0) => {
            Err(format!("{what} must be more than 0, not {v}"))
        }
        _ => Ok(()),
    }
}

impl NewFood {
    fn check(&self) -> std::result::Result<(), String> {
        if self.name.trim().is_empty() || self.name.contains('\n') {
            return Err("a food needs a name (one line)".into());
        }
        let sources = [self.nevo.is_some(), self.custom.is_some(), self.negligible];
        if sources.iter().filter(|s| **s).count() > 1 {
            return Err("pick one of: a NEVO food, own values, negligible".into());
        }
        positive("piece_g", self.piece_g)?;
        positive("density", self.density)?;
        for (unit, g) in &self.units {
            positive(&format!("units: {unit}"), Some(*g))?;
        }
        Ok(())
    }

    fn block(&self) -> String {
        let mut out = format!("  {}:\n", scalar(&self.name));
        if let Some(code) = self.nevo {
            let note = self
                .nevo_name
                .as_deref()
                .map(|n| n.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|n| !n.is_empty());
            match note {
                Some(n) => out.push_str(&format!("    nevo: {code:<12}# {n}\n")),
                None => out.push_str(&format!("    nevo: {code}\n")),
            }
        }
        if let Some(c) = &self.custom {
            out.push_str(&format!("    custom: {}\n", scalar(c)));
        }
        if self.negligible {
            out.push_str("    negligible: true\n");
        }
        if let Some(g) = self.piece_g {
            out.push_str(&format!("    piece_g: {}\n", fmt_number(g)));
        }
        if let Some(d) = self.density {
            out.push_str(&format!("    density: {}\n", fmt_number(d)));
        }
        if !self.units.is_empty() {
            let units: Vec<String> = self
                .units
                .iter()
                .map(|(u, g)| format!("{}: {}", scalar(u), fmt_number(*g)))
                .collect();
            out.push_str(&format!("    units: {{ {} }}\n", units.join(", ")));
        }
        if let Some(a) = self.aisle.as_deref().filter(|a| !a.trim().is_empty()) {
            out.push_str(&format!("    aisle: {}\n", scalar(a)));
        }
        let synonyms: Vec<String> = self
            .synonyms
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(scalar)
            .collect();
        if !synonyms.is_empty() {
            out.push_str(&format!("    synonyms: [{}]\n", synonyms.join(", ")));
        }
        out
    }
}

/// Append a food to the text of `aliases.yaml`. The rest of the file is left
/// as it is; the result is parsed (and the new food looked up in it) before
/// it's returned, so writing it can't break the file.
pub fn append_food(existing: &str, food: &NewFood, path: &Path) -> Result<String> {
    let config = |message: String| Error::Config {
        path: path.into(),
        message,
    };
    food.check().map_err(config)?;
    let name = food.name.trim().to_owned();
    let food = NewFood {
        name: name.clone(),
        ..food.clone()
    };

    let current = FoodDb::from_yaml(existing, path)?;
    if let Some(found) = current.resolve(&name) {
        return Err(config(format!(
            "'{name}' is already in the file (as '{}')",
            found.canonical
        )));
    }

    let mut out = String::new();
    if existing.trim().is_empty() {
        out.push_str("foods:\n");
    } else {
        out.push_str(existing);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out.push_str(&food.block());

    // The append assumes `foods:` is the last key, in block style.
    let manual = |why: String| {
        config(format!(
            "couldn't add to the file automatically ({why}); add it by hand"
        ))
    };
    let parsed = FoodDb::from_yaml(&out, path).map_err(|e| match e {
        // A synonym that belongs to another food is the user's to fix.
        Error::Config { message, .. } => config(message),
        other => manual(other.to_string()),
    })?;
    let ok = parsed.resolve(&name).is_some_and(|r| {
        r.canonical == normalize_name(&name)
            && r.info.nevo == food.nevo
            && r.info.custom == food.custom
            && r.info.negligible == food.negligible
    });
    if !ok {
        return Err(manual("it didn't end up under foods:".into()));
    }
    Ok(out)
}

/// Is `unit` one that `units:` may not use? (re-exported for the web API)
pub fn is_fixed_size_unit(unit: &str) -> bool {
    units::is_fixed_size(unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COOK: &str = "---\ntitle: Chana masala\nservings: 4\ntime: 40 min\ntags: [diner, indiaas, veganistisch mogelijk]\nauthor: Oma\nsource: https://example.com/x\n---\n\nKook @rijst{300%g}.\n\nBak ~{30%seconds}.\n";

    #[test]
    fn recipe_text_round_trips() {
        let s = RecipeSource::from_text(COOK).unwrap();
        assert_eq!(s.title, "Chana masala");
        assert_eq!(s.servings, Some(4.0));
        assert_eq!(s.time.as_deref(), Some("40 min"));
        assert_eq!(s.tags, ["diner", "indiaas", "veganistisch mogelijk"]);
        assert_eq!(s.extra, "author: Oma\nsource: https://example.com/x");
        assert!(s.body.starts_with("Kook @rijst{300%g}."));
        assert_eq!(s.to_text(), COOK);
    }

    #[test]
    fn aliases_become_canonical_and_odd_values_stay() {
        let s = RecipeSource::from_text(
            "---\ntag: a, b\nserves: 2\nduration: 45\ncuisine: thai\n---\nCook.",
        )
        .unwrap();
        assert_eq!(s.tags, ["a", "b"]);
        assert_eq!(s.servings, Some(2.0));
        assert_eq!(s.time.as_deref(), Some("45"));
        assert_eq!(s.extra, "cuisine: thai");
        let text = s.to_text();
        assert!(
            text.contains("time: 45\n") && text.contains("tags: [a, b]\n"),
            "{text}"
        );

        // A servings value that isn't a number isn't consumed.
        let s = RecipeSource::from_text("---\nservings: a few\n---\nx").unwrap();
        assert_eq!(s.servings, None);
        assert_eq!(s.extra, "servings: a few");
    }

    #[test]
    fn unknown_keys_come_back_as_they_were() {
        let text = "---\n# where from\nsource: https://example.com\ntitle: T\ndescription: |\n  first\n\n  second\nnotes:\n- a\n- b\n# last words\n---\nCook.\n";
        let s = RecipeSource::from_text(text).unwrap();
        assert_eq!(
            s.extra,
            "# where from\nsource: https://example.com\ndescription: |\n  first\n\n  second\nnotes:\n- a\n- b\n# last words"
        );
        assert_eq!(s.title, "T");
        let again = RecipeSource::from_text(&s.to_text()).unwrap();
        assert_eq!(again, s);
    }

    #[test]
    fn text_without_front_matter_and_bad_front_matter() {
        let s = RecipeSource::from_text("Just @salt.\n").unwrap();
        assert_eq!((s.title.as_str(), s.body.as_str()), ("", "Just @salt."));
        assert_eq!(s.to_text(), "Just @salt.\n");
        assert!(RecipeSource::from_text("---\ntitle: [oops\n---\nx").is_err());
    }

    #[test]
    fn titles_with_colons_are_quoted() {
        let s = RecipeSource {
            title: "Soep: de beste".into(),
            body: "x".into(),
            ..Default::default()
        };
        let text = s.to_text();
        assert!(
            text.contains("title: 'Soep: de beste'") || text.contains("title: \"Soep: de beste\""),
            "{text}"
        );
        assert_eq!(
            RecipeSource::from_text(&text).unwrap().title,
            "Soep: de beste"
        );
    }

    #[test]
    fn recipe_paths_stay_inside_recipes() {
        assert_eq!(
            recipe_rel_path("Diner/Soep", "Tomaat: de ultieme?").unwrap(),
            "Diner/Soep/Tomaat de ultieme.cook"
        );
        assert_eq!(recipe_rel_path("", "Pasta").unwrap(), "Pasta.cook");
        for bad in [
            ("..", "x"),
            ("a/../b", "x"),
            ("", ".."),
            ("", ""),
            (".git", "x"),
            ("", ".hidden"),
        ] {
            assert!(recipe_rel_path(bad.0, bad.1).is_err(), "{bad:?}");
        }
        // Separators in a title can't make a path.
        assert!(recipe_rel_path("", "../etc/passwd")
            .unwrap_err()
            .contains("dot"));
        assert_eq!(recipe_rel_path("", "a/b").unwrap(), "a b.cook");
    }

    #[test]
    fn create_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("grocery-author-{}", std::process::id()));
        let file = dir.join("Soep/New.cook");
        create_file(&file, "one").unwrap();
        assert!(matches!(
            create_file(&file, "two"),
            Err(CreateError::Exists)
        ));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "one");
        replace_file(&file, "three").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "three");
        // no temp files left behind
        assert_eq!(std::fs::read_dir(dir.join("Soep")).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    const ALIASES: &str = "# my foods\naisles: [groente]\n\nfoods:\n  onion:  # the usual\n    nevo: 63\n    synonyms: [ui]\n";

    fn path() -> &'static Path {
        Path::new("aliases.yaml")
    }

    #[test]
    fn appends_a_food_and_keeps_the_rest() {
        let out = append_food(
            ALIASES,
            &NewFood {
                name: "dried thyme".into(),
                negligible: true,
                aisle: Some("kruiden & specerijen".into()),
                synonyms: vec!["gedroogde tijm".into(), "thyme".into()],
                ..Default::default()
            },
            path(),
        )
        .unwrap();
        assert!(out.starts_with(ALIASES), "{out}");
        assert!(out.ends_with(
            "  dried thyme:\n    negligible: true\n    aisle: kruiden & specerijen\n    synonyms: [gedroogde tijm, thyme]\n"
        ), "{out}");
        let db = FoodDb::from_yaml(&out, path()).unwrap();
        assert!(db.resolve("Gedroogde Tijm").unwrap().info.negligible);

        let nevo = append_food(
            ALIASES,
            &NewFood {
                name: "leek".into(),
                nevo: Some(99),
                nevo_name: Some("Prei rauw".into()),
                piece_g: Some(150.0),
                units: BTreeMap::from([("bos".to_owned(), 300.0)]),
                ..Default::default()
            },
            path(),
        )
        .unwrap();
        assert!(
            nevo.contains("    nevo: 99          # Prei rauw\n"),
            "{nevo}"
        );
        assert!(nevo.contains("    units: { bos: 300 }\n"), "{nevo}");
    }

    #[test]
    fn refuses_what_would_break_the_file() {
        let food = |name: &str| NewFood {
            name: name.into(),
            negligible: true,
            ..Default::default()
        };
        let err = |f: NewFood| append_food(ALIASES, &f, path()).unwrap_err().to_string();
        assert!(err(food("Ui")).contains("already in the file"));
        assert!(err(food("")).contains("needs a name"));
        let mut both = food("x");
        both.nevo = Some(1);
        assert!(err(both).contains("pick one"));
        let mut taken = food("shallot");
        taken.synonyms = vec!["uien".into(), "ui".into()];
        assert!(err(taken).contains("listed for both"));
        let mut bad = food("y");
        bad.density = Some(-1.0);
        assert!(err(bad).contains("more than 0"));
        let mut tl = food("z");
        tl.units = BTreeMap::from([("tl".to_owned(), 3.0)]);
        assert!(err(tl).contains("density"));
        // foods: isn't last → can't append safely
        let odd = "foods:\n  a: {}\nextra: 1\n";
        assert!(append_food(odd, &food("b"), path()).is_err());
    }
}
