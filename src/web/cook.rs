//! Page "Que cuisiner ?" (`/cook`): one-choice buttons for the tag groups of `choices`, other
//! tag filters, the stock, and the recipes that can be cooked with it.
//!
//! The state is in the query string (tag filters and repeated `have`). Choices and removals are
//! plain links to the resulting state; adding an ingredient is a form (`add`), applied when the
//! page is rendered: adding twice the same ingredient changes nothing, so a reload is harmless.

use askama::Template;
use axum::extract::{Query, State};
use axum::response::Response;

use super::flow::{Field, cook_fields, encode, hidden, parse, stock_fields};
use super::{AppState, Pairs, catalog_errors, render};
use crate::catalog::Catalog;
use crate::catalog::normalize::closest;
use crate::cook::suggest;
use crate::i18n::{MESSAGES, Messages, interpolate};
use crate::menu::{Criteria, Menu, TagFilter};

struct ChoiceButton {
    label: String,
    url: String,
    selected: bool,
}

struct TagChoice {
    name: String,
    required: bool,
    excluded: bool,
}

struct StockItem {
    name: String,
    remove_url: String,
}

struct CookRecipe {
    name: String,
    ingredients: String,
    missing: String,
    shop_url: String,
}

#[derive(Template)]
#[template(path = "cook.html")]
struct CookPage {
    t: &'static Messages,
    catalog_errors: Vec<String>,
    groups: Vec<Vec<ChoiceButton>>,
    other_tags: Vec<TagChoice>,
    other_open: bool,
    /// Hidden fields of the "other criteria" form: the stock and the group choices.
    other_fields: Vec<Field>,
    /// Hidden fields of the stock form: the whole state.
    state_fields: Vec<Field>,
    notices: Vec<String>,
    stock: Vec<StockItem>,
    all_ingredients: Vec<String>,
    focus_search: bool,
    feasible: Vec<CookRecipe>,
    near: Vec<CookRecipe>,
}

fn url(criteria: &Criteria, have: &[String]) -> String {
    format!("/cook?{}", encode(&cook_fields(criteria, have)))
}

pub(super) async fn page(State(state): State<AppState>, Query(pairs): Pairs) -> Response {
    let snapshot = state.live.snapshot();
    let catalog = &*snapshot.catalog;
    let query = parse(&pairs, catalog);
    let criteria = query.menu.criteria;
    let mut have: Vec<String> = Vec::new();
    for name in &query.have {
        if let Some(ingredient) = catalog.ingredient(name)
            && !ingredient.staple
            && !have.contains(&ingredient.name)
        {
            have.push(ingredient.name.clone());
        }
    }
    let notices = match &query.add {
        Some(typed) => add(catalog, typed.trim(), &mut have),
        None => Vec::new(),
    };

    let grouped: Vec<&String> = catalog.choices.iter().flatten().collect();
    let groups = catalog
        .choices
        .iter()
        .map(|group| choice_buttons(group, &criteria, &have))
        .collect();
    let other_tags: Vec<TagChoice> = catalog
        .tags
        .iter()
        .filter(|tag| !grouped.contains(tag))
        .map(|name| TagChoice {
            name: name.clone(),
            required: criteria.tags.get(name) == Some(&TagFilter::Required),
            excluded: criteria.tags.get(name) == Some(&TagFilter::Excluded),
        })
        .collect();
    let group_criteria = Criteria {
        tags: criteria
            .tags
            .iter()
            .filter(|(tag, _)| grouped.contains(tag))
            .map(|(tag, filter)| (tag.clone(), *filter))
            .collect(),
        ..Criteria::default()
    };
    let other_fields = cook_fields(&group_criteria, &have);

    let stock = have
        .iter()
        .map(|name| {
            let rest: Vec<String> = have.iter().filter(|n| *n != name).cloned().collect();
            StockItem {
                name: name.clone(),
                remove_url: url(&criteria, &rest),
            }
        })
        .collect();

    let found = suggest(catalog, &criteria, &have);
    let view = |suggestion: &crate::cook::Suggestion| {
        let recipe = suggestion.recipe;
        let missing = suggestion.missing.join(", ");
        let menu = Menu {
            criteria: Criteria {
                meals: 1,
                tags: criteria.tags.clone(),
            },
            proposed: vec![recipe.id.clone()],
            rejected: Vec::new(),
        };
        CookRecipe {
            name: recipe.name.clone(),
            ingredients: recipe.ingredients.join(", "),
            missing: interpolate(&MESSAGES.cook.missing, &[("ingredients", &missing)]),
            shop_url: format!(
                "/menu/accepted?{}",
                encode(&stock_fields(&menu, &have, true))
            ),
        }
    };
    let feasible = found.feasible.iter().map(view).collect();
    let near = found.near.iter().map(view).collect();

    render(&CookPage {
        t: &MESSAGES,
        catalog_errors: catalog_errors(&snapshot),
        groups,
        other_open: other_tags.iter().any(|tag| tag.required || tag.excluded),
        other_tags,
        other_fields: hidden(other_fields),
        state_fields: hidden(cook_fields(&criteria, &have)),
        notices,
        stock,
        all_ingredients: catalog
            .ingredients
            .iter()
            .filter(|ingredient| !ingredient.staple)
            .map(|ingredient| ingredient.name.clone())
            .collect(),
        focus_search: query.add.is_some(),
        feasible,
        near,
    })
}

/// One button per tag of the group, plus "Peu importe"; each one links to the state where it
/// is the group's only choice.
fn choice_buttons(group: &[String], criteria: &Criteria, have: &[String]) -> Vec<ChoiceButton> {
    let without_group = || {
        let mut criteria = criteria.clone();
        criteria.tags.retain(|tag, _| !group.contains(tag));
        criteria
    };
    let chosen = group
        .iter()
        .find(|tag| criteria.tags.get(*tag) == Some(&TagFilter::Required));
    let mut buttons: Vec<ChoiceButton> = group
        .iter()
        .map(|tag| {
            let mut next = without_group();
            next.tags.insert(tag.clone(), TagFilter::Required);
            ChoiceButton {
                label: tag.clone(),
                url: url(&next, have),
                selected: chosen == Some(tag),
            }
        })
        .collect();
    buttons.push(ChoiceButton {
        label: MESSAGES.cook.any.clone(),
        url: url(&without_group(), have),
        selected: chosen.is_none(),
    });
    buttons
}

/// Adds a typed ingredient to the stock; returns the notices to show.
fn add(catalog: &Catalog, typed: &str, have: &mut Vec<String>) -> Vec<String> {
    let m = &MESSAGES.cook;
    if typed.is_empty() {
        return Vec::new();
    }
    let Some(ingredient) = catalog.ingredient(typed) else {
        let names = catalog
            .ingredients
            .iter()
            .filter(|i| !i.staple)
            .map(|i| i.name.as_str());
        let notice = match closest(typed, names) {
            Some(name) => interpolate(&m.unknown_suggestion, &[("typed", &typed), ("name", &name)]),
            None => interpolate(&m.unknown, &[("typed", &typed)]),
        };
        return vec![notice];
    };
    if ingredient.staple {
        return vec![interpolate(&m.staple, &[("name", &ingredient.name)])];
    }
    if !have.contains(&ingredient.name) {
        have.push(ingredient.name.clone());
    }
    if ingredient.name == typed {
        Vec::new()
    } else {
        vec![interpolate(
            &m.mapped,
            &[("typed", &typed), ("name", &ingredient.name)],
        )]
    }
}

#[cfg(test)]
mod tests;
