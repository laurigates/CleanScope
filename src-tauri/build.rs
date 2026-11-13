//! Build script for `CleanScope`
//!
//! Configures Android-specific build settings and runs Tauri's build process.

use std::process::Command;

fn main() {
    // Generate build info
    generate_build_info();
    emit_android_page_size_link_args();
    // Only run Android-specific logic when building for Android
    #[cfg(target_os = "android")]
    {
        // Link required Android system libraries
        println!("cargo:rustc-link-lib=log");
        println!("cargo:rustc-link-lib=android");

        // If we need to compile libuvc from source, we would use the cc crate here
        // For now, we rely on the uvc crate's "vendor" feature when it's enabled

        // Set up include paths for Android NDK if needed
        if let Ok(ndk_home) = std::env::var("NDK_HOME") {
            println!("cargo:warning=Using NDK from: {}", ndk_home);
        }
    }

    // Run Tauri's build process
    tauri_build::build();
}

/// Link the Android shared library with 16 KB ELF segment alignment.
///
/// Devices with 16 KB memory pages need every LOAD segment aligned to 16384,
/// and Google Play requires it for apps targeting Android 15+. NDK r28+ links
/// this way by default; NDK r27 and lower need both flags, so they are passed
/// explicitly to keep the result independent of the installed NDK. See
/// <https://developer.android.com/guide/practices/page-sizes>.
///
/// A build script is compiled for the host, so the target OS is read from
/// `CARGO_CFG_TARGET_OS` rather than `cfg!(target_os)`.
fn emit_android_page_size_link_args() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android") {
        return;
    }
    println!("cargo:rustc-link-arg-cdylib=-Wl,-z,max-page-size=16384");
    println!("cargo:rustc-link-arg-cdylib=-Wl,-z,common-page-size=16384");
}

/// Generate build info environment variables for compile-time inclusion
fn generate_build_info() {
    // Get git commit hash
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Get build timestamp
    let build_time = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();

    // Check if working directory is dirty
    let is_dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    let git_hash_display = if is_dirty {
        format!("{}+", git_hash)
    } else {
        git_hash
    };

    println!("cargo:rustc-env=BUILD_GIT_HASH={}", git_hash_display);
    println!("cargo:rustc-env=BUILD_TIMESTAMP={}", build_time);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
}
