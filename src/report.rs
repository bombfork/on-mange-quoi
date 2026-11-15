//! French rendering of catalog problems, for the command line and the web pages.

use crate::catalog::{Problem, ProblemKind};
use crate::i18n::{MESSAGES, interpolate};

/// One line: the location (`path:line:column` when known) and the message.
pub fn describe(problem: &Problem) -> String {
    let location = match problem.position {
        Some(position) => format!(
            "{}:{}:{}",
            problem.path.display(),
            position.line,
            position.column
        ),
        None => problem.path.display().to_string(),
    };
    interpolate(
        &MESSAGES.catalog.problem,
        &[
            ("location", &location),
            ("message", &message(&problem.kind)),
        ],
    )
}

/// The message alone, without location.
pub fn message(kind: &ProblemKind) -> String {
    let m = &MESSAGES.catalog;
    match kind {
        ProblemKind::Unreadable(error) => interpolate(&m.unreadable, &[("error", error)]),
        ProblemKind::Missing => m.missing.clone(),
        ProblemKind::Syntax(error) => interpolate(&m.syntax, &[("error", error)]),
        ProblemKind::UnknownFileType => m.unknown_file_type.clone(),
        ProblemKind::SeveralFilesOfKind => m.several_files_of_kind.clone(),
        ProblemKind::UnexpectedFile => m.unexpected_file.clone(),
        ProblemKind::InvalidRecipeId(id) => interpolate(&m.invalid_recipe_id, &[("id", id)]),
        ProblemKind::EmptyName => m.empty_name.clone(),
        ProblemKind::NoIngredients => m.no_ingredients.clone(),
        ProblemKind::DuplicateIngredient { name, other, line } => interpolate(
            &m.duplicate_ingredient,
            &[("name", name), ("other", other), ("line", line)],
        ),
        ProblemKind::DuplicateTag { name, other, line } => interpolate(
            &m.duplicate_tag,
            &[("name", name), ("other", other), ("line", line)],
        ),
        ProblemKind::DuplicateIngredientTag { name, other, line } => interpolate(
            &m.duplicate_ingredient_tag,
            &[("name", name), ("other", other), ("line", line)],
        ),
        ProblemKind::RepeatedInIngredientTag { tag, name } => interpolate(
            &m.repeated_in_ingredient_tag,
            &[("tag", tag), ("name", name)],
        ),
        ProblemKind::EmptyChoice => m.empty_choice.clone(),
        ProblemKind::RepeatedChoice(name) => interpolate(&m.repeated_choice, &[("name", name)]),
        ProblemKind::DuplicateRecipeName { name, other } => interpolate(
            &m.duplicate_recipe_name,
            &[("name", name), ("other", &other.display())],
        ),
        ProblemKind::RepeatedInRecipe(name) => {
            interpolate(&m.repeated_in_recipe, &[("name", name)])
        }
        ProblemKind::UnknownIngredient { name, suggestion } => {
            interpolate(&m.unknown_ingredient, &[("name", name)]) + &did_you_mean(suggestion)
        }
        ProblemKind::UnknownTag { name, suggestion } => {
            interpolate(&m.unknown_tag, &[("name", name)]) + &did_you_mean(suggestion)
        }
        ProblemKind::IngredientInUse { name, recipes } => interpolate(
            &m.ingredient_in_use,
            &[("name", name), ("recipes", &recipes.join(", "))],
        ),
        ProblemKind::TagInUse { name, recipes } => interpolate(
            &m.tag_in_use,
            &[("name", name), ("recipes", &recipes.join(", "))],
        ),
    }
}

fn did_you_mean(suggestion: &Option<String>) -> String {
    suggestion
        .as_ref()
        .map(|name| interpolate(&MESSAGES.catalog.did_you_mean, &[("name", name)]))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::catalog::Position;

    fn problem(kind: ProblemKind) -> Problem {
        Problem {
            path: PathBuf::from("recipes/omelette.yml"),
            position: Some(Position { line: 4, column: 7 }),
            kind,
        }
    }

    #[test]
    fn describes_location_and_suggestion() {
        let text = describe(&problem(ProblemKind::UnknownIngredient {
            name: "oeufs".into(),
            suggestion: Some("œufs".into()),
        }));
        assert!(
            text.starts_with("recipes/omelette.yml:4:7\u{a0}: "),
            "{text}"
        );
        assert!(text.contains("oeufs") && text.contains("œufs"), "{text}");
    }

    #[test]
    fn every_kind_fills_its_placeholders() {
        let kinds = [
            ProblemKind::Unreadable("e".into()),
            ProblemKind::Missing,
            ProblemKind::Syntax("e".into()),
            ProblemKind::UnknownFileType,
            ProblemKind::SeveralFilesOfKind,
            ProblemKind::UnexpectedFile,
            ProblemKind::InvalidRecipeId("X".into()),
            ProblemKind::EmptyName,
            ProblemKind::NoIngredients,
            ProblemKind::DuplicateIngredient {
                name: "a".into(),
                other: "b".into(),
                line: 1,
            },
            ProblemKind::DuplicateTag {
                name: "a".into(),
                other: "b".into(),
                line: 1,
            },
            ProblemKind::DuplicateRecipeName {
                name: "a".into(),
                other: "b.yml".into(),
            },
            ProblemKind::EmptyChoice,
            ProblemKind::RepeatedChoice("a".into()),
            ProblemKind::DuplicateIngredientTag {
                name: "a".into(),
                other: "b".into(),
                line: 1,
            },
            ProblemKind::RepeatedInIngredientTag {
                tag: "t".into(),
                name: "a".into(),
            },
            ProblemKind::RepeatedInRecipe("a".into()),
            ProblemKind::UnknownIngredient {
                name: "a".into(),
                suggestion: Some("b".into()),
            },
            ProblemKind::UnknownTag {
                name: "a".into(),
                suggestion: None,
            },
            ProblemKind::IngredientInUse {
                name: "a".into(),
                recipes: vec!["r".into()],
            },
            ProblemKind::TagInUse {
                name: "a".into(),
                recipes: vec!["r".into()],
            },
        ];
        for kind in kinds {
            let text = describe(&problem(kind));
            assert!(!text.contains('{') && !text.contains('}'), "{text}");
        }
    }
}
