# Publishing to Google Play Store

This document covers the one-time manual setup required before the automated GitOps pipeline can publish releases to Google Play.

## Overview

The pipeline works as follows:

```
git commit -m "feat: ..."
  → push to main
  → release-please.yml bumps versions and opens a PR
  → PR merged → GitHub release created
  → android-release.yml builds signed AAB and uploads to Play Store internal track
```

Version files updated automatically by release-please:
- `package.json` (primary)
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`

## Prerequisites (One-Time Setup)

### 1. Generate Android Upload Keystore

Run locally and keep `upload-keystore.jks` secure — **never commit it**.

```bash
keytool -genkeypair -v \
  -keystore upload-keystore.jks \
  -alias upload \
  -keyalg RSA -keysize 2048 -validity 10000 \
  -dname "CN=CleanScope, O=CleanScope, C=FI"

# Encode for GitHub secret
base64 -i upload-keystore.jks | pbcopy   # macOS — copies to clipboard
```

Store `upload-keystore.jks` in a secure location (password manager or encrypted backup). If lost, you cannot update the app on Play Store.

### 2. Google Play Console Setup

1. Create a [Google Play Developer account](https://play.google.com/console) ($25 one-time fee) if not already done.
2. Create the app with package name `com.cleanscope.app`.
3. **Upload the first AAB manually** — the Play Store API can only update existing apps, not create new ones.

Build and upload the first AAB locally:

```bash
# Create a keystore.properties at src-tauri/gen/android/keystore.properties
cat > src-tauri/gen/android/keystore.properties << EOF
keyAlias=upload
password=<your-keystore-password>
storeFile=/absolute/path/to/upload-keystore.jks
EOF

# Build the AAB
npm run tauri android build -- --aab

# The AAB is at:
# src-tauri/gen/android/app/build/outputs/bundle/universalRelease/app-universal-release.aab
```

Upload this AAB through Play Console → Testing → Internal testing → Create new release.

### 3. Google Cloud Service Account

1. Go to [Google Cloud Console](https://console.cloud.google.com) → IAM & Admin → Service Accounts.
2. Create a service account (e.g., `github-play-deploy`).
3. Create a JSON key for the service account and download it.
4. In Play Console: Setup → API access → link to your Google Cloud project.
5. In Play Console: Users and permissions → Invite new users → enter the service account email → grant **Release to internal testing track** for CleanScope.

### 4. GitHub Secrets

In the repository Settings → Secrets and variables → Actions, add:

| Secret | Value |
|--------|-------|
| `ANDROID_KEY_BASE64` | Output of `base64 -i upload-keystore.jks` |
| `ANDROID_KEY_ALIAS` | `upload` |
| `ANDROID_KEY_PASSWORD` | Password chosen during keytool |
| `GOOGLE_PLAY_SERVICE_ACCOUNT_JSON` | Full contents of the service account JSON key file |

## Workflow Files

| File | Purpose |
|------|---------|
| `.github/workflows/release-please.yml` | Runs on push to main; creates version bump PRs and GitHub releases |
| `.github/workflows/android-release.yml` | Runs when a GitHub release is published; builds and uploads AAB |
| `release-please-config.json` | release-please configuration |
| `.release-please-manifest.json` | Current version state tracked by release-please |
| `distribution/whatsnew/whatsnew-en-US` | Release notes shown in Play Store (update before each release) |

## Signing Configuration

`src-tauri/gen/android/app/build.gradle.kts` reads signing credentials from `src-tauri/gen/android/keystore.properties` at build time. This file is gitignored and must be created:

- **Locally**: Create manually for local release builds (see step 2 above).
- **CI**: Written by the `android-release.yml` workflow from GitHub Secrets.

## Releasing

### Automated (normal flow)

1. Use conventional commits on the `main` branch:
   - `feat: ...` → minor version bump
   - `fix: ...` → patch version bump
   - `feat!: ...` or `BREAKING CHANGE:` → major version bump
2. release-please opens a PR when it detects releasable commits.
3. Review the PR (check version bump and CHANGELOG entries), then merge.
4. release-please creates a GitHub release → `android-release.yml` triggers automatically.
5. Check Play Console → Testing → Internal testing for the new release.

### Manual (workflow_dispatch)

Trigger `android-release.yml` manually from the GitHub Actions tab for reruns or hotfixes without a new version bump.

### Promoting to Higher Tracks

The pipeline only uploads to the **internal** track. To promote:

- Use Play Console manually: Internal → Promote to closed testing / open testing / production.
- Or create a separate `workflow_dispatch` workflow that calls the Play Store API to promote an existing release.

## Caveats

- **NDK r26d vs r28**: Google Play requires 16KB page alignment for new submissions targeting devices with 16KB page sizes (policy effective 2025). NDK r26d may work initially, but validate with NDK r28 before the first production release — the `libusb1-sys` vendored build needs testing with r28.
- **`build.gradle.kts` is not auto-regenerated**: Unlike `tauri.properties`, this file persists across `tauri android init` runs. Signing changes added here are safe.
- **First upload must be manual**: See step 2 above.

## Verification

After setup, verify the full pipeline:

1. Make a conventional commit and push to `main`.
2. Confirm release-please opens a PR that bumps all three version files.
3. Merge the PR; confirm a GitHub release is created.
4. Confirm `android-release.yml` triggers and completes successfully.
5. Check Play Console internal track for the new release.

Verify local signing:

```bash
jarsigner -verify \
  src-tauri/gen/android/app/build/outputs/bundle/universalRelease/app-universal-release.aab
```
