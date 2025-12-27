Run complete Android build verification.

Steps:
1. Run `just check-prereqs`
2. Run `just rust-check` (cargo check)
3. Run `just rust-clippy` (cargo clippy)
4. Run `just typecheck` (npm run check)
5. Run `just android-build`
6. Report: Build success/failure, warnings, APK size

Output:
- Prerequisites status
- Rust check results
- Clippy warnings (if any)
- TypeScript check results
- Android build status
- APK location and size
- Summary with pass/fail status for each step
