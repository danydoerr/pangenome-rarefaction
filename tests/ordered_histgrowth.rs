use assert_cmd::prelude::*; // Add methods on commands
use predicates::prelude::*; // Used for writing assertions
use std::process::Command; // Run programs

#[ignore]
#[test]
fn ordered_histgrowth_table_gets_written_successfully() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("panacus")?;

    cmd.arg("ordered-histgrowth")
        .arg("tests/test_files/t_groups.gfa");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("panacus\tordered-growth"));
    Ok(())
}

#[ignore]
#[test]
fn ordered_histgrowth_html_gets_written_successfully() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("panacus")?;

    cmd.arg("ordered-histgrowth")
        .arg("tests/test_files/t_groups.gfa")
        .arg("-o")
        .arg("html");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains(
            "<canvas id=\"chart-bar-pan-growth-node\"></canvas>",
        ))
        .stdout(predicate::str::contains("panacus\tordered-growth"));
    Ok(())
}
