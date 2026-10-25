//! The HTTP API against a scratch copy of `tests/fixtures/library`.

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use grocery_core::library::Library;
use grocery_server::{api, App};
use grocery_store::Store;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

struct Fixture {
    _dir: tempfile::TempDir,
    app: Arc<App>,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/library"),
            dir.path(),
        );
        let app = App::new(Library::new(dir.path()), Store::open_in_memory().unwrap());
        app.sync(false).unwrap();
        Self { _dir: dir, app }
    }

    fn router(&self) -> Router {
        api::router(self.app.clone())
    }

    async fn call(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, String, String) {
        let mut req = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(v) => {
                req = req.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let resp = self
            .router()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let ctype = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_owned())
            .unwrap_or_default();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, ctype, String::from_utf8(bytes.to_vec()).unwrap())
    }

    async fn json(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let (status, _, text) = self.call(method, uri, body).await;
        let value = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
        };
        (status, value)
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_dir(&path, &target);
        } else {
            std::fs::copy(&path, &target).unwrap();
        }
    }
}

#[tokio::test]
async fn recipes_search_and_export() {
    let f = Fixture::new();

    let (status, all) = f.json(Method::GET, "/api/recipes", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(all.as_array().unwrap().len(), 3);

    let (_, found) = f.json(Method::GET, "/api/recipes?q=boerenkool", None).await;
    assert_eq!(found[0]["path"], "Stamppot boerenkool.cook");
    assert_eq!(found.as_array().unwrap().len(), 1);

    let (status, one) = f
        .json(Method::GET, "/api/recipes/Shakshuka.cook", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(one["servings"], 2.0);
    assert_eq!(one["tags"], json!(["eggs", "vegetarian"]));

    let (status, ctype, md) = f
        .call(
            Method::GET,
            "/api/recipes/Pasta%20arrabbiata.cook?format=md&servings=2",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ctype.starts_with("text/markdown"), "{ctype}");
    assert!(
        md.starts_with("# Pasta arrabbiata\n\nServes 2 (recipe: 4)"),
        "{md}"
    );
    assert!(md.contains("- spaghetti — 200 g\n"), "{md}");
    assert!(md.contains("## Method\n\n1. Bring a large pot"), "{md}");

    let (_, ctype, txt) = f
        .call(Method::GET, "/api/recipes/Shakshuka.cook?format=txt", None)
        .await;
    assert!(ctype.starts_with("text/plain"), "{ctype}");
    assert!(txt.contains("• passata — 500 g\n"), "{txt}");

    let (status, _) = f.json(Method::GET, "/api/recipes/Nope.cook", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Only indexed files can be read, whatever the path says.
    let (status, _) = f
        .json(
            Method::GET,
            "/api/recipes/..%2F..%2Fetc%2Fpasswd?format=txt",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = f
        .json(
            Method::GET,
            "/api/recipes/Shakshuka.cook?format=md&servings=0",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn broken_recipes_are_listed_with_their_error() {
    let f = Fixture::new();
    let recipes = f.app.library.recipes_dir();
    std::fs::write(recipes.join("Broken.cook"), "Add @&flour{1%g}.").unwrap();
    f.app.sync(false).unwrap();

    let (_, one) = f.json(Method::GET, "/api/recipes/Broken.cook", None).await;
    assert!(
        one["error"].as_str().unwrap().contains("Broken.cook"),
        "{one}"
    );
    let (status, err) = f
        .json(Method::GET, "/api/recipes/Broken.cook?format=md", None)
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(err["error"].is_string());

    let (_, st) = f.json(Method::GET, "/api/status", None).await;
    assert_eq!(st["recipes"], 4);
    assert_eq!(st["recipes_with_errors"], 1);
    assert_eq!(st["nevo"], Value::Null);
}

#[tokio::test]
async fn shopping_list_lifecycle() {
    let f = Fixture::new();

    // Bad servings are refused; an empty list is fine (see lists_start_empty_and_grow).
    let (status, _) = f
        .json(
            Method::POST,
            "/api/lists",
            Some(json!({ "recipes": [{ "path": "Pasta arrabbiata.cook", "servings": 0 }] })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, created) = f
        .json(
            Method::POST,
            "/api/lists",
            Some(json!({
                "title": "Week 42",
                "recipes": [
                    { "path": "Pasta arrabbiata.cook", "servings": 2 },
                    { "path": "Shakshuka.cook" }
                ]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_i64().unwrap();
    assert_eq!(created["title"], "Week 42");
    assert_eq!(created["status"], "open");
    assert_eq!(
        created["item_count"].as_u64().unwrap() as usize,
        created["items"].as_array().unwrap().len()
    );
    assert_eq!(created["recipes"][0]["path"], "Pasta arrabbiata.cook");
    assert_eq!(
        created["unmapped"],
        Value::Null,
        "nothing unmapped in the example"
    );
    let items = created["items"].as_array().unwrap();
    // Each recipe's own word for it: the fixtures mix English and Dutch.
    let garlic = items
        .iter()
        .find(|i| i["name"] == "garlic / knoflook")
        .unwrap();
    assert_eq!(garlic["quantity"], "4 cloves");
    assert_eq!(garlic["aisle"], "groente & fruit");

    // tick off garlic, add an item by hand
    let item = garlic["id"].as_i64().unwrap();
    let (status, _) = f
        .json(
            Method::PATCH,
            &format!("/api/lists/{id}/items/{item}"),
            Some(json!({ "checked": true })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, added) = f
        .json(
            Method::POST,
            &format!("/api/lists/{id}/items"),
            Some(json!({ "name": "toilet paper" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(added["id"].is_i64());

    let (_, ctype, md) = f
        .call(Method::GET, &format!("/api/lists/{id}?format=md"), None)
        .await;
    assert!(ctype.starts_with("text/markdown"));
    assert!(
        md.starts_with("# Week 42\n\nFor: Pasta arrabbiata (2 servings), Shakshuka (2 servings)\n"),
        "{md}"
    );
    assert!(md.contains("- [x] garlic / knoflook — 4 cloves\n"), "{md}");
    assert!(md.contains("## Other\n\n- [ ] toilet paper\n"), "{md}");

    let (_, _, txt) = f
        .call(Method::GET, &format!("/api/lists/{id}?format=txt"), None)
        .await;
    assert!(!txt.contains("garlic"), "ticked items are left out: {txt}");
    assert!(txt.contains("• toilet paper"), "{txt}");

    let (status, _) = f
        .json(
            Method::PATCH,
            &format!("/api/lists/{id}"),
            Some(json!({ "status": "done" })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = f
        .json(
            Method::PATCH,
            &format!("/api/lists/{id}"),
            Some(json!({ "status": "lost" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (_, all) = f.json(Method::GET, "/api/lists", None).await;
    assert_eq!(all[0]["status"], "done");
    assert_eq!(all[0]["checked_count"], 1);

    let (status, _) = f
        .json(Method::DELETE, &format!("/api/lists/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = f.json(Method::GET, &format!("/api/lists/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_list_is_a_snapshot() {
    let f = Fixture::new();
    let (_, created) = f
        .json(
            Method::POST,
            "/api/lists",
            Some(json!({ "recipes": [{ "path": "Shakshuka.cook" }] })),
        )
        .await;
    let id = created["id"].as_i64().unwrap();
    assert!(created["title"]
        .as_str()
        .unwrap()
        .starts_with("Shopping list 20"));

    // Editing the recipe afterwards doesn't change the list being shopped from.
    let path = f.app.library.recipes_dir().join("Shakshuka.cook");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("@egg{4}", "@egg{6}")).unwrap();
    f.app.sync(false).unwrap();

    let (_, list) = f.json(Method::GET, &format!("/api/lists/{id}"), None).await;
    let egg = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "egg")
        .unwrap();
    assert_eq!(egg["quantity"], "4");
}

#[tokio::test]
async fn nutrition_needs_nevo() {
    let f = Fixture::new();
    let (status, err) = f
        .json(Method::GET, "/api/nutrition/Shakshuka.cook", None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        err["error"].as_str().unwrap().contains("import-nevo"),
        "{err}"
    );
}

/// Read the next `data:` line from an SSE body, or panic after a second.
async fn next_event(body: &mut Body) -> Value {
    loop {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(1), body.frame())
            .await
            .expect("an event within a second")
            .expect("stream still open")
            .unwrap();
        let Ok(data) = frame.into_data() else {
            continue;
        };
        let text = String::from_utf8(data.to_vec()).unwrap();
        if let Some(json) = text.lines().find_map(|l| l.strip_prefix("data: ")) {
            return serde_json::from_str(json).unwrap();
        }
    }
}

#[tokio::test]
async fn changes_are_pushed_as_events() {
    let f = Fixture::new();
    let resp = f
        .router()
        .oneshot(Request::get("/api/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()[header::CONTENT_TYPE], "text/event-stream");
    let mut events = resp.into_body();

    let (_, created) = f
        .json(
            Method::POST,
            "/api/lists",
            Some(json!({ "recipes": [{ "path": "Shakshuka.cook" }] })),
        )
        .await;
    let id = created["id"].as_i64().unwrap();
    assert_eq!(
        next_event(&mut events).await,
        json!({ "type": "list", "id": id })
    );

    let item = created["items"][0]["id"].as_i64().unwrap();
    f.json(
        Method::PATCH,
        &format!("/api/lists/{id}/items/{item}"),
        Some(json!({ "checked": true })),
    )
    .await;
    assert_eq!(
        next_event(&mut events).await,
        json!({ "type": "list", "id": id })
    );

    std::fs::write(
        f.app.library.recipes_dir().join("New.cook"),
        "Boil @water{1%l}.",
    )
    .unwrap();
    f.app.sync(false).unwrap();
    assert_eq!(next_event(&mut events).await, json!({ "type": "recipes" }));
}

#[tokio::test]
async fn recipe_json_has_a_display_version() {
    let f = Fixture::new();
    let (_, r) = f
        .json(Method::GET, "/api/recipes/Shakshuka.cook?servings=4", None)
        .await;
    let display = &r["display"];
    assert_eq!(display["info"], "Serves 4 (recipe: 2) · eggs, vegetarian");
    assert!(display["ingredients"]
        .as_array()
        .unwrap()
        .contains(&json!("passata — 1000 g")));
    assert_eq!(display["sections"][0]["steps"][0]["type"], "step");
    assert!(display["sections"][0]["steps"][0]["text"]
        .as_str()
        .unwrap()
        .starts_with("Fry ui, knoflook"));

    // Steps carry their ingredients (scaled amounts) and timers.
    let step = &display["sections"][0]["steps"][1];
    assert_eq!(
        step["items"][0],
        json!({ "type": "text", "text": "Pour in " })
    );
    assert_eq!(
        step["items"][1],
        json!({ "type": "ingredient", "name": "passata", "amount": "1000 g" })
    );
    assert_eq!(step["items"][3]["type"], "timer");
    assert_eq!(
        step["items"][3]["seconds"],
        json!({ "min": 600.0, "max": 600.0 })
    );
    assert_eq!(step["seconds"], json!({ "min": 600.0, "max": 600.0 }));
    assert_eq!(display["time"]["stated"], json!(null));
    assert_eq!(display["time"]["timers"]["min"], 600.0);

    // The list endpoint stays light.
    let (_, all) = f.json(Method::GET, "/api/recipes", None).await;
    assert!(all[0].get("display").is_none());
}

#[tokio::test]
async fn unknown_api_routes_are_json_404s() {
    let f = Fixture::new();
    let (status, body) = f.json(Method::GET, "/api/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].is_string());
    // Whether or not web/dist is built, `/` answers with HTML.
    let (_, ctype, _) = f.call(Method::GET, "/", None).await;
    assert!(ctype.starts_with("text/html"), "{ctype}");
}

#[tokio::test]
async fn a_broken_custom_yaml_is_reported_not_ignored() {
    let f = Fixture::new();
    std::fs::write(
        f.app.library.root().join("Food/custom.yaml"),
        "foods:\n  tahini: { protien: 17 }\n",
    )
    .unwrap();
    let (status, err) = f
        .json(Method::GET, "/api/nutrition/Shakshuka.cook", None)
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        err["error"].as_str().unwrap().contains("custom.yaml"),
        "{err}"
    );
}

/// A fixture with the synthetic NEVO data from grocery-store's tests and a
/// recipe whose foods map onto it.
fn with_nevo() -> Fixture {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../grocery-store/tests/fixtures");
    let data = grocery_store::read_nevo(
        std::fs::File::open(fixtures.join("nevo_synthetic.csv")).unwrap(),
        Some(std::fs::File::open(fixtures.join("nutrients_synthetic.csv")).unwrap()),
    )
    .unwrap();
    let mut store = Store::open_in_memory().unwrap();
    store.import_nevo(&data).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("Recipes")).unwrap();
    std::fs::create_dir_all(root.join("Food")).unwrap();
    std::fs::write(
        root.join("Food/aliases.yaml"),
        "foods:\n  onion: { nevo: 1, piece_g: 100, synonyms: [ui] }\n  \
         kale: { nevo: 2, synonyms: [boerenkool] }\n  \
         olive oil: { nevo: 4, density: 0.9, synonyms: [olijfolie] }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("Recipes/Kale.cook"),
        "---\nservings: 2\n---\nMix @ui{2}, @boerenkool{400%g} and @olijfolie{2%el}.",
    )
    .unwrap();
    std::fs::write(root.join("Recipes/Loose.cook"), "Eat @ui{1}.").unwrap();
    let app = App::new(Library::new(root), store);
    app.sync(false).unwrap();
    Fixture { _dir: dir, app }
}

#[tokio::test]
async fn people_and_plans() {
    let f = with_nevo();
    let mut events = f
        .router()
        .oneshot(Request::get("/api/events").body(Body::empty()).unwrap())
        .await
        .unwrap()
        .into_body();

    let (status, me) = f
        .json(
            Method::POST,
            "/api/people",
            Some(json!({ "name": "Peter" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(next_event(&mut events).await, json!({ "type": "people" }));
    let (status, _) = f
        .json(
            Method::POST,
            "/api/people",
            Some(json!({ "name": "peter" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "names are unique");
    let me = me["id"].as_i64().unwrap();

    let add = |day: &str, recipe: &str, portions: f64| json!({ "person": me, "day": day, "meal": "dinner", "recipe": recipe, "portions": portions });
    let (status, entry) = f
        .json(
            Method::POST,
            "/api/plan",
            Some(add("2026-10-14", "Kale.cook", 1.0)),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(entry["title"], "Kale");
    assert_eq!(
        next_event(&mut events).await,
        json!({ "type": "plan", "person": me })
    );
    let (_, second) = f
        .json(
            Method::POST,
            "/api/plan",
            Some(add("2026-10-15", "Kale.cook", 1.0)),
        )
        .await;
    f.json(
        Method::POST,
        "/api/plan",
        Some(add("2026-10-15", "Loose.cook", 1.0)),
    )
    .await;

    // Mistakes are clear errors.
    let (status, _) = f
        .json(
            Method::POST,
            "/api/plan",
            Some(add("2026-10-15", "Nope.cook", 1.0)),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = f
        .json(
            Method::POST,
            "/api/plan",
            Some(add("2026-02-30", "Kale.cook", 1.0)),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = f
        .json(
            Method::POST,
            "/api/plan",
            Some(add("2026-10-15", "Kale.cook", 0.0)),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 1.5 portions, and no meal any more.
    let (status, changed) = f
        .json(
            Method::PATCH,
            &format!("/api/plan/{}", second["id"]),
            Some(json!({ "portions": 1.5, "meal": null })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (changed["portions"].as_f64(), &changed["meal"]),
        (Some(1.5), &Value::Null)
    );

    let (_, plan) = f
        .json(
            Method::GET,
            &format!("/api/plan?person={me}&from=2026-10-12&to=2026-10-18"),
            None,
        )
        .await;
    assert_eq!(plan.as_array().unwrap().len(), 3);

    let (status, n) = f
        .json(
            Method::GET,
            &format!("/api/plan/nutrition?person={me}&from=2026-10-14&to=2026-10-16"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{n}");
    assert!(n["citation"]
        .as_str()
        .unwrap()
        .contains("NEVO online version 2099/1.0"));
    assert_eq!(n["rows"][0]["code"], "ENERCC");
    let days = n["days"].as_array().unwrap();
    assert_eq!(days.len(), 3);
    // (200 g × 25 + 400 g × 30 + 27 g × 880) / 100 / 2 servings
    let per_portion = (50.0 + 120.0 + 237.6) / 2.0;
    let kcal = |d: usize| days[d]["nutrients"]["ENERCC"]["value"].as_f64().unwrap();
    assert!((kcal(0) - per_portion).abs() < 1e-9);
    assert!((kcal(1) - 1.5 * per_portion).abs() < 1e-9);
    assert_eq!(days[2]["nutrients"], json!({}));
    // Olive oil has no sodium value: salt is a lower bound, and says why.
    assert_eq!(days[0]["nutrients"]["SALT"]["lower_bound"], true);
    assert_eq!(days[0]["missing"][0]["food"], "Olie olijf-");
    // A recipe without servings can't be portioned: said, not guessed.
    let entry = |id: &Value| {
        days[1]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == *id)
            .unwrap()
            .clone()
    };
    let loose = entry(&json!(3));
    assert!(
        loose["problem"].as_str().unwrap().contains("serves"),
        "{loose}"
    );
    assert!((entry(&second["id"])["kcal"].as_f64().unwrap() - 1.5 * per_portion).abs() < 1e-9);

    let (status, _) = f
        .json(
            Method::GET,
            &format!("/api/plan/nutrition?person={me}&from=2026-10-14&to=2026-01-01"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Removing a person removes their plan.
    let (status, _) = f
        .json(Method::DELETE, &format!("/api/people/{me}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, plan) = f
        .json(
            Method::GET,
            &format!("/api/plan?person={me}&from=2026-10-12&to=2026-10-18"),
            None,
        )
        .await;
    assert_eq!(plan, json!([]));
}

#[tokio::test]
async fn recipes_can_be_created_and_edited() {
    let f = Fixture::new();
    let source = json!({
        "title": "Tomatensoep",
        "servings": 2,
        "time": "30 min",
        "tags": ["soep"],
        "extra": "author: Oma",
        "body": "Kook @tomaat{500%g} en @ui{1} met ~{20%minutes}.",
    });

    // Check before saving: parse result, and which ingredients are new.
    let (status, check) = f
        .json(
            Method::POST,
            "/api/recipe-check",
            Some(json!({ "source": source })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(check["error"], Value::Null);
    assert_eq!(check["steps"], 1);
    assert_eq!(check["timers_seconds"], 1200.0);
    let by_name = |n: &str| {
        check["ingredients"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["name"] == n)
            .unwrap()
            .clone()
    };
    assert_eq!(by_name("tomaat")["status"], "unknown");
    assert_eq!(by_name("tomaat")["amount"], "500 g");
    assert_eq!(by_name("ui")["status"], "known");
    assert_eq!(by_name("ui")["food"], "onion");
    assert_eq!(by_name("ui")["nutrition"], "nevo");
    assert_eq!(by_name("ui")["weight"], "ok");

    let (_, bad) = f
        .json(
            Method::POST,
            "/api/recipe-check",
            Some(json!({ "source": { "title": "x", "body": "Add @&flour{1%g}." } })),
        )
        .await;
    assert!(bad["error"].as_str().unwrap().contains("flour"), "{bad}");

    // Create in a new folder; it's indexed at once.
    let (status, saved) = f
        .json(
            Method::POST,
            "/api/recipe-files",
            Some(json!({ "folder": "Soep", "source": source })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{saved}");
    assert_eq!(saved["path"], "Soep/Tomatensoep.cook");
    let (_, found) = f
        .json(Method::GET, "/api/recipes?q=tomatensoep", None)
        .await;
    assert_eq!(found[0]["path"], "Soep/Tomatensoep.cook");
    let (_, folders) = f.json(Method::GET, "/api/recipe-folders", None).await;
    assert_eq!(folders, json!(["Soep"]));
    let on_disk =
        std::fs::read_to_string(f.app.library.recipes_dir().join("Soep/Tomatensoep.cook")).unwrap();
    assert!(
        on_disk.starts_with(
            "---\ntitle: Tomatensoep\nservings: 2\ntime: 30 min\ntags: [soep]\nauthor: Oma\n---\n"
        ),
        "{on_disk}"
    );

    // Never overwrites; mistakes are clear errors.
    let (status, _) = f
        .json(
            Method::POST,
            "/api/recipe-files",
            Some(json!({ "folder": "Soep", "source": source })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    for (folder, title) in [("../outside", "x"), ("", "../x"), (".git", "x")] {
        let (status, _) = f
            .json(
                Method::POST,
                "/api/recipe-files",
                Some(json!({ "folder": folder, "source": { "title": title, "body": "Cook." } })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{folder:?} {title:?}");
    }
    let (status, _) = f
        .json(
            Method::POST,
            "/api/recipe-files",
            Some(json!({ "source": { "title": "Broken", "body": "Add @&flour{1%g}." } })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!f.app.library.recipes_dir().join("Broken.cook").exists());

    // Read back as fields, edit, and a stale version is refused.
    let (status, got) = f
        .json(Method::GET, "/api/recipe-files/Soep/Tomatensoep.cook", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(got["source"]["tags"], json!(["soep"]));
    let version = got["version"].clone();
    let mut edited = got["source"].clone();
    edited["servings"] = json!(4);
    let (status, _) = f
        .json(
            Method::PUT,
            "/api/recipe-files/Soep/Tomatensoep.cook",
            Some(json!({ "source": edited, "version": version })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, shown) = f
        .json(Method::GET, "/api/recipes/Soep/Tomatensoep.cook", None)
        .await;
    assert_eq!(shown["servings"], 4.0);
    let (status, err) = f
        .json(
            Method::PUT,
            "/api/recipe-files/Soep/Tomatensoep.cook",
            Some(json!({ "source": edited, "version": version })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{err}");
    let (status, _) = f
        .json(
            Method::PUT,
            "/api/recipe-files/Nope.cook",
            Some(json!({ "source": edited, "version": "x" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn foods_can_be_added() {
    let f = with_nevo();
    let aliases = f.app.library.aliases_path();
    let before = std::fs::read_to_string(&aliases).unwrap();

    // NEVO search to find a code.
    let (status, hits) = f
        .json(Method::GET, "/api/nevo/search?q=boerenkool", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        hits.as_array().unwrap().iter().any(|h| h["code"] == 2),
        "{hits}"
    );

    let (status, _) = f
        .json(
            Method::POST,
            "/api/foods",
            Some(json!({
                "name": "tomaat", "aisle": "groente", "synonyms": ["tomaten"],
                "nevo": 1, "piece_g": 120
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let text = std::fs::read_to_string(&aliases).unwrap();
    assert!(
        text.starts_with(&before),
        "the rest of the file is untouched"
    );
    assert!(text.contains("  tomaat:\n    nevo: 1"), "{text}");
    assert!(text.contains("# Ui rauw") || text.contains("# "), "{text}");

    // Own values go to custom.yaml and are used from the food.
    let (status, added) = f
        .json(
            Method::POST,
            "/api/foods",
            Some(json!({
                "name": "tahini",
                "own": { "per": "100g", "source": "jar", "energy_kcal": 600, "protein": 17 },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{added}");
    assert_eq!(added["own_values"], true);
    let custom = std::fs::read_to_string(f.app.library.custom_path()).unwrap();
    assert!(custom.contains("tahini"), "{custom}");
    let (_, foods) = f.json(Method::GET, "/api/foods", None).await;
    let tahini = foods
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["name"] == "tahini")
        .unwrap();
    assert_eq!(tahini["custom"], "tahini");

    // Mistakes change nothing.
    let snapshot = (
        std::fs::read_to_string(&aliases).unwrap(),
        std::fs::read_to_string(f.app.library.custom_path()).unwrap(),
    );
    for body in [
        json!({ "name": "tomaten", "negligible": true }), // already known
        json!({ "name": "x", "nevo": 999999 }),           // not in NEVO
        json!({ "name": "y", "custom": "nope" }),         // not in custom.yaml
        json!({ "name": "z", "nevo": 1, "negligible": true }), // two sources
        json!({ "name": "w", "density": -1 }),            // nonsense number
        json!({ "name": "v", "own": { "energy_kcal": -5 } }), // bad own values
        json!({ "name": "u", "units": { "tl": 3 } }),     // fixed-size unit
    ] {
        let (status, err) = f.json(Method::POST, "/api/foods", Some(body.clone())).await;
        assert!(status.is_client_error(), "{body}: {status} {err}");
    }
    assert_eq!(
        snapshot,
        (
            std::fs::read_to_string(&aliases).unwrap(),
            std::fs::read_to_string(f.app.library.custom_path()).unwrap()
        )
    );
}

#[tokio::test]
async fn lists_start_empty_and_grow() {
    let f = Fixture::new();
    let (status, list) = f
        .json(
            Method::POST,
            "/api/lists",
            Some(json!({ "title": "Weekend" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{list}");
    assert_eq!(list["items"], json!([]));
    let id = list["id"].as_i64().unwrap();

    // A typed name that's a known food becomes that food, in its aisle.
    let (status, _) = f
        .json(
            Method::POST,
            &format!("/api/lists/{id}/items"),
            Some(json!({ "name": "melk" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, list) = f.json(Method::GET, &format!("/api/lists/{id}"), None).await;
    assert_eq!(list["items"][0]["name"], "melk");
    assert_eq!(list["items"][0]["aisle"], "zuivel & eieren");

    // Recipes merge in.
    let (status, list) = f
        .json(
            Method::POST,
            &format!("/api/lists/{id}/recipes"),
            Some(json!({ "recipes": [{ "path": "Pasta arrabbiata.cook", "servings": 2 }] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["recipes"][0]["title"], "Pasta arrabbiata");
    let names: Vec<_> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].clone())
        .collect();
    assert!(
        names.contains(&json!("spaghetti")) && names.contains(&json!("melk")),
        "{names:?}"
    );

    let (status, _) = f
        .json(
            Method::POST,
            &format!("/api/lists/{id}/recipes"),
            Some(json!({ "recipes": [] })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = f
        .json(
            Method::POST,
            "/api/lists/999/recipes",
            Some(json!({ "recipes": [{ "path": "Pasta arrabbiata.cook" }] })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn nutrition_of_one_food() {
    let f = with_nevo();
    // By a synonym; values per 100 g, with the citation.
    let (status, n) = f.json(Method::GET, "/api/foods/ui/nutrition", None).await;
    assert_eq!(status, StatusCode::OK, "{n}");
    assert_eq!(n["food"], "onion");
    assert_eq!(n["source"], "nevo");
    assert_eq!(n["per"], "100g");
    assert_eq!(n["piece_g"], 100.0);
    assert!(n["citation"].as_str().unwrap().contains("NEVO"));
    let row = |code: &str| {
        n["nutrients"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["code"] == code)
            .unwrap()
            .clone()
    };
    assert_eq!(row("ENERCC")["value"], 25.0);
    // Unknown is null, not 0.
    assert!(n["nutrients"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["value"].is_null()));

    let (status, _) = f
        .json(Method::GET, "/api/foods/nothing/nutrition", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn single_foods_in_plans() {
    let f = with_nevo();
    let (_, me) = f
        .json(
            Method::POST,
            "/api/people",
            Some(json!({ "name": "Peter" })),
        )
        .await;
    let me = me["id"].as_i64().unwrap();
    let food = |name: &str, amount: Value, unit: Value| json!({ "person": me, "day": "2026-10-14", "meal": "snack", "food": name, "amount": amount, "unit": unit });

    let (status, onion) = f
        .json(
            Method::POST,
            "/api/plan",
            Some(food("Ui", json!(1), Value::Null)),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{onion}");
    assert_eq!(
        (&onion["title"], &onion["path"], &onion["food"]["text"]),
        (&json!("Ui"), &json!(""), &json!("1"))
    );
    let (_, kale) = f
        .json(
            Method::POST,
            "/api/plan",
            Some(food("boerenkool", json!(50), json!("g"))),
        )
        .await;
    assert_eq!(kale["food"]["text"], "50 g");
    // Kale has no piece weight: kept, but not counted, and said why.
    let (_, piece) = f
        .json(
            Method::POST,
            "/api/plan",
            Some(food("boerenkool", json!(1), Value::Null)),
        )
        .await;

    // Only foods from aliases.yaml; one of recipe or food.
    let (status, _) = f
        .json(
            Method::POST,
            "/api/plan",
            Some(food("cashews", json!(25), json!("g"))),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = f
        .json(
            Method::POST,
            "/api/plan",
            Some(food("ui", json!(0), Value::Null)),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = f
        .json(
            Method::POST,
            "/api/plan",
            Some(json!({ "person": me, "day": "2026-10-14", "recipe": "Kale.cook", "food": "ui" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, changed) = f
        .json(
            Method::PATCH,
            &format!("/api/plan/{}", kale["id"]),
            Some(json!({ "amount": 100, "unit": "g" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["food"]["qty"], 100.0);

    let (status, n) = f
        .json(
            Method::GET,
            &format!("/api/plan/nutrition?person={me}&from=2026-10-14&to=2026-10-14"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{n}");
    let day = &n["days"][0];
    let entry = |id: &Value| {
        day["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == *id)
            .unwrap()
            .clone()
    };
    // 100 g of onion (one piece) at 25 kcal, 100 g of kale at 30.
    assert_eq!(entry(&onion["id"])["kcal"], 25.0);
    assert_eq!(entry(&kale["id"])["kcal"], 30.0);
    let piece = entry(&piece["id"]);
    assert!(
        piece["problem"].as_str().unwrap().contains("piece weight"),
        "{piece}"
    );
    assert_eq!(day["nutrients"]["ENERCC"]["value"], 55.0);
}

#[tokio::test]
async fn list_items_can_have_an_amount() {
    let f = Fixture::new();
    // From a plan: recipes and single foods together; the same food adds up.
    let (status, list) = f
        .json(
            Method::POST,
            "/api/lists",
            Some(json!({
                "recipes": [{ "path": "Pasta arrabbiata.cook", "servings": 4 }],
                "items": [{ "name": "spaghetti", "amount": 100, "unit": "g" },
                          { "name": "melk", "amount": 1, "unit": "l" }]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{list}");
    let item = |list: &Value, name: &str| {
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["name"] == name)
            .unwrap_or_else(|| panic!("no {name} in {list}"))
            .clone()
    };
    assert_eq!(item(&list, "spaghetti")["quantity"], "500 g");
    assert_eq!(item(&list, "melk")["quantity"], "1 l");
    let id = list["id"].as_i64().unwrap();

    let (status, _) = f
        .json(
            Method::POST,
            &format!("/api/lists/{id}/items"),
            Some(json!({ "name": "toilet paper", "amount": 2, "unit": "pak" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, list) = f.json(Method::GET, &format!("/api/lists/{id}"), None).await;
    assert_eq!(item(&list, "toilet paper")["quantity"], "2 pak");

    let (status, _) = f
        .json(
            Method::POST,
            &format!("/api/lists/{id}/items"),
            Some(json!({ "name": "kaas", "amount": -1 })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
