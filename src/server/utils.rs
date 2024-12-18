use sha2::{Digest, Sha256};
use std::process::Command;

pub fn to_sha256(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input);
    format!("{:x}", hasher.finalize())
}

pub fn terminate_proxer() {
    let _ = Command::new("sh")
        .args(["-c", "kill $(pgrep proxer-cli)"])
        .output()
        .expect(
            "Failed to execute `kill $(pgrep proxer-cli)` command to terminate proxer processes",
        );
}

pub fn tracing_error(message: &str) {
    tracing::error!("\x1B[31m{message}\x1B[0m");
}
