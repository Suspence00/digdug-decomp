use std::process::Command;

#[test]
fn help_succeeds_without_loading_arcade_data() {
    let output = Command::new(env!("CARGO_BIN_EXE_verify_decomp"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("--strict-registers"));
    assert!(help.contains("2 inconclusive"));
}

#[test]
fn report_only_does_not_mask_invalid_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_verify_decomp"))
        .args(["--frames", "0", "--report-only"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(64));
    assert!(String::from_utf8(output.stderr).unwrap().contains("greater than zero"));
}
