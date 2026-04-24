use std::process::Command;

#[test]
#[ignore]
fn installed_binary_matches_cargo_build() {
    let which_output = Command::new("which")
        .arg("rigor")
        .output()
        .expect("which rigor");

    if !which_output.status.success() {
        eprintln!("No installed rigor binary found in PATH — skipping drift check");
        return;
    }

    let installed_path = String::from_utf8_lossy(&which_output.stdout)
        .trim()
        .to_string();

    let installed = Command::new(&installed_path)
        .args(["validate", "--path", "rigor.yaml"])
        .output()
        .expect("run installed rigor validate");

    let fresh = Command::new(env!("CARGO_BIN_EXE_rigor"))
        .args(["validate", "--path", "rigor.yaml"])
        .output()
        .expect("run fresh rigor validate");

    if fresh.status.success() && !installed.status.success() {
        let stderr = String::from_utf8_lossy(&installed.stderr);
        panic!(
            "STALE BINARY DETECTED\n\
             Installed: {}\n\
             Fresh build passes but installed binary fails:\n\
             {}\n\n\
             Fix: cargo install --path crates/rigor --force",
            installed_path, stderr
        );
    }
}
