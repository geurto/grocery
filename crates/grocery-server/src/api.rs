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
//! POST   /api/lists                             {"title"?, "recipes": [{"path", "servings"?}]}
//! GET    /api/lists/{id}?format=                md: all items, ticked as [x]
//!                                               txt: only what's left to buy
//! PATCH  /api/lists/{id}                        {"title"?, "status"?: "open" | "done"}
//! DELETE /api/lists/{id}
//! POST   /api/lists/{id}/items                  {"name": "toilet paper"}
//! PATCH  /api/lists/{id}/items/{item}           {"checked": true}
//! DELETE /api/lists/{id}/items/{item}
//! ```
//!
//! `{path}` is the recipe's path relative to `Recipes/`, as listed by
//! `/api/recipes` ("Pasta arrabbiata.cook", "soups/Tomato soup.cook").
//!
//! `/api/events` sends `{"type": "list", "id": 3}` when a list changes (or is
//! created or deleted) and `{"type": "recipes"}` when the index changes; then
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
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use grocery_core::aggregate::{self, Selection};
use grocery_core::model::{Content, Recipe};
use grocery_core::nutrition::Options;
use grocery_core::render;
use grocery_store::{IndexedRecipe, ListStatus, ListSummary, StoredList};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};

use crate::{App, Event};

type Shared = Arc<App>;

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
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn not_found(what: impl std::fmt::Display) -> Self {
        Self::new(StatusCode::NOT_FOUND, format!("{what} not found"))
    }

    fn bad_request(message: impl Into<String>) -> Self {
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

type ApiResult<T> = Result<T, ApiError>;

/// Run store and file work off the async threads.
async fn blocking<T, F>(app: &Shared, f: F) -> ApiResult<T>
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
    /// "Serves 2 (recipe: 4) · pasta, quick"
    info: String,
    /// "spaghetti — 200 g"
    ingredients: Vec<String>,
    sections: Vec<SectionJson>,
}

#[derive(Serialize)]
struct SectionJson {
    name: Option<String>,
    steps: Vec<StepJson>,
}

#[derive(Serialize)]
#[serde(tag = "type", content = "text", rename_all = "lowercase")]
enum StepJson {
    Step(String),
    Note(String),
}

impl DisplayJson {
    fn new(recipe: &Recipe, servings: Option<f64>) -> Self {
        let view = render::RecipeView::new(recipe, servings);
        Self {
            info: view.info,
            ingredients: view.ingredients,
            sections: recipe
                .sections
                .iter()
                .map(|s| SectionJson {
                    name: s.name.clone(),
                    steps: s
                        .content
                        .iter()
                        .map(|c| match c {
                            Content::Step(t) => StepJson::Step(t.clone()),
                            Content::Note(t) => StepJson::Note(t.clone()),
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
fn load_recipe(app: &App, path: &str) -> ApiResult<(IndexedRecipe, Recipe)> {
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
        let report = app.store().nutrition_report(
            &recipe,
            &foods,
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
}

impl From<StoredList> for ListJson {
    fn from(l: StoredList) -> Self {
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
                    id: i.id,
                    quantity: i.item.quantity.as_ref().map(render::format_qty),
                    name: i.item.name,
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
    recipes: Vec<NewListRecipe>,
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
    if req.recipes.is_empty() {
        return Err(ApiError::bad_request("a list needs at least one recipe"));
    }
    for r in &req.recipes {
        check_servings(r.servings)?;
    }
    blocking(&app, move |app| {
        let mut recipes = Vec::new();
        for r in &req.recipes {
            recipes.push((load_recipe(app, &r.path)?.1, r.servings));
        }
        let selections: Vec<_> = recipes
            .iter()
            .map(|(recipe, servings)| Selection {
                recipe,
                servings: *servings,
            })
            .collect();
        let built = aggregate::build(&selections, &app.library.food_db()?, &app.library.pantry()?);

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
        let mut json = ListJson::from(stored);
        json.warnings = built.warnings;
        json.unmapped = built.unmapped;
        Ok((StatusCode::CREATED, Json(json)))
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
            Format::Json => Json(ListJson::from(stored)).into_response(),
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

#[derive(Deserialize)]
struct NewItem {
    name: String,
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
        match app.store().add_item(id, &req.name)? {
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
