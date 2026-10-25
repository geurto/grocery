//! HTTP routes. JSON unless `?format=md` (Markdown) or `?format=txt` (plain
//! text) is given on an export route.
//!
//! ```text
//! GET    /api/status
//! GET    /api/events                            server-sent events, see below
//! POST   /api/reindex                           re-parse every recipe
//! GET    /api/recipes?q=boerenkool              search title, tags, ingredients
//! GET    /api/recipes/{path}?format=&servings=  one recipe; md/txt export
//! GET    /api/nutrition/{path}?all=&optional=   per-serving report (text)
//! GET    /api/lists                             newest first
//! POST   /api/lists                             {"title"?, "recipes"?: [{"path", "servings"?}],
//!                                                "items"?: [{"name", "amount"?, "unit"?}]}
//!                                               (nothing: an empty list to fill as you go)
//! GET    /api/lists/{id}?format=                md: all items, ticked as [x]
//!                                               txt: only what's left to buy
//! PATCH  /api/lists/{id}                        {"title"?, "status"?: "open" | "done"}
//! DELETE /api/lists/{id}
//! POST   /api/lists/{id}/items                  {"name": "limoen", "amount"?: 2, "unit"?}; a known
//!                                               food gets its aisle and joins its line
//! POST   /api/lists/{id}/recipes                {"recipes": [{"path", "servings"?}]}: merged in
//! DELETE /api/lists/{id}/recipes/{path}         its share comes off every line
//! PATCH  /api/lists/{id}/items/{item}           {"checked": true}
//! DELETE /api/lists/{id}/items/{item}
//! POST   /api/recipe-check                      {"source"} → parse result, ingredients vs aliases.yaml
//! GET    /api/recipe-folders                    folders under Recipes/
//! GET    /api/recipe-files/{path}               {"source", "version"}: a recipe as editor fields
//! POST   /api/recipe-files                      {"folder"?, "source"} → 201 {"path"}; 409 if it exists
//! PUT    /api/recipe-files/{path}               {"source", "version"}; 409 if the file changed
//! GET    /api/foods                             what aliases.yaml knows
//! GET    /api/foods/{name}/nutrition            per 100 g (or ml), with its source
//! POST   /api/foods                             {"name", "aisle"?, "synonyms"?, "nevo"? | "custom"? |
//!                                                "own"? | "negligible"?, "piece_g"?, "density"?, "units"?}
//! GET    /api/aisles
//! GET    /api/nevo/search?q=&limit=             NEVO foods by name (409 until imported)
//! GET    /api/people
//! POST   /api/people                            {"name": "Sam"}
//! PATCH  /api/people/{id}                       {"name": "Sam"}
//! DELETE /api/people/{id}                       and their plan
//! GET    /api/plan?person=&from=&to=            entries, days as YYYY-MM-DD (inclusive)
//! POST   /api/plan                              {"person", "day", "recipe", "meal"?, "portions"?}
//!                                               or {"person", "day", "food", "amount"?, "unit"?, "meal"?}
//! PATCH  /api/plan/{id}                         {"day"?, "meal"?: "dinner" | null, "portions"?}
//!                                               (a food: {"amount", "unit"?})
//! DELETE /api/plan/{id}
//! GET    /api/plan/nutrition?person=&from=&to=  per-day totals, see `plans`
//! ```
//!
//! `{path}` is the recipe's path relative to `Recipes/`, as listed by
//! `/api/recipes` ("Pasta arrabbiata.cook", "soups/Tomato soup.cook").
//!
//! `/api/events` sends `{"type": "list", "id": 3}` when a list changes (or is
//! created or deleted), `{"type": "plan", "person": 1}` when someone's plan
//! changes, `{"type": "people"}` when people are added, renamed or removed,
//! and `{"type": "recipes"}` when the index changes; then
//! fetch what you show again. `{"type": "resync"}` means events were missed:
//! refetch everything. Also refetch after reconnecting.

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::sse::{self, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use grocery_core::aggregate::{self, PartSource, Selection};
use grocery_core::food::FoodDb;
use grocery_core::model::{self, Content, Recipe, Step, StepItem};
use grocery_core::nutrition::Options;
use grocery_core::render;
use grocery_store::{IndexedRecipe, ListStatus, ListSummary, StoredList};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};

use crate::{edit, plans, App, Event};

pub(crate) type Shared = Arc<App>;

pub fn router(app: Shared) -> Router {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/events", get(events))
        .route("/api/reindex", post(reindex))
        .route("/api/recipes", get(search_recipes))
        .route("/api/recipes/{*path}", get(recipe))
        .route("/api/nutrition/{*path}", get(nutrition))
        .route("/api/lists", get(lists).post(create_list))
        .route(
            "/api/lists/{id}",
            get(list).patch(update_list).delete(delete_list),
        )
        .route("/api/lists/{id}/items", post(add_item))
        .route("/api/lists/{id}/recipes", post(add_recipes))
        .route("/api/lists/{id}/recipes/{*path}", delete(remove_recipe))
        .route("/api/recipe-check", post(edit::check))
        .route("/api/recipe-folders", get(edit::folders))
        .route("/api/recipe-files", post(edit::create_recipe))
        .route(
            "/api/recipe-files/{*path}",
            get(edit::get_source).put(edit::update_recipe),
        )
        .route("/api/foods", get(edit::foods).post(edit::add_food))
        .route("/api/foods/{name}/nutrition", get(edit::food_nutrition))
        .route("/api/aisles", get(edit::aisles))
        .route("/api/nevo/search", get(edit::nevo_search))
        .route("/api/people", get(plans::people).post(plans::add_person))
        .route(
            "/api/people/{id}",
            patch(plans::rename_person).delete(plans::delete_person),
        )
        .route("/api/plan", get(plans::plan).post(plans::add_entry))
        .route("/api/plan/nutrition", get(plans::nutrition))
        .route(
            "/api/plan/{id}",
            patch(plans::update_entry).delete(plans::delete_entry),
        )
        .route(
            "/api/lists/{id}/items/{item}",
            patch(update_item).delete(delete_item),
        )
        .fallback(crate::web::static_file)
        .with_state(app)
}

// ---------------------------------------------------------------- errors

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    pub(crate) message: String,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub(crate) fn not_found(what: impl std::fmt::Display) -> Self {
        Self::new(StatusCode::NOT_FOUND, format!("{what} not found"))
    }

    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

impl From<grocery_store::Error> for ApiError {
    fn from(e: grocery_store::Error) -> Self {
        let status = match &e {
            grocery_store::Error::NotImported => StatusCode::CONFLICT,
            grocery_store::Error::Invalid(_) => StatusCode::BAD_REQUEST,
            grocery_store::Error::Library(inner) => return inner_error(inner),
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, e.to_string())
    }
}

impl From<grocery_core::Error> for ApiError {
    fn from(e: grocery_core::Error) -> Self {
        inner_error(&e)
    }
}

fn inner_error(e: &grocery_core::Error) -> ApiError {
    use grocery_core::Error::*;
    let status = match e {
        // A recipe that doesn't parse, or aliases/pantry YAML with a mistake:
        // the request was fine, the files need fixing.
        Parse { .. } | Yaml { .. } | Config { .. } => StatusCode::UNPROCESSABLE_ENTITY,
        Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, e.to_string())
}

pub(crate) type ApiResult<T> = Result<T, ApiError>;

/// Run store and file work off the async threads.
pub(crate) async fn blocking<T, F>(app: &Shared, f: F) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce(&App) -> ApiResult<T> + Send + 'static,
{
    let app = app.clone();
    tokio::task::spawn_blocking(move || f(&app))
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

// ---------------------------------------------------------------- formats

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Format {
    #[default]
    Json,
    #[serde(alias = "markdown")]
    Md,
    #[serde(alias = "text", alias = "plain")]
    Txt,
}

fn markdown(body: String) -> Response {
    (
        [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
        body,
    )
        .into_response()
}

fn text(body: String) -> Response {
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

fn check_servings(servings: Option<f64>) -> ApiResult<()> {
    match servings {
        Some(s) if !(s > 0.0 && s.is_finite()) => Err(ApiError::bad_request(format!(
            "servings must be a positive number, not {s}"
        ))),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------- status

#[derive(Serialize)]
struct Status {
    library: PathBuf,
    recipes: usize,
    recipes_with_errors: usize,
    nevo: Option<NevoStatus>,
}

#[derive(Serialize)]
struct NevoStatus {
    version: String,
    foods: u32,
    citation: String,
}

async fn status(State(app): State<Shared>) -> ApiResult<Json<Status>> {
    blocking(&app, |app| {
        let store = app.store();
        let recipes = store.recipes(None)?;
        Ok(Json(Status {
            library: app.library.root().to_path_buf(),
            recipes: recipes.len(),
            recipes_with_errors: recipes.iter().filter(|r| r.error.is_some()).count(),
            nevo: store.nevo_info()?.map(|i| NevoStatus {
                citation: i.citation(),
                version: i.version,
                foods: i.foods,
            }),
        }))
    })
    .await
}

async fn events(
    State(app): State<Shared>,
) -> (
    [(&'static str, &'static str); 1],
    Sse<impl Stream<Item = Result<sse::Event, Infallible>>>,
) {
    let stream = BroadcastStream::new(app.subscribe()).map(|msg| {
        let data = match msg {
            Ok(event) => serde_json::to_string(&event).expect("events serialize"),
            Err(BroadcastStreamRecvError::Lagged(_)) => r#"{"type":"resync"}"#.to_owned(),
        };
        Ok(sse::Event::default().data(data))
    });
    (
        // Tell nginx and friends not to buffer the stream.
        [("x-accel-buffering", "no")],
        // Comments every 20 s keep proxies and phones from dropping an idle
        // connection, and let the client notice a dead one.
        Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(20))),
    )
}

#[derive(Serialize)]
struct Reindexed {
    added: usize,
    updated: usize,
    removed: usize,
    unchanged: usize,
    failed: usize,
}

async fn reindex(State(app): State<Shared>) -> ApiResult<Json<Reindexed>> {
    blocking(&app, |app| {
        let r = app.sync(true)?;
        Ok(Json(Reindexed {
            added: r.added,
            updated: r.updated,
            removed: r.removed,
            unchanged: r.unchanged,
            failed: r.failed,
        }))
    })
    .await
}

// ---------------------------------------------------------------- recipes

#[derive(Serialize)]
struct RecipeJson {
    path: String,
    title: String,
    servings: Option<f64>,
    tags: Vec<String>,
    description: Option<String>,
    ingredients: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Only on `/api/recipes/{path}`: the recipe ready to show, scaled to
    /// `?servings=` if given.
    #[serde(skip_serializing_if = "Option::is_none")]
    display: Option<DisplayJson>,
}

#[derive(Serialize)]
struct DisplayJson {
    /// "Serves 2 (recipe: 4) · 40 min · pasta, quick"
    info: String,
    /// "spaghetti — 200 g"
    ingredients: Vec<String>,
    time: TimeJson,
    sections: Vec<SectionJson>,
}

#[derive(Serialize)]
struct TimeJson {
    /// What the recipe's metadata says: "40 min", "prep 10 min, cook 30 min".
    stated: Option<String>,
    stated_minutes: Option<f64>,
    /// Sum of all timers, in seconds: a lower bound on the time it takes.
    timers: Option<SecondsJson>,
}

#[derive(Serialize)]
struct SecondsJson {
    min: f64,
    max: f64,
}

impl From<model::Duration> for SecondsJson {
    fn from(d: model::Duration) -> Self {
        Self {
            min: d.min,
            max: d.max,
        }
    }
}

#[derive(Serialize)]
struct SectionJson {
    name: Option<String>,
    steps: Vec<StepJson>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum StepJson {
    Step {
        /// The step as plain text.
        text: String,
        items: Vec<StepItemJson>,
        /// Sum of this step's timers.
        #[serde(skip_serializing_if = "Option::is_none")]
        seconds: Option<SecondsJson>,
    },
    Note {
        text: String,
    },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum StepItemJson {
    Text {
        text: String,
    },
    /// With the amount for this step, scaled.
    Ingredient {
        name: String,
        amount: Option<String>,
    },
    Cookware {
        name: String,
    },
    Timer {
        name: Option<String>,
        text: String,
        seconds: Option<SecondsJson>,
    },
    Quantity {
        text: String,
    },
}

impl DisplayJson {
    fn new(recipe: &Recipe, servings: Option<f64>) -> Self {
        let view = render::RecipeView::new(recipe, servings);
        let factor = view.factor;
        let step = |step: &Step| StepJson::Step {
            text: step.text(),
            seconds: step.duration().map(Into::into),
            items: step
                .items
                .iter()
                .map(|item| match item {
                    StepItem::Text(t) => StepItemJson::Text { text: t.clone() },
                    StepItem::Ingredient { name, amount } => StepItemJson::Ingredient {
                        name: name.clone(),
                        amount: render::amount_text(amount, factor),
                    },
                    StepItem::Cookware(name) => StepItemJson::Cookware { name: name.clone() },
                    StepItem::Timer {
                        name,
                        text,
                        duration,
                    } => StepItemJson::Timer {
                        name: name.clone(),
                        text: text.clone(),
                        seconds: duration.map(Into::into),
                    },
                    StepItem::Quantity(t) => StepItemJson::Quantity { text: t.clone() },
                })
                .collect(),
        };
        Self {
            info: view.info,
            ingredients: view.ingredients,
            time: TimeJson {
                stated: recipe.time.as_ref().map(render::time_text),
                stated_minutes: recipe.time.and_then(|t| t.minutes()),
                timers: recipe.timers_total().map(Into::into),
            },
            sections: recipe
                .sections
                .iter()
                .map(|s| SectionJson {
                    name: s.name.clone(),
                    steps: s
                        .content
                        .iter()
                        .map(|c| match c {
                            Content::Step(s) => step(s),
                            Content::Note(t) => StepJson::Note { text: t.clone() },
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

impl From<IndexedRecipe> for RecipeJson {
    fn from(r: IndexedRecipe) -> Self {
        Self {
            path: r.path,
            title: r.title,
            servings: r.servings,
            tags: r.tags,
            description: r.description,
            ingredients: r.ingredients,
            warnings: r.warnings,
            error: r.error,
            display: None,
        }
    }
}

#[derive(Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

async fn search_recipes(
    State(app): State<Shared>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<Vec<RecipeJson>>> {
    blocking(&app, move |app| {
        let found = app.store().recipes(query.q.as_deref())?;
        Ok(Json(found.into_iter().map(Into::into).collect()))
    })
    .await
}

/// Parse a recipe by its index path. Going through the index means only
/// files under `Recipes/` can be read, whatever the URL says.
pub(crate) fn load_recipe(app: &App, path: &str) -> ApiResult<(IndexedRecipe, Recipe)> {
    let entry = app
        .store()
        .recipe(path)?
        .ok_or_else(|| ApiError::not_found(format!("recipe '{path}'")))?;
    if let Some(error) = &entry.error {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            error.clone(),
        ));
    }
    let mut recipe = app
        .library
        .parser
        .parse_file(&app.library.recipes_dir().join(path))?;
    // Lists record where a recipe came from; keep that relative.
    recipe.path = PathBuf::from(path);
    Ok((entry, recipe))
}

#[derive(Deserialize)]
struct RecipeQuery {
    #[serde(default)]
    format: Format,
    servings: Option<f64>,
}

async fn recipe(
    State(app): State<Shared>,
    Path(path): Path<String>,
    Query(query): Query<RecipeQuery>,
) -> ApiResult<Response> {
    check_servings(query.servings)?;
    blocking(&app, move |app| {
        if query.format == Format::Json {
            let entry = app
                .store()
                .recipe(&path)?
                .ok_or_else(|| ApiError::not_found(format!("recipe '{path}'")))?;
            if entry.error.is_some() {
                // Still useful: the error says what to fix.
                return Ok(Json(RecipeJson::from(entry)).into_response());
            }
            let (entry, recipe) = load_recipe(app, &path)?;
            let mut json = RecipeJson::from(entry);
            json.display = Some(DisplayJson::new(&recipe, query.servings));
            return Ok(Json(json).into_response());
        }
        let (_, recipe) = load_recipe(app, &path)?;
        Ok(match query.format {
            Format::Md => markdown(render::recipe_markdown(&recipe, query.servings)),
            _ => text(render::recipe_plain(&recipe, query.servings)),
        })
    })
    .await
}

#[derive(Deserialize)]
struct NutritionQuery {
    #[serde(default)]
    all: bool,
    /// Count optional ingredients too.
    #[serde(default)]
    optional: bool,
}

async fn nutrition(
    State(app): State<Shared>,
    Path(path): Path<String>,
    Query(query): Query<NutritionQuery>,
) -> ApiResult<Response> {
    blocking(&app, move |app| {
        let (_, recipe) = load_recipe(app, &path)?;
        let foods = app.library.food_db()?;
        let custom = app.library.custom_foods()?;
        let report = app.store().nutrition_report(
            &recipe,
            &foods,
            &custom,
            Options {
                include_optional: query.optional,
            },
            query.all,
        )?;
        Ok(text(report))
    })
    .await
}

// ---------------------------------------------------------------- lists

#[derive(Serialize)]
struct ListSummaryJson {
    id: i64,
    title: String,
    created: String,
    status: &'static str,
    item_count: usize,
    checked_count: usize,
}

impl From<ListSummary> for ListSummaryJson {
    fn from(s: ListSummary) -> Self {
        Self {
            id: s.id,
            title: s.title,
            created: s.created,
            status: s.status.as_str(),
            item_count: s.items,
            checked_count: s.checked,
        }
    }
}

#[derive(Serialize)]
struct ListJson {
    #[serde(flatten)]
    summary: ListSummaryJson,
    recipes: Vec<SourceJson>,
    items: Vec<ItemJson>,
    pantry: Vec<String>,
    /// Only when the list was just created: problems worth fixing in the
    /// recipes or `aliases.yaml`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unmapped: Vec<String>,
}

#[derive(Serialize)]
struct SourceJson {
    path: String,
    title: String,
    servings: Option<f64>,
}

#[derive(Serialize)]
struct ItemJson {
    id: i64,
    name: String,
    aisle: Option<String>,
    /// Formatted for display: "200 g", "3 cloves".
    quantity: Option<String>,
    notes: Vec<String>,
    optional: bool,
    recipes: Vec<String>,
    checked: bool,
    /// Who needs how much, in the line's unit where it converts. Empty for
    /// lines saved before this was recorded.
    parts: Vec<PartJson>,
}

#[derive(Serialize)]
struct PartJson {
    /// The recipe's title; `null` for what was added by hand.
    recipe: Option<String>,
    quantity: Option<String>,
}

impl ListJson {
    fn new(l: StoredList, db: &FoodDb) -> Self {
        Self {
            summary: l.summary.into(),
            recipes: l
                .sources
                .into_iter()
                .map(|s| SourceJson {
                    path: s.path.to_string_lossy().into_owned(),
                    title: s.title,
                    servings: s.servings,
                })
                .collect(),
            items: l
                .items
                .into_iter()
                .map(|i| ItemJson {
                    parts: aggregate::breakdown(&i.item, db)
                        .into_iter()
                        .map(|p| PartJson {
                            recipe: match p.source {
                                PartSource::Recipe { title, .. } => Some(title),
                                PartSource::Hand => None,
                                PartSource::Earlier { titles } if titles.is_empty() => None,
                                PartSource::Earlier { titles } => Some(titles.join(", ")),
                            },
                            quantity: p.quantity.as_ref().map(render::format_qty),
                        })
                        .collect(),
                    id: i.id,
                    quantity: i.item.quantity.as_ref().map(render::format_qty),
                    name: i.item.shown(),
                    aisle: i.item.aisle,
                    notes: i.item.notes,
                    optional: i.item.optional,
                    recipes: i.item.recipes,
                    checked: i.item.checked,
                })
                .collect(),
            pantry: l.pantry_skipped,
            warnings: vec![],
            unmapped: vec![],
        }
    }
}

async fn lists(State(app): State<Shared>) -> ApiResult<Json<Vec<ListSummaryJson>>> {
    blocking(&app, |app| {
        Ok(Json(
            app.store().lists()?.into_iter().map(Into::into).collect(),
        ))
    })
    .await
}

#[derive(Deserialize)]
struct NewList {
    title: Option<String>,
    /// May be empty: a list to fill as you go.
    #[serde(default)]
    recipes: Vec<NewListRecipe>,
    /// Single foods (from a plan), merged with the recipes' items.
    #[serde(default)]
    items: Vec<NewItem>,
}

#[derive(Deserialize)]
struct NewListRecipe {
    path: String,
    servings: Option<f64>,
}

async fn create_list(
    State(app): State<Shared>,
    Json(req): Json<NewList>,
) -> ApiResult<(StatusCode, Json<ListJson>)> {
    for r in &req.recipes {
        check_servings(r.servings)?;
    }
    blocking(&app, move |app| {
        let mut built = build_from(app, &req.recipes)?;
        if !req.items.is_empty() {
            let db = app.library.food_db()?;
            let mut extra = Vec::new();
            for i in &req.items {
                if i.name.trim().is_empty() || i.name.contains('\n') {
                    return Err(ApiError::bad_request(
                        "item name must be one non-empty line",
                    ));
                }
                extra.push(aggregate::hand_amount(&i.name, i.quantity()?, &db));
            }
            aggregate::merge_into(&mut built.items, extra, &db);
            aggregate::sort_items(&mut built.items, db.aisle_order());
        }

        let now = chrono::Local::now();
        let title = req
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Shopping list {}", now.format("%Y-%m-%d")));
        let created = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();

        let mut store = app.store();
        let id = store.create_list(&title, &created, &built)?;
        app.notify(Event::List { id });
        let stored = store
            .list(id)?
            .ok_or_else(|| ApiError::not_found(format!("list {id}")))?;
        let mut json = ListJson::new(stored, &app.library.food_db()?);
        json.warnings = built.warnings;
        json.unmapped = built.unmapped;
        Ok((StatusCode::CREATED, Json(json)))
    })
    .await
}

/// Load and aggregate the requested recipes.
fn build_from(app: &App, requested: &[NewListRecipe]) -> ApiResult<aggregate::ShoppingList> {
    let mut recipes = Vec::new();
    for r in requested {
        recipes.push((load_recipe(app, &r.path)?.1, r.servings));
    }
    let selections: Vec<_> = recipes
        .iter()
        .map(|(recipe, servings)| Selection {
            recipe,
            servings: *servings,
        })
        .collect();
    Ok(aggregate::build(
        &selections,
        &app.library.food_db()?,
        &app.library.pantry()?,
    ))
}

#[derive(Deserialize)]
struct AddRecipes {
    recipes: Vec<NewListRecipe>,
}

/// Add recipes to a list that already exists; their items merge into the
/// lines already there.
async fn add_recipes(
    State(app): State<Shared>,
    Path(id): Path<i64>,
    Json(req): Json<AddRecipes>,
) -> ApiResult<Json<ListJson>> {
    if req.recipes.is_empty() {
        return Err(ApiError::bad_request("pick at least one recipe"));
    }
    for r in &req.recipes {
        check_servings(r.servings)?;
    }
    blocking(&app, move |app| {
        let built = build_from(app, &req.recipes)?;
        let db = app.library.food_db()?;
        let mut store = app.store();
        if !store.add_recipes(id, &built, &db)? {
            return Err(ApiError::not_found(format!("list {id}")));
        }
        app.notify(Event::List { id });
        let stored = store
            .list(id)?
            .ok_or_else(|| ApiError::not_found(format!("list {id}")))?;
        let mut json = ListJson::new(stored, &app.library.food_db()?);
        json.warnings = built.warnings;
        json.unmapped = built.unmapped;
        Ok(Json(json))
    })
    .await
}

/// Take a recipe off a list: its share comes off every line it's on.
async fn remove_recipe(
    State(app): State<Shared>,
    Path((id, path)): Path<(i64, String)>,
) -> ApiResult<Json<ListJson>> {
    blocking(&app, move |app| {
        let db = app.library.food_db()?;
        let list = app
            .store()
            .list(id)?
            .ok_or_else(|| ApiError::not_found(format!("list {id}")))?;
        let source = list
            .sources
            .iter()
            .find(|s| s.path.to_string_lossy() == path)
            .ok_or_else(|| ApiError::not_found(format!("recipe '{path}' on list {id}")))?;
        // Lines saved before parts were recorded only have a total: what
        // the recipe adds now, as many times as it was put on the list, is
        // what comes off. A recipe that's gone or broken takes nothing off
        // those.
        let legacy = match load_recipe(app, &path) {
            Ok((_, recipe)) => {
                let mut items = aggregate::build(
                    &[Selection {
                        recipe: &recipe,
                        servings: None,
                    }],
                    &db,
                    &app.library.pantry()?,
                )
                .items;
                for q in items.iter_mut().filter_map(|i| i.quantity.as_mut()) {
                    q.value *= source.factor;
                }
                items
            }
            Err(_) => vec![],
        };
        let mut store = app.store();
        if !store.remove_recipe(id, &path, &legacy, &db)? {
            return Err(ApiError::not_found(format!("recipe '{path}' on list {id}")));
        }
        app.notify(Event::List { id });
        let stored = store
            .list(id)?
            .ok_or_else(|| ApiError::not_found(format!("list {id}")))?;
        Ok(Json(ListJson::new(stored, &db)))
    })
    .await
}

#[derive(Deserialize)]
struct FormatQuery {
    #[serde(default)]
    format: Format,
}

async fn list(
    State(app): State<Shared>,
    Path(id): Path<i64>,
    Query(query): Query<FormatQuery>,
) -> ApiResult<Response> {
    blocking(&app, move |app| {
        let stored = app
            .store()
            .list(id)?
            .ok_or_else(|| ApiError::not_found(format!("list {id}")))?;
        Ok(match query.format {
            Format::Json => Json(ListJson::new(stored, &app.library.food_db()?)).into_response(),
            Format::Md => markdown(render::markdown(
                &stored.shopping_list(),
                &stored.summary.title,
            )),
            Format::Txt => text(render::plain(&stored.shopping_list())),
        })
    })
    .await
}

#[derive(Deserialize)]
struct ListUpdate {
    title: Option<String>,
    status: Option<String>,
}

async fn update_list(
    State(app): State<Shared>,
    Path(id): Path<i64>,
    Json(req): Json<ListUpdate>,
) -> ApiResult<StatusCode> {
    let status = req
        .status
        .as_deref()
        .map(|s| {
            ListStatus::parse(s).ok_or_else(|| {
                ApiError::bad_request(format!("status must be 'open' or 'done', not '{s}'"))
            })
        })
        .transpose()?;
    let title = req.title.map(|t| t.trim().to_owned());
    if title.as_deref() == Some("") {
        return Err(ApiError::bad_request("title can't be empty"));
    }
    blocking(&app, move |app| {
        if app.store().update_list(id, title.as_deref(), status)? {
            app.notify(Event::List { id });
            Ok(StatusCode::NO_CONTENT)
        } else {
            Err(ApiError::not_found(format!("list {id}")))
        }
    })
    .await
}

async fn delete_list(State(app): State<Shared>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    blocking(&app, move |app| {
        if app.store().delete_list(id)? {
            app.notify(Event::List { id });
            Ok(StatusCode::NO_CONTENT)
        } else {
            Err(ApiError::not_found(format!("list {id}")))
        }
    })
    .await
}

/// An item typed in: "kaas", optionally with an amount ("500" "g").
#[derive(Deserialize)]
struct NewItem {
    name: String,
    amount: Option<f64>,
    /// With `amount`; missing = pieces.
    unit: Option<String>,
}

impl NewItem {
    fn quantity(&self) -> ApiResult<Option<aggregate::Qty>> {
        let Some(qty) = self.amount else {
            return Ok(None);
        };
        if !(qty.is_finite() && qty > 0.0 && qty <= 100_000.0) {
            return Err(ApiError::bad_request(
                "an amount must be more than 0 (and at most 100000)",
            ));
        }
        let unit = self
            .unit
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty());
        if unit.is_some_and(|u| u.contains('\n') || u.chars().count() > 20) {
            return Err(ApiError::bad_request(
                "a unit is one word of at most 20 characters",
            ));
        }
        Ok(Some(aggregate::Qty::of(qty, unit)))
    }
}

#[derive(Serialize)]
struct Created {
    id: i64,
}

async fn add_item(
    State(app): State<Shared>,
    Path(id): Path<i64>,
    Json(req): Json<NewItem>,
) -> ApiResult<(StatusCode, Json<Created>)> {
    blocking(&app, move |app| {
        let db = app.library.food_db()?;
        match app.store().add_item(id, &req.name, req.quantity()?, &db)? {
            Some(item) => {
                app.notify(Event::List { id });
                Ok((StatusCode::CREATED, Json(Created { id: item })))
            }
            None => Err(ApiError::not_found(format!("list {id}"))),
        }
    })
    .await
}

#[derive(Deserialize)]
struct ItemUpdate {
    checked: bool,
}

async fn update_item(
    State(app): State<Shared>,
    Path((id, item)): Path<(i64, i64)>,
    Json(req): Json<ItemUpdate>,
) -> ApiResult<StatusCode> {
    blocking(&app, move |app| {
        if app.store().set_checked(id, item, req.checked)? {
            app.notify(Event::List { id });
            Ok(StatusCode::NO_CONTENT)
        } else {
            Err(ApiError::not_found(format!("item {item} on list {id}")))
        }
    })
    .await
}

async fn delete_item(
    State(app): State<Shared>,
    Path((id, item)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    blocking(&app, move |app| {
        if app.store().delete_item(id, item)? {
            app.notify(Event::List { id });
            Ok(StatusCode::NO_CONTENT)
        } else {
            Err(ApiError::not_found(format!("item {item} on list {id}")))
        }
    })
    .await
}
