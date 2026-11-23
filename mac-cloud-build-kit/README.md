# Build a Mac app without installing Xcode

For when your Mac is too full for Xcode (10+ GB) or a full developer setup.
GitHub builds the app on its own Mac, and you download the finished app.
Nothing gets installed on your laptop except the app.

This kit is for **Tauri** apps (Rust with a web-based screen), which is what I
used. A Tauri app is tiny: mine was 1.4 MB. If your app uses something else,
use the prompt at the bottom.

## What you need

- A free GitHub account
- Your app's code. An AI coding tool such as Claude Code can write it for you.
- About 15 minutes the first time

## Steps

1. **Make a repository.** On github.com click **New**, give it a name, choose
   **Public** (Mac builds are free for public repositories), and create it.
2. **Put your app code in it.** Your AI coding tool can do this for you.
3. **Add the build file.** In your repository create a file at exactly
   `.github/workflows/build-mac-app.yml` and paste in the contents of
   [`build-mac-app.yml`](build-mac-app.yml) from this folder.
4. **Run it.** Saving the file to the `main` branch starts a build. Open the
   **Actions** tab and watch it. The first build takes around 5 to 10 minutes.
5. **Download your app.** When the run shows a green tick, open it, scroll to
   **Artifacts** at the bottom and download **mac-app**. Unzip it.
6. **Open it for the first time.** The app is unsigned, so macOS blocks it at
   first. Move it to Applications and run this in Terminal:

   ```
   xattr -dr com.apple.quarantine "/Applications/YOUR APP NAME.app"
   ```

   Or double-click it once, then open System Settings, then Privacy &
   Security, and click **Open Anyway**.

## If something goes wrong

- **Red cross on the build:** click the run, click the failed step, and copy the
  error text to your AI coding tool. Ask it to fix the build.
- **"The configuration ... is not supported":** GitHub retired that Mac type.
  Make sure the file says `runs-on: macos-latest`.
- **No Artifacts section:** the build did not finish. Check the run for a red cross.
- **Apple Silicon only:** `macos-latest` builds for Apple Silicon Macs (M1 and later).

## Good to know

- Each test build takes a few minutes, not seconds. Make your changes in
  batches.
- Private repositories get a limited free allowance of build time, and Mac time
  counts extra. Check GitHub's current pricing before relying on it.
- Unsigned apps are fine for your own use. To give an app to other people
  without warnings, you need an Apple Developer account ($99 a year).

## Prompt for your AI coding tool

> Set up a GitHub Actions workflow that builds my Mac app on the `macos-latest`
> runner and uploads the finished .app as a downloadable artifact. I have no
> Xcode or developer tools installed and don't want to install them. Tell me
> the steps to download and open the app.
