//! On-disk YAML file types. The top-level key gives the file type.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde::de::{DeserializeOwned, IgnoredAny};
use serde_saphyr::{MessageFormatter, Spanned, UserMessageFormatter};

use super::{Position, Problem, ProblemKind};

pub type Name = Spanned<String>;

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IngredientsFile {
    #[cfg_attr(test, schemars(with = "Option<Vec<String>>"))]
    pub ingredients: Option<Vec<Name>>,
    #[cfg_attr(test, schemars(with = "Option<Vec<String>>"))]
    pub staples: Option<Vec<Name>>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TagsFile {
    #[cfg_attr(test, schemars(with = "Option<Vec<String>>"))]
    pub tags: Option<Vec<Name>>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RecipeFile {
    pub recipe: RecipeEntry,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RecipeEntry {
    #[cfg_attr(test, schemars(with = "String"))]
    pub name: Name,
    // Optional here so that a missing list is reported as "no ingredients", not as a parse error.
    #[cfg_attr(test, schemars(with = "Vec<String>", length(min = 1)))]
    pub ingredients: Option<Vec<Name>>,
    #[cfg_attr(test, schemars(with = "Option<Vec<String>>"))]
    pub tags: Option<Vec<Name>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Ingredients,
    Tags,
    Recipe,
}

/// Detects the file type from its top-level keys, for files given outside the catalog layout.
pub fn detect_kind(path: &Path, text: &str) -> Result<FileKind, Problem> {
    let keys: BTreeMap<String, IgnoredAny> =
        serde_saphyr::from_str(text).map_err(|error| syntax_problem(path, &error))?;
    if keys.contains_key("recipe") {
        Ok(FileKind::Recipe)
    } else if keys.contains_key("tags") {
        Ok(FileKind::Tags)
    } else if keys.contains_key("ingredients") || keys.contains_key("staples") {
        Ok(FileKind::Ingredients)
    } else {
        Err(Problem::new(path, None, ProblemKind::UnknownFileType))
    }
}

pub fn parse<T: DeserializeOwned>(path: &Path, text: &str) -> Result<T, Problem> {
    serde_saphyr::from_str(text).map_err(|error| syntax_problem(path, &error))
}

pub fn position(name: &Name) -> Option<Position> {
    let location = name.referenced;
    (location.line() > 0).then(|| Position {
        line: location.line(),
        column: location.column(),
    })
}

fn syntax_problem(path: &Path, error: &serde_saphyr::Error) -> Problem {
    let message = UserMessageFormatter.format_message(error.without_snippet());
    // The message may quote file content: keep it on one line, without control characters.
    let message: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let position = error.location().and_then(|location| {
        (location.line() > 0).then(|| Position {
            line: location.line(),
            column: location.column(),
        })
    });
    Problem::new(path, position, ProblemKind::Syntax(message))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use schemars::schema_for;

    use super::*;

    fn kind(text: &str) -> Result<FileKind, ProblemKind> {
        detect_kind(Path::new("f.yml"), text).map_err(|problem| problem.kind)
    }

    #[test]
    fn detects_file_kind_from_top_level_key() {
        assert_eq!(kind("recipe:\n  name: X\n"), Ok(FileKind::Recipe));
        assert_eq!(kind("tags: [rapide]\n"), Ok(FileKind::Tags));
        assert_eq!(kind("ingredients: [lait]\n"), Ok(FileKind::Ingredients));
        assert_eq!(kind("staples: [sel]\n"), Ok(FileKind::Ingredients));
        assert_eq!(kind("autre: 1\n"), Err(ProblemKind::UnknownFileType));
        assert_eq!(kind(""), Err(ProblemKind::UnknownFileType));
        assert!(matches!(kind("- a\n"), Err(ProblemKind::Syntax(_))));
    }

    #[test]
    fn parses_recipe_with_positions_and_empty_tags() {
        let text = "recipe:\n  name: Croque-monsieur\n  ingredients:\n    - pain de mie\n  tags:\n";
        let file: RecipeFile = parse(Path::new("r.yml"), text).unwrap();
        assert_eq!(file.recipe.name.value, "Croque-monsieur");
        let ingredients = file.recipe.ingredients.unwrap();
        assert_eq!(
            position(&ingredients[0]),
            Some(Position { line: 4, column: 7 })
        );
        assert!(file.recipe.tags.is_none());
    }

    #[test]
    fn rejects_unknown_keys_with_position() {
        let text = "recipe:\n  name: X\n  ingredient: [a]\n";
        let problem = parse::<RecipeFile>(Path::new("r.yml"), text).unwrap_err();
        assert!(
            matches!(&problem.kind, ProblemKind::Syntax(message) if message.contains("ingredient"))
        );
        assert_eq!(problem.position, Some(Position { line: 3, column: 3 }));
    }

    /// Run with `UPDATE_SCHEMAS=1` (`mise run schemas`) to regenerate the committed schemas.
    #[test]
    fn schemas_are_up_to_date() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas");
        let schemas = [
            ("ingredients.schema.json", schema_for!(IngredientsFile)),
            ("tags.schema.json", schema_for!(TagsFile)),
            ("recipe.schema.json", schema_for!(RecipeFile)),
        ];
        for (file, schema) in schemas {
            let generated = serde_json::to_string_pretty(&schema).unwrap() + "\n";
            let path = dir.join(file);
            if std::env::var_os("UPDATE_SCHEMAS").is_some() {
                fs::create_dir_all(&dir).unwrap();
                fs::write(&path, &generated).unwrap();
            } else {
                let committed = fs::read_to_string(&path).unwrap_or_default();
                assert!(
                    committed == generated,
                    "{file} is out of date, run `mise run schemas`"
                );
            }
        }
    }
}
