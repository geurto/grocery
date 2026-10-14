//! The HTTP API against a scratch copy of `examples/library`.

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
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/library"),
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

    let (status, _) = f
        .json(Method::POST, "/api/lists", Some(json!({ "recipes": [] })))
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
    let garlic = items.iter().find(|i| i["name"] == "garlic").unwrap();
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
    assert!(md.contains("- [x] garlic — 4 cloves\n"), "{md}");
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
