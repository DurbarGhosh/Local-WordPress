# WP Local Lite

A small Mac app for running WordPress sites locally while developing, built to
open sites already created with Local (by WP Engine) without converting them.

**Status:** milestone 1. The app lists the sites Local knows about and checks
that the programs and folders each one needs are present. It is read-only:
it does not start, stop, change or delete anything.

## Get the app

1. Open the **Actions** tab, choose the latest **build** run.
2. Download the **WP-Local-Lite-mac-arm64** artifact and unzip it.
3. Move `WP Local Lite.app` to Applications, then **right-click → Open** the first time
   (the app is unsigned). If macOS says it is damaged, run:
   `xattr -dr com.apple.quarantine "/Applications/WP Local Lite.app"`

Apple Silicon only for now.

## Develop

Core logic lives in `crates/core` and is tested on any machine:
`cargo test -p wplocal-core`. The Mac app (Tauri) is in `src-tauri`, its screen
in `ui/`. Builds run in GitHub Actions, so no Xcode is needed locally.
