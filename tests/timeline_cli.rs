use std::process::Command;

#[test]
fn help_and_invalid_arguments_do_not_require_arcade_data() {
    let help = Command::new(env!("CARGO_BIN_EXE_timeline"))
        .arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8(help.stdout).unwrap().contains("2 inconclusive"));
    let invalid = Command::new(env!("CARGO_BIN_EXE_timeline"))
        .args(["capture", "--script", "absent", "--output", "absent", "--frames", "0"])
        .output().unwrap();
    assert_eq!(invalid.status.code(), Some(64));
}

#[test]
fn incomplete_captures_cannot_pass_the_cli() {
    let directory = std::env::temp_dir().join(format!("digdug-timeline-cli-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("incomplete.txt");
    let text = format!(
        "# digdug-timeline-v2\n# dip_a=66 dip_b=DB earom={} rom_hash=12345678 script_start=1 frames=2\n1 00 {}\n",
        "00".repeat(64), "00".repeat(8192)
    );
    std::fs::write(&path, text).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_timeline"))
        .arg("compare").arg("--native").arg(&path).arg("--reference").arg(&path)
        .output().unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&directory).unwrap();
    assert_eq!(output.status.code(), Some(2), "{}", String::from_utf8_lossy(&output.stderr));
}
