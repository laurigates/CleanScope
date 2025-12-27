Build the CleanScope Android APK.

Arguments:
- $ARGUMENTS: Optional flags (--release for production build)

Steps:
1. Run `just check-prereqs` to validate environment
2. If --release: Run `just android-release`
3. Otherwise: Run `just android-build`
4. Report: APK path, size, and build time
5. If device connected: Offer to install with `adb install -r <apk>`

Output:
- Build success/failure status
- APK location: `src-tauri/gen/android/app/build/outputs/apk/`
- APK file size
- Build duration
- Device installation prompt if applicable
