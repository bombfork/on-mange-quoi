//! Pages of the catalog editor: recipes, ingredients and tags.
//!
//! Pages show the catalog as it is on disk. Changes are POST forms; a successful change reloads
//! the served catalog and redirects (303) to the list page with a notice.

use std::path::Path as FilePath;

use askama::Template;
use axum::extract::{Form, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};

use super::flow::{Field, encode};
use super::{AppState, Pairs, catalog_errors, render};
use crate::catalog::{Catalog, INGREDIENTS_FILE, TAGS_FILE};
use crate::editor::{EditError, RecipeInput, recipe_path, recipes_using};
use crate::i18n::{MESSAGES, Messages, interpolate};
use crate::report;

type FormPairs = Form<Vec<(String, String)>>;

fn first(pairs: &[(String, String)], key: &str) -> String {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, value)| value.trim().to_string())
        .unwrap_or_default()
}

fn all(pairs: &[(String, String)], key: &str) -> Vec<String> {
    pairs
        .iter()
        .filter(|(k, _)| k == key)
        .map(|(_, value)| value.clone())
        .collect()
}

fn notices(pairs: &[(String, String)]) -> Vec<String> {
    let m = &MESSAGES.editor;
    match first(pairs, "notice").as_str() {
        "created" => vec![m.created.clone()],
        "updated" => vec![m.updated.clone()],
        "deleted" => vec![m.deleted.clone()],
        _ => Vec::new(),
    }
}

fn error_messages(error: &EditError) -> Vec<String> {
    let m = &MESSAGES.editor;
    match error {
        EditError::ReadOnly => vec![m.read_only.clone()],
        EditError::Conflict => vec![m.conflict.clone()],
        EditError::NotFound => vec![m.not_found.clone()],
        EditError::InUse(recipes) => {
            vec![interpolate(&m.in_use, &[("recipes", &recipes.join(", "))])]
        }
        EditError::Invalid(problems) => std::iter::once(m.invalid.clone())
            .chain(problems.iter().map(report::describe))
            .collect(),
        EditError::Io(error) => vec![interpolate(&m.io_error, &[("error", error)])],
    }
}

fn done(url: &str, notice: &str) -> Response {
    Redirect::to(&format!("{url}?notice={notice}")).into_response()
}

#[derive(Template)]
#[template(path = "editor/message.html")]
struct MessagePage {
    t: &'static Messages,
    catalog_errors: Vec<String>,
    notices: Vec<String>,
    errors: Vec<String>,
    writable: bool,
}

/// A page with only the editor navigation and the error.
fn message_page(state: &AppState, error: &EditError) -> Response {
    let status = match error {
        EditError::NotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::OK,
    };
    let page = render(&MessagePage {
        t: &MESSAGES,
        catalog_errors: catalog_errors(&state.live.snapshot()),
        notices: Vec::new(),
        errors: error_messages(error),
        writable: state.editor.is_writable(),
    });
    (status, page).into_response()
}

#[derive(Template)]
#[template(path = "editor/confirm.html")]
struct ConfirmPage {
    t: &'static Messages,
    catalog_errors: Vec<String>,
    notices: Vec<String>,
    errors: Vec<String>,
    writable: bool,
    heading: String,
    help: String,
    items: Vec<String>,
    fields: Vec<Field>,
    action: String,
    confirm_label: String,
    cancel_url: String,
}

// Recipes

struct RecipeRow {
    id: String,
    name: String,
    tags: String,
}

#[derive(Template)]
#[template(path = "editor/recipes.html")]
struct RecipesPage {
    t: &'static Messages,
    catalog_errors: Vec<String>,
    notices: Vec<String>,
    errors: Vec<String>,
    writable: bool,
    recipes: Vec<RecipeRow>,
}

pub(super) async fn recipes(State(state): State<AppState>, Query(pairs): Pairs) -> Response {
    let catalog = match state.editor.catalog() {
        Ok(catalog) => catalog,
        Err(error) => return message_page(&state, &error),
    };
    render(&RecipesPage {
        t: &MESSAGES,
        catalog_errors: catalog_errors(&state.live.snapshot()),
        notices: notices(&pairs),
        errors: Vec::new(),
        writable: state.editor.is_writable(),
        recipes: catalog
            .recipes
            .iter()
            .map(|recipe| RecipeRow {
                id: recipe.id.clone(),
                name: recipe.name.clone(),
                tags: recipe.tags.join(", "),
            })
            .collect(),
    })
}

struct Choice {
    name: String,
    checked: bool,
}

#[derive(Template)]
#[template(path = "editor/recipe.html")]
struct RecipePage {
    t: &'static Messages,
    catalog_errors: Vec<String>,
    notices: Vec<String>,
    errors: Vec<String>,
    writable: bool,
    heading: String,
    action: String,
    version: String,
    name: String,
    ingredients: Vec<Choice>,
    tags: Vec<Choice>,
    delete_url: Option<String>,
}

/// Where a recipe form posts to, and what it edits.
enum RecipeTarget {
    New,
    Existing { id: String, name: String },
}

fn recipe_page(
    state: &AppState,
    catalog: &Catalog,
    target: &RecipeTarget,
    version: String,
    input: &RecipeInput,
    errors: Vec<String>,
) -> Response {
    let choices = |names: Vec<&String>, selected: &[String]| {
        names
            .into_iter()
            .map(|name| Choice {
                name: name.clone(),
                checked: selected.contains(name),
            })
            .collect()
    };
    let (heading, action, delete_url) = match target {
        RecipeTarget::New => (
            MESSAGES.editor.new_recipe.clone(),
            "/catalog/recipes/new".to_string(),
            None,
        ),
        RecipeTarget::Existing { id, name } => (
            interpolate(&MESSAGES.editor.edit_recipe, &[("name", name)]),
            format!("/catalog/recipes/{id}"),
            Some(format!("/catalog/recipes/{id}/delete")),
        ),
    };
    render(&RecipePage {
        t: &MESSAGES,
        catalog_errors: catalog_errors(&state.live.snapshot()),
        notices: Vec::new(),
        errors,
        writable: state.editor.is_writable(),
        heading,
        action,
        version,
        name: input.name.clone(),
        ingredients: choices(
            catalog.ingredients.iter().map(|i| &i.name).collect(),
            &input.ingredients,
        ),
        tags: choices(catalog.tags.iter().collect(), &input.tags),
        delete_url,
    })
}

fn recipe_input(pairs: &[(String, String)]) -> RecipeInput {
    RecipeInput {
        name: first(pairs, "name"),
        ingredients: all(pairs, "ingredient"),
        tags: all(pairs, "tag"),
    }
}

pub(super) async fn new_recipe(State(state): State<AppState>) -> Response {
    match state.editor.catalog() {
        Ok(catalog) => recipe_page(
            &state,
            &catalog,
            &RecipeTarget::New,
            String::new(),
            &RecipeInput::default(),
            Vec::new(),
        ),
        Err(error) => message_page(&state, &error),
    }
}

pub(super) async fn create_recipe(
    State(state): State<AppState>,
    Form(pairs): FormPairs,
) -> Response {
    let input = recipe_input(&pairs);
    match state.editor.create_recipe(&input) {
        Ok(_) => {
            state.live.reload();
            done("/catalog/recipes", "created")
        }
        Err(error) => match state.editor.catalog() {
            Ok(catalog) => recipe_page(
                &state,
                &catalog,
                &RecipeTarget::New,
                String::new(),
                &input,
                error_messages(&error),
            ),
            Err(_) => message_page(&state, &error),
        },
    }
}

fn existing(catalog: &Catalog, id: &str) -> Result<RecipeTarget, EditError> {
    catalog
        .recipes
        .iter()
        .find(|recipe| recipe.id == id)
        .map(|recipe| RecipeTarget::Existing {
            id: recipe.id.clone(),
            name: recipe.name.clone(),
        })
        .ok_or(EditError::NotFound)
}

pub(super) async fn edit_recipe(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let found = state.editor.catalog().and_then(|catalog| {
        let target = existing(&catalog, &id)?;
        Ok((catalog, target))
    });
    let (catalog, target) = match found {
        Ok(found) => found,
        Err(error) => return message_page(&state, &error),
    };
    let recipe = catalog
        .recipes
        .iter()
        .find(|r| r.id == id)
        .expect("found above");
    let input = RecipeInput {
        name: recipe.name.clone(),
        ingredients: recipe.ingredients.clone(),
        tags: recipe.tags.clone(),
    };
    let version = state.editor.version(&recipe_path(&id));
    recipe_page(&state, &catalog, &target, version, &input, Vec::new())
}

pub(super) async fn update_recipe(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(pairs): FormPairs,
) -> Response {
    let input = recipe_input(&pairs);
    let version = first(&pairs, "version");
    match state.editor.update_recipe(&id, &version, &input) {
        Ok(()) => {
            state.live.reload();
            done("/catalog/recipes", "updated")
        }
        Err(error) => {
            let page = state.editor.catalog().and_then(|catalog| {
                let target = existing(&catalog, &id)?;
                Ok(recipe_page(
                    &state,
                    &catalog,
                    &target,
                    version,
                    &input,
                    error_messages(&error),
                ))
            });
            page.unwrap_or_else(|_| message_page(&state, &error))
        }
    }
}

pub(super) async fn confirm_delete_recipe(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let target = state
        .editor
        .catalog()
        .and_then(|catalog| existing(&catalog, &id));
    let name = match target {
        Ok(RecipeTarget::Existing { name, .. }) => name,
        Ok(RecipeTarget::New) => unreachable!("existing() returns existing recipes"),
        Err(error) => return message_page(&state, &error),
    };
    render(&ConfirmPage {
        t: &MESSAGES,
        catalog_errors: catalog_errors(&state.live.snapshot()),
        notices: Vec::new(),
        errors: Vec::new(),
        writable: state.editor.is_writable(),
        heading: interpolate(&MESSAGES.editor.delete_recipe, &[("name", &name)]),
        help: MESSAGES.editor.delete_recipe_help.clone(),
        items: Vec::new(),
        fields: vec![Field {
            name: "version".into(),
            value: state.editor.version(&recipe_path(&id)),
        }],
        action: format!("/catalog/recipes/{id}/delete"),
        confirm_label: MESSAGES.editor.delete.clone(),
        cancel_url: format!("/catalog/recipes/{id}"),
    })
}

pub(super) async fn delete_recipe(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(pairs): FormPairs,
) -> Response {
    match state.editor.delete_recipe(&id, &first(&pairs, "version")) {
        Ok(()) => {
            state.live.reload();
            done("/catalog/recipes", "deleted")
        }
        Err(error) => message_page(&state, &error),
    }
}

// Ingredients and tags

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Ingredient,
    Tag,
}

impl Kind {
    fn base(self) -> &'static str {
        match self {
            Kind::Ingredient => "/catalog/ingredients",
            Kind::Tag => "/catalog/tags",
        }
    }

    fn file(self) -> &'static FilePath {
        FilePath::new(match self {
            Kind::Ingredient => INGREDIENTS_FILE,
            Kind::Tag => TAGS_FILE,
        })
    }

    fn edit_url(self, name: &str) -> String {
        let query = encode(&[("name".to_string(), name.to_string())]);
        format!("{}/edit?{query}", self.base())
    }

    /// Catalog names, with whether each one is a pantry staple.
    fn names(self, catalog: &Catalog) -> Vec<(String, bool)> {
        match self {
            Kind::Ingredient => catalog
                .ingredients
                .iter()
                .map(|i| (i.name.clone(), i.staple))
                .collect(),
            Kind::Tag => catalog.tags.iter().map(|t| (t.clone(), false)).collect(),
        }
    }

    fn users(self, catalog: &Catalog, name: &str) -> Vec<String> {
        match self {
            Kind::Ingredient => recipes_using(catalog, |recipe| &recipe.ingredients, name),
            Kind::Tag => recipes_using(catalog, |recipe| &recipe.tags, name),
        }
    }
}

struct NameRow {
    name: String,
    edit_url: String,
    staple: bool,
    uses: String,
}

#[derive(Template)]
#[template(path = "editor/names.html")]
struct NamesPage {
    t: &'static Messages,
    catalog_errors: Vec<String>,
    notices: Vec<String>,
    errors: Vec<String>,
    writable: bool,
    heading: String,
    add_action: String,
    version: String,
    with_staple: bool,
    new_name: String,
    new_staple: bool,
    items: Vec<NameRow>,
}

fn names_page(
    state: &AppState,
    kind: Kind,
    notices: Vec<String>,
    errors: Vec<String>,
    new_name: String,
    new_staple: bool,
) -> Response {
    let catalog = match state.editor.catalog() {
        Ok(catalog) => catalog,
        Err(error) => return message_page(state, &error),
    };
    let items = kind
        .names(&catalog)
        .into_iter()
        .map(|(name, staple)| {
            let count = kind.users(&catalog, &name).len();
            NameRow {
                edit_url: kind.edit_url(&name),
                uses: if count == 0 {
                    String::new()
                } else {
                    interpolate(&MESSAGES.editor.uses, &[("count", &count)])
                },
                name,
                staple,
            }
        })
        .collect();
    render(&NamesPage {
        t: &MESSAGES,
        catalog_errors: catalog_errors(&state.live.snapshot()),
        notices,
        errors,
        writable: state.editor.is_writable(),
        heading: match kind {
            Kind::Ingredient => MESSAGES.editor.ingredients.clone(),
            Kind::Tag => MESSAGES.editor.tags.clone(),
        },
        add_action: format!("{}/new", kind.base()),
        version: state.editor.version(kind.file()),
        with_staple: kind == Kind::Ingredient,
        new_name,
        new_staple,
        items,
    })
}

pub(super) async fn ingredients(State(state): State<AppState>, Query(pairs): Pairs) -> Response {
    names_page(
        &state,
        Kind::Ingredient,
        notices(&pairs),
        Vec::new(),
        String::new(),
        false,
    )
}

pub(super) async fn tags(State(state): State<AppState>, Query(pairs): Pairs) -> Response {
    names_page(
        &state,
        Kind::Tag,
        notices(&pairs),
        Vec::new(),
        String::new(),
        false,
    )
}

fn add_name(state: &AppState, kind: Kind, pairs: &[(String, String)]) -> Response {
    let name = first(pairs, "name");
    let staple = first(pairs, "staple") == "1";
    let version = first(pairs, "version");
    let result = match kind {
        Kind::Ingredient => state.editor.add_ingredient(&version, &name, staple),
        Kind::Tag => state.editor.add_tag(&version, &name),
    };
    match result {
        Ok(()) => {
            state.live.reload();
            done(kind.base(), "created")
        }
        Err(error) => names_page(
            state,
            kind,
            Vec::new(),
            error_messages(&error),
            name,
            staple,
        ),
    }
}

pub(super) async fn add_ingredient(
    State(state): State<AppState>,
    Form(pairs): FormPairs,
) -> Response {
    add_name(&state, Kind::Ingredient, &pairs)
}

pub(super) async fn add_tag(State(state): State<AppState>, Form(pairs): FormPairs) -> Response {
    add_name(&state, Kind::Tag, &pairs)
}

#[derive(Template)]
#[template(path = "editor/name.html")]
struct NamePage {
    t: &'static Messages,
    catalog_errors: Vec<String>,
    notices: Vec<String>,
    errors: Vec<String>,
    writable: bool,
    heading: String,
    save_action: String,
    delete_action: String,
    version: String,
    old_name: String,
    new_name: String,
    with_staple: bool,
    staple: bool,
    used_by: String,
    list_url: String,
}

/// Edit page of one ingredient or tag; `form` holds the values to show again after an error.
fn name_page(
    state: &AppState,
    kind: Kind,
    name: &str,
    form: Option<(&str, &str, bool)>,
    errors: Vec<String>,
) -> Response {
    let catalog = match state.editor.catalog() {
        Ok(catalog) => catalog,
        Err(error) => return message_page(state, &error),
    };
    let Some((_, current_staple)) = kind.names(&catalog).into_iter().find(|(n, _)| n == name)
    else {
        return message_page(state, &EditError::NotFound);
    };
    let current_version = state.editor.version(kind.file());
    let (version, new_name, staple) = form.unwrap_or((&current_version, name, current_staple));
    let users = kind.users(&catalog, name);
    let m = &MESSAGES.editor;
    let heading = match kind {
        Kind::Ingredient => &m.edit_ingredient,
        Kind::Tag => &m.edit_tag,
    };
    render(&NamePage {
        t: &MESSAGES,
        catalog_errors: catalog_errors(&state.live.snapshot()),
        notices: Vec::new(),
        errors,
        writable: state.editor.is_writable(),
        heading: interpolate(heading, &[("name", &name)]),
        save_action: format!("{}/edit", kind.base()),
        delete_action: format!("{}/delete", kind.base()),
        version: version.to_string(),
        old_name: name.to_string(),
        new_name: new_name.to_string(),
        with_staple: kind == Kind::Ingredient,
        staple,
        used_by: if users.is_empty() {
            String::new()
        } else {
            interpolate(&m.used_by, &[("recipes", &users.join(", "))])
        },
        list_url: kind.base().to_string(),
    })
}

pub(super) async fn edit_ingredient(
    State(state): State<AppState>,
    Query(pairs): Pairs,
) -> Response {
    name_page(
        &state,
        Kind::Ingredient,
        &first(&pairs, "name"),
        None,
        Vec::new(),
    )
}

pub(super) async fn edit_tag(State(state): State<AppState>, Query(pairs): Pairs) -> Response {
    name_page(&state, Kind::Tag, &first(&pairs, "name"), None, Vec::new())
}

/// Saves a rename or staple change. Renaming a name used by recipes asks for confirmation
/// first, listing the recipes that will be rewritten.
fn update_name(state: &AppState, kind: Kind, pairs: &[(String, String)]) -> Response {
    let old = first(pairs, "name");
    let new = first(pairs, "new_name");
    let staple = first(pairs, "staple") == "1";
    let version = first(pairs, "version");

    if old != new && first(pairs, "confirm") != "1" {
        let users = match state.editor.catalog() {
            Ok(catalog) => kind.users(&catalog, &old),
            Err(error) => return message_page(state, &error),
        };
        if !users.is_empty() {
            let mut fields: Vec<Field> = ["name", "new_name", "version"]
                .into_iter()
                .map(|key| Field {
                    name: key.into(),
                    value: first(pairs, key),
                })
                .collect();
            if staple {
                fields.push(Field {
                    name: "staple".into(),
                    value: "1".into(),
                });
            }
            fields.push(Field {
                name: "confirm".into(),
                value: "1".into(),
            });
            return render(&ConfirmPage {
                t: &MESSAGES,
                catalog_errors: catalog_errors(&state.live.snapshot()),
                notices: Vec::new(),
                errors: Vec::new(),
                writable: state.editor.is_writable(),
                heading: interpolate(&MESSAGES.editor.rename, &[("old", &old), ("new", &new)]),
                help: MESSAGES.editor.rename_help.clone(),
                items: users,
                fields,
                action: format!("{}/edit", kind.base()),
                confirm_label: MESSAGES.editor.confirm.clone(),
                cancel_url: kind.edit_url(&old),
            });
        }
    }

    let result = match kind {
        Kind::Ingredient => state.editor.update_ingredient(&version, &old, &new, staple),
        Kind::Tag => state.editor.update_tag(&version, &old, &new),
    };
    match result {
        Ok(()) => {
            state.live.reload();
            done(kind.base(), "updated")
        }
        Err(error) => name_page(
            state,
            kind,
            &old,
            Some((&version, &new, staple)),
            error_messages(&error),
        ),
    }
}

pub(super) async fn update_ingredient(
    State(state): State<AppState>,
    Form(pairs): FormPairs,
) -> Response {
    update_name(&state, Kind::Ingredient, &pairs)
}

pub(super) async fn update_tag(State(state): State<AppState>, Form(pairs): FormPairs) -> Response {
    update_name(&state, Kind::Tag, &pairs)
}

fn delete_name(state: &AppState, kind: Kind, pairs: &[(String, String)]) -> Response {
    let name = first(pairs, "name");
    let version = first(pairs, "version");
    let result = match kind {
        Kind::Ingredient => state.editor.delete_ingredient(&version, &name),
        Kind::Tag => state.editor.delete_tag(&version, &name),
    };
    match result {
        Ok(()) => {
            state.live.reload();
            done(kind.base(), "deleted")
        }
        Err(error) => name_page(state, kind, &name, None, error_messages(&error)),
    }
}

pub(super) async fn delete_ingredient(
    State(state): State<AppState>,
    Form(pairs): FormPairs,
) -> Response {
    delete_name(&state, Kind::Ingredient, &pairs)
}

pub(super) async fn delete_tag(State(state): State<AppState>, Form(pairs): FormPairs) -> Response {
    delete_name(&state, Kind::Tag, &pairs)
}

#[cfg(test)]
mod tests;
