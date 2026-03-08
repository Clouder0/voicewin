use std::path::Path;
use std::process::Command;

fn emit_git_build_metadata() {
    println!("cargo:rerun-if-env-changed=VOICEWIN_GIT_SHA");

    let git_head = Path::new("../../.git/HEAD");
    if git_head.exists() {
        println!("cargo:rerun-if-changed={}", git_head.display());

        if let Ok(head) = std::fs::read_to_string(git_head) {
            if let Some(reference) = head.trim().strip_prefix("ref: ") {
                let ref_path = Path::new("../../.git").join(reference);
                if ref_path.exists() {
                    println!("cargo:rerun-if-changed={}", ref_path.display());
                }
            }
        }
    }

    let git_sha = std::env::var("VOICEWIN_GIT_SHA").unwrap_or_else(|_| {
        Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|sha| !sha.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    });

    println!("cargo:rustc-env=VOICEWIN_GIT_SHA={git_sha}");
}

fn main() {
    emit_git_build_metadata();
    tauri_build::build()
}
