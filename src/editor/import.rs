//! Import of raw YAML: any number of documents, each one either ingredients (`ingredients`,
//! `staples` and/or `ingredient_tags`, as in `ingredients.yml`) or one recipe (`recipe`, as in a
//! recipe file). Ingredient tags are merged into the catalog's.
//!
//! The whole input is accepted or refused. Ingredients already in the catalog are skipped, and
//! names that only differ from a known name by spelling are imported as that name, so the same
//! input can be pasted again with new recipes. A recipe may only use ingredients from the
//! catalog or declared in the input.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_saphyr::Spanned;

use super::{Change, EditError, Editor, RecipeInput, ingredient_lists, recipe_path, recipe_yaml};
use crate::catalog::normalize::{closest, normalize};
use crate::catalog::{Catalog, INGREDIENTS_FILE, IngredientTag, ProblemKind};

const KEYS: [&str; 4] = ["ingredients", "staples", "ingredient_tags", "recipe"];

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Names of the imported recipes.
    pub recipes: Vec<String>,
    /// Ingredients added to the catalog.
    pub new_ingredients: Vec<String>,
    /// Names imported under the spelling of a known name: (typed, known).
    pub mapped: Vec<(String, String)>,
    /// Ingredient tags created or given new ingredients.
    pub ingredient_tags: Vec<String>,
}

#[derive(Debug)]
pub enum ImportError {
    /// Nothing to import: no document, or only empty ones.
    Empty,
    /// The input is refused; nothing was written.
    Invalid(Vec<ImportProblem>),
    /// The catalog cannot be edited (read-only, invalid on disk, write failure).
    Edit(EditError),
}

#[derive(Debug, PartialEq, Eq)]
pub struct ImportProblem {
    /// Line in the input, when the problem can be located.
    pub line: Option<u64>,
    pub kind: ImportProblemKind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImportProblemKind {
    /// Invalid YAML or unexpected structure; carries the parser message.
    Syntax(String),
    UnknownKey(String),
    /// A document holds both a recipe and ingredients.
    Mixed,
    /// The imported catalog breaks a catalog rule.
    Catalog(ProblemKind),
}

type Name = Spanned<String>;

#[derive(Deserialize)]
struct Document {
    ingredients: Option<Vec<Name>>,
    staples: Option<Vec<Name>>,
    ingredient_tags: Option<HashMap<Name, Option<Vec<Name>>>>,
    recipe: Option<RecipeDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeDocument {
    name: Name,
    ingredients: Option<Vec<Name>>,
    tags: Option<Vec<Name>>,
}

/// An imported recipe, with the input line of each name to locate problems.
struct ImportedRecipe {
    id: String,
    name_line: u64,
    input: RecipeInput,
    /// Resolved ingredient and tag names with their line.
    lines: HashMap<String, u64>,
}

impl Editor {
    pub fn import(&self, text: &str) -> Result<ImportReport, ImportError> {
        let _guard = self.lock();
        let catalog = self.catalog().map_err(ImportError::Edit)?;
        let documents = parse(text)?;

        let mut resolver = Resolver::new(&catalog);
        let mut report = ImportReport::default();
        let mut problems = Vec::new();
        let mut declared: Vec<(String, bool, u64)> = Vec::new();
        for document in &documents {
            let lists = [(&document.ingredients, false), (&document.staples, true)];
            for (list, staple) in lists {
                for name in list.iter().flatten() {
                    let line = name.referenced.line();
                    let typed = name.value.trim();
                    if typed.is_empty() {
                        problems.push(located(line, ProblemKind::EmptyName));
                        continue;
                    }
                    if resolver.ingredient(typed, &mut report.mapped).is_some() {
                        continue;
                    }
                    resolver.declare_ingredient(typed);
                    declared.push((typed.to_string(), staple, line));
                }
            }
        }

        let mut ingredient_tags = catalog.ingredient_tags.clone();
        for document in &documents {
            let mut entries: Vec<_> = document.ingredient_tags.iter().flatten().collect();
            entries.sort_by_key(|(tag, _)| tag.referenced.line());
            for (tag, members) in entries {
                let typed = tag.value.trim();
                if typed.is_empty() {
                    problems.push(located(tag.referenced.line(), ProblemKind::EmptyName));
                    continue;
                }
                let names: Vec<String> = members
                    .iter()
                    .flatten()
                    .filter_map(|member| {
                        let resolved = resolver.ingredient(member.value.trim(), &mut report.mapped);
                        if resolved.is_none() {
                            let known = resolver.ingredient_names();
                            problems.push(located(
                                member.referenced.line(),
                                ProblemKind::UnknownIngredient {
                                    name: member.value.clone(),
                                    suggestion: closest(&member.value, known.iter().copied())
                                        .map(str::to_string),
                                },
                            ));
                        }
                        resolved
                    })
                    .collect();
                let existing = ingredient_tags.iter().map(|t| t.name.as_str());
                let name = resolve(typed, existing, &mut report.mapped)
                    .unwrap_or_else(|| typed.to_string());
                let position = match ingredient_tags.iter().position(|t| t.name == name) {
                    Some(position) => position,
                    None => {
                        ingredient_tags.push(IngredientTag {
                            name: name.clone(),
                            ingredients: Vec::new(),
                        });
                        ingredient_tags.len() - 1
                    }
                };
                let tag = &mut ingredient_tags[position];
                for member in names {
                    if !tag.ingredients.contains(&member) {
                        tag.ingredients.push(member);
                        if !report.ingredient_tags.contains(&name) {
                            report.ingredient_tags.push(name.clone());
                        }
                    }
                }
            }
        }

        let mut taken: HashSet<String> = HashSet::new();
        let mut recipes = Vec::new();
        for document in &documents {
            let Some(recipe) = &document.recipe else {
                continue;
            };
            let mut lines = HashMap::new();
            let mut resolve_names = |names: &Option<Vec<Name>>, tag: bool, mapped: &mut Vec<_>| {
                names
                    .iter()
                    .flatten()
                    .map(|name| {
                        let typed = name.value.trim();
                        let resolved = if tag {
                            resolver.tag(typed, mapped)
                        } else {
                            resolver.ingredient(typed, mapped)
                        }
                        .unwrap_or_else(|| typed.to_string());
                        lines.insert(resolved.clone(), name.referenced.line());
                        resolved
                    })
                    .collect::<Vec<_>>()
            };
            let input = RecipeInput {
                name: recipe.name.value.trim().to_string(),
                ingredients: resolve_names(&recipe.ingredients, false, &mut report.mapped),
                tags: resolve_names(&recipe.tags, true, &mut report.mapped),
            };
            let id = self.unused_id(&catalog, &input.name, &taken);
            taken.insert(id.clone());
            recipes.push(ImportedRecipe {
                id,
                name_line: recipe.name.referenced.line(),
                input,
                lines,
            });
        }

        if !problems.is_empty() {
            return Err(ImportError::Invalid(problems));
        }
        if declared.is_empty() && recipes.is_empty() && report.ingredient_tags.is_empty() {
            return Err(ImportError::Empty);
        }

        let mut changes = Vec::new();
        if !declared.is_empty() || !report.ingredient_tags.is_empty() {
            let (mut ingredients, mut staples) = ingredient_lists(&catalog);
            for (name, staple, _) in &declared {
                let list = if *staple {
                    &mut staples
                } else {
                    &mut ingredients
                };
                list.push(name.clone());
            }
            changes.push(Change {
                path: INGREDIENTS_FILE.into(),
                content: Some(super::ingredients_yaml(
                    &ingredients,
                    &staples,
                    &ingredient_tags,
                )),
            });
        }
        for recipe in &recipes {
            changes.push(Change {
                path: recipe_path(&recipe.id),
                content: Some(recipe_yaml(&recipe.input)),
            });
        }

        match self.apply(&[], changes) {
            Ok(()) => {
                report.recipes = recipes.into_iter().map(|r| r.input.name).collect();
                report.new_ingredients = declared.into_iter().map(|(name, ..)| name).collect();
                Ok(report)
            }
            Err(EditError::Invalid(found)) => {
                let lines: HashMap<&str, u64> = declared
                    .iter()
                    .map(|(name, _, line)| (name.as_str(), *line))
                    .collect();
                let mut problems: Vec<ImportProblem> = found
                    .into_iter()
                    .map(|problem| ImportProblem {
                        line: locate(&self.dir, &problem.path, &problem.kind, &recipes, &lines),
                        kind: ImportProblemKind::Catalog(problem.kind),
                    })
                    .collect();
                problems.sort_by_key(|problem| problem.line);
                Err(ImportError::Invalid(problems))
            }
            Err(other) => Err(ImportError::Edit(other)),
        }
    }
}

fn located(line: u64, kind: ProblemKind) -> ImportProblem {
    ImportProblem {
        line: Some(line),
        kind: ImportProblemKind::Catalog(kind),
    }
}

fn parse(text: &str) -> Result<Vec<Document>, ImportError> {
    let syntax = |error: serde_saphyr::Error| {
        let message = error.without_snippet().to_string();
        ImportError::Invalid(vec![ImportProblem {
            line: error.location().map(|location| location.line()),
            kind: ImportProblemKind::Syntax(message),
        }])
    };
    // Top-level keys first, with their position, for precise messages on unknown keys.
    let keys: Vec<HashMap<Spanned<String>, IgnoredAny>> =
        serde_saphyr::from_str_multiple(text).map_err(syntax)?;
    let mut problems = Vec::new();
    for document in &keys {
        let mut known = HashSet::new();
        for key in document.keys() {
            if KEYS.contains(&key.value.as_str()) {
                known.insert(key.value.as_str());
            } else {
                problems.push(ImportProblem {
                    line: Some(key.referenced.line()),
                    kind: ImportProblemKind::UnknownKey(key.value.clone()),
                });
            }
        }
        if known.contains("recipe") && known.len() > 1 {
            let line = document.keys().map(|key| key.referenced.line()).min();
            problems.push(ImportProblem {
                line,
                kind: ImportProblemKind::Mixed,
            });
        }
    }
    if !problems.is_empty() {
        problems.sort_by_key(|problem| problem.line);
        return Err(ImportError::Invalid(problems));
    }
    serde_saphyr::from_str_multiple(text).map_err(syntax)
}

/// Resolves typed names to known spellings: catalog names, then names declared in the input.
struct Resolver<'a> {
    catalog: &'a Catalog,
    declared: HashMap<String, String>,
}

impl<'a> Resolver<'a> {
    fn new(catalog: &'a Catalog) -> Self {
        Self {
            catalog,
            declared: HashMap::new(),
        }
    }

    fn declare_ingredient(&mut self, name: &str) {
        self.declared
            .entry(normalize(name))
            .or_insert_with(|| name.to_string());
    }

    fn ingredient_names(&self) -> Vec<&str> {
        let catalog = self.catalog.ingredients.iter().map(|i| i.name.as_str());
        catalog
            .chain(self.declared.values().map(String::as_str))
            .collect()
    }

    /// The known ingredient `typed` stands for, if any; records spelling differences.
    fn ingredient(&self, typed: &str, mapped: &mut Vec<(String, String)>) -> Option<String> {
        let catalog = self.catalog.ingredients.iter().map(|i| i.name.as_str());
        let declared = self.declared.values().map(String::as_str);
        resolve(typed, catalog.chain(declared), mapped)
    }

    fn tag(&self, typed: &str, mapped: &mut Vec<(String, String)>) -> Option<String> {
        resolve(typed, self.catalog.tags.iter().map(String::as_str), mapped)
    }
}

fn resolve<'a>(
    typed: &str,
    known: impl Iterator<Item = &'a str> + Clone,
    mapped: &mut Vec<(String, String)>,
) -> Option<String> {
    if typed.is_empty() {
        return None;
    }
    if known.clone().any(|name| name == typed) {
        return Some(typed.to_string());
    }
    let key = normalize(typed);
    let name = known.into_iter().find(|name| normalize(name) == key)?;
    let pair = (typed.to_string(), name.to_string());
    if !mapped.contains(&pair) {
        mapped.push(pair);
    }
    Some(name.to_string())
}

/// Line in the input of a problem found on the staged catalog.
fn locate(
    dir: &Path,
    path: &Path,
    kind: &ProblemKind,
    recipes: &[ImportedRecipe],
    declared: &HashMap<&str, u64>,
) -> Option<u64> {
    let named = match kind {
        ProblemKind::UnknownIngredient { name, .. }
        | ProblemKind::UnknownTag { name, .. }
        | ProblemKind::DuplicateIngredient { name, .. }
        | ProblemKind::DuplicateTag { name, .. }
        | ProblemKind::RepeatedInRecipe(name) => Some(name.as_str()),
        _ => None,
    };
    if path == dir.join(INGREDIENTS_FILE) {
        return named.and_then(|name| declared.get(name).copied());
    }
    let recipe = recipes
        .iter()
        .find(|recipe| dir.join(recipe_path(&recipe.id)) == path)?;
    Some(
        named
            .and_then(|name| recipe.lines.get(name).copied())
            .unwrap_or(recipe.name_line),
    )
}
