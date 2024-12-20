use assert_cmd::prelude::*; // Add methods on commands
use predicates::prelude::*; // Used for writing assertions
use std::process::Command; // Run programs

#[test]
fn growth_table_gets_written_successfully() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("panacus")?;

    cmd.arg("growth").arg("tests/test_files/t_groups.hist.tsv");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("panacus\thist\tgrowth"));
    Ok(())
}

#[ignore]
#[test]
fn growth_html_gets_written_successfully() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("panacus")?;

    cmd.arg("growth")
        .arg("tests/test_files/t_groups.hist.tsv")
        .arg("-o")
        .arg("html");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains(
            "<canvas id=\"chart-bar-pan-growth-node\"></canvas>",
        ))
        .stdout(predicate::str::contains("panacus\thist\tgrowth"));
    Ok(())
}
