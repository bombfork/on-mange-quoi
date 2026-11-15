use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::*;

const INGREDIENTS: &str = "\
ingredients:
  - œufs
  - coquillettes
  - jambon
  - beurre
staples:
  - sel
";

const TAGS: &str = "\
tags:
  - enfants
  - rapide
";

const RECIPE: &str = "\
recipe:
  name: Coquillettes au jambon
  ingredients:
    - coquillettes
    - jambon
    - sel
  tags:
    - enfants
";

/// A catalog directory with the base files, where `files` add or override entries.
fn catalog_dir(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    let base = [
        ("ingredients.yml", INGREDIENTS),
        ("tags.yml", TAGS),
        ("recipes/coquillettes-au-jambon.yml", RECIPE),
    ];
    for (name, content) in base.iter().chain(files) {
        write(dir.path(), name, content);
    }
    dir
}

fn write(dir: &Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();
    path
}

fn problems(result: Result<Catalog, Vec<Problem>>) -> Vec<ProblemKind> {
    match result {
        Ok(_) => Vec::new(),
        Err(problems) => problems.into_iter().map(|problem| problem.kind).collect(),
    }
}

fn load_problems(files: &[(&str, &str)]) -> Vec<ProblemKind> {
    let dir = catalog_dir(files);
    problems(load(dir.path()))
}

fn recipe(name: &str, ingredients: &[&str], tags: &[&str]) -> String {
    let list = |items: &[&str]| {
        items
            .iter()
            .map(|item| format!("\n    - {item}"))
            .collect::<String>()
    };
    format!(
        "recipe:\n  name: {name}\n  ingredients:{}\n  tags:{}\n",
        list(ingredients),
        list(tags)
    )
}

#[test]
fn example_catalog_is_valid() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/catalog");
    let catalog = load(&dir).unwrap();
    assert_eq!(catalog.recipes.len(), 6);
    assert!(
        catalog
            .ingredients
            .iter()
            .any(|i| i.name == "sel" && i.staple)
    );
}

#[test]
fn loads_base_catalog() {
    let dir = catalog_dir(&[]);
    let catalog = load(dir.path()).unwrap();
    assert_eq!(catalog.ingredients.len(), 5);
    assert_eq!(catalog.tags, ["enfants", "rapide"]);
    assert_eq!(
        catalog.recipes,
        [Recipe {
            id: "coquillettes-au-jambon".into(),
            name: "Coquillettes au jambon".into(),
            ingredients: vec!["coquillettes".into(), "jambon".into(), "sel".into()],
            tags: vec!["enfants".into()],
        }]
    );
}

#[test]
fn missing_layout_entries_are_reported() {
    let dir = TempDir::new().unwrap();
    let errors = load(dir.path()).unwrap_err();
    let paths: Vec<_> = errors
        .iter()
        .map(|p| p.path.strip_prefix(dir.path()).unwrap().to_path_buf())
        .collect();
    assert_eq!(
        paths,
        [
            PathBuf::from("ingredients.yml"),
            PathBuf::from("recipes"),
            PathBuf::from("tags.yml")
        ]
    );
    assert!(errors.iter().all(|p| p.kind == ProblemKind::Missing));
}

#[test]
fn syntax_errors_carry_position() {
    let dir = catalog_dir(&[("recipes/casse.yml", "recipe:\n  name: [x\n")]);
    let errors = load(dir.path()).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(matches!(errors[0].kind, ProblemKind::Syntax(_)));
    assert_eq!(errors[0].position.map(|p| p.line), Some(2));
}

#[test]
fn unknown_keys_are_rejected() {
    let kinds = load_problems(&[("tags.yml", "tags: [rapide]\ncolor: red\n")]);
    assert!(matches!(kinds.as_slice(), [ProblemKind::Syntax(m)] if m.contains("color")));
}

#[test]
fn duplicate_ingredients_are_detected_after_normalization() {
    let kinds = load_problems(&[(
        "ingredients.yml",
        "ingredients:\n  - pâtes\n  - Pates\n  - jambon\n  - coquillettes\nstaples:\n  - sel\n  - du sel\n",
    )]);
    assert_eq!(
        kinds,
        [
            ProblemKind::DuplicateIngredient {
                name: "Pates".into(),
                other: "pâtes".into(),
                line: 2
            },
            ProblemKind::DuplicateIngredient {
                name: "du sel".into(),
                other: "sel".into(),
                line: 7
            },
        ]
    );
}

#[test]
fn a_name_cannot_be_both_ingredient_and_staple() {
    let kinds = load_problems(&[(
        "ingredients.yml",
        "ingredients: [coquillettes, jambon, sel]\nstaples: [sel]\n",
    )]);
    assert!(matches!(
        kinds.as_slice(),
        [ProblemKind::DuplicateIngredient { name, .. }] if name == "sel"
    ));
}

#[test]
fn duplicate_tags_are_detected() {
    let kinds = load_problems(&[("tags.yml", "tags: [enfants, rapide, Rapides]\n")]);
    assert!(matches!(
        kinds.as_slice(),
        [ProblemKind::DuplicateTag { name, other, .. }] if name == "Rapides" && other == "rapide"
    ));
}

#[test]
fn duplicate_recipe_names_are_detected() {
    let other = recipe("coquillettes au Jambon", &["coquillettes"], &[]);
    let kinds = load_problems(&[("recipes/autre.yml", &other)]);
    assert!(matches!(
        kinds.as_slice(),
        [ProblemKind::DuplicateRecipeName { other, .. }] if other.ends_with("autre.yml")
    ));
}

#[test]
fn recipe_file_names_must_be_lowercase_ascii() {
    let other = recipe("Jambon beurre", &["jambon", "beurre"], &[]);
    let kinds = load_problems(&[("recipes/Jambon-beurré.yml", &other)]);
    assert_eq!(
        kinds,
        [ProblemKind::InvalidRecipeId("Jambon-beurré".into())]
    );
}

#[test]
fn only_yml_files_are_accepted_in_recipes() {
    let other = recipe("Jambon beurre", &["jambon", "beurre"], &[]);
    let dir = catalog_dir(&[
        ("recipes/jambon-beurre.yaml", &other),
        ("recipes/.hidden.swp", "ignored"),
    ]);
    let errors = load(dir.path()).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ProblemKind::UnexpectedFile);
    assert!(errors[0].path.ends_with("jambon-beurre.yaml"));
}

#[test]
fn recipes_need_a_name_and_ingredients() {
    let kinds = load_problems(&[
        (
            "recipes/sans-nom.yml",
            "recipe:\n  name: ''\n  ingredients: [jambon]\n",
        ),
        (
            "recipes/vide.yml",
            "recipe:\n  name: Vide\n  ingredients:\n",
        ),
        ("recipes/absent.yml", "recipe:\n  name: Absent\n"),
    ]);
    assert_eq!(
        kinds,
        [
            ProblemKind::NoIngredients,
            ProblemKind::EmptyName,
            ProblemKind::NoIngredients
        ]
    );
}

#[test]
fn ingredients_and_tags_appear_once_per_recipe() {
    let doubled = recipe("Double", &["jambon", "jambon"], &["rapide", "rapide"]);
    let kinds = load_problems(&[("recipes/double.yml", &doubled)]);
    assert_eq!(
        kinds,
        [
            ProblemKind::RepeatedInRecipe("jambon".into()),
            ProblemKind::RepeatedInRecipe("rapide".into())
        ]
    );
}

#[test]
fn tags_may_be_absent_or_empty() {
    let kinds = load_problems(&[
        (
            "recipes/a.yml",
            "recipe:\n  name: A\n  ingredients: [jambon]\n",
        ),
        (
            "recipes/b.yml",
            "recipe:\n  name: B\n  ingredients: [beurre]\n  tags:\n",
        ),
    ]);
    assert!(kinds.is_empty(), "{kinds:?}");
}

#[test]
fn unknown_references_suggest_the_closest_name() {
    let bad = recipe(
        "Omelette",
        &["oeufs", "coquilettes", "chocolat"],
        &["Rapide"],
    );
    let dir = catalog_dir(&[("recipes/omelette.yml", &bad)]);
    let errors = load(dir.path()).unwrap_err();
    let kinds: Vec<_> = errors.iter().map(|p| &p.kind).collect();
    assert_eq!(
        kinds,
        [
            &ProblemKind::UnknownIngredient {
                name: "oeufs".into(),
                suggestion: Some("œufs".into())
            },
            &ProblemKind::UnknownIngredient {
                name: "coquilettes".into(),
                suggestion: Some("coquillettes".into())
            },
            &ProblemKind::UnknownIngredient {
                name: "chocolat".into(),
                suggestion: None
            },
            &ProblemKind::UnknownTag {
                name: "Rapide".into(),
                suggestion: Some("rapide".into())
            },
        ]
    );
    assert_eq!(errors[0].position, Some(Position { line: 4, column: 7 }));
}

#[test]
fn references_are_not_checked_without_the_ingredients_file() {
    let dir = catalog_dir(&[]);
    fs::remove_file(dir.path().join("ingredients.yml")).unwrap();
    assert_eq!(problems(load(dir.path())), [ProblemKind::Missing]);
}

#[test]
fn extra_recipe_is_checked_against_the_catalog() {
    let dir = catalog_dir(&[]);
    let outside = TempDir::new().unwrap();
    let good = write(
        outside.path(),
        "jambon-beurre.yml",
        &recipe("Jambon beurre", &["jambon", "beurre"], &["rapide"]),
    );
    let catalog = load_with(dir.path(), &[good]).unwrap();
    assert_eq!(catalog.recipes.len(), 2);

    let bad = write(
        outside.path(),
        "pain-perdu.yml",
        &recipe("Pain perdu", &["pain", "œufs"], &[]),
    );
    assert!(matches!(
        problems(load_with(dir.path(), &[bad])).as_slice(),
        [ProblemKind::UnknownIngredient { name, .. }] if name == "pain"
    ));
}

#[test]
fn extra_recipe_replaces_the_catalog_recipe_of_the_same_name() {
    let dir = catalog_dir(&[]);
    let outside = TempDir::new().unwrap();
    let renamed = write(
        outside.path(),
        "coquillettes-au-jambon.yml",
        &recipe("Coquillettes au jambon mouliné", &["coquillettes"], &[]),
    );
    let catalog = load_with(dir.path(), &[renamed]).unwrap();
    assert_eq!(catalog.recipes.len(), 1);
    assert_eq!(catalog.recipes[0].name, "Coquillettes au jambon mouliné");
}

#[test]
fn new_ingredients_can_come_from_an_extra_ingredients_file() {
    let dir = catalog_dir(&[]);
    let outside = TempDir::new().unwrap();
    let new_recipe = write(
        outside.path(),
        "pain-perdu.yml",
        &recipe("Pain perdu", &["pain", "œufs"], &[]),
    );
    let ingredients = write(
        outside.path(),
        "ingredients.yml",
        &format!("{INGREDIENTS}  - poivre\n").replace("  - beurre\n", "  - beurre\n  - pain\n"),
    );
    let catalog = load_with(dir.path(), &[new_recipe, ingredients]).unwrap();
    assert!(catalog.ingredients.iter().any(|i| i.name == "pain"));
}

#[test]
fn removing_a_used_ingredient_is_reported_on_the_new_file() {
    let dir = catalog_dir(&[]);
    let outside = TempDir::new().unwrap();
    let ingredients = write(
        outside.path(),
        "ingredients.yml",
        &INGREDIENTS.replace("  - jambon\n", ""),
    );
    let errors = load_with(dir.path(), std::slice::from_ref(&ingredients)).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].path, ingredients);
    assert_eq!(
        errors[0].kind,
        ProblemKind::IngredientInUse {
            name: "jambon".into(),
            recipes: vec!["coquillettes-au-jambon".into()]
        }
    );
}

#[test]
fn removing_a_used_tag_is_reported_on_the_new_file() {
    let dir = catalog_dir(&[]);
    let outside = TempDir::new().unwrap();
    let tags = write(outside.path(), "tags.yml", "tags: [rapide]\n");
    assert_eq!(
        problems(load_with(dir.path(), &[tags])),
        [ProblemKind::TagInUse {
            name: "enfants".into(),
            recipes: vec!["coquillettes-au-jambon".into()]
        }]
    );
}

#[test]
fn extra_files_of_unknown_kind_or_given_twice_are_rejected() {
    let dir = catalog_dir(&[]);
    let outside = TempDir::new().unwrap();
    let unknown = write(outside.path(), "autre.yml", "menu: []\n");
    let tags_a = write(outside.path(), "a.yml", TAGS);
    let tags_b = write(outside.path(), "b.yml", TAGS);
    assert_eq!(
        problems(load_with(dir.path(), &[unknown, tags_a, tags_b])),
        [
            ProblemKind::UnknownFileType,
            ProblemKind::SeveralFilesOfKind
        ]
    );
}

#[test]
fn missing_extra_file_is_reported() {
    let dir = catalog_dir(&[]);
    let missing = dir.path().join("nope.yml");
    assert_eq!(
        problems(load_with(dir.path(), &[missing])),
        [ProblemKind::Missing]
    );
}

#[test]
fn fingerprint_follows_the_files_load_reads() {
    let dir = catalog_dir(&[]);
    let base = fingerprint(dir.path());
    assert_eq!(fingerprint(dir.path()), base);

    write(dir.path(), "recipes/.recette.yml.swp", "ignored");
    assert_eq!(fingerprint(dir.path()), base, "hidden files are ignored");

    write(
        dir.path(),
        "tags.yml",
        "tags: [enfants, rapide, végétarien]\n",
    );
    let edited = fingerprint(dir.path());
    assert_ne!(edited, base);

    let added = write(dir.path(), "recipes/autre.yml", RECIPE);
    assert_ne!(fingerprint(dir.path()), edited);
    fs::remove_file(added).unwrap();
    assert_eq!(fingerprint(dir.path()), edited);
}
