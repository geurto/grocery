//! Meal plan routes (listed in [`crate::api`]): people, their plans, and
//! per-day nutrition.
//!
//! `GET /api/plan/nutrition` answers, for each day from `from` to `to`:
//!
//! ```json
//! { "citation": "Based on data from NEVO online version …",
//!   "rows": [{ "code": "ENERCC", "label": "Energy", "unit": "kcal", "ri": 2000, … }],
//!   "days": [{ "day": "2026-10-14",
//!              "nutrients": { "ENERCC": { "value": 1830.5, "lower_bound": false } },
//!              "missing": [{ "food": "Tofu", "nutrients": ["Vitamin D"] }],
//!              "entries": [{ "id": 3, "kcal": 610.2, "coverage": 0.97, "problem": null }],
//!              "own_foods": ["fresh coriander (package)"] }] }
//! ```
//!
//! The same rules as the recipe report: unknown is not zero (`lower_bound`
//! and `missing` say where values are absent), and the NEVO citation goes
//! with every number.

use std::collections::{BTreeMap, HashMap};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::NaiveDate;
use grocery_core::nutrition::{self, Composition, FoodKey, Nutrition, Options, Status};
use grocery_store::plans::parse_day;
use grocery_store::{EntryChange, Meal, NewEntry, Person, PlanEntry};
use serde::{Deserialize, Deserializer, Serialize};

use crate::api::{blocking, load_recipe, ApiError, ApiResult, Shared};
use crate::Event;

/// Longest range one request may ask for.
const MAX_DAYS: i64 = 62;

// ---------------------------------------------------------------- people

#[derive(Serialize)]
pub struct PersonJson {
    id: i64,
    name: String,
}

impl From<Person> for PersonJson {
    fn from(p: Person) -> Self {
        Self {
            id: p.id,
            name: p.name,
        }
    }
}

#[derive(Deserialize)]
pub struct Name {
    name: String,
}

pub async fn people(State(app): State<Shared>) -> ApiResult<Json<Vec<PersonJson>>> {
    blocking(&app, |app| {
        Ok(Json(
            app.store().people()?.into_iter().map(Into::into).collect(),
        ))
    })
    .await
}

pub async fn add_person(
    State(app): State<Shared>,
    Json(req): Json<Name>,
) -> ApiResult<(StatusCode, Json<PersonJson>)> {
    blocking(&app, move |app| {
        let person = app.store().add_person(&req.name)?;
        app.notify(Event::People);
        Ok((StatusCode::CREATED, Json(person.into())))
    })
    .await
}

pub async fn rename_person(
    State(app): State<Shared>,
    Path(id): Path<i64>,
    Json(req): Json<Name>,
) -> ApiResult<StatusCode> {
    blocking(&app, move |app| {
        if !app.store().rename_person(id, &req.name)? {
            return Err(ApiError::not_found(format!("person {id}")));
        }
        app.notify(Event::People);
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

pub async fn delete_person(
    State(app): State<Shared>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    blocking(&app, move |app| {
        if !app.store().delete_person(id)? {
            return Err(ApiError::not_found(format!("person {id}")));
        }
        app.notify(Event::People);
        app.notify(Event::Plan { person: id });
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

// ---------------------------------------------------------------- entries

#[derive(Serialize)]
pub struct EntryJson {
    id: i64,
    person: i64,
    day: String,
    meal: Option<&'static str>,
    path: String,
    title: String,
    portions: f64,
}

impl From<PlanEntry> for EntryJson {
    fn from(e: PlanEntry) -> Self {
        Self {
            id: e.id,
            person: e.person,
            day: e.day.to_string(),
            meal: e.meal.map(Meal::as_str),
            path: e.path,
            title: e.title,
            portions: e.portions,
        }
    }
}

#[derive(Deserialize)]
pub struct Range {
    person: i64,
    from: String,
    to: String,
}

impl Range {
    fn days(&self) -> ApiResult<(NaiveDate, NaiveDate)> {
        let (from, to) = (parse_day(&self.from)?, parse_day(&self.to)?);
        let span = (to - from).num_days();
        if !(0..MAX_DAYS).contains(&span) {
            return Err(ApiError::bad_request(format!(
                "from must be on or before to, at most {MAX_DAYS} days apart"
            )));
        }
        Ok((from, to))
    }
}

pub async fn plan(
    State(app): State<Shared>,
    Query(range): Query<Range>,
) -> ApiResult<Json<Vec<EntryJson>>> {
    let (from, to) = range.days()?;
    blocking(&app, move |app| {
        let entries = app.store().plan(range.person, from, to)?;
        Ok(Json(entries.into_iter().map(Into::into).collect()))
    })
    .await
}

fn meal(s: Option<&str>) -> ApiResult<Option<Meal>> {
    s.map(|m| {
        Meal::parse(m).ok_or_else(|| {
            ApiError::bad_request(format!(
                "meal is breakfast, lunch, dinner or snack, not '{m}'"
            ))
        })
    })
    .transpose()
}

#[derive(Deserialize)]
pub struct AddEntry {
    person: i64,
    day: String,
    meal: Option<String>,
    /// Recipe path, as in `/api/recipes`.
    recipe: String,
    portions: Option<f64>,
}

pub async fn add_entry(
    State(app): State<Shared>,
    Json(req): Json<AddEntry>,
) -> ApiResult<(StatusCode, Json<EntryJson>)> {
    let day = parse_day(&req.day)?;
    let meal = meal(req.meal.as_deref())?;
    blocking(&app, move |app| {
        let store = app.store();
        let recipe = store
            .recipe(&req.recipe)?
            .ok_or_else(|| ApiError::not_found(format!("recipe '{}'", req.recipe)))?;
        let entry = store
            .add_plan_entry(&NewEntry {
                person: req.person,
                day,
                meal,
                path: recipe.path,
                title: recipe.title,
                portions: req.portions.unwrap_or(1.0),
            })?
            .ok_or_else(|| ApiError::not_found(format!("person {}", req.person)))?;
        drop(store);
        app.notify(Event::Plan {
            person: entry.person,
        });
        Ok((StatusCode::CREATED, Json(entry.into())))
    })
    .await
}

/// `"meal": null` clears it; a missing `meal` leaves it alone.
fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
pub struct UpdateEntry {
    day: Option<String>,
    #[serde(default, deserialize_with = "present")]
    meal: Option<Option<String>>,
    portions: Option<f64>,
}

pub async fn update_entry(
    State(app): State<Shared>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateEntry>,
) -> ApiResult<Json<EntryJson>> {
    let change = EntryChange {
        day: req.day.as_deref().map(parse_day).transpose()?,
        meal: req.meal.map(|m| meal(m.as_deref())).transpose()?,
        portions: req.portions,
    };
    blocking(&app, move |app| {
        let entry = app
            .store()
            .update_plan_entry(id, &change)?
            .ok_or_else(|| ApiError::not_found(format!("plan entry {id}")))?;
        app.notify(Event::Plan {
            person: entry.person,
        });
        Ok(Json(entry.into()))
    })
    .await
}

pub async fn delete_entry(State(app): State<Shared>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    blocking(&app, move |app| {
        let store = app.store();
        let entry = store
            .plan_entry(id)?
            .ok_or_else(|| ApiError::not_found(format!("plan entry {id}")))?;
        store.delete_plan_entry(id)?;
        drop(store);
        app.notify(Event::Plan {
            person: entry.person,
        });
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

// ---------------------------------------------------------------- nutrition

#[derive(Serialize)]
pub struct PlanNutrition {
    citation: String,
    rows: Vec<RowJson>,
    days: Vec<DayJson>,
}

#[derive(Serialize)]
struct RowJson {
    code: String,
    label: String,
    unit: String,
    ri: Option<f64>,
    indent: bool,
    section: Option<String>,
}

#[derive(Serialize)]
struct DayJson {
    day: String,
    nutrients: BTreeMap<String, ValueJson>,
    /// Foods without a value, and for which of the rows.
    missing: Vec<MissingJson>,
    /// Per entry; one that isn't counted says why in `problem`.
    entries: Vec<EntryNutritionJson>,
    /// Foods counted with the user's own values (custom.yaml), not NEVO's.
    own_foods: Vec<String>,
}

#[derive(Serialize)]
struct ValueJson {
    value: f64,
    /// Some counted food has no value for this: the real value is higher.
    lower_bound: bool,
}

#[derive(Serialize)]
struct MissingJson {
    food: String,
    nutrients: Vec<String>,
}

#[derive(Serialize)]
struct EntryNutritionJson {
    id: i64,
    /// For the portions eaten.
    kcal: Option<f64>,
    kcal_lower_bound: bool,
    /// Share of the recipe's known ingredient weight that was counted.
    coverage: Option<f64>,
    /// Why it isn't counted at all.
    problem: Option<String>,
}

pub async fn nutrition(
    State(app): State<Shared>,
    Query(range): Query<Range>,
) -> ApiResult<Json<PlanNutrition>> {
    let (from, to) = range.days()?;
    blocking(&app, move |app| {
        let entries = app.store().plan(range.person, from, to)?;
        let info = app.store().require_nevo()?;
        let foods = app.library.food_db()?;
        let custom = app.library.custom_foods()?;

        // Each recipe once, however often it's planned.
        let mut recipes = HashMap::new();
        for e in &entries {
            if !recipes.contains_key(&e.path) {
                recipes.insert(e.path.clone(), load_recipe(app, &e.path).map(|(_, r)| r));
            }
        }
        let loaded: Vec<_> = recipes.values().filter_map(|r| r.as_ref().ok()).collect();
        let table = app.store().composition_table(&loaded, &foods, &custom)?;
        let computed: HashMap<&String, Result<Nutrition, String>> = recipes
            .iter()
            .map(|(path, r)| {
                let n = match r {
                    Ok(recipe) => Ok(nutrition::compute(
                        recipe,
                        &foods,
                        &table,
                        Options::default(),
                    )),
                    Err(e) => Err(e.message.clone()),
                };
                (path, n)
            })
            .collect();

        let rows = app.store().display_rows(false)?;
        let mut days = Vec::new();
        for day in from.iter_days().take_while(|d| *d <= to) {
            let todays: Vec<&PlanEntry> = entries.iter().filter(|e| e.day == day).collect();
            let mut parts = Vec::new();
            let mut entry_json = Vec::new();
            for e in &todays {
                let mut json = EntryNutritionJson {
                    id: e.id,
                    kcal: None,
                    kcal_lower_bound: false,
                    coverage: None,
                    problem: None,
                };
                match &computed[&e.path] {
                    Err(problem) => json.problem = Some(problem.clone()),
                    Ok(n) if !n.servings.is_some_and(|s| s > 0.0) => {
                        json.problem = Some("the recipe doesn't say how many it serves".into());
                    }
                    Ok(n) => {
                        parts.push((n, e.portions));
                        json.coverage = n.coverage();
                        if let Some((per, total)) = n.per_serving("ENERCC") {
                            json.kcal = Some(per * e.portions);
                            json.kcal_lower_bound = !total.missing_in.is_empty();
                        }
                    }
                }
                entry_json.push(json);
            }
            let combined = nutrition::combine(&parts);

            let mut missing: BTreeMap<&str, Vec<String>> = BTreeMap::new();
            let mut nutrients = BTreeMap::new();
            for row in &rows {
                let Some(total) = combined.totals.get(&row.code) else {
                    continue;
                };
                for food in &total.missing_in {
                    missing.entry(food).or_default().push(row.label.clone());
                }
                nutrients.insert(
                    row.code.clone(),
                    ValueJson {
                        value: total.value,
                        lower_bound: !total.missing_in.is_empty(),
                    },
                );
            }
            days.push(DayJson {
                day: day.to_string(),
                nutrients,
                missing: missing
                    .into_iter()
                    .map(|(food, nutrients)| MissingJson {
                        food: food.to_owned(),
                        nutrients,
                    })
                    .collect(),
                entries: entry_json,
                own_foods: own_foods(parts.iter().map(|(n, _)| *n), &table),
            });
        }

        Ok(Json(PlanNutrition {
            citation: info.citation(),
            rows: rows
                .into_iter()
                .map(|r| RowJson {
                    code: r.code,
                    label: r.label,
                    unit: r.unit,
                    ri: r.ri,
                    indent: r.indent,
                    section: r.section,
                })
                .collect(),
            days,
        }))
    })
    .await
}

/// "fresh coriander (package)" for each custom food counted.
fn own_foods<'a>(
    recipes: impl Iterator<Item = &'a Nutrition>,
    table: &nutrition::CompositionTable,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in recipes {
        for line in &n.lines {
            let Status::Counted {
                key: key @ FoodKey::Custom(_),
                ..
            } = &line.status
            else {
                continue;
            };
            let Some(Composition { name, source, .. }) = table.get(key) else {
                continue;
            };
            let text = match source {
                Some(s) => format!("{name} ({s})"),
                None => name.clone(),
            };
            if !out.contains(&text) {
                out.push(text);
            }
        }
    }
    out
}
