use std::fs;
use std::path::Path as FilePath;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use tempfile::TempDir;

use crate::catalog::{self, RECIPES_DIR};
use crate::i18n::MESSAGES;
use crate::live::LiveCatalog;
use crate::web::test_support::{body_text, get_live, post_form, send};

/// A copy of the example catalog, served live.
fn served_copy() -> (TempDir, Arc<LiveCatalog>) {
    let source = FilePath::new(env!("CARGO_MANIFEST_DIR")).join("examples/catalog");
    let dir = TempDir::new().unwrap();
    for name in ["ingredients.yml", "tags.yml"] {
        fs::copy(source.join(name), dir.path().join(name)).unwrap();
    }
    fs::create_dir(dir.path().join(RECIPES_DIR)).unwrap();
    for entry in fs::read_dir(source.join(RECIPES_DIR)).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(
            &path,
            dir.path().join(RECIPES_DIR).join(path.file_name().unwrap()),
        )
        .unwrap();
    }
    let catalog = catalog::load(dir.path()).unwrap();
    let live = Arc::new(LiveCatalog::new(dir.path().to_path_buf(), catalog));
    (dir, live)
}

async fn page(live: &Arc<LiveCatalog>, uri: &str) -> String {
    let response = get_live(Arc::clone(live), uri).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    body_text(response).await
}

fn location(response: &Response) -> &str {
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    response.headers()[header::LOCATION].to_str().unwrap()
}

/// The version token of the first form of a page.
fn version(body: &str) -> String {
    let start = body
        .find(r#"name="version" value=""#)
        .expect("version field")
        + 22;
    body[start..].split('"').next().unwrap().to_string()
}

fn form(fields: &[(&str, &str)]) -> String {
    serde_urlencoded::to_string(fields).unwrap()
}

#[tokio::test]
async fn lists_recipes_with_links_to_edit_them() {
    let (_dir, live) = served_copy();
    let body = page(&live, "/catalog/recipes").await;
    assert!(body.contains(r#"<a href="/catalog/recipes/ratatouille">Ratatouille</a>"#));
    assert!(body.contains(&MESSAGES.editor.new_recipe));
}

#[tokio::test]
async fn creates_a_recipe_and_serves_it_at_once() {
    let (dir, live) = served_copy();
    let body = page(&live, "/catalog/recipes/new").await;
    assert!(body.contains(r#"<option value="pain de mie">"#));

    let response = post_form(
        Arc::clone(&live),
        "/catalog/recipes/new",
        &form(&[
            ("version", ""),
            ("name", "Jambon beurre"),
            ("ingredient", "pain de mie"),
            ("ingredient", "jambon"),
            ("tag", "rapide"),
            ("action", "save"),
        ]),
    )
    .await;
    assert_eq!(location(&response), "/catalog/recipes?notice=created");
    assert!(dir.path().join("recipes/jambon-beurre.yml").exists());
    assert_eq!(live.snapshot().catalog.recipes.len(), 7);

    let body = page(&live, "/catalog/recipes?notice=created").await;
    assert!(body.contains(&MESSAGES.editor.created));
}

#[tokio::test]
async fn invalid_recipe_shows_the_problems_and_keeps_the_input() {
    let (_dir, live) = served_copy();
    let response = post_form(
        Arc::clone(&live),
        "/catalog/recipes/new",
        &form(&[("name", "Ratatouille"), ("ingredient", "courgettes")]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains(&MESSAGES.editor.invalid));
    assert!(body.contains("nom de recette en double"));
    assert!(body.contains(r#"name="ingredient" value="courgettes" />"#));
}

/// Posts an action of the new recipe form and returns the page shown again.
async fn form_action(live: &Arc<LiveCatalog>, fields: &[(&str, &str)]) -> String {
    let response = post_form(Arc::clone(live), "/catalog/recipes/new", &form(fields)).await;
    assert_eq!(response.status(), StatusCode::OK);
    body_text(response).await
}

fn selected(body: &str, name: &str) -> bool {
    body.contains(&format!(r#"name="ingredient" value="{name}" />"#))
}

#[tokio::test]
async fn searching_adds_catalog_ingredients_without_saving() {
    let (dir, live) = served_copy();
    let body = form_action(
        &live,
        &[
            ("name", "Tartine"),
            ("add", "pain de mie"),
            ("action", "add"),
        ],
    )
    .await;
    assert!(selected(&body, "pain de mie"));
    assert!(body.contains(r#"value="Tartine""#), "the name is kept");
    assert!(body.contains(" autofocus"), "ready for the next ingredient");
    assert!(
        !page(&live, "/catalog/recipes/new")
            .await
            .contains(" autofocus")
    );
    assert!(!dir.path().join("recipes/tartine.yml").exists());

    // A spelling variant is added under its catalog name.
    let body = form_action(
        &live,
        &[
            ("ingredient", "pain de mie"),
            ("add", "Oeufs"),
            ("action", "add"),
        ],
    )
    .await;
    assert!(selected(&body, "pain de mie") && selected(&body, "œufs"));
    assert!(body.contains("ajouté sous la forme"));
}

#[tokio::test]
async fn unknown_ingredients_are_new_and_typos_can_be_replaced() {
    let (_dir, live) = served_copy();
    let body = form_action(&live, &[("add", "cannelle"), ("action", "add")]).await;
    assert!(selected(&body, "cannelle"));
    assert!(body.contains(&MESSAGES.editor.new_marker));
    assert!(body.contains(&MESSAGES.editor.new_help));

    let body = form_action(
        &live,
        &[
            ("ingredient", "cannelle"),
            ("add", "coquilettes"),
            ("action", "add"),
        ],
    )
    .await;
    assert!(body.contains(r#"name="replace" value="1""#));
    assert!(body.contains("Remplacer par «\u{a0}coquillettes\u{a0}»"));

    let body = form_action(
        &live,
        &[
            ("ingredient", "cannelle"),
            ("ingredient", "coquilettes"),
            ("replace", "1"),
        ],
    )
    .await;
    assert!(selected(&body, "coquillettes") && !selected(&body, "coquilettes"));

    let body = form_action(
        &live,
        &[
            ("ingredient", "cannelle"),
            ("ingredient", "coquillettes"),
            ("remove", "cannelle"),
        ],
    )
    .await;
    assert!(!selected(&body, "cannelle") && selected(&body, "coquillettes"));
}

#[tokio::test]
async fn saving_a_recipe_creates_its_new_ingredients() {
    let (dir, live) = served_copy();
    let response = post_form(
        Arc::clone(&live),
        "/catalog/recipes/new",
        &form(&[
            ("name", "Pain perdu"),
            ("ingredient", "pain"),
            ("ingredient", "œufs"),
            ("ingredient", "lait"),
            ("action", "save"),
        ]),
    )
    .await;
    assert_eq!(location(&response), "/catalog/recipes?notice=created");
    let ingredients = fs::read_to_string(dir.path().join("ingredients.yml")).unwrap();
    assert!(ingredients.contains("  - pain\n"));
    assert!(
        live.snapshot()
            .catalog
            .ingredients
            .iter()
            .any(|i| i.name == "pain")
    );
}

#[tokio::test]
async fn edits_a_recipe_and_refuses_stale_versions() {
    let (dir, live) = served_copy();
    let body = page(&live, "/catalog/recipes/ratatouille").await;
    assert!(body.contains(r#"name="ingredient" value="courgettes" />"#));
    let shown = version(&body);

    let edit = |name: &'static str, version: String| {
        let live = Arc::clone(&live);
        async move {
            post_form(
                live,
                "/catalog/recipes/ratatouille",
                &form(&[
                    ("version", &version),
                    ("name", name),
                    ("ingredient", "courgettes"),
                ]),
            )
            .await
        }
    };
    let response = edit("Ratatouille express", shown.clone()).await;
    assert_eq!(location(&response), "/catalog/recipes?notice=updated");
    let file = fs::read_to_string(dir.path().join("recipes/ratatouille.yml")).unwrap();
    assert!(file.contains("name: Ratatouille express"));

    let body = body_text(edit("Autre nom", shown).await).await;
    assert!(body.contains(&MESSAGES.editor.conflict));
}

#[tokio::test]
async fn deletes_a_recipe_after_confirmation() {
    let (dir, live) = served_copy();
    let body = page(&live, "/catalog/recipes/ratatouille/delete").await;
    let response = post_form(
        Arc::clone(&live),
        "/catalog/recipes/ratatouille/delete",
        &form(&[("version", &version(&body))]),
    )
    .await;
    assert_eq!(location(&response), "/catalog/recipes?notice=deleted");
    assert!(!dir.path().join("recipes/ratatouille.yml").exists());
}

#[tokio::test]
async fn unknown_recipe_is_not_found() {
    let (_dir, live) = served_copy();
    let response = get_live(live, "/catalog/recipes/inconnue").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn adds_ingredients_and_reports_duplicates() {
    let (_dir, live) = served_copy();
    let body = page(&live, "/catalog/ingredients").await;
    let response = post_form(
        Arc::clone(&live),
        "/catalog/ingredients/new",
        &form(&[("version", &version(&body)), ("name", "pain")]),
    )
    .await;
    assert_eq!(location(&response), "/catalog/ingredients?notice=created");

    let body = page(&live, "/catalog/ingredients").await;
    let response = post_form(
        Arc::clone(&live),
        "/catalog/ingredients/new",
        &form(&[("version", &version(&body)), ("name", "Pates")]),
    )
    .await;
    let body = body_text(response).await;
    assert!(body.contains("ingrédient en double"));
    assert!(body.contains(r#"value="Pates""#));
}

#[tokio::test]
async fn renaming_a_used_ingredient_asks_for_confirmation() {
    let (dir, live) = served_copy();
    let body = page(&live, "/catalog/ingredients/edit?name=jambon").await;
    assert!(body.contains("Croque-monsieur"), "used by is shown");
    let fields = [
        ("version", version(&body)),
        ("name", "jambon".to_string()),
        ("new_name", "jambon blanc".to_string()),
    ];
    let fields: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let response = post_form(
        Arc::clone(&live),
        "/catalog/ingredients/edit",
        &form(&fields),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains("Coquillettes au jambon mouliné") && body.contains("Croque-monsieur"));
    assert!(body.contains(r#"name="confirm" value="1""#));

    let mut confirmed = fields.clone();
    confirmed.push(("confirm", "1"));
    let response = post_form(
        Arc::clone(&live),
        "/catalog/ingredients/edit",
        &form(&confirmed),
    )
    .await;
    assert_eq!(location(&response), "/catalog/ingredients?notice=updated");
    let croque = fs::read_to_string(dir.path().join("recipes/croque-monsieur.yml")).unwrap();
    assert!(croque.contains("- jambon blanc"));
}

#[tokio::test]
async fn a_used_ingredient_cannot_be_deleted() {
    let (_dir, live) = served_copy();
    let body = page(&live, "/catalog/ingredients/edit?name=jambon").await;
    let response = post_form(
        Arc::clone(&live),
        "/catalog/ingredients/delete",
        &form(&[("version", &version(&body)), ("name", "jambon")]),
    )
    .await;
    let body = body_text(response).await;
    assert!(body.contains("Suppression impossible"));
}

#[tokio::test]
async fn tags_can_be_added_and_deleted() {
    let (_dir, live) = served_copy();
    let body = page(&live, "/catalog/tags").await;
    let response = post_form(
        Arc::clone(&live),
        "/catalog/tags/new",
        &form(&[("version", &version(&body)), ("name", "hiver")]),
    )
    .await;
    assert_eq!(location(&response), "/catalog/tags?notice=created");
    let body = page(&live, "/catalog/tags/edit?name=hiver").await;
    let response = post_form(
        Arc::clone(&live),
        "/catalog/tags/delete",
        &form(&[("version", &version(&body)), ("name", "hiver")]),
    )
    .await;
    assert_eq!(location(&response), "/catalog/tags?notice=deleted");
    assert!(!live.snapshot().catalog.tags.contains(&"hiver".to_string()));
}

#[tokio::test]
async fn posts_from_other_sites_are_refused() {
    let (dir, live) = served_copy();
    let request = |headers: &[(&str, &str)]| {
        let mut builder = Request::post("/catalog/tags/new")
            .header("content-type", "application/x-www-form-urlencoded");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(Body::from("name=pirate")).unwrap()
    };
    let cross_origin = request(&[
        ("host", "omq.local:8080"),
        ("origin", "http://evil.example"),
    ]);
    let cross_site = request(&[("sec-fetch-site", "cross-site")]);
    for request in [cross_origin, cross_site] {
        let response = send(Arc::clone(&live), request).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    assert!(
        !fs::read_to_string(dir.path().join("tags.yml"))
            .unwrap()
            .contains("pirate")
    );

    let same_origin = request(&[
        ("host", "omq.local:8080"),
        ("origin", "http://omq.local:8080"),
    ]);
    // Behind a reverse proxy that rewrites Host, the browser's verdict wins.
    let proxied = request(&[
        ("host", "127.0.0.1:8080"),
        ("origin", "https://omq.example"),
        ("sec-fetch-site", "same-origin"),
    ]);
    for request in [same_origin, proxied] {
        let response = send(Arc::clone(&live), request).await;
        assert_ne!(response.status(), StatusCode::FORBIDDEN);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn read_only_catalog_disables_the_editor() {
    use std::os::unix::fs::PermissionsExt;

    let (dir, live) = served_copy();
    let set_mode = |mode| {
        for sub in [dir.path().to_path_buf(), dir.path().join(RECIPES_DIR)] {
            fs::set_permissions(sub, fs::Permissions::from_mode(mode)).unwrap();
        }
    };
    set_mode(0o555);
    let probe = dir.path().join(".probe");
    let root = fs::write(&probe, "").is_ok();
    let body = page(&live, "/catalog/recipes").await;
    set_mode(0o755);
    let _ = fs::remove_file(probe);
    // Permissions do not apply to root: nothing to check then.
    if root {
        return;
    }
    assert!(body.contains(&MESSAGES.editor.read_only));
    assert!(!body.contains(&MESSAGES.editor.new_recipe));
}

#[tokio::test]
async fn import_page_explains_the_format() {
    let (_dir, live) = served_copy();
    let body = page(&live, "/catalog/import").await;
    assert!(body.contains(r#"<textarea name="yaml""#));
    assert!(body.contains("name: Pain perdu"), "example shown");
}

#[tokio::test]
async fn import_adds_recipes_and_reports_spelling_changes() {
    let (dir, live) = served_copy();
    let yaml = "ingredients:\r\n  - pain\r\n  - Oeufs\r\n---\r\nrecipe:\r\n  name: Pain perdu\r\n  ingredients: [pain, œufs, lait]\r\n";
    let response = post_form(
        Arc::clone(&live),
        "/catalog/import",
        &form(&[("yaml", yaml)]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains("Import enregistré (recettes\u{a0}: 1, nouveaux ingrédients\u{a0}: 1)"));
    assert!(body.contains("«\u{a0}Oeufs\u{a0}» importé sous la forme «\u{a0}œufs\u{a0}»"));
    assert!(dir.path().join("recipes/pain-perdu.yml").exists());
    assert_eq!(live.snapshot().catalog.recipes.len(), 7);
}

#[tokio::test]
async fn refused_import_keeps_the_text_and_locates_errors() {
    let (dir, live) = served_copy();
    let yaml = "recipe:\n  name: Pain perdu\n  ingredients:\n    - pain\n";
    let response = post_form(
        Arc::clone(&live),
        "/catalog/import",
        &form(&[("yaml", yaml)]),
    )
    .await;
    let body = body_text(response).await;
    assert!(body.contains(&MESSAGES.editor.import_invalid));
    assert!(body.contains("Ligne 4\u{a0}: ingrédient inconnu\u{a0}: «\u{a0}pain\u{a0}»"));
    assert!(
        body.contains("    - pain\n</textarea>"),
        "text kept as typed"
    );
    assert!(!dir.path().join("recipes/pain-perdu.yml").exists());
}

#[tokio::test]
async fn ingredient_tags_are_edited_on_the_ingredient_page() {
    let (_dir, live) = served_copy();
    let body = page(&live, "/catalog/ingredients/edit?name=poulet").await;
    assert!(body.contains(r#"name="ingredient_tag" value="viande blanche" checked"#));
    assert!(body.contains(r#"name="ingredient_tag" value="charcuterie" />"#));

    let response = post_form(
        Arc::clone(&live),
        "/catalog/ingredients/edit",
        &form(&[
            ("version", &version(&body)),
            ("name", "poulet"),
            ("new_name", "poulet"),
            ("ingredient_tag", "viande blanche"),
            ("new_tag", "volaille"),
        ]),
    )
    .await;
    assert_eq!(location(&response), "/catalog/ingredients?notice=updated");
    let tags: Vec<String> = live
        .snapshot()
        .catalog
        .ingredient_tags
        .iter()
        .filter(|tag| tag.ingredients.iter().any(|i| i == "poulet"))
        .map(|tag| tag.name.clone())
        .collect();
    assert_eq!(tags, ["viande blanche", "volaille"]);

    let body = page(&live, "/catalog/ingredients").await;
    assert!(body.contains("<mark>viande blanche, volaille</mark>"));
}
