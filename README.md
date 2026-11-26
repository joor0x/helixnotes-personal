# HelixNotes Personal

Personal fork of [HelixNotes](https://gitlab.com/ArkHost/HelixNotes),
originally developed by Yuri Karamian / ArkHost.

This fork adds features for my personal workflow.
Distributed under the original AGPL-3.0 license.

## Changes

Features and implementation progress are tracked in GitHub Issues.

### Added in this fork

- **Task reminders**: `due:` accepts a time and `remind:YYYY-MM-DDTHH:MM` sets a reminder. On desktop, a system notification is shown at each reminder while the app runs. The Tasks view can also filter by tag.
- **Native Android reminders**: reminders are scheduled as exact alarms, so they fire while the app is closed. Notifications can be snoozed (10 minutes, 1 hour, tomorrow) without opening the app, open their note when tapped, and are restored after a reboot, an app update or a clock change.
- **Recurring tasks**: add `repeat:daily|weekdays|weekly|monthly|yearly` or `repeat:<N><d|w|m|y>` to a task. Completing it keeps the completed line and opens the next occurrence, with `due:` and `remind:` moved forward. Periods already past are skipped.
- **Habit tracker**: an opt-in, per-vault Habits view. Each note in the vault's habits folder is a habit, and the days you mark are saved in the note's `entries` frontmatter property, compatible with Obsidian's Habit Tracker 21. Shows streaks.
- **LanguageTool grammar and spelling checks**: optional, in the rich editor, using the public API or your own server. Wavy underlines with suggestions and Ignore; code is never checked. A note can set its own language with a `lang:` frontmatter key or from the Info panel.
- **Bulk task actions**: check or uncheck all tasks in a note, delete completed tasks, or archive them under a "Done" heading. A frontmatter flag keeps a note's tasks out of the Tasks view.
- **[Git sync](docs/git-sync.md)**: opt-in per vault, coexists with Syncthing. The repository lives outside the vault, conflicts keep both sides, and sync can run manually, on an interval, on change or on open.
- **[CLI AI providers](docs/ai-cli-providers.md)**: use a locally installed and signed-in Claude Code, Codex or Gemini CLI as a text-only AI provider (desktop only).
- **[Organize with AI](docs/organize.md)**: right-click Unfiled Notes or a notebook to have the configured AI provider (Ollama, OpenAI, Anthropic, a CLI…) suggest a notebook for each note, with empty notes proposed for the Trash. You review the list before anything moves (desktop only).
- **Edit external Markdown files in place**: files opened from outside the vault can be edited and autosave back to the original file, without being copied into the vault.
- **Search extra folders**: global search can also cover folders outside the vault; results open in place.
- **Image editing**: resize from the corners, drag to move between blocks, captions from the image title, and an optional name prompt when pasting.
- **[Portable mode](docs/fork-build.md)**: a `portable` marker next to the executable keeps all app state in a `data/` folder beside it, with a portable Windows build.

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Fork releases](https://img.shields.io/github/v/release/joor0x/helixnotes-personal?label=fork%20release)](https://github.com/joor0x/helixnotes-personal/releases)
[![Upstream](https://img.shields.io/badge/upstream-HelixNotes%201.3.8-purple)](https://gitlab.com/ArkHost/HelixNotes)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows%20%7C%20Android-lightgrey)]()

A local markdown note-taking app built with Tauri, SvelteKit, and Rust.

Your notes are stored as standard Markdown files on your local filesystem.
No cloud, no lock-in.

## Download

> **The packages on helixnotes.com, the ArkHost Flatpak/APT/DNF repositories, the AUR
> and the GitLab releases are upstream HelixNotes, not this fork.** They do not include
> the features listed above. Get this fork from its own releases or build it yourself.

Fork releases: [github.com/joor0x/helixnotes-personal/releases](https://github.com/joor0x/helixnotes-personal/releases)

| Platform | How to get it |
|---|---|
| Windows | `HelixNotes_<version>_windows_x64_portable.zip` from the releases page. Unzip and run `HelixNotes.exe`; it is [portable](docs/fork-build.md#portable-mode-windows-and-linux). Needs the WebView2 runtime (included in Windows 11 and current Windows 10). |
| Linux | `HelixNotes_<version>_amd64.deb` (Debian, Ubuntu 22.04+, Mint) or `HelixNotes_<version>_amd64.AppImage` from the releases page. Or build them: `pnpm fork:build:linux` ([details](docs/fork-build.md#linux)). |
| Linux (Nix) | `nix build github:joor0x/helixnotes-personal`, or use the flake below. |
| Android | Build and sign the APK: `pnpm fork:build:android` ([details](docs/fork-build.md#android)). |
| macOS | Not built or tested for this fork. |

Fork builds have the in-app updater turned off, so they never replace themselves with the
upstream release. Update by installing a newer fork build over the old one.

On Android the fork uses upstream's application ID: uninstall the official HelixNotes APK
before installing it (vaults in shared storage are kept).

<details>
<summary>NixOS flake</summary>

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    helix-notes.url = "github:joor0x/helixnotes-personal";
  };

  outputs = { nixpkgs, helix-notes, ... }: {
    nixosConfigurations.default = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      specialArgs = { inherit helix-notes; };
      modules = [ /path/to/configuration.nix ];
    };
  };
}
```

In `configuration.nix`:

```nix
{ pkgs, helix-notes, ... }:
{
  users.users.<USERNAME>.packages = [
    helix-notes.packages.${pkgs.stdenv.hostPlatform.system}.default
  ];
}
```
</details>

## Releasing a new version

1. **Choose the version.** Fork releases use the version of the upstream base, as
   `fork-v<version>` (for example `fork-v1.3.8`). For a second release on the same base,
   add a suffix: `fork-v1.3.8-2`.
2. **Bump the version** if the upstream base changed (after merging `upstream/main`).
   It lives in three files, which must match:
   - `package.json` → `"version"`
   - `src-tauri/tauri.conf.json` → `"version"`
   - `src-tauri/Cargo.toml` → `version` (then run `cargo check --manifest-path src-tauri/Cargo.toml` to update `Cargo.lock`)
3. **Check** that everything passes:
   ```bash
   pnpm verify
   ```
4. **Tag and push:**
   ```bash
   git tag fork-v1.3.8
   git push origin main fork-v1.3.8
   ```
   The [`Fork builds`](.github/workflows/fork-builds.yml) workflow builds the Windows
   portable zip, the `.deb` and the AppImage, then creates the GitHub release for the tag
   with all three attached and notes generated from the commits. Follow it with
   `gh run watch`.
5. **Attach the Android APK** built locally, once the release exists (see
   [docs/fork-build.md](docs/fork-build.md#android)):
   ```bash
   pnpm fork:build:android
   gh release upload fork-v1.3.8 \
     src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk
   ```
   Sign the APK with the same keystore every time, or installed copies cannot be updated.

Upstream releases and its own download options: [gitlab.com/ArkHost/HelixNotes/-/releases](https://gitlab.com/ArkHost/HelixNotes/-/releases)

## Features

- Markdown editor with toolbar, slash commands, source mode, code highlighting, Find and Replace
- Side-by-side image grids, image reordering and hover text
- **Tasks view**: aggregate `- [ ]` checklists from across all notes, set priority and due dates, filter Today or Overdue tasks, work in a list or a calendar (drag a task to reschedule)
- `[[Wiki-links]]` and graph view
- Full-text search (Tantivy), CJK-aware for Chinese, Japanese, and Korean
- Outline panel, daily notes with calendar view, tags with autocomplete, drag-and-drop
- Live KaTeX math editor (`/math`, `/imath`) with modal preview, double-click to edit
- Mermaid diagrams (opt-in render, copy as PNG, save as PNG/SVG)
- Encrypted secret blocks (`/secret`) stored as portable `helix-secret` markdown fences
- Insert date/time (`/date`, `/time`, `/now`) with 12-hour or 24-hour timestamps, color swatches (`/color`), configurable week start
- Manual notebook sorting (drag to reorder above, into, or below)
- External `.md` viewer mode with import-to-vault flow
- PDF preview, Obsidian import, "Show in File Manager"
- [Editable Live preview](docs/live-preview.md) alongside rich text and Markdown source, with one compact editor-mode selector and a three-button new-note preference
- [Quick vault switching](docs/vault-switching.md) on desktop and mobile, with saved-vault paths, Add vault and Manage Vaults
- AI writing tools (Ollama / OpenAI-compatible / Anthropic / OpenAI)
- [Provider model discovery and opt-in image input](docs/ai-models-and-images.md)
- **Optional WebDAV sync** to your own server (Nextcloud, ownCloud, a NAS): manual or automatic, with keep-both conflict copies
- Version history with main-area diffs and rendered read-only snapshots, automatic backups
- Multi-window, file associations, focus mode, view mode
- Themes (light, dark, and built-in palettes), accent colors, fonts including bundled Ubuntu and Ubuntu Mono, 80-200% interface scale
- [Daily-note templates](docs/daily-note-templates.md), with per-vault Markdown import/editing and date/title placeholders, applied only to new notes
- Interface languages: English, Russian, Dutch, German, Spanish, French, Italian, Portuguese (Portugal) and Portuguese (Brazil), with live switching
- [Desktop MCP integration](docs/mcp.md) in Settings, with a packaged helper, per-vault read-only access and opt-in native note editing; no Node.js installation required
- Local plain-text files, no company cloud

### Version history

Changes compares the selected snapshot with the current editor contents, including unsaved edits. Added and removed lines show what restoring that version would change; long unchanged sections can be expanded.

Snapshot displays the stored note read-only, with View source available for its Markdown body. Metadata is separate and is not restored. Snapshot links are inactive, secrets stay locked, and diagrams render only when requested.

Restore replaces the current note's body and asks for confirmation if it would replace an unsaved draft. Inspecting versions does not replace the live editor contents or reset their undo history.

### Note Info

Open **Note info** from the editor toolbar. Expand **Frontmatter** to inspect the original YAML header, including custom keys, without editing it. This shows the last loaded or saved metadata; unsaved changes are not included, and the display updates after saving.

### Saving notes

Notes autosave after three seconds without edits. During continuous typing, a save is queued at least every ten seconds. Ctrl+S (Cmd+S on macOS), switching notes, and closing an editor flush pending changes immediately. A failed save leaves the note marked **Unsaved** and blocks leaving that editor.

Note writes run off the UI thread and replace the file atomically. Unchanged notes are not rewritten. Ordinary saves update only the affected note in the list, wiki-link cache, and Tasks view; search-index updates are batched in the background. External file changes are still picked up by the watcher.

With **Close to tray** enabled and a tray icon available, closing the main window hides it after saving. With the setting off, closing the main window closes the application. The close-to-tray setting takes effect immediately; enabling the tray icon itself requires a restart. Explicit **Quit** exits rather than hiding to the tray.

Upstream documentation (does not cover the fork's additions): [helixnotes.com/docs](https://helixnotes.com/docs.html)

## Contributing translations

Want HelixNotes in your language? Copy the [English JSON catalog](src/lib/i18n/locales/en.json), translate its values, and attach the file to a GitLab issue using the **Translation** template. No build or programming experience is required.

See the [translation guide and example file](docs/translations/README.md) for placeholders, plural forms and submission instructions.

## Tech Stack

- **Frontend**: SvelteKit (Svelte 5) + TailwindCSS v4 + TipTap v3
- **Backend**: Rust (Tauri 2.0) + Tantivy (search) + Notify (file watcher)
- **Platforms**: Linux (deb, AppImage, Nix), Windows (portable), Android

## Building from Source

> **Fork builds:** see [docs/fork-build.md](docs/fork-build.md) for the `pnpm fork:*`
> scripts, Linux packages and Android APK signing. They disable the upstream updater.

### Prerequisites

- [Rust](https://rustup.rs/) (1.88+)
- [Node.js](https://nodejs.org/) (22 LTS or later)
- [pnpm](https://pnpm.io/)
- System dependencies for Tauri: see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)

### Development

```bash
pnpm install
pnpm tauri dev
```

### Verification

Run the frontend checks and tests, MCP safety/native stdio tests, Rust tests and lints, and production frontend build with one command. The native MCP test also builds a debug app/helper executable; it does not create an installer or release package:

```bash
pnpm verify
```

### Production Build

Use the `fork:*` scripts (`pnpm fork:build:linux`, `pnpm fork:build:android`); plain
`pnpm tauri build` needs upstream's updater signing key and fails without it.

## Screenshots

![Editor](https://cdn.helixnotes.com/assets/screenshots/screenshot-1.png)
![Tasks calendar](https://cdn.helixnotes.com/assets/screenshots/screenshot-2.png)
![Graph view](https://cdn.helixnotes.com/assets/screenshots/screenshot-7.png)
![Daily notes](https://cdn.helixnotes.com/assets/screenshots/screenshot-4.png)
![Themes](https://cdn.helixnotes.com/assets/screenshots/screenshot-6.png)

## License

[AGPL-3.0](https://gitlab.com/ArkHost/HelixNotes/-/blob/main/LICENSE)
