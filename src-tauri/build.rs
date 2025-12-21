fn main() {
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
