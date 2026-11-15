//! Rules that span the whole catalog: uniqueness after normalization and references from
//! recipes to ingredients and tags.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use super::files::{Name, position};
use super::normalize::{closest, normalize};
use super::{Catalog, Ingredient, IngredientTag, Problem, ProblemKind, Recipe, Sources};

pub(super) fn validate(
    sources: &Sources,
    mut problems: Vec<Problem>,
) -> Result<Catalog, Vec<Problem>> {
    let ingredients = validate_ingredients(sources, &mut problems);
    let ingredient_tags = validate_ingredient_tags(sources, &ingredients, &mut problems);
    let tags = validate_tags(sources, &mut problems);
    let choices = validate_choices(sources, &tags, &mut problems);

    // Without a readable ingredients (or tags) file, every reference would be reported as
    // unknown: skip those checks, the missing file is already reported.
    let mut ingredient_refs = References {
        kind: ReferenceKind::Ingredient,
        known: sources
            .ingredients
            .as_ref()
            .map(|_| ingredients.iter().map(|i| i.name.clone()).collect()),
        replaced: &sources.replaced_ingredients,
        in_use: BTreeMap::new(),
    };
    let mut tag_refs = References {
        kind: ReferenceKind::Tag,
        known: sources
            .tags
            .as_ref()
            .map(|_| tags.iter().cloned().collect()),
        replaced: &sources.replaced_tags,
        in_use: BTreeMap::new(),
    };

    let mut recipe_names: HashMap<String, &Path> = HashMap::new();
    let mut recipes = Vec::new();
    for (id, source) in &sources.recipes {
        let path = source.path.as_path();
        let entry = &source.file.recipe;

        if entry.name.value.trim().is_empty() {
            problems.push(Problem::new(
                path,
                position(&entry.name),
                ProblemKind::EmptyName,
            ));
        } else {
            match recipe_names.entry(normalize(&entry.name.value)) {
                Entry::Occupied(other) => problems.push(Problem::new(
                    path,
                    position(&entry.name),
                    ProblemKind::DuplicateRecipeName {
                        name: entry.name.value.clone(),
                        other: other.get().to_path_buf(),
                    },
                )),
                Entry::Vacant(slot) => {
                    slot.insert(path);
                }
            }
        }

        let recipe_ingredients = entry.ingredients.as_deref().unwrap_or_default();
        if recipe_ingredients.is_empty() {
            problems.push(Problem::new(path, None, ProblemKind::NoIngredients));
        }
        let recipe_tags = entry.tags.as_deref().unwrap_or_default();
        ingredient_refs.check(path, id, recipe_ingredients, &mut problems);
        tag_refs.check(path, id, recipe_tags, &mut problems);

        recipes.push(Recipe {
            id: id.clone(),
            name: entry.name.value.clone(),
            ingredients: recipe_ingredients.iter().map(|n| n.value.clone()).collect(),
            tags: recipe_tags.iter().map(|n| n.value.clone()).collect(),
        });
    }

    if let Some(source) = &sources.ingredients {
        for (name, recipes) in ingredient_refs.in_use {
            problems.push(Problem::new(
                &source.path,
                None,
                ProblemKind::IngredientInUse { name, recipes },
            ));
        }
    }
    if let Some(source) = &sources.tags {
        for (name, recipes) in tag_refs.in_use {
            problems.push(Problem::new(
                &source.path,
                None,
                ProblemKind::TagInUse { name, recipes },
            ));
        }
    }

    if problems.is_empty() {
        Ok(Catalog {
            ingredients,
            tags,
            choices,
            ingredient_tags,
            recipes,
        })
    } else {
        problems.sort_by(|a, b| (&a.path, a.position).cmp(&(&b.path, b.position)));
        Err(problems)
    }
}

fn validate_ingredients(sources: &Sources, problems: &mut Vec<Problem>) -> Vec<Ingredient> {
    let Some(source) = &sources.ingredients else {
        return Vec::new();
    };
    let mut seen = Seen::default();
    let mut ingredients = Vec::new();
    for (list, staple) in [
        (&source.file.ingredients, false),
        (&source.file.staples, true),
    ] {
        for name in list.iter().flatten() {
            let duplicate = |other: String, line| ProblemKind::DuplicateIngredient {
                name: name.value.clone(),
                other,
                line,
            };
            if seen.accept(&source.path, name, duplicate, problems) {
                ingredients.push(Ingredient {
                    name: name.value.clone(),
                    staple,
                });
            }
        }
    }
    ingredients
}

fn validate_tags(sources: &Sources, problems: &mut Vec<Problem>) -> Vec<String> {
    let Some(source) = &sources.tags else {
        return Vec::new();
    };
    let mut seen = Seen::default();
    let mut tags = Vec::new();
    for name in source.file.tags.iter().flatten() {
        let duplicate = |other: String, line| ProblemKind::DuplicateTag {
            name: name.value.clone(),
            other,
            line,
        };
        if seen.accept(&source.path, name, duplicate, problems) {
            tags.push(name.value.clone());
        }
    }
    tags
}

/// Ingredient tags: unique names, each one listing catalog ingredients once.
fn validate_ingredient_tags(
    sources: &Sources,
    ingredients: &[Ingredient],
    problems: &mut Vec<Problem>,
) -> Vec<IngredientTag> {
    let Some(source) = &sources.ingredients else {
        return Vec::new();
    };
    let mut entries: Vec<_> = source.file.ingredient_tags.iter().flatten().collect();
    entries.sort_by_key(|(tag, _)| (tag.referenced.line(), tag.referenced.column()));
    let names: Vec<&str> = ingredients.iter().map(|i| i.name.as_str()).collect();
    let mut seen = Seen::default();
    let mut tags = Vec::new();
    for (tag, members) in entries {
        let duplicate = |other: String, line| ProblemKind::DuplicateIngredientTag {
            name: tag.value.clone(),
            other,
            line,
        };
        if !seen.accept(&source.path, tag, duplicate, problems) {
            continue;
        }
        let mut accepted: Vec<String> = Vec::new();
        for member in members.iter().flatten() {
            let value = member.value.as_str();
            let kind = if value.trim().is_empty() {
                ProblemKind::EmptyName
            } else if !names.contains(&value) {
                ProblemKind::UnknownIngredient {
                    name: value.to_string(),
                    suggestion: closest(value, names.iter().copied()).map(str::to_string),
                }
            } else if accepted.iter().any(|name| name == value) {
                ProblemKind::RepeatedInIngredientTag {
                    tag: tag.value.clone(),
                    name: value.to_string(),
                }
            } else {
                accepted.push(value.to_string());
                continue;
            };
            problems.push(Problem::new(&source.path, position(member), kind));
        }
        tags.push(IngredientTag {
            name: tag.value.clone(),
            ingredients: accepted,
        });
    }
    tags
}

/// Groups of `choices`: declared tags only, each one in a single group.
fn validate_choices(
    sources: &Sources,
    tags: &[String],
    problems: &mut Vec<Problem>,
) -> Vec<Vec<String>> {
    let Some(source) = &sources.tags else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    let mut choices = Vec::new();
    for group in source.file.choices.iter().flatten() {
        if group.is_empty() {
            problems.push(Problem::new(&source.path, None, ProblemKind::EmptyChoice));
            continue;
        }
        let mut accepted = Vec::new();
        for name in group {
            let value = name.value.as_str();
            let kind = if !tags.iter().any(|tag| tag == value) {
                let suggestion = closest(value, tags.iter().map(String::as_str));
                ProblemKind::UnknownTag {
                    name: value.to_string(),
                    suggestion: suggestion.map(str::to_string),
                }
            } else if !seen.insert(value) {
                ProblemKind::RepeatedChoice(value.to_string())
            } else {
                accepted.push(value.to_string());
                continue;
            };
            problems.push(Problem::new(&source.path, position(name), kind));
        }
        choices.push(accepted);
    }
    choices
}

/// Catalog names already accepted, by normalized form.
#[derive(Default)]
struct Seen {
    names: HashMap<String, (String, u64)>,
}

impl Seen {
    /// Accepts a non-empty name not seen yet under the same normalized form; otherwise reports
    /// a problem and returns false.
    fn accept(
        &mut self,
        path: &Path,
        name: &Name,
        duplicate: impl FnOnce(String, u64) -> ProblemKind,
        problems: &mut Vec<Problem>,
    ) -> bool {
        if name.value.trim().is_empty() {
            problems.push(Problem::new(path, position(name), ProblemKind::EmptyName));
            return false;
        }
        match self.names.entry(normalize(&name.value)) {
            Entry::Occupied(other) => {
                let (other, line) = other.get().clone();
                problems.push(Problem::new(path, position(name), duplicate(other, line)));
                false
            }
            Entry::Vacant(slot) => {
                slot.insert((name.value.clone(), name.referenced.line()));
                true
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ReferenceKind {
    Ingredient,
    Tag,
}

/// Checks the names a recipe references against one catalog list.
struct References<'a> {
    kind: ReferenceKind,
    /// `None` when the catalog list could not be read.
    known: Option<BTreeSet<String>>,
    replaced: &'a BTreeSet<String>,
    /// Removed names still referenced, with the recipes that reference them.
    in_use: BTreeMap<String, Vec<String>>,
}

impl References<'_> {
    fn check(&mut self, path: &Path, recipe_id: &str, names: &[Name], problems: &mut Vec<Problem>) {
        let mut in_recipe = BTreeSet::new();
        for name in names {
            let value = name.value.as_str();
            let at = position(name);
            if value.trim().is_empty() {
                problems.push(Problem::new(path, at, ProblemKind::EmptyName));
                continue;
            }
            if !in_recipe.insert(value) {
                problems.push(Problem::new(
                    path,
                    at,
                    ProblemKind::RepeatedInRecipe(value.to_string()),
                ));
                continue;
            }
            let Some(known) = &self.known else {
                continue;
            };
            if known.contains(value) {
                continue;
            }
            if self.replaced.contains(value) {
                self.in_use
                    .entry(value.to_string())
                    .or_default()
                    .push(recipe_id.to_string());
                continue;
            }
            let suggestion = closest(value, known.iter().map(String::as_str)).map(str::to_string);
            let name = value.to_string();
            let kind = match self.kind {
                ReferenceKind::Ingredient => ProblemKind::UnknownIngredient { name, suggestion },
                ReferenceKind::Tag => ProblemKind::UnknownTag { name, suggestion },
            };
            problems.push(Problem::new(path, at, kind));
        }
    }
}
