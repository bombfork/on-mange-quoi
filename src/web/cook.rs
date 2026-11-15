//! Page "Que cuisiner ?" (`/cook`): one-choice buttons for the tag groups of `choices`, other
//! tag filters, an "affiner" step on ingredient tags, the stock with one-click additions, and
//! the recipes that can be cooked with it.
//!
//! The state is in the query string (tag filters, `refine`, repeated `have`). Choices,
//! one-click additions and removals are plain links to the resulting state; typing an
//! ingredient is a form (`add`), applied when the page is rendered: adding twice the same
//! ingredient changes nothing, so a reload is harmless.

use askama::Template;
use axum::extract::{Query, State};
use axum::response::Response;

use super::flow::{Field, cook_fields, encode, hidden, parse, stock_fields};
use super::{AppState, Pairs, render};
use crate::catalog::normalize::closest;
use crate::catalog::{Catalog, IngredientTag};
use crate::cook::{Suggestion, quick_additions, refinements, suggest};
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

/// An ingredient with the link that removes it from, or adds it to, the stock.
struct StockLink {
    name: String,
    url: String,
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
    refine_buttons: Vec<ChoiceButton>,
    other_tags: Vec<TagChoice>,
    other_open: bool,
    /// Hidden fields of the "other criteria" form: the rest of the state.
    other_fields: Vec<Field>,
    /// Hidden fields of the stock form: the whole state.
    state_fields: Vec<Field>,
    notices: Vec<String>,
    quick: Vec<StockLink>,
    stock: Vec<StockLink>,
    all_ingredients: Vec<String>,
    focus_search: bool,
    feasible: Vec<CookRecipe>,
    near: Vec<CookRecipe>,
}

/// The state of the page, to build the links to the next states.
#[derive(Clone)]
struct CookState {
    criteria: Criteria,
    refine: Option<String>,
    have: Vec<String>,
}

impl CookState {
    fn fields(&self) -> Vec<(String, String)> {
        cook_fields(&self.criteria, self.refine.as_deref(), &self.have)
    }

    fn url(&self) -> String {
        format!("/cook?{}", encode(&self.fields()))
    }

    fn with(&self, change: impl FnOnce(&mut CookState)) -> String {
        let mut next = self.clone();
        change(&mut next);
        next.url()
    }
}

pub(super) async fn page(State(state): State<AppState>, Query(pairs): Pairs) -> Response {
    let snapshot = state.live.snapshot();
    let catalog = &*snapshot.catalog;
    let query = parse(&pairs, catalog);
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
    let criteria = query.menu.criteria;

    // The chosen ingredient tag only counts while it is offered.
    let offered = refinements(catalog, &criteria);
    let refine: Option<&IngredientTag> = query
        .refine
        .as_deref()
        .and_then(|name| offered.iter().copied().find(|tag| tag.name == name));
    let current = CookState {
        criteria: criteria.clone(),
        refine: refine.map(|tag| tag.name.clone()),
        have: have.clone(),
    };

    let grouped: Vec<&String> = catalog.choices.iter().flatten().collect();
    let groups = catalog
        .choices
        .iter()
        .map(|group| choice_buttons(group, &current))
        .collect();
    let mut refine_buttons: Vec<ChoiceButton> = offered
        .iter()
        .map(|tag| ChoiceButton {
            label: tag.name.clone(),
            url: current.with(|next| next.refine = Some(tag.name.clone())),
            selected: refine.is_some_and(|chosen| chosen.name == tag.name),
        })
        .collect();
    if !refine_buttons.is_empty() {
        refine_buttons.push(ChoiceButton {
            label: MESSAGES.cook.any.clone(),
            url: current.with(|next| next.refine = None),
            selected: refine.is_none(),
        });
    }
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
    let without_other_tags = CookState {
        criteria: Criteria {
            tags: criteria
                .tags
                .iter()
                .filter(|(tag, _)| grouped.contains(tag))
                .map(|(tag, filter)| (tag.clone(), *filter))
                .collect(),
            ..Criteria::default()
        },
        ..current.clone()
    };

    let quick = quick_additions(catalog, &criteria, refine, &have)
        .into_iter()
        .map(|name| StockLink {
            name: name.to_string(),
            url: current.with(|next| next.have.push(name.to_string())),
        })
        .collect();
    let stock = have
        .iter()
        .map(|name| StockLink {
            name: name.clone(),
            url: current.with(|next| next.have.retain(|n| n != name)),
        })
        .collect();

    let found = suggest(catalog, &criteria, refine, &have);
    let view = |suggestion: &Suggestion| recipe_view(suggestion, &criteria, &have);

    render(&CookPage {
        t: &MESSAGES,
        catalog_errors: state.catalog_errors(&snapshot),
        groups,
        refine_buttons,
        other_open: other_tags.iter().any(|tag| tag.required || tag.excluded),
        other_tags,
        other_fields: hidden(without_other_tags.fields()),
        state_fields: hidden(current.fields()),
        notices,
        quick,
        stock,
        all_ingredients: catalog
            .ingredients
            .iter()
            .filter(|ingredient| !ingredient.staple)
            .map(|ingredient| ingredient.name.clone())
            .collect(),
        focus_search: query.add.is_some(),
        feasible: found.feasible.iter().map(view).collect(),
        near: found.near.iter().map(view).collect(),
    })
}

fn recipe_view(suggestion: &Suggestion, criteria: &Criteria, have: &[String]) -> CookRecipe {
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
            encode(&stock_fields(&menu, have, true))
        ),
    }
}

/// One button per tag of the group, plus "Peu importe"; each one links to the state where it
/// is the group's only choice.
fn choice_buttons(group: &[String], current: &CookState) -> Vec<ChoiceButton> {
    let tags = &current.criteria.tags;
    let chosen = group
        .iter()
        .find(|tag| tags.get(*tag) == Some(&TagFilter::Required));
    let choose = |tag: Option<&String>| {
        current.with(|next| {
            next.criteria.tags.retain(|name, _| !group.contains(name));
            if let Some(tag) = tag {
                next.criteria.tags.insert(tag.clone(), TagFilter::Required);
            }
        })
    };
    let mut buttons: Vec<ChoiceButton> = group
        .iter()
        .map(|tag| ChoiceButton {
            label: tag.clone(),
            url: choose(Some(tag)),
            selected: chosen == Some(tag),
        })
        .collect();
    buttons.push(ChoiceButton {
        label: MESSAGES.cook.any.clone(),
        url: choose(None),
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
