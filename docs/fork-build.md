# Building and running HelixNotes Personal

How to run this fork locally and produce packages for Linux, Windows (portable) and Android.
Commands assume Ubuntu/Debian; adapt package names for other distros.

## Why fork builds differ from upstream

Two upstream settings would interfere with a personal build:

- **In-app updater.** `src-tauri/tauri.conf.json` points the updater at
  `helixnotes.com`. A fork build that checks it would offer the upstream release
  and overwrite the fork's changes. The `fork:*` scripts build with
  `HELIXNOTES_INSTALL_TYPE=fork`, which makes the app treat itself as a managed
  install: no update check, no update UI.
- **Updater signing.** `bundle.createUpdaterArtifacts` requires upstream's
  private signing key (`TAURI_SIGNING_PRIVATE_KEY`), so `tauri build` fails
  without it. `src-tauri/tauri.fork.conf.json` turns it off and is merged in via
  `--config`.

Use the `fork:*` scripts below instead of plain `pnpm tauri build`.

## Common prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.88+ | via [rustup](https://rustup.rs/) |
| Node.js | 22 LTS+ | |
| pnpm | 11.x | `corepack enable` picks the version pinned in `package.json` |

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
corepack enable
pnpm install
```

## Linux

### System dependencies

```bash
sudo apt install build-essential curl wget file pkg-config \
  libwebkit2gtk-4.1-dev libssl-dev libxdo-dev \
  libayatana-appindicator3-dev librsvg2-dev
```

See [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for other distros.

### Run in development

```bash
pnpm fork:dev
```

Starts Vite on `http://localhost:5173` and opens the app with hot reload.

### Build packages

```bash
pnpm fork:build:linux
```

Outputs:

- `src-tauri/target/release/bundle/deb/HelixNotes_<version>_amd64.deb`
- `src-tauri/target/release/bundle/appimage/HelixNotes_<version>_amd64.AppImage`

Building the AppImage downloads `linuxdeploy` on first run, so it needs network access.
Install the `.deb` with `sudo apt install ./HelixNotes_<version>_amd64.deb`.

Alternative: `nix build` uses `flake.nix`, which also disables the updater.

## Portable mode (Windows and Linux)

A portable copy runs without installing and keeps all its state in a `data/` folder
next to the executable: `config.json` (settings and vault list), default backups,
the search index, the webview's `localStorage` and the window size and position.
Nothing goes to `%APPDATA%`, `%LOCALAPPDATA%`, `~/.config` or `~/.local/share`.

Portable mode is on when any of these holds:

- the build was made with `HELIXNOTES_INSTALL_TYPE=portable` (the Windows zip is);
- a file named `portable` sits next to the executable, or next to the `.AppImage`;
- the `HELIXNOTES_PORTABLE_DIR` environment variable names the folder to use.

Vault and folder paths inside the portable folder are saved relative to it, so a
copy on a USB drive keeps finding its vaults when the drive gets another letter.
Paths outside it stay absolute. A portable copy never runs the in-app updater.

The portable copy and an installed HelixNotes share the single-instance lock:
while one is open, starting the other only brings the open one to the front.

### Windows

The `Windows portable build` GitHub Actions workflow
(`.github/workflows/windows-portable.yml`) builds `HelixNotes.exe` and uploads
`HelixNotes_<version>_windows_x64_portable.zip` as a workflow artifact. It runs on
every push to `main`, on pull requests and on demand from the Actions tab. Pushing a `fork-v*` tag
(for example `fork-v1.3.8`) also publishes the zip as a GitHub release.

To build it on a Windows machine instead, install Rust, Node.js 22 and the
[Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows),
then in PowerShell:

```powershell
corepack enable
pnpm install
$env:HELIXNOTES_INSTALL_TYPE = "portable"
pnpm tauri build --no-bundle --config src-tauri/tauri.fork.conf.json
```

The exe is `src-tauri\target\release\helixnotes.exe`.

The exe needs the WebView2 runtime. Windows 11 always ships it and up-to-date
Windows 10 does too. The zip does not bundle it.

### Linux

Build the AppImage as above and put an empty file named `portable` next to it:

```bash
touch portable   # in the folder that holds HelixNotes_<version>_amd64.AppImage
```

## Android

The Android project is already generated and tracked in `src-tauri/gen/android`;
do **not** run `tauri android init`, it would overwrite local changes.

### Toolchain

- **JDK 17 or 21.** Gradle 8.14 does not run on JDK 25, so point `JAVA_HOME` at 21
  even if a newer JDK is the system default.
- **Android SDK** with platform `android-36` and build tools (Android Studio or `sdkmanager`).
- **Android NDK** (27.x tested with Tauri 2).
- **Rust Android targets.**

```bash
export JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64
export ANDROID_HOME="$HOME/Android/Sdk"
export NDK_HOME="$ANDROID_HOME/ndk/27.0.12077973"
export PATH="$ANDROID_HOME/platform-tools:$PATH"

rustup target add aarch64-linux-android armv7-linux-androideabi \
  i686-linux-android x86_64-linux-android
```

Put the exports in your shell profile so every session has them.

### Run on a device or emulator

Enable USB debugging on the phone, connect it (check with `adb devices`), then:

```bash
pnpm fork:dev:android
```

### Build an APK

```bash
pnpm fork:build:android
```

This builds an `arm64-v8a` APK (almost every current phone). Drop `--target aarch64`
from the script to build a universal APK for all ABIs.

Output: `src-tauri/gen/android/app/build/outputs/apk/universal/release/`.
Without a signing key the file is `app-universal-release-unsigned.apk` and Android
will refuse to install it; configure signing first.

### Release signing

Create a keystore once and keep it **outside the repo** and backed up. Losing it means
future APKs cannot update the installed app.

```bash
keytool -genkey -v -keystore ~/keys/helixnotes-personal.jks \
  -keyalg RSA -keysize 4096 -validity 10000 -alias helixnotes
```

Create `src-tauri/gen/android/keystore.properties` (git-ignored):

```properties
storeFile=/home/<user>/keys/helixnotes-personal.jks
storePassword=<password>
keyAlias=helixnotes
keyPassword=<password>
```

`app/build.gradle.kts` picks it up automatically and the release APK comes out signed
(`app-universal-release.apk`). Install it with:

```bash
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk
```

### Coexisting with the upstream app

The fork keeps upstream's application ID, `com.helixnotes.app`. On Android, that means:

- It cannot be installed next to the official HelixNotes APK.
- Installing over the official app fails with a signature mismatch; uninstall it first
  (this deletes its app-private data, though vaults in shared storage stay).

## Checks before building

```bash
pnpm verify
```

Runs the frontend checks, JS/MCP/Rust tests, Rust lints and a production frontend build.

### Browser tests

`pnpm test:browser` runs the Playwright tests in a headless Chromium, which is
downloaded separately. Install it once, and again after updating `@playwright/test`:

```bash
pnpm exec playwright install chromium
```

Playwright does not officially support every distro release (Ubuntu 26.04, for
example). There the install fails with `Playwright does not support chromium on
<distro>`; download the Ubuntu 24.04 build instead, which runs fine:

```bash
PLAYWRIGHT_HOST_PLATFORM_OVERRIDE=ubuntu24.04-x64 pnpm exec playwright install chromium
```

## Troubleshooting

- **`Unsupported class file major version 69`** or a Gradle crash at startup: Gradle is running on JDK 25. Set `JAVA_HOME` to JDK 21.
- **`NDK_HOME` / `ANDROID_HOME` not set**: export them as shown above.
- **`A public key has been found, but no private key`**: you ran plain `tauri build`; use `pnpm fork:build:linux`.
- **`webkit2gtk-4.1` not found**: install `libwebkit2gtk-4.1-dev`.
- **`Executable doesn't exist at .../ms-playwright/...`** in `pnpm test:browser`: install Playwright's Chromium (see [Browser tests](#browser-tests)).
