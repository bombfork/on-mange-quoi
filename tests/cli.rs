use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;

fn on_mange_quoi() -> Command {
    cargo_bin_cmd!("on-mange-quoi")
}

#[test]
fn help_lists_options() {
    on_mange_quoi()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("--catalog"))
        .stdout(contains("--port"))
        .stdout(contains("--host"));
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
