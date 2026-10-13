//! Ranked search over NEVO food names.
//!
//! NEVO names put the head noun first and mark compounds with a hyphen:
//! "Kool boeren- rauw" is boerenkool, "Olie olijf-" is olijfolie, "Worst
//! rook- gekookt gem" is rookworst. Searching for the everyday word only
//! works if those compounds are rebuilt, so each name also gets its joined
//! forms. English names and the synonym column are searched too.
//!
//! Every query word must match. Ties prefer raw/fresh products (recipes are
//! weighed raw), NEVO averages ("gem"), ingredients over prepared dishes, and
//! shorter names.

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub code: u32,
    pub name_nl: String,
    pub name_en: String,
    pub synonyms: String,
    pub group_en: String,
    /// kcal per 100 g/ml, to tell raw pasta from cooked at a glance.
    pub kcal: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub candidate: Candidate,
    pub score: i32,
    /// Words in the Dutch name; fewer = more generic. Tie-breaker when
    /// merging results from several queries.
    pub name_words: usize,
}

impl Hit {
    /// Best first: score, then shorter name, then code.
    pub fn order(a: &Hit, b: &Hit) -> std::cmp::Ordering {
        b.score
            .cmp(&a.score)
            .then(a.name_words.cmp(&b.name_words))
            .then(a.candidate.code.cmp(&b.candidate.code))
    }
}

/// Lowercase and strip the accents that occur in NEVO names.
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

fn words(s: &str) -> Vec<String> {
    fold(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// "Kool boeren- rauw" → ["boerenkool"]; "Saus knoflook- 20-<30% olie" → ["knoflooksaus"].
pub fn compounds(name_nl: &str) -> Vec<String> {
    let tokens: Vec<&str> = name_nl.split_whitespace().collect();
    let Some(head) = tokens.first() else {
        return Vec::new();
    };
    let head = fold(head.trim_end_matches('-'));
    tokens
        .iter()
        .skip(1)
        .filter_map(|t| t.strip_suffix('-'))
        .filter(|m| m.chars().all(char::is_alphabetic) && !m.is_empty())
        .map(|m| format!("{}{head}", fold(m)))
        .collect()
}

struct Indexed {
    words: Vec<String>,
    /// Hyphenated modifiers in the Dutch name: "roomboter" in "Koekje
    /// roomboter-" (a butter biscuit). The food is *made with* this, so it
    /// matches, but weaker than the food itself.
    modifiers: Vec<String>,
    compounds: Vec<String>,
    /// First word of the Dutch and English names: the food itself.
    head: String,
    head_en: String,
    nl_word_count: usize,
    bonus: i32,
}

fn index(c: &Candidate) -> Indexed {
    let mut ws = Vec::new();
    let mut modifiers = Vec::new();
    for (i, token) in c.name_nl.split_whitespace().enumerate() {
        if i > 0 && token.ends_with('-') {
            modifiers.extend(words(token));
        } else {
            ws.extend(words(token));
        }
    }
    let nl_word_count = ws.len() + modifiers.len();
    let head = ws.first().cloned().unwrap_or_default();
    let en = words(&c.name_en);
    let head_en = en.first().cloned().unwrap_or_default();
    ws.extend(en);
    ws.extend(words(&c.synonyms));
    let has = |w: &str| ws.iter().any(|x| x == w);
    let mut bonus = 0;
    if has("rauw") || has("raw") || has("vers") || has("fresh") {
        bonus += 1;
    }
    if has("gem") || has("av") {
        bonus += 1;
    }
    // Prepared dishes are rarely what an ingredient line means.
    if has("bereid") || has("prepared") || has("homemade") {
        bonus -= 1;
    }
    Indexed {
        compounds: compounds(&c.name_nl),
        words: ws,
        modifiers,
        head,
        head_en,
        nl_word_count,
        bonus,
    }
}

/// Exact, or the query plus a plural ending: "onion" matches "Onions raw".
fn same_word(w: &str, t: &str) -> bool {
    w == t
        || w.strip_prefix(t)
            .is_some_and(|rest| matches!(rest, "s" | "es" | "en"))
}

fn token_score(t: &str, ix: &Indexed) -> Option<i32> {
    if ix.words.iter().any(|w| same_word(w, t)) || ix.compounds.iter().any(|w| w == t) {
        let head = ix.head == t || same_word(&ix.head_en, t) || ix.compounds.iter().any(|w| w == t);
        return Some(if head { 6 } else { 4 });
    }
    if ix.modifiers.iter().any(|w| same_word(w, t)) {
        return Some(2);
    }
    if t.chars().count() >= 3 && ix.words.iter().any(|w| w.starts_with(t)) {
        return Some(2);
    }
    if t.chars().count() >= 4
        && ix
            .words
            .iter()
            .chain(&ix.modifiers)
            .chain(&ix.compounds)
            .any(|w| w.contains(t))
    {
        return Some(1);
    }
    None
}

pub fn rank(query: &str, items: &[Candidate], limit: usize) -> Vec<Hit> {
    let q = words(query);
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<Hit> = items
        .iter()
        .filter_map(|c| {
            let ix = index(c);
            let mut score = 0;
            for t in &q {
                score += token_score(t, &ix)?;
            }
            Some(Hit {
                candidate: c.clone(),
                score: score + ix.bonus,
                name_words: ix.nl_word_count,
            })
        })
        .collect();
    hits.sort_by(Hit::order);
    hits.truncate(limit);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(code: u32, nl: &str, en: &str, syn: &str) -> Candidate {
        Candidate {
            code,
            name_nl: nl.into(),
            name_en: en.into(),
            synonyms: syn.into(),
            group_en: String::new(),
            kcal: None,
        }
    }

    // Names in NEVO's style; codes are made up.
    fn items() -> Vec<Candidate> {
        vec![
            c(1, "Ui rauw", "Onions raw", ""),
            c(2, "Bonen tuin- rauw", "Beans broad raw", ""),
            c(3, "Soep ui- bereid", "Soup onion prepared", ""),
            c(4, "Kool boeren- rauw", "Kale curly raw", ""),
            c(5, "Kool boeren- gekookt", "Kale curly boiled", ""),
            c(
                6,
                "Stamppot boerenkool z vlees bereid",
                "Kale curly w mashed potato",
                "",
            ),
            c(7, "Olie olijf-", "Oil olive", ""),
            c(
                8,
                "Pasta witte rauw",
                "Pasta white raw",
                "Macaroni/spaghetti/tagliatelle witte onbereid",
            ),
            c(9, "Pasta witte gem gekookt", "Pasta white av boiled", ""),
            c(10, "Ei kippen- rauw gem", "Egg whole chicken av raw", ""),
            c(
                11,
                "Ei kippen- scharrel rauw",
                "Egg whole chicken free-range raw",
                "",
            ),
            c(12, "Luteïne test", "Lutein test", ""),
            c(
                13,
                "Spaghetti bolognese m kaas huishoudelijk bereid",
                "Spaghetti bolognese homemade",
                "",
            ),
            c(14, "Koekje roomboter- gem", "Biscuits w butter av", ""),
            c(
                15,
                "Boter ongezouten",
                "Butter unsalted",
                "Roomboter ongezouten",
            ),
            c(
                16,
                "Kaas komijne- 48+ gem",
                "Cheese w cumin 48+ average",
                "",
            ),
            c(17, "Komijnzaad gedroogd", "Cumin seed dried", ""),
        ]
    }

    fn top(q: &str) -> Vec<u32> {
        rank(q, &items(), 3)
            .into_iter()
            .map(|h| h.candidate.code)
            .collect()
    }

    #[test]
    fn compound_forms() {
        assert_eq!(compounds("Kool boeren- rauw"), ["boerenkool"]);
        assert_eq!(compounds("Saus knoflook- 20-<30% olie"), ["knoflooksaus"]);
        assert!(compounds("Ui rauw").is_empty());
    }

    #[test]
    fn short_words_match_exactly_not_inside_other_words() {
        assert_eq!(top("ui")[0], 1);
        assert!(!top("ui").contains(&2), "'tuin' must not match 'ui'");
    }

    #[test]
    fn everyday_compounds_find_nevo_names() {
        assert_eq!(top("boerenkool")[..2], [4, 5], "raw first");
        assert_eq!(top("olijfolie")[0], 7);
        assert_eq!(top("olive oil")[0], 7);
    }

    #[test]
    fn synonyms_english_and_preferences() {
        assert_eq!(top("spaghetti")[0], 8);
        assert_eq!(top("onion")[0], 1);
        assert_eq!(top("ei")[0], 10, "average over specific variety");
        assert_eq!(top("pasta")[0], 8, "raw over cooked");
        assert_eq!(top("luteine")[0], 12, "accents folded");
        assert_eq!(top("roomboter")[0], 15, "butter, not a butter biscuit");
        assert_eq!(top("butter")[0], 15, "English head word counts too");
        assert_eq!(top("cumin")[0], 17, "cumin, not cumin cheese");
        assert_eq!(
            top("ui"),
            [1, 3],
            "onion soup matches via its modifier, ranked lower"
        );
        assert!(top("xyz").is_empty());
        assert!(top("").is_empty());
    }
}
