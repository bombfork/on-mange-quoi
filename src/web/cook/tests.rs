use std::path::Path;

use axum::http::StatusCode;

use crate::catalog::{self, Catalog};
use crate::i18n::{MESSAGES, interpolate};
use crate::web::test_support::{body_text, get};

fn example() -> Catalog {
    catalog::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/catalog")).unwrap()
}

async fn page(uri: &str) -> String {
    let response = get(example(), uri).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    body_text(response).await
}

/// The rendered button of a choice: (whether it is selected, its link).
fn button(body: &str, label: &str) -> (bool, String) {
    let end = body
        .find(&format!(">{label}</a>"))
        .unwrap_or_else(|| panic!("no {label} button"));
    let start = body[..end].rfind("<a ").unwrap();
    let tag = &body[start..end];
    let href = tag
        .split("href=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    (!tag.contains("outline"), href.replace("&#38;", "&"))
}

#[tokio::test]
async fn choice_groups_are_one_choice_buttons() {
    let body = page("/cook").await;
    assert!(button(&body, &MESSAGES.cook.any).0);
    let (selected, href) = button(&body, "viande");
    assert!(!selected);
    assert_eq!(href, "/cook?tag%3Aviande=required");

    let body = page("/cook?tag:viande=required").await;
    assert!(button(&body, "viande").0);
    assert!(!button(&body, &MESSAGES.cook.any).0);
    // Choosing another tag of the group replaces the choice.
    assert_eq!(button(&body, "poisson").1, "/cook?tag%3Apoisson=required");
    // Tags outside the groups are other criteria.
    assert!(body.contains(r#"type="radio" name="tag:enfants""#));
    assert!(!body.contains(r#"type="radio" name="tag:viande""#));
    // The choice travels with the stock form.
    assert!(body.contains(r#"type="hidden" name="tag:viande" value="required""#));
}

#[tokio::test]
async fn adding_to_the_stock_resolves_names() {
    let body = page("/cook?add=Oeufs").await;
    assert!(body.contains(r#"name="have" value="œufs""#));
    assert!(body.contains(&interpolate(
        &MESSAGES.cook.mapped,
        &[("typed", &"Oeufs"), ("name", &"œufs")]
    )));
    assert!(body.contains(" autofocus"));

    let body = page("/cook?have=%C5%93ufs&add=sel").await;
    assert!(body.contains(&interpolate(&MESSAGES.cook.staple, &[("name", &"sel")])));
    assert!(!body.contains(r#"name="have" value="sel""#));

    let body = page("/cook?add=coquilettes").await;
    assert!(body.contains("n’est pas au catalogue, vouliez-vous dire"));
    assert!(!body.contains(r#"name="have""#));
}

#[tokio::test]
async fn feasible_and_near_recipes_are_shown_apart() {
    let body = page("/cook?have=coquillettes&have=jambon&have=beurre").await;
    let feasible = body.find(&MESSAGES.cook.feasible_heading).unwrap();
    let near = body.find(&MESSAGES.cook.near_heading).unwrap();
    let coquillettes = body.find("Coquillettes au jambon mouliné").unwrap();
    let croque = body.find("Croque-monsieur").unwrap();
    assert!(feasible < coquillettes && coquillettes < near && near < croque);
    assert!(body.contains("Il manque\u{a0}: pain de mie, emmental"));
    // The shopping link goes to the stock check of that recipe, with the stock ticked.
    assert!(
        body.contains(
            "/menu/accepted?meals=1&#38;proposed=croque-monsieur&#38;in_stock=coquillettes"
        )
    );
    // Feasible recipes need no shopping.
    let feasible_part = &body[feasible..near];
    assert!(!feasible_part.contains(&MESSAGES.cook.shop));

    let body = page("/cook?tag:viande=required&have=coquillettes&have=jambon&have=beurre").await;
    assert!(
        !body.contains("Omelette aux champignons"),
        "vegetarian recipe filtered out"
    );
}

#[tokio::test]
async fn removing_from_the_stock_is_a_link() {
    let body = page("/cook?have=coquillettes&have=jambon").await;
    assert!(body.contains(r#"href="/cook?have=jambon""#));
    assert!(body.contains(r#"href="/cook?have=coquillettes""#));
}

#[tokio::test]
async fn refine_offers_the_ingredient_tags_of_the_candidates() {
    let body = page("/cook?tag:viande=required").await;
    let refine = &body[body.find(&MESSAGES.cook.refine).unwrap()..];
    for tag in ["charcuterie", "viande blanche", "légumes"] {
        assert!(refine.contains(&format!(">{tag}</a>")), "{tag}");
    }
    assert_eq!(
        button(refine, "charcuterie").1,
        "/cook?tag%3Aviande=required&refine=charcuterie"
    );

    // A vegetarian choice offers no meat tag, and drops a meat refinement.
    let body = page("/cook?tag:v%C3%A9g%C3%A9tarien=required&refine=charcuterie").await;
    let refine = &body[body.find(&MESSAGES.cook.refine).unwrap()..];
    assert!(!refine.contains(">charcuterie</a>"));
    assert!(button(refine, &MESSAGES.cook.any).0);
}

#[tokio::test]
async fn a_chosen_ingredient_tag_narrows_recipes_and_offers_its_ingredients() {
    let body = page(
        "/cook?tag:viande=required&refine=charcuterie&have=coquillettes&have=jambon&have=beurre",
    )
    .await;
    assert!(button(&body, "charcuterie").0);
    assert!(body.contains("Coquillettes au jambon mouliné"));
    assert!(!body.contains("Poulet rôti"), "no charcuterie in it");
    // One click adds lardons; jambon is already in stock.
    assert!(body.contains(">+ lardons</a>"));
    assert!(!body.contains(">+ jambon</a>"));
    assert!(body.contains("have=beurre&#38;have=lardons"));
}

#[tokio::test]
async fn without_a_tag_the_most_used_ingredients_are_offered() {
    let body = page("/cook").await;
    let quick = &body[body.find(&MESSAGES.cook.quick).unwrap()..];
    // beurre is in three recipes, more than any other non-staple ingredient.
    let first = quick.find(">+ ").unwrap();
    assert!(quick[first..].starts_with(">+ beurre</a>"));
    assert!(!quick.contains(">+ sel</a>"), "no staples");
}
