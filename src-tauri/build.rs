use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Build scripts run for the host, so gate on Cargo's requested target.
    // This keeps Android/iOS cross-builds on a Mac from linking AppKit.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("native/flash_drop.m")
            .flag_if_supported("-fobjc-arc")
            .compile("dashbeam_flash_drop");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rerun-if-changed=native/flash_drop.m");
    }

    // Read version from package.json (single source of truth)
    // build.rs is in src-tauri/, so package.json is in the same directory
    let manifest_dir =
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set by Cargo");
    let package_json_path = PathBuf::from(manifest_dir).join("../package.json");

    if package_json_path.exists() {
        let package_json_str =
            fs::read_to_string(&package_json_path).expect("Failed to read package.json");
        let package_json: serde_json::Value =
            serde_json::from_str(&package_json_str).expect("Failed to parse package.json");

        if let Some(version) = package_json.get("version").and_then(|v| v.as_str()) {
            println!("cargo:rustc-env=APP_VERSION={}", version);
            println!("cargo:rerun-if-changed=package.json");
        } else {
            // Fallback to Cargo.toml version if version field is missing
            println!("cargo:rustc-env=APP_VERSION={}", env!("CARGO_PKG_VERSION"));
            eprintln!("Warning: version not found in package.json, using Cargo.toml version");
        }
    } else {
        // Fallback to Cargo.toml version if package.json doesn't exist
        println!("cargo:rustc-env=APP_VERSION={}", env!("CARGO_PKG_VERSION"));
        eprintln!("Warning: package.json not found, using Cargo.toml version");
    }

    tauri_build::build()
}
