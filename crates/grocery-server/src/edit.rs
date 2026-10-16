//! Routes that write to the library (listed in [`crate::api`]): recipes
//! (create, edit, check), foods (add, search NEVO), and the lists the forms
//! need (folders, aisles, foods).
//!
//! The library's files stay the source of truth. Every write is built from
//! text, parsed again, and only then put on disk: new recipes can't
//! overwrite an existing file, edits are refused if the file changed since
//! the editor loaded it (`version`), and nothing outside `Recipes/` and
//! `Food/` is ever touched.

use std::path::Path as FsPath;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use grocery_core::author::{
    self, append_food, create_file, recipe_rel_path, replace_file, CreateError, NewFood,
    RecipeSource,
};
use grocery_core::custom::{append_entry, CustomEntry};
use grocery_core::food::FoodDb;
use grocery_core::model::Amount;
use grocery_core::render;
use grocery_core::units;
use serde::{Deserialize, Serialize};

use crate::api::{blocking, ApiError, ApiResult, Shared};
use crate::App;

// ---------------------------------------------------------------- recipes

fn parse_error(e: &grocery_core::Error) -> String {
    match e {
        grocery_core::Error::Parse { message, .. } => message.clone(),
        other => other.to_string(),
    }
}

/// Parse text the way a file would be, before anything is written.
fn parse_source(app: &App, source: &RecipeSource) -> Result<grocery_core::model::Recipe, String> {
    app.library
        .parser
        .parse(FsPath::new("recipe.cook"), &source.to_text())
        .map_err(|e| parse_error(&e))
}

#[derive(Deserialize)]
pub struct CheckRequest {
    source: RecipeSource,
}

#[derive(Serialize)]
pub struct CheckJson {
    /// Why it can't be saved as is.
    error: Option<String>,
    warnings: Vec<String>,
    ingredients: Vec<CheckIngredient>,
    steps: usize,
    /// Sum of the timers, in seconds (a lower bound on the time it takes).
    timers_seconds: Option<f64>,
}

#[derive(Serialize)]
struct CheckIngredient {
    name: String,
    amount: Option<String>,
    optional: bool,
    /// `unknown`: not in aliases.yaml yet. `known`: it is.
    status: &'static str,
    /// What `aliases.yaml` calls it, when known.
    food: Option<String>,
    /// `nevo`, `custom`, `negligible`, or `none` (known, no nutrition data).
    nutrition: Option<&'static str>,
    /// Whether this amount converts to grams (`ok`/`missing`); not for
    /// "to taste" and the like.
    weight: Option<&'static str>,
}

pub async fn check(
    State(app): State<Shared>,
    Json(req): Json<CheckRequest>,
) -> ApiResult<Json<CheckJson>> {
    blocking(&app, move |app| {
        let foods = app.library.food_db()?;
        let recipe = match parse_source(app, &req.source) {
            Ok(r) => r,
            Err(error) => {
                return Ok(Json(CheckJson {
                    error: Some(error),
                    warnings: vec![],
                    ingredients: vec![],
                    steps: 0,
                    timers_seconds: None,
                }))
            }
        };
        Ok(Json(CheckJson {
            error: None,
            warnings: recipe.warnings.clone(),
            ingredients: check_ingredients(&recipe, &foods),
            steps: recipe
                .sections
                .iter()
                .flat_map(|s| &s.content)
                .filter(|c| matches!(c, grocery_core::model::Content::Step(_)))
                .count(),
            timers_seconds: recipe.timers_total().map(|d| d.min),
        }))
    })
    .await
}

fn check_ingredients(recipe: &grocery_core::model::Recipe, foods: &FoodDb) -> Vec<CheckIngredient> {
    // One row per ingredient name; amounts of the same name are joined.
    struct Row {
        name: String,
        amounts: Vec<String>,
        optional: bool,
        /// `None`: no amount that needs converting; else whether all convert.
        weight_ok: Option<bool>,
    }
    let mut rows: Vec<Row> = Vec::new();
    for ing in &recipe.ingredients {
        let i = match rows
            .iter()
            .position(|r| r.name.eq_ignore_ascii_case(&ing.name))
        {
            Some(i) => i,
            None => {
                rows.push(Row {
                    name: ing.name.clone(),
                    amounts: vec![],
                    optional: true,
                    weight_ok: None,
                });
                rows.len() - 1
            }
        };
        let row = &mut rows[i];
        row.optional &= ing.optional;
        row.amounts.extend(render::amount_text(&ing.amount, 1.0));
        if let Amount::Value { qty, unit, .. } = &ing.amount {
            let converts = foods.resolve(&ing.name).is_some_and(|r| {
                r.info.negligible
                    || FoodDb::grams(r.info, &units::normalize(*qty, unit.as_deref())).is_some()
            });
            row.weight_ok = Some(row.weight_ok.unwrap_or(true) && converts);
        }
    }
    rows.into_iter()
        .map(|row| {
            let resolved = foods.resolve(&row.name);
            CheckIngredient {
                amount: (!row.amounts.is_empty()).then(|| row.amounts.join(" + ")),
                optional: row.optional,
                status: if resolved.is_some() {
                    "known"
                } else {
                    "unknown"
                },
                food: resolved.map(|r| r.canonical.to_owned()),
                nutrition: resolved.map(|r| {
                    if r.info.negligible {
                        "negligible"
                    } else if r.info.nevo.is_some() {
                        "nevo"
                    } else if r.info.custom.is_some() {
                        "custom"
                    } else {
                        "none"
                    }
                }),
                weight: row.weight_ok.map(|ok| if ok { "ok" } else { "missing" }),
                name: row.name,
            }
        })
        .collect()
}

pub async fn folders(State(app): State<Shared>) -> ApiResult<Json<Vec<String>>> {
    blocking(&app, |app| Ok(Json(app.library.recipe_folders()?))).await
}

#[derive(Serialize)]
pub struct SourceJson {
    path: String,
    source: RecipeSource,
    /// Send this back when saving: the save is refused if the file changed.
    version: String,
    /// Set when the file's front matter couldn't be read into fields; the
    /// whole text is then in `source.body`.
    note: Option<String>,
}

pub async fn get_source(
    State(app): State<Shared>,
    Path(path): Path<String>,
) -> ApiResult<Json<SourceJson>> {
    blocking(&app, move |app| {
        app.store()
            .recipe(&path)?
            .ok_or_else(|| ApiError::not_found(format!("recipe '{path}'")))?;
        let file = app.library.recipes_dir().join(&path);
        let text = std::fs::read_to_string(&file).map_err(|e| grocery_core::Error::Io {
            path: file,
            source: e,
        })?;
        let (source, note) = match RecipeSource::from_text(&text) {
            Ok(s) => (s, None),
            Err(why) => (
                RecipeSource {
                    body: text.clone(),
                    ..Default::default()
                },
                Some(why),
            ),
        };
        Ok(Json(SourceJson {
            version: author::version(&text),
            path,
            source,
            note,
        }))
    })
    .await
}

fn needs_title(source: &RecipeSource) -> ApiResult<()> {
    if source.title.trim().is_empty() {
        return Err(ApiError::bad_request("a recipe needs a title"));
    }
    if source.body.trim().is_empty() {
        return Err(ApiError::bad_request("a recipe needs a method"));
    }
    if let Some(s) = source.servings {
        if !(s > 0.0 && s.is_finite() && s <= 1000.0) {
            return Err(ApiError::bad_request("servings must be more than 0"));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct CreateRecipe {
    /// Under `Recipes/`, e.g. "Diner"; empty for the top.
    #[serde(default)]
    folder: String,
    source: RecipeSource,
}

#[derive(Serialize)]
pub struct Saved {
    path: String,
}

pub async fn create_recipe(
    State(app): State<Shared>,
    Json(req): Json<CreateRecipe>,
) -> ApiResult<(StatusCode, Json<Saved>)> {
    needs_title(&req.source)?;
    let rel = recipe_rel_path(&req.folder, &req.source.title).map_err(ApiError::bad_request)?;
    blocking(&app, move |app| {
        parse_source(app, &req.source)
            .map_err(|m| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, m))?;
        let file = app.library.recipes_dir().join(&rel);
        match create_file(&file, &req.source.to_text()) {
            Ok(()) => {}
            Err(CreateError::Exists) => {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    format!("there's already a recipe '{rel}'; pick another title or folder"),
                ))
            }
            Err(CreateError::Other(e)) => return Err(e.into()),
        }
        app.sync(false)?;
        Ok((StatusCode::CREATED, Json(Saved { path: rel })))
    })
    .await
}

#[derive(Deserialize)]
pub struct UpdateRecipe {
    source: RecipeSource,
    version: String,
}

pub async fn update_recipe(
    State(app): State<Shared>,
    Path(path): Path<String>,
    Json(req): Json<UpdateRecipe>,
) -> ApiResult<Json<Saved>> {
    needs_title(&req.source)?;
    blocking(&app, move |app| {
        app.store()
            .recipe(&path)?
            .ok_or_else(|| ApiError::not_found(format!("recipe '{path}'")))?;
        let file = app.library.recipes_dir().join(&path);
        let current = std::fs::read_to_string(&file).map_err(|e| grocery_core::Error::Io {
            path: file.clone(),
            source: e,
        })?;
        if author::version(&current) != req.version {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "this recipe was changed somewhere else since you opened it; \
                 reopen it to see the new version (copy your text first)",
            ));
        }
        parse_source(app, &req.source)
            .map_err(|m| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, m))?;
        replace_file(&file, &req.source.to_text())?;
        app.sync(false)?;
        Ok(Json(Saved { path }))
    })
    .await
}

// ---------------------------------------------------------------- foods

#[derive(Serialize)]
pub struct FoodJson {
    name: String,
    aisle: Option<String>,
    synonyms: Vec<String>,
    nevo: Option<u32>,
    custom: Option<String>,
    negligible: bool,
    piece_g: Option<f64>,
    density: Option<f64>,
}

pub async fn foods(State(app): State<Shared>) -> ApiResult<Json<Vec<FoodJson>>> {
    blocking(&app, |app| {
        let db = app.library.food_db()?;
        Ok(Json(
            db.foods()
                .map(|(name, i)| FoodJson {
                    name: name.to_owned(),
                    aisle: i.aisle.clone(),
                    synonyms: i.synonyms.clone(),
                    nevo: i.nevo,
                    custom: i.custom.clone(),
                    negligible: i.negligible,
                    piece_g: i.piece_g,
                    density: i.density,
                })
                .collect(),
        ))
    })
    .await
}

pub async fn aisles(State(app): State<Shared>) -> ApiResult<Json<Vec<String>>> {
    blocking(&app, |app| {
        let db = app.library.food_db()?;
        let mut aisles: Vec<String> = db.aisle_order().to_vec();
        // Aisles used by foods but not in the walking order.
        for (_, info) in db.foods() {
            if let Some(a) = &info.aisle {
                if !aisles.contains(a) {
                    aisles.push(a.clone());
                }
            }
        }
        Ok(Json(aisles))
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddFood {
    name: String,
    #[serde(default)]
    aisle: Option<String>,
    #[serde(default)]
    synonyms: Vec<String>,
    /// A NEVO food code (from `/api/nevo/search`).
    #[serde(default)]
    nevo: Option<u32>,
    /// Name of an existing entry in custom.yaml.
    #[serde(default)]
    custom: Option<String>,
    /// New own values (label numbers); stored in custom.yaml under `name`.
    #[serde(default)]
    own: Option<CustomEntry>,
    #[serde(default)]
    negligible: bool,
    #[serde(default)]
    piece_g: Option<f64>,
    #[serde(default)]
    density: Option<f64>,
    #[serde(default)]
    units: std::collections::BTreeMap<String, f64>,
}

#[derive(Serialize)]
pub struct AddedFood {
    name: String,
    /// What was written to custom.yaml, if anything.
    own_values: bool,
}

pub async fn add_food(
    State(app): State<Shared>,
    Json(req): Json<AddFood>,
) -> ApiResult<(StatusCode, Json<AddedFood>)> {
    blocking(&app, move |app| {
        let name = req.name.trim().to_owned();
        let aliases = app.library.aliases_path();
        let custom_path = app.library.custom_path();
        let read = |p: &FsPath| match std::fs::read_to_string(p) {
            Ok(t) => Ok(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(grocery_core::Error::Io {
                path: p.into(),
                source: e,
            }),
        };

        let mut food = NewFood {
            name: name.clone(),
            aisle: req.aisle,
            synonyms: req.synonyms,
            nevo: req.nevo,
            nevo_name: None,
            custom: req.custom.clone(),
            negligible: req.negligible,
            piece_g: req.piece_g,
            density: req.density,
            units: req.units,
        };

        if let Some(code) = req.nevo {
            let record = app.store().food(code)?.ok_or_else(|| {
                ApiError::bad_request(format!("NEVO has no food with code {code}"))
            })?;
            food.nevo_name = Some(record.name_nl);
        }
        if let Some(existing) = &req.custom {
            if !app.library.custom_foods()?.contains(existing) {
                return Err(ApiError::bad_request(format!(
                    "'{existing}' isn't in custom.yaml"
                )));
            }
        }

        // Build both files' new text first; write only if both are fine.
        let new_custom = match &req.own {
            Some(entry) => {
                if req.custom.is_some() {
                    return Err(ApiError::bad_request(
                        "give own values or the name of an existing custom food, not both",
                    ));
                }
                food.custom = Some(name.clone());
                Some(append_entry(
                    &read(&custom_path)?,
                    &name,
                    entry,
                    &custom_path,
                )?)
            }
            None => None,
        };
        let new_aliases = append_food(&read(&aliases)?, &food, &aliases)?;

        if let Some(text) = &new_custom {
            replace_file(&custom_path, text)?;
        }
        replace_file(&aliases, &new_aliases)?;
        Ok((
            StatusCode::CREATED,
            Json(AddedFood {
                name,
                own_values: new_custom.is_some(),
            }),
        ))
    })
    .await
}

#[derive(Serialize)]
pub struct FoodNutrition {
    /// The food's name in aliases.yaml.
    food: String,
    /// `nevo`, `custom` (own values), `negligible` or `none`.
    source: &'static str,
    /// The NEVO food the values come from.
    nevo: Option<NevoRef>,
    /// For own values: where the numbers came from.
    own_source: Option<String>,
    /// `100g` or `100ml`; `None` when there are no values.
    per: Option<&'static str>,
    piece_g: Option<f64>,
    /// The label-style rows; `value` is `null` when unknown (not zero).
    nutrients: Vec<NutrientValue>,
    /// Required with NEVO numbers by its conditions of use.
    citation: Option<String>,
}

#[derive(Serialize)]
struct NevoRef {
    code: u32,
    name: String,
}

#[derive(Serialize)]
struct NutrientValue {
    code: String,
    label: String,
    unit: String,
    value: Option<f64>,
    indent: bool,
    section: Option<String>,
}

/// Nutrition of one food per 100 g (or ml): what a line on a shopping list
/// or a name in the food list stands for.
pub async fn food_nutrition(
    State(app): State<Shared>,
    Path(name): Path<String>,
) -> ApiResult<Json<FoodNutrition>> {
    use grocery_core::nutrition::{Basis, Composition, FoodKey};
    blocking(&app, move |app| {
        let db = app.library.food_db()?;
        let resolved = db
            .resolve(&name)
            .ok_or_else(|| ApiError::not_found(format!("food '{name}'")))?;
        let info = resolved.info;
        let mut out = FoodNutrition {
            food: resolved.canonical.to_owned(),
            source: "none",
            nevo: None,
            own_source: None,
            per: None,
            piece_g: info.piece_g,
            nutrients: vec![],
            citation: None,
        };
        let store = app.store();
        let composition: Option<Composition> = if info.negligible {
            out.source = "negligible";
            None
        } else if let Some(code) = info.nevo {
            out.source = "nevo";
            let nevo = store.require_nevo()?;
            out.citation = Some(nevo.citation());
            let table = store.compositions(&std::collections::BTreeSet::from([code]))?;
            let comp = table.nevo(code).cloned();
            out.nevo = comp.as_ref().map(|c| NevoRef {
                code,
                name: c.name.clone(),
            });
            comp
        } else if let Some(custom) = &info.custom {
            out.source = "custom";
            let key = FoodKey::Custom(grocery_core::food::normalize_name(custom));
            let comp = app
                .library
                .custom_foods()?
                .compositions()
                .into_iter()
                .find(|c| c.key == key);
            out.own_source = comp.as_ref().and_then(|c| c.source.clone());
            comp
        } else {
            None
        };
        if let Some(c) = composition {
            out.per = Some(match c.basis {
                Basis::Per100g => "100g",
                Basis::Per100ml => "100ml",
            });
            let mut values = c.values.clone();
            if let Some(na) = values.get("NA").copied() {
                values.insert("SALT".into(), na * 2.5 / 1000.0);
            }
            out.nutrients = store
                .display_rows(false)?
                .into_iter()
                .map(|r| NutrientValue {
                    value: values.get(&r.code).copied(),
                    code: r.code,
                    label: r.label,
                    unit: r.unit,
                    indent: r.indent,
                    section: r.section,
                })
                .collect();
        }
        Ok(Json(out))
    })
    .await
}

// ---------------------------------------------------------------- NEVO

#[derive(Deserialize)]
pub struct SearchQuery {
    q: String,
    limit: Option<usize>,
}

#[derive(Serialize)]
pub struct NevoHit {
    code: u32,
    name_nl: String,
    name_en: String,
    group: String,
    /// kcal per 100 g (or ml), to tell raw from cooked at a glance.
    kcal: Option<f64>,
}

pub async fn nevo_search(
    State(app): State<Shared>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<Vec<NevoHit>>> {
    blocking(&app, move |app| {
        let store = app.store();
        store.require_nevo()?;
        if query.q.trim().is_empty() {
            return Ok(Json(vec![]));
        }
        let limit = query.limit.unwrap_or(12).clamp(1, 50);
        Ok(Json(
            store
                .search(&query.q, limit)?
                .into_iter()
                .map(|h| NevoHit {
                    code: h.candidate.code,
                    name_nl: h.candidate.name_nl,
                    name_en: h.candidate.name_en,
                    group: h.candidate.group_en,
                    kcal: h.candidate.kcal,
                })
                .collect(),
        ))
    })
    .await
}
