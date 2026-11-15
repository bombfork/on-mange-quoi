use std::fs;
use std::path::Path;

use serde::Deserialize;
use tempfile::TempDir;

use super::*;
use crate::catalog::ProblemKind;

/// A copy of the example catalog, safe to modify.
fn example_copy() -> TempDir {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/catalog");
    let dir = TempDir::new().unwrap();
    for name in [INGREDIENTS_FILE, TAGS_FILE] {
        fs::copy(source.join(name), dir.path().join(name)).unwrap();
    }
    fs::create_dir(dir.path().join(RECIPES_DIR)).unwrap();
    for entry in fs::read_dir(source.join(RECIPES_DIR)).unwrap() {
        let path = entry.unwrap().path();
        let target = dir.path().join(RECIPES_DIR).join(path.file_name().unwrap());
        fs::copy(&path, target).unwrap();
    }
    dir
}

fn editor(dir: &TempDir) -> Editor {
    Editor::new(dir.path().to_path_buf())
}

fn read(dir: &TempDir, path: impl AsRef<Path>) -> String {
    fs::read_to_string(dir.path().join(path)).unwrap()
}

fn input(name: &str, ingredients: &[&str], tags: &[&str]) -> RecipeInput {
    RecipeInput {
        name: name.into(),
        ingredients: ingredients.iter().map(|s| s.to_string()).collect(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
    }
}

fn kinds(error: EditError) -> Vec<ProblemKind> {
    match error {
        EditError::Invalid(problems) => problems.into_iter().map(|p| p.kind).collect(),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

/// No temporary or probe file is left behind.
fn assert_no_hidden_files(dir: &TempDir) {
    for sub in [dir.path().to_path_buf(), dir.path().join(RECIPES_DIR)] {
        for entry in fs::read_dir(sub).unwrap() {
            let name = entry.unwrap().file_name();
            assert!(
                !name.to_string_lossy().starts_with('.'),
                "{name:?} left behind"
            );
        }
    }
}

#[test]
fn written_names_parse_back_identically() {
    #[derive(Deserialize)]
    struct File {
        recipe: Entry,
    }
    #[derive(Deserialize)]
    struct Entry {
        name: String,
        ingredients: Vec<String>,
    }
    let names = [
        "lait",
        "huile d'olive",
        "crème fraîche",
        "Œufs",
        "1984",
        "yes",
        "Null",
        "a: b",
        "#tag",
        "- x",
        "\"guillemets\"",
        "pâtes (sans gluten)",
        "l’huile",
    ];
    let yaml = recipe_yaml(&input("Nom: spécial #1", &names, &[]));
    let file: File = serde_saphyr::from_str(&yaml).unwrap();
    assert_eq!(file.recipe.name, "Nom: spécial #1");
    assert_eq!(file.recipe.ingredients, names);
}

#[test]
fn writes_the_documented_layout() {
    assert_eq!(
        recipe_yaml(&input(
            "Croque-monsieur",
            &["pain de mie", "jambon"],
            &["rapide"]
        )),
        "recipe:\n  name: Croque-monsieur\n  ingredients:\n    - pain de mie\n    - jambon\n  tags:\n    - rapide\n"
    );
    assert_eq!(
        ingredients_yaml(&["lait".into()], &["sel".into()]),
        "ingredients:\n  - lait\nstaples:\n  - sel\n"
    );
    assert_eq!(ingredients_yaml(&[], &[]), "ingredients: []\n");
    assert_eq!(tags_yaml(&[]), "tags: []\n");
}

#[test]
fn slugs_are_lowercase_ascii() {
    assert_eq!(
        slug("Coquillettes au jambon mouliné"),
        "coquillettes-au-jambon-mouline"
    );
    assert_eq!(slug("  Œufs brouillés !"), "oeufs-brouilles");
    assert_eq!(slug("L’omelette"), "l-omelette");
    assert_eq!(slug("!!!"), "recette");
}

#[test]
fn creates_a_recipe_named_after_its_title() {
    let dir = example_copy();
    let editor = editor(&dir);
    let id = editor
        .create_recipe(&input(
            "Jambon beurre",
            &["pain de mie", "jambon", "beurre"],
            &["rapide"],
        ))
        .unwrap();
    assert_eq!(id, "jambon-beurre");
    assert!(read(&dir, recipe_path(&id)).contains("name: Jambon beurre"));
    assert_eq!(editor.catalog().unwrap().recipes.len(), 7);
    assert_no_hidden_files(&dir);
}

#[test]
fn a_recipe_can_add_new_ingredients_to_the_catalog() {
    let dir = example_copy();
    let editor = editor(&dir);
    editor
        .create_recipe(&input(
            "Pain perdu",
            &["pain", "œufs", "lait", "cannelle"],
            &[],
        ))
        .unwrap();
    let catalog = editor.catalog().unwrap();
    let regular: Vec<&str> = catalog
        .ingredients
        .iter()
        .filter(|i| !i.staple)
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(regular[regular.len() - 2..], ["pain", "cannelle"]);
    assert!(catalog.recipes.iter().any(|r| r.id == "pain-perdu"));

    // A new name that duplicates an existing one is refused, and nothing is written.
    let before = read(&dir, INGREDIENTS_FILE);
    let error = editor
        .create_recipe(&input("Pâtes au beurre", &["Pates", "beurre"], &[]))
        .unwrap_err();
    let kinds = kinds(error);
    assert!(
        kinds
            .iter()
            .any(|kind| matches!(kind, ProblemKind::DuplicateIngredient { .. })),
        "{kinds:?}"
    );
    assert_eq!(read(&dir, INGREDIENTS_FILE), before);
    assert!(!dir.path().join(recipe_path("pates-au-beurre")).exists());
}

#[test]
fn new_recipe_identifiers_do_not_collide() {
    let dir = example_copy();
    let id = editor(&dir)
        .create_recipe(&input("Ratatouille !", &["courgettes"], &[]))
        .unwrap();
    assert_eq!(id, "ratatouille-2");
}

#[test]
fn duplicate_recipe_name_is_refused() {
    let dir = example_copy();
    let error = editor(&dir)
        .create_recipe(&input("ratatouille", &["courgettes"], &[]))
        .unwrap_err();
    assert!(
        matches!(
            kinds(error).as_slice(),
            [ProblemKind::DuplicateRecipeName { other, .. }]
                if *other == dir.path().join(recipe_path("ratatouille"))
        ),
        "points at the real file, not the staging copy"
    );
}

#[test]
fn updates_a_recipe_in_place_when_the_version_matches() {
    let dir = example_copy();
    let editor = editor(&dir);
    let path = recipe_path("ratatouille");
    let version = editor.version(&path);
    let updated = input(
        "Ratatouille niçoise",
        &["courgettes", "tomates"],
        &["végétarien"],
    );

    editor
        .update_recipe("ratatouille", &version, &updated)
        .unwrap();
    assert_eq!(read(&dir, &path), recipe_yaml(&updated));

    // The version shown before the first update is now stale.
    let error = editor
        .update_recipe("ratatouille", &version, &input("Autre", &["ail"], &[]))
        .unwrap_err();
    assert!(matches!(error, EditError::Conflict));
    assert_eq!(read(&dir, &path), recipe_yaml(&updated));
}

#[test]
fn deletes_a_recipe() {
    let dir = example_copy();
    let editor = editor(&dir);
    let path = recipe_path("ratatouille");
    editor
        .delete_recipe("ratatouille", &editor.version(&path))
        .unwrap();
    assert!(!dir.path().join(path).exists());
    assert!(matches!(
        editor.delete_recipe("ratatouille", "x"),
        Err(EditError::NotFound)
    ));
}

#[test]
fn adds_ingredients_and_staples_and_refuses_duplicates() {
    let dir = example_copy();
    let editor = editor(&dir);
    let version = || editor.version(Path::new(INGREDIENTS_FILE));
    editor.add_ingredient(&version(), "pain", false).unwrap();
    editor.add_ingredient(&version(), "muscade", true).unwrap();
    let catalog = editor.catalog().unwrap();
    let last_regular = catalog.ingredients.iter().rfind(|i| !i.staple).unwrap();
    assert_eq!(last_regular.name, "pain");
    assert_eq!(catalog.ingredients.last().unwrap().name, "muscade");

    let error = editor
        .add_ingredient(&version(), "Pates", false)
        .unwrap_err();
    assert!(matches!(
        kinds(error).as_slice(),
        [ProblemKind::DuplicateIngredient { other, .. }] if other == "pâtes"
    ));
}

#[test]
fn renaming_an_ingredient_renames_it_in_recipes() {
    let dir = example_copy();
    let editor = editor(&dir);
    let version = editor.version(Path::new(INGREDIENTS_FILE));
    editor
        .update_ingredient(&version, "jambon", "jambon blanc", false)
        .unwrap();
    let catalog = editor.catalog().unwrap();
    let position = |name: &str| catalog.ingredients.iter().position(|i| i.name == name);
    assert_eq!(position("jambon"), None);
    assert_eq!(position("jambon blanc"), Some(7), "same place in the list");
    let croque = catalog
        .recipes
        .iter()
        .find(|r| r.id == "croque-monsieur")
        .unwrap();
    assert_eq!(
        croque.ingredients,
        ["pain de mie", "jambon blanc", "emmental", "beurre"]
    );
}

#[test]
fn an_ingredient_can_become_a_staple() {
    let dir = example_copy();
    let editor = editor(&dir);
    let version = editor.version(Path::new(INGREDIENTS_FILE));
    editor
        .update_ingredient(&version, "beurre", "beurre", true)
        .unwrap();
    let catalog = editor.catalog().unwrap();
    let last = catalog.ingredients.last().unwrap();
    assert_eq!((last.name.as_str(), last.staple), ("beurre", true));
}

#[test]
fn a_used_ingredient_cannot_be_deleted() {
    let dir = example_copy();
    let editor = editor(&dir);
    let version = || editor.version(Path::new(INGREDIENTS_FILE));
    let error = editor.delete_ingredient(&version(), "jambon").unwrap_err();
    let EditError::InUse(recipes) = error else {
        panic!("expected InUse, got {error:?}");
    };
    assert_eq!(
        recipes,
        ["Coquillettes au jambon mouliné", "Croque-monsieur"]
    );

    editor.add_ingredient(&version(), "pain", false).unwrap();
    editor.delete_ingredient(&version(), "pain").unwrap();
    assert!(
        !editor
            .catalog()
            .unwrap()
            .ingredients
            .iter()
            .any(|i| i.name == "pain")
    );
}

#[test]
fn tags_can_be_added_renamed_and_deleted() {
    let dir = example_copy();
    let editor = editor(&dir);
    let version = || editor.version(Path::new(TAGS_FILE));
    editor.add_tag(&version(), "hiver").unwrap();
    editor.update_tag(&version(), "rapide", "express").unwrap();
    let catalog = editor.catalog().unwrap();
    assert_eq!(catalog.tags, ["végétarien", "enfants", "express", "hiver"]);
    let croque = catalog
        .recipes
        .iter()
        .find(|r| r.id == "croque-monsieur")
        .unwrap();
    assert_eq!(croque.tags, ["enfants", "express"]);

    assert!(matches!(
        editor.delete_tag(&version(), "express"),
        Err(EditError::InUse(_))
    ));
    editor.delete_tag(&version(), "hiver").unwrap();
    assert!(matches!(
        editor.add_tag(&version(), "Enfant"),
        Err(EditError::Invalid(_))
    ));
}

#[test]
fn an_invalid_catalog_on_disk_cannot_be_edited() {
    let dir = example_copy();
    fs::write(dir.path().join(TAGS_FILE), "tags: [rapide]\n").unwrap();
    let error = editor(&dir)
        .create_recipe(&input("Pain", &["beurre"], &[]))
        .unwrap_err();
    assert!(matches!(error, EditError::Invalid(_)));
}

#[cfg(unix)]
#[test]
fn a_read_only_catalog_cannot_be_edited() {
    use std::os::unix::fs::PermissionsExt;

    let dir = example_copy();
    let set_mode = |mode| {
        for sub in [dir.path().to_path_buf(), dir.path().join(RECIPES_DIR)] {
            fs::set_permissions(sub, fs::Permissions::from_mode(mode)).unwrap();
        }
    };
    set_mode(0o555);
    let editor = editor(&dir);
    let writable = editor.is_writable();
    let result = editor.create_recipe(&input("Pain", &["beurre"], &[]));
    set_mode(0o755);
    // Permissions do not apply to root: nothing to check then.
    if writable {
        return;
    }
    assert!(matches!(result, Err(EditError::ReadOnly)));
}

mod import {
    use super::*;
    use crate::editor::{ImportError, ImportProblem, ImportProblemKind, ImportReport};

    fn problems(error: ImportError) -> Vec<ImportProblem> {
        match error {
            ImportError::Invalid(problems) => problems,
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    const PAIN_PERDU: &str = "\
ingredients:
  - pain
  - Oeufs
  - lait
  - cannelle
staples:
  - sel
  - sucre
---
recipe:
  name: Pain perdu
  ingredients:
    - pain
    - oeufs
    - lait
    - cannelle
    - sucre
  tags:
    - Enfants
";

    #[test]
    fn imports_ingredients_and_recipes_and_skips_known_names() {
        let dir = example_copy();
        let editor = editor(&dir);
        let report = editor.import(PAIN_PERDU).unwrap();
        assert_eq!(
            report,
            ImportReport {
                recipes: vec!["Pain perdu".into()],
                new_ingredients: vec!["pain".into(), "cannelle".into(), "sucre".into()],
                mapped: vec![
                    ("Oeufs".into(), "œufs".into()),
                    ("oeufs".into(), "œufs".into()),
                    ("Enfants".into(), "enfants".into()),
                ],
            }
        );
        let catalog = editor.catalog().unwrap();
        let sucre = catalog
            .ingredients
            .iter()
            .find(|i| i.name == "sucre")
            .unwrap();
        assert!(sucre.staple);
        let recipe = catalog
            .recipes
            .iter()
            .find(|r| r.id == "pain-perdu")
            .unwrap();
        assert_eq!(
            recipe.ingredients,
            ["pain", "œufs", "lait", "cannelle", "sucre"]
        );
        assert_eq!(recipe.tags, ["enfants"]);
        assert_no_hidden_files(&dir);
    }

    #[test]
    fn several_recipes_in_one_input() {
        let dir = example_copy();
        let input = "\
recipe:
  name: Tartines
  ingredients: [pain de mie, beurre]
---
recipe:
  name: Omelette nature
  ingredients: [œufs, sel]
";
        let report = editor(&dir).import(input).unwrap();
        assert_eq!(report.recipes, ["Tartines", "Omelette nature"]);
        assert!(report.new_ingredients.is_empty());
        assert!(dir.path().join(recipe_path("omelette-nature")).exists());
    }

    #[test]
    fn undeclared_ingredients_are_refused_with_their_line() {
        let dir = example_copy();
        let before = read(&dir, INGREDIENTS_FILE);
        let input = "\
ingredients: [cannelle]
---
recipe:
  name: Pain perdu
  ingredients:
    - pain
    - cannelle
";
        let problems = problems(editor(&dir).import(input).unwrap_err());
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].line, Some(6));
        assert!(matches!(
            &problems[0].kind,
            ImportProblemKind::Catalog(ProblemKind::UnknownIngredient { name, .. }) if name == "pain"
        ));
        assert_eq!(read(&dir, INGREDIENTS_FILE), before, "nothing written");
        assert!(!dir.path().join(recipe_path("pain-perdu")).exists());
    }

    #[test]
    fn known_recipe_names_are_refused() {
        let dir = example_copy();
        let input = "recipe:\n  name: ratatouille\n  ingredients: [courgettes]\n";
        let problems = problems(editor(&dir).import(input).unwrap_err());
        assert!(matches!(
            problems.as_slice(),
            [ImportProblem {
                line: Some(2),
                kind: ImportProblemKind::Catalog(ProblemKind::DuplicateRecipeName { .. })
            }]
        ));
    }

    #[test]
    fn unknown_keys_mixed_documents_and_syntax_errors_are_located() {
        let dir = example_copy();
        let editor = editor(&dir);
        let problems_of = |input: &str| problems(editor.import(input).unwrap_err());

        assert_eq!(
            problems_of("ingredients: [pain]\n---\ntags: [hiver]\n"),
            [ImportProblem {
                line: Some(3),
                kind: ImportProblemKind::UnknownKey("tags".into())
            }]
        );
        assert_eq!(
            problems_of("ingredients: [pain]\nrecipe:\n  name: X\n  ingredients: [pain]\n"),
            [ImportProblem {
                line: Some(1),
                kind: ImportProblemKind::Mixed
            }]
        );
        assert!(matches!(
            problems_of("recipe:\n  name: [x\n").as_slice(),
            [ImportProblem {
                line: Some(2),
                kind: ImportProblemKind::Syntax(_)
            }]
        ));
        assert!(matches!(
            problems_of("ingredients:\n  - ''\n").as_slice(),
            [ImportProblem {
                line: Some(2),
                kind: ImportProblemKind::Catalog(ProblemKind::EmptyName)
            }]
        ));
    }

    #[test]
    fn empty_input_imports_nothing() {
        let dir = example_copy();
        for input in ["", "# rien\n", "---\n---\n", "ingredients: [œufs, lait]\n"] {
            assert!(
                matches!(editor(&dir).import(input), Err(ImportError::Empty)),
                "{input:?}"
            );
        }
    }
}
