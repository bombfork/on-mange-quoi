//! Catalog edits from the web interface.
//!
//! Every edit is first applied to a staging copy of the catalog and validated with the same
//! rules as `--validate`; only then are the real files written, each one atomically (temporary
//! file, then rename). A version token, the hash of a file as it was shown to the user, prevents
//! overwriting a file changed in the meantime. Written files are normalized: comments and hand
//! formatting are lost.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::catalog::normalize::fold;
use crate::catalog::{self, Catalog, INGREDIENTS_FILE, Problem, RECIPES_DIR, Recipe, TAGS_FILE};

#[derive(Debug)]
pub enum EditError {
    ReadOnly,
    /// The file changed since the form was displayed.
    Conflict,
    NotFound,
    /// The name is used by these recipes (names), so it cannot be deleted.
    InUse(Vec<String>),
    /// The edit would make the catalog invalid, or the catalog on disk is already invalid.
    Invalid(Vec<Problem>),
    Io(io::Error),
}

impl From<io::Error> for EditError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecipeInput {
    pub name: String,
    pub ingredients: Vec<String>,
    pub tags: Vec<String>,
}

/// A catalog file, relative to the catalog directory, with its new content; `None` deletes it.
struct Change {
    path: PathBuf,
    content: Option<String>,
}

pub struct Editor {
    dir: PathBuf,
    /// Serializes edits made by this server, so that version checks and writes do not
    /// interleave.
    lock: Mutex<()>,
}

pub fn recipe_path(id: &str) -> PathBuf {
    Path::new(RECIPES_DIR).join(format!("{id}.yml"))
}

impl Editor {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            lock: Mutex::new(()),
        }
    }

    /// Whether files can be created in the catalog directory and in its recipes directory.
    pub fn is_writable(&self) -> bool {
        [self.dir.clone(), self.dir.join(RECIPES_DIR)]
            .iter()
            .all(|dir| {
                let probe = dir.join(format!(".write-probe-{}", std::process::id()));
                let writable = fs::write(&probe, b"").is_ok();
                let _ = fs::remove_file(&probe);
                writable
            })
    }

    /// The catalog as it is on disk now, which the pages of the editor show.
    pub fn catalog(&self) -> Result<Catalog, EditError> {
        catalog::load(&self.dir).map_err(EditError::Invalid)
    }

    /// Version token of a catalog file (path relative to the catalog directory).
    pub fn version(&self, path: &Path) -> String {
        let mut hasher = DefaultHasher::new();
        fs::read(self.dir.join(path)).ok().hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Creates a recipe and returns its identifier, derived from its name. Ingredients
    /// missing from the catalog are added to it.
    pub fn create_recipe(&self, input: &RecipeInput) -> Result<String, EditError> {
        let _guard = self.lock();
        let catalog = self.catalog()?;
        let id = self.unused_id(&catalog, &input.name, &HashSet::new());
        self.apply(&[], recipe_changes(&catalog, &id, input))?;
        Ok(id)
    }

    /// Updates a recipe; its file name, hence its identifier, does not change. Ingredients
    /// missing from the catalog are added to it.
    pub fn update_recipe(
        &self,
        id: &str,
        version: &str,
        input: &RecipeInput,
    ) -> Result<(), EditError> {
        let _guard = self.lock();
        let catalog = self.catalog()?;
        find_recipe(&catalog, id)?;
        self.apply(
            &[(&recipe_path(id), version)],
            recipe_changes(&catalog, id, input),
        )
    }

    pub fn delete_recipe(&self, id: &str, version: &str) -> Result<(), EditError> {
        let _guard = self.lock();
        let catalog = self.catalog()?;
        find_recipe(&catalog, id)?;
        let path = recipe_path(id);
        self.apply(
            &[(&path, version)],
            vec![Change {
                path: path.clone(),
                content: None,
            }],
        )
    }

    pub fn add_ingredient(&self, version: &str, name: &str, staple: bool) -> Result<(), EditError> {
        let _guard = self.lock();
        let catalog = self.catalog()?;
        let (mut ingredients, mut staples) = ingredient_lists(&catalog);
        if staple {
            staples.push(name.to_string());
        } else {
            ingredients.push(name.to_string());
        }
        let path = PathBuf::from(INGREDIENTS_FILE);
        self.apply(
            &[(&path, version)],
            vec![Change {
                path: path.clone(),
                content: Some(ingredients_yaml(&ingredients, &staples)),
            }],
        )
    }

    /// Renames an ingredient and/or changes whether it is a pantry staple. A renamed
    /// ingredient is renamed in every recipe that uses it too.
    pub fn update_ingredient(
        &self,
        version: &str,
        old: &str,
        new: &str,
        staple: bool,
    ) -> Result<(), EditError> {
        let _guard = self.lock();
        let catalog = self.catalog()?;
        let current = catalog
            .ingredients
            .iter()
            .find(|ingredient| ingredient.name == old)
            .ok_or(EditError::NotFound)?;
        let (mut ingredients, mut staples) = ingredient_lists(&catalog);
        if current.staple == staple {
            let list = if staple {
                &mut staples
            } else {
                &mut ingredients
            };
            for name in list.iter_mut().filter(|name| *name == old) {
                *name = new.to_string();
            }
        } else {
            ingredients.retain(|name| name != old);
            staples.retain(|name| name != old);
            let list = if staple {
                &mut staples
            } else {
                &mut ingredients
            };
            list.push(new.to_string());
        }
        let path = PathBuf::from(INGREDIENTS_FILE);
        let mut changes = vec![Change {
            path: path.clone(),
            content: Some(ingredients_yaml(&ingredients, &staples)),
        }];
        if old != new {
            changes.extend(rename_in_recipes(&catalog, old, new, |recipe| {
                &mut recipe.ingredients
            }));
        }
        self.apply(&[(&path, version)], changes)
    }

    pub fn delete_ingredient(&self, version: &str, name: &str) -> Result<(), EditError> {
        let _guard = self.lock();
        let catalog = self.catalog()?;
        if !catalog
            .ingredients
            .iter()
            .any(|ingredient| ingredient.name == name)
        {
            return Err(EditError::NotFound);
        }
        let users = recipes_using(&catalog, |recipe| &recipe.ingredients, name);
        if !users.is_empty() {
            return Err(EditError::InUse(users));
        }
        let (mut ingredients, mut staples) = ingredient_lists(&catalog);
        ingredients.retain(|n| n != name);
        staples.retain(|n| n != name);
        let path = PathBuf::from(INGREDIENTS_FILE);
        self.apply(
            &[(&path, version)],
            vec![Change {
                path: path.clone(),
                content: Some(ingredients_yaml(&ingredients, &staples)),
            }],
        )
    }

    pub fn add_tag(&self, version: &str, name: &str) -> Result<(), EditError> {
        let _guard = self.lock();
        let catalog = self.catalog()?;
        let mut tags = catalog.tags.clone();
        tags.push(name.to_string());
        let path = PathBuf::from(TAGS_FILE);
        self.apply(
            &[(&path, version)],
            vec![Change {
                path: path.clone(),
                content: Some(tags_yaml(&tags)),
            }],
        )
    }

    /// Renames a tag, in every recipe that uses it too.
    pub fn update_tag(&self, version: &str, old: &str, new: &str) -> Result<(), EditError> {
        let _guard = self.lock();
        let catalog = self.catalog()?;
        if !catalog.tags.iter().any(|tag| tag == old) {
            return Err(EditError::NotFound);
        }
        let tags: Vec<String> = catalog
            .tags
            .iter()
            .map(|tag| {
                if tag == old {
                    new.to_string()
                } else {
                    tag.clone()
                }
            })
            .collect();
        let path = PathBuf::from(TAGS_FILE);
        let mut changes = vec![Change {
            path: path.clone(),
            content: Some(tags_yaml(&tags)),
        }];
        if old != new {
            changes.extend(rename_in_recipes(&catalog, old, new, |recipe| {
                &mut recipe.tags
            }));
        }
        self.apply(&[(&path, version)], changes)
    }

    pub fn delete_tag(&self, version: &str, name: &str) -> Result<(), EditError> {
        let _guard = self.lock();
        let catalog = self.catalog()?;
        if !catalog.tags.iter().any(|tag| tag == name) {
            return Err(EditError::NotFound);
        }
        let users = recipes_using(&catalog, |recipe| &recipe.tags, name);
        if !users.is_empty() {
            return Err(EditError::InUse(users));
        }
        let tags: Vec<String> = catalog
            .tags
            .iter()
            .filter(|t| *t != name)
            .cloned()
            .collect();
        let path = PathBuf::from(TAGS_FILE);
        self.apply(
            &[(&path, version)],
            vec![Change {
                path: path.clone(),
                content: Some(tags_yaml(&tags)),
            }],
        )
    }

    /// A recipe identifier derived from `name`, used neither by the catalog, nor by a file of
    /// the recipes directory, nor in `reserved`.
    fn unused_id(&self, catalog: &Catalog, name: &str, reserved: &HashSet<String>) -> String {
        let base = slug(name);
        let taken = |id: &str| {
            reserved.contains(id)
                || catalog.recipes.iter().any(|recipe| recipe.id == id)
                || self.dir.join(recipe_path(id)).exists()
        };
        (1..)
            .map(|n| {
                if n == 1 {
                    base.clone()
                } else {
                    format!("{base}-{n}")
                }
            })
            .find(|id| !taken(id))
            .expect("an unused identifier exists")
    }

    fn lock(&self) -> MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Checks the versions, validates the changes on a staging copy, then writes them.
    /// The caller holds the lock.
    fn apply(&self, expected: &[(&Path, &str)], changes: Vec<Change>) -> Result<(), EditError> {
        if !self.is_writable() {
            return Err(EditError::ReadOnly);
        }
        if expected
            .iter()
            .any(|(path, version)| self.version(path) != *version)
        {
            return Err(EditError::Conflict);
        }
        self.validate(&changes)?;
        for change in changes {
            let path = self.dir.join(&change.path);
            match change.content {
                Some(content) => write_atomically(&path, &content)?,
                None => fs::remove_file(&path)?,
            }
        }
        Ok(())
    }

    fn validate(&self, changes: &[Change]) -> Result<(), EditError> {
        let staging = tempfile::TempDir::new()?;
        let staged = staging.path();
        for name in [INGREDIENTS_FILE, TAGS_FILE] {
            let source = self.dir.join(name);
            if source.exists() {
                fs::copy(&source, staged.join(name))?;
            }
        }
        fs::create_dir(staged.join(RECIPES_DIR))?;
        for source in catalog::recipe_paths(&self.dir.join(RECIPES_DIR))? {
            let target = staged
                .join(RECIPES_DIR)
                .join(source.file_name().unwrap_or_default());
            if source.is_file() {
                fs::copy(&source, target)?;
            } else {
                fs::create_dir(target)?;
            }
        }
        for change in changes {
            let path = staged.join(&change.path);
            match &change.content {
                Some(content) => fs::write(&path, content)?,
                None => fs::remove_file(&path)?,
            }
        }
        catalog::load(staged).map(|_| ()).map_err(|problems| {
            let problems = problems
                .into_iter()
                .map(|mut problem| {
                    // Problems name staged files: point them at the real ones.
                    let real = |path: &mut PathBuf| {
                        if let Ok(relative) = path.strip_prefix(staged) {
                            *path = self.dir.join(relative);
                        }
                    };
                    real(&mut problem.path);
                    if let catalog::ProblemKind::DuplicateRecipeName { other, .. } =
                        &mut problem.kind
                    {
                        real(other);
                    }
                    problem
                })
                .collect();
            EditError::Invalid(problems)
        })
    }
}

/// The recipe file, plus the ingredients file when the recipe uses ingredients missing from the
/// catalog: they are appended to the regular ingredients.
fn recipe_changes(catalog: &Catalog, id: &str, input: &RecipeInput) -> Vec<Change> {
    let mut changes = vec![Change {
        path: recipe_path(id),
        content: Some(recipe_yaml(input)),
    }];
    let (mut ingredients, staples) = ingredient_lists(catalog);
    let known = ingredients.len() + staples.len();
    for name in &input.ingredients {
        let name = name.trim();
        let listed = ingredients
            .iter()
            .chain(&staples)
            .any(|known| known == name);
        if !name.is_empty() && !listed {
            ingredients.push(name.to_string());
        }
    }
    if ingredients.len() + staples.len() > known {
        changes.push(Change {
            path: PathBuf::from(INGREDIENTS_FILE),
            content: Some(ingredients_yaml(&ingredients, &staples)),
        });
    }
    changes
}

fn find_recipe<'a>(catalog: &'a Catalog, id: &str) -> Result<&'a Recipe, EditError> {
    catalog
        .recipes
        .iter()
        .find(|recipe| recipe.id == id)
        .ok_or(EditError::NotFound)
}

/// Names of the recipes whose list (ingredients or tags) contains `name`.
pub fn recipes_using(
    catalog: &Catalog,
    list: impl Fn(&Recipe) -> &Vec<String>,
    name: &str,
) -> Vec<String> {
    catalog
        .recipes
        .iter()
        .filter(|recipe| list(recipe).iter().any(|item| item == name))
        .map(|recipe| recipe.name.clone())
        .collect()
}

fn rename_in_recipes(
    catalog: &Catalog,
    old: &str,
    new: &str,
    list: impl Fn(&mut RecipeInput) -> &mut Vec<String>,
) -> Vec<Change> {
    catalog
        .recipes
        .iter()
        .filter_map(|recipe| {
            let mut input = RecipeInput {
                name: recipe.name.clone(),
                ingredients: recipe.ingredients.clone(),
                tags: recipe.tags.clone(),
            };
            let items = list(&mut input);
            let position = items.iter().position(|item| item == old)?;
            items[position] = new.to_string();
            Some(Change {
                path: recipe_path(&recipe.id),
                content: Some(recipe_yaml(&input)),
            })
        })
        .collect()
}

/// Ingredient names split into regular ingredients and pantry staples, in catalog order.
fn ingredient_lists(catalog: &Catalog) -> (Vec<String>, Vec<String>) {
    let (staples, ingredients): (Vec<_>, Vec<_>) = catalog
        .ingredients
        .iter()
        .partition(|ingredient| ingredient.staple);
    let names =
        |list: Vec<&catalog::Ingredient>| list.into_iter().map(|i| i.name.clone()).collect();
    (names(ingredients), names(staples))
}

fn write_atomically(path: &Path, content: &str) -> io::Result<()> {
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    // Hidden, so the catalog loader never reads it.
    let temporary = path.with_file_name(format!(".{file_name}.tmp-{}", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Recipe identifier derived from its name: lowercase ASCII letters, digits and hyphens.
pub fn slug(name: &str) -> String {
    let mut slug = String::new();
    for c in fold(name).chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "recette".to_string()
    } else {
        slug.to_string()
    }
}

pub fn recipe_yaml(input: &RecipeInput) -> String {
    let mut yaml = format!("recipe:\n  name: {}\n", scalar(input.name.trim()));
    write_list(&mut yaml, "  ", "ingredients", &input.ingredients);
    write_list(&mut yaml, "  ", "tags", &input.tags);
    yaml
}

pub fn ingredients_yaml(ingredients: &[String], staples: &[String]) -> String {
    let mut yaml = String::new();
    if ingredients.is_empty() {
        yaml.push_str("ingredients: []\n");
    }
    write_list(&mut yaml, "", "ingredients", ingredients);
    write_list(&mut yaml, "", "staples", staples);
    yaml
}

pub fn tags_yaml(tags: &[String]) -> String {
    if tags.is_empty() {
        return "tags: []\n".to_string();
    }
    let mut yaml = String::new();
    write_list(&mut yaml, "", "tags", tags);
    yaml
}

fn write_list(yaml: &mut String, indent: &str, key: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    let _ = writeln!(yaml, "{indent}{key}:");
    for item in items {
        let _ = writeln!(yaml, "{indent}  - {}", scalar(item.trim()));
    }
}

/// A YAML scalar for `value`: plain when that is unambiguous, double-quoted otherwise.
fn scalar(value: &str) -> String {
    let plain = value.chars().next().is_some_and(char::is_alphabetic)
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || " -'’".contains(c))
        && !value.ends_with(' ')
        && !matches!(
            value.to_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "y" | "n"
        );
    if plain {
        value.to_string()
    } else {
        // A JSON string is a valid double-quoted YAML scalar.
        serde_json::to_string(value).expect("strings always serialize")
    }
}

mod import;

pub use import::{ImportError, ImportProblem, ImportProblemKind, ImportReport};

#[cfg(test)]
mod tests;
