use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;
use tempfile::TempDir;

const EXAMPLE_CATALOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/catalog");

fn on_mange_quoi() -> Command {
    cargo_bin_cmd!("on-mange-quoi")
}

fn write(dir: &Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();
    path
}

/// A copy of the example catalog, safe to modify.
fn example_copy() -> TempDir {
    let dir = TempDir::new().unwrap();
    for entry in ["ingredients.yml", "tags.yml"] {
        fs::copy(
            Path::new(EXAMPLE_CATALOG).join(entry),
            dir.path().join(entry),
        )
        .unwrap();
    }
    fs::create_dir(dir.path().join("recipes")).unwrap();
    for entry in fs::read_dir(Path::new(EXAMPLE_CATALOG).join("recipes")).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(
            &path,
            dir.path().join("recipes").join(path.file_name().unwrap()),
        )
        .unwrap();
    }
    dir
}

#[test]
fn help_lists_options() {
    on_mange_quoi()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("--catalog"))
        .stdout(contains("--port"))
        .stdout(contains("--host"))
        .stdout(contains("--validate"));
}

#[test]
fn catalog_is_required() {
    on_mange_quoi()
        .assert()
        .failure()
        .stderr(contains("--catalog"));
}

#[test]
fn missing_catalog_directory_is_rejected() {
    on_mange_quoi()
        .args(["--catalog", "does-not-exist", "--port", "0"])
        .assert()
        .failure()
        .stderr(contains("does-not-exist"));
}

#[test]
fn invalid_catalog_stops_the_server_with_every_problem() {
    let dir = example_copy();
    write(
        dir.path(),
        "recipes/pain-perdu.yml",
        "recipe:\n  name: Pain perdu\n  ingredients:\n    - pain\n    - oeufs\n",
    );
    on_mange_quoi()
        .arg("--catalog")
        .arg(dir.path())
        .args(["--port", "0"])
        .assert()
        .failure()
        .stderr(contains("pain-perdu.yml:4:7"))
        .stderr(contains("pain-perdu.yml:5:7"))
        .stderr(contains("vouliez-vous dire «\u{a0}œufs\u{a0}»"))
        .stderr(contains("2)"));
}

#[test]
fn validate_accepts_a_valid_recipe_without_serving() {
    let outside = TempDir::new().unwrap();
    let recipe = write(
        outside.path(),
        "jambon-beurre.yml",
        "recipe:\n  name: Jambon beurre\n  ingredients: [jambon, beurre]\n  tags: [rapide]\n",
    );
    on_mange_quoi()
        .args(["--catalog", EXAMPLE_CATALOG, "--validate"])
        .arg(&recipe)
        .assert()
        .success()
        .stdout(contains("recettes\u{a0}: 7"));
}

#[test]
fn validate_rejects_unknown_ingredients() {
    let outside = TempDir::new().unwrap();
    let recipe = write(
        outside.path(),
        "pain-perdu.yml",
        "recipe:\n  name: Pain perdu\n  ingredients: [pain, œufs]\n",
    );
    on_mange_quoi()
        .args(["--catalog", EXAMPLE_CATALOG, "--validate"])
        .arg(&recipe)
        .assert()
        .failure()
        .stderr(contains("ingrédient inconnu\u{a0}: «\u{a0}pain\u{a0}»"));
}

#[test]
fn validate_takes_new_ingredients_from_a_second_file() {
    let outside = TempDir::new().unwrap();
    let recipe = write(
        outside.path(),
        "pain-perdu.yml",
        "recipe:\n  name: Pain perdu\n  ingredients: [pain, œufs, lait]\n",
    );
    let example_ingredients =
        fs::read_to_string(Path::new(EXAMPLE_CATALOG).join("ingredients.yml")).unwrap();
    let ingredients = write(
        outside.path(),
        "ingredients.yml",
        &example_ingredients.replace("ingredients:\n", "ingredients:\n  - pain\n"),
    );
    on_mange_quoi()
        .args(["--catalog", EXAMPLE_CATALOG])
        .arg("--validate")
        .arg(&recipe)
        .arg("--validate")
        .arg(&ingredients)
        .assert()
        .success();
}

#[test]
fn validate_rejects_removing_a_used_ingredient() {
    let outside = TempDir::new().unwrap();
    let example_ingredients =
        fs::read_to_string(Path::new(EXAMPLE_CATALOG).join("ingredients.yml")).unwrap();
    let ingredients = write(
        outside.path(),
        "ingredients.yml",
        &example_ingredients.replace("  - jambon\n", ""),
    );
    on_mange_quoi()
        .args(["--catalog", EXAMPLE_CATALOG, "--validate"])
        .arg(&ingredients)
        .assert()
        .failure()
        .stderr(contains("«\u{a0}jambon\u{a0}»"))
        .stderr(contains("coquillettes-au-jambon-mouline, croque-monsieur"));
}

#[test]
fn validate_cannot_be_combined_with_server_options() {
    on_mange_quoi()
        .args(["--catalog", EXAMPLE_CATALOG, "--port", "9000", "--validate"])
        .arg(Path::new(EXAMPLE_CATALOG).join("tags.yml"))
        .assert()
        .failure()
        .stderr(contains("--port"));
}

#[test]
fn validate_without_file_checks_the_whole_catalog() {
    on_mange_quoi()
        .args(["--catalog", EXAMPLE_CATALOG, "--validate"])
        .assert()
        .success()
        .stdout(contains("recettes\u{a0}: 6"));

    let dir = example_copy();
    write(dir.path(), "tags.yml", "tags: [rapide]\n");
    on_mange_quoi()
        .arg("--catalog")
        .arg(dir.path())
        .arg("--validate")
        .assert()
        .failure()
        .stderr(contains("tag inconnu"));
}
