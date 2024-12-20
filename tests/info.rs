use assert_cmd::prelude::*; // Add methods on commands
use predicates::prelude::*; // Used for writing assertions
use std::process::Command; // Run programs

#[ignore]
#[test]
fn info_table_gets_written_successfully() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("panacus")?;

    cmd.arg("info").arg("tests/test_files/t_groups.gfa");
    cmd.assert().success().stdout(predicate::str::contains(
        "feature\tcategory\tcountable\tvalue",
    ));
    Ok(())
}

#[ignore]
#[test]
fn info_html_gets_written_successfully() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("panacus")?;

    cmd.arg("info")
        .arg("tests/test_files/t_groups.gfa")
        .arg("-o")
        .arg("html");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("<th scope=\"col\">feature</th>"))
        .stdout(predicate::str::contains(
            "feature\tcategory\tcountable\tvalue",
        ));
    Ok(())
}

#[ignore]
#[test]
fn info_table_groups_get_written() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("panacus")?;

    cmd.arg("info")
        .arg("tests/test_files/t_groups.gfa")
        .arg("-S");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("group\tx\tbp\t50"))
        .stdout(predicate::str::contains("group\tx\tnode\t10"))
        .stdout(predicate::str::contains("group\ty\tbp\t50"))
        .stdout(predicate::str::contains("group\ty\tnode\t10"));
    Ok(())
}
