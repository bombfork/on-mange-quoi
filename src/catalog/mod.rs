//! The catalog: ingredients, tags and recipes read from a directory of YAML files and validated
//! as a whole. The layout and the rules are described under "Catalog format" in CLAUDE.md.
//!
//! This module has no HTTP, template or i18n dependency: problems are returned as data and
//! rendered elsewhere.

mod files;
pub(crate) mod normalize;
mod validate;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use files::{FileKind, IngredientsFile, RecipeFile, TagsFile};

pub const INGREDIENTS_FILE: &str = "ingredients.yml";
pub const TAGS_FILE: &str = "tags.yml";
pub const RECIPES_DIR: &str = "recipes";
const EXTENSION: &str = "yml";

#[derive(Debug, Default)]
pub struct Catalog {
    pub ingredients: Vec<Ingredient>,
    pub tags: Vec<String>,
    /// Groups of tags, each one offered as a single choice ("cru" or "cuit").
    pub choices: Vec<Vec<String>>,
    pub recipes: Vec<Recipe>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ingredient {
    pub name: String,
    /// Pantry staple: always in stock, never asked about in the stock check.
    pub staple: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipe {
    /// File name without extension.
    pub id: String,
    pub name: String,
    pub ingredients: Vec<String>,
    pub tags: Vec<String>,
}

impl Catalog {
    /// The catalog ingredient `typed` stands for: same name, or same name after normalization.
    pub fn ingredient(&self, typed: &str) -> Option<&Ingredient> {
        let typed = typed.trim();
        self.ingredients
            .iter()
            .find(|ingredient| ingredient.name == typed)
            .or_else(|| {
                let key = normalize::normalize(typed);
                self.ingredients
                    .iter()
                    .find(|ingredient| normalize::normalize(&ingredient.name) == key)
            })
    }
}

/// 1-indexed position in a YAML file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub line: u64,
    pub column: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Problem {
    pub path: PathBuf,
    pub position: Option<Position>,
    pub kind: ProblemKind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProblemKind {
    /// The file exists but cannot be read; carries the system error.
    Unreadable(String),
    /// A required file or directory of the layout is missing.
    Missing,
    /// Invalid YAML or unexpected structure; carries the parser message.
    Syntax(String),
    /// No known top-level key in a file given on the command line.
    UnknownFileType,
    /// More than one ingredients file, or more than one tags file, on the command line.
    SeveralFilesOfKind,
    /// Something other than a `.yml` file in the recipes directory.
    UnexpectedFile,
    InvalidRecipeId(String),
    EmptyName,
    NoIngredients,
    DuplicateIngredient {
        name: String,
        other: String,
        line: u64,
    },
    DuplicateTag {
        name: String,
        other: String,
        line: u64,
    },
    /// A group of `choices` without any tag.
    EmptyChoice,
    /// A tag listed more than once in `choices`.
    RepeatedChoice(String),
    DuplicateRecipeName {
        name: String,
        other: PathBuf,
    },
    RepeatedInRecipe(String),
    UnknownIngredient {
        name: String,
        suggestion: Option<String>,
    },
    UnknownTag {
        name: String,
        suggestion: Option<String>,
    },
    /// An ingredient removed by a replacement ingredients file is still used by these recipes.
    IngredientInUse {
        name: String,
        recipes: Vec<String>,
    },
    /// A tag removed by a replacement tags file is still used by these recipes.
    TagInUse {
        name: String,
        recipes: Vec<String>,
    },
}

impl Problem {
    fn new(path: &Path, position: Option<Position>, kind: ProblemKind) -> Self {
        Self {
            path: path.to_path_buf(),
            position,
            kind,
        }
    }
}

/// Loads and validates the catalog in `dir`. Every problem found is returned, not only the
/// first one.
pub fn load(dir: &Path) -> Result<Catalog, Vec<Problem>> {
    load_with(dir, &[])
}

/// Same as [`load`], with extra files checked on top of the catalog: each one is added to it,
/// or replaces the catalog file of the same kind (ingredients, tags) or name (recipe).
pub fn load_with(dir: &Path, extra_files: &[PathBuf]) -> Result<Catalog, Vec<Problem>> {
    let mut problems = Vec::new();
    let mut sources = read_dir(dir, &mut problems);
    overlay(&mut sources, extra_files, &mut problems);
    validate::validate(&sources, problems)
}

struct Source<T> {
    path: PathBuf,
    file: T,
}

#[derive(Default)]
struct Sources {
    ingredients: Option<Source<IngredientsFile>>,
    tags: Option<Source<TagsFile>>,
    recipes: BTreeMap<String, Source<RecipeFile>>,
    /// Names from the catalog file replaced by an extra file, to report removed names that
    /// recipes still use.
    replaced_ingredients: BTreeSet<String>,
    replaced_tags: BTreeSet<String>,
}

fn read_dir(dir: &Path, problems: &mut Vec<Problem>) -> Sources {
    let mut sources = Sources {
        ingredients: read_source(&dir.join(INGREDIENTS_FILE), problems),
        tags: read_source(&dir.join(TAGS_FILE), problems),
        ..Sources::default()
    };

    let recipes_dir = dir.join(RECIPES_DIR);
    let paths = match recipe_paths(&recipes_dir) {
        Ok(paths) => paths,
        Err(error) => {
            problems.push(io_problem(&recipes_dir, &error));
            return sources;
        }
    };

    for path in paths {
        let is_yml = path.extension().and_then(|ext| ext.to_str()) == Some(EXTENSION);
        if !path.is_file() || !is_yml {
            problems.push(Problem::new(&path, None, ProblemKind::UnexpectedFile));
            continue;
        }
        let id = recipe_id(&path, problems);
        if let Some(source) = read_source(&path, problems) {
            sources.recipes.insert(id, source);
        }
    }
    sources
}

/// Entries of the recipes directory, sorted, without hidden files (editor swap files, etc.).
pub(crate) fn recipe_paths(recipes_dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(recipes_dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.'))
        })
        .collect();
    paths.sort();
    Ok(paths)
}

/// Hash of every file [`load`] reads, to detect changes without parsing. Equal fingerprints
/// mean [`load`] would give the same result.
pub fn fingerprint(dir: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    let mut hash_file = |path: &Path| {
        path.file_name().hash(&mut hasher);
        match fs::read(path) {
            Ok(content) => content.hash(&mut hasher),
            Err(error) => error.kind().hash(&mut hasher),
        }
    };
    hash_file(&dir.join(INGREDIENTS_FILE));
    hash_file(&dir.join(TAGS_FILE));
    match recipe_paths(&dir.join(RECIPES_DIR)) {
        Ok(paths) => paths.iter().for_each(|path| hash_file(path)),
        Err(error) => error.kind().hash(&mut hasher),
    }
    hasher.finish()
}

fn overlay(sources: &mut Sources, paths: &[PathBuf], problems: &mut Vec<Problem>) {
    let mut seen_ingredients = false;
    let mut seen_tags = false;
    for path in paths {
        let Some(text) = read_text(path, problems) else {
            continue;
        };
        let kind = match files::detect_kind(path, &text) {
            Ok(kind) => kind,
            Err(problem) => {
                problems.push(problem);
                continue;
            }
        };
        match kind {
            FileKind::Ingredients => {
                if std::mem::replace(&mut seen_ingredients, true) {
                    problems.push(Problem::new(path, None, ProblemKind::SeveralFilesOfKind));
                    continue;
                }
                if let Some(new) = parse_source(path, &text, problems)
                    && let Some(old) = sources.ingredients.replace(new)
                {
                    let IngredientsFile {
                        ingredients,
                        staples,
                    } = old.file;
                    sources.replaced_ingredients = values([ingredients, staples]);
                }
            }
            FileKind::Tags => {
                if std::mem::replace(&mut seen_tags, true) {
                    problems.push(Problem::new(path, None, ProblemKind::SeveralFilesOfKind));
                    continue;
                }
                if let Some(new) = parse_source(path, &text, problems)
                    && let Some(old) = sources.tags.replace(new)
                {
                    sources.replaced_tags = values([old.file.tags]);
                }
            }
            FileKind::Recipe => {
                let id = recipe_id(path, problems);
                if let Some(new) = parse_source(path, &text, problems) {
                    sources.recipes.insert(id, new);
                }
            }
        }
    }
}

fn values(lists: impl IntoIterator<Item = Option<Vec<files::Name>>>) -> BTreeSet<String> {
    lists
        .into_iter()
        .flatten()
        .flatten()
        .map(|name| name.value)
        .collect()
}

fn recipe_id(path: &Path, problems: &mut Vec<Problem>) -> String {
    let id = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let valid = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid {
        problems.push(Problem::new(
            path,
            None,
            ProblemKind::InvalidRecipeId(id.clone()),
        ));
    }
    id
}

fn read_source<T: DeserializeOwned>(path: &Path, problems: &mut Vec<Problem>) -> Option<Source<T>> {
    let text = read_text(path, problems)?;
    parse_source(path, &text, problems)
}

fn parse_source<T: DeserializeOwned>(
    path: &Path,
    text: &str,
    problems: &mut Vec<Problem>,
) -> Option<Source<T>> {
    match files::parse(path, text) {
        Ok(file) => Some(Source {
            path: path.to_path_buf(),
            file,
        }),
        Err(problem) => {
            problems.push(problem);
            None
        }
    }
}

fn read_text(path: &Path, problems: &mut Vec<Problem>) -> Option<String> {
    fs::read_to_string(path)
        .map_err(|error| problems.push(io_problem(path, &error)))
        .ok()
}

fn io_problem(path: &Path, error: &io::Error) -> Problem {
    let kind = match error.kind() {
        io::ErrorKind::NotFound => ProblemKind::Missing,
        _ => ProblemKind::Unreadable(error.to_string()),
    };
    Problem::new(path, None, kind)
}
