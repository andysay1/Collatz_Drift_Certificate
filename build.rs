// build.rs
use std::{process::Command, env};

fn main() {
    // git rev
    let git = Command::new("git").args(["rev-parse", "--short", "HEAD"]).output();
    if let Ok(o) = git {
        println!("cargo:rustc-env=BUILD_GIT_REV={}", String::from_utf8_lossy(&o.stdout).trim());
    } else {
        println!("cargo:rustc-env=BUILD_GIT_REV=unknown");
    }
    // rustc -V
    let rv = Command::new(env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
        .arg("-V").output();
    if let Ok(o) = rv {
        println!("cargo:rustc-env=BUILD_RUSTC={}", String::from_utf8_lossy(&o.stdout).trim());
    } else {
        println!("cargo:rustc-env=BUILD_RUSTC=unknown");
    }
}
