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
- **Edit external Markdown files in place**: files opened from outside the vault can be edited and autosave back to the original file, without being copied into the vault.
- **Search extra folders**: global search can also cover folders outside the vault; results open in place.
- **Image editing**: resize from the corners, drag to move between blocks, captions from the image title, and an optional name prompt when pasting.
- **[Portable mode](docs/fork-build.md)**: a `portable` marker next to the executable keeps all app state in a `data/` folder beside it, with a portable Windows build.

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](https://gitlab.com/ArkHost/HelixNotes/-/blob/main/LICENSE)
[![Latest Release](https://img.shields.io/badge/release-v1.3.8-green)](https://gitlab.com/ArkHost/HelixNotes/-/releases/v1.3.8)
[![Website](https://img.shields.io/badge/web-helixnotes.com-purple)](https://helixnotes.com)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows%20%7C%20macOS%20%7C%20Android-lightgrey)]()



A local markdown note-taking app built with Tauri, SvelteKit, and Rust.

Your notes are stored as standard Markdown files on your local filesystem.
No cloud, no lock-in.

## Download (v1.3.8)

### Linux

#### Flatpak

Install from the signed ArkHost repository:

```bash
flatpak install --user https://repo.arkhost.com/flatpak/HelixNotes.flatpakref
flatpak run com.helixnotes.HelixNotes
```

Install [Flatpak](https://flatpak.org/setup/) first if it is not already available on your system.

#### AppImage

The AppImage works only on Fedora 43+, Arch Linux, and openSUSE Tumbleweed (x86_64).

[Download AppImage](https://download.helixnotes.com/releases/v1.3.8/HelixNotes_1.3.8_amd64.AppImage)

#### Distro-specific packages

##### Fedora 43+ (DNF)

```bash
sudo dnf config-manager addrepo \
  --from-repofile=https://repo.arkhost.com/helixnotes.repo
sudo dnf install helix-notes
```

##### Debian / Ubuntu / Mint (APT)

```bash
curl -fsSL https://repo.arkhost.com/gpg.key | sudo gpg --dearmor -o /usr/share/keyrings/arkhost.gpg && echo "deb [signed-by=/usr/share/keyrings/arkhost.gpg arch=amd64] https://repo.arkhost.com stable main" | sudo tee /etc/apt/sources.list.d/helixnotes.list && sudo apt update && sudo apt install helix-notes
```

##### Arch / Manjaro (AUR)

```bash
yay -S helixnotes-appimage-bin
```

##### Solus (EOPKG)

```bash
sudo eopkg it helixnotes
```

##### NixOS

<details>
<summary>flake.nix</summary>

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    helix-notes = {
      url = "git+https://gitlab.com/ArkHost/HelixNotes";
      # inputs.nixpkgs.follows = "nixpkgs";
    }
  };

  outputs = {
    nixpkgs,
    helix-notes,
    ...
  }: let
    system = "x86_64-linux";
    pkgs = nixpkgs.legacyPackages.${system};
  in {
    nixosConfigurations.default = nixpkgs.lib.nixosSystem {
      system = system;
      specialArgs = { inherit helix-notes; };

      modules = [
        /path/to/configuration.nix
      ];
    };
  };
}
```
</details>


<details>
<summary>configuration.nix</summary>

```nix
{
  config,
  lib,
  pkgs,
  helix-notes,
  ...
}:
{
  users.users.<USERNAME> = {
    packages = with pkgs; [
      (helix-notes.packages.${pkgs.stdenv.hostPlatform.system}.default)
    ];
  };
}
```
</details>

#### Manual package downloads

- [.deb](https://download.helixnotes.com/releases/v1.3.8/HelixNotes_1.3.8_amd64.deb) (Ubuntu 22.04+)
- [.rpm](https://download.helixnotes.com/releases/v1.3.8/HelixNotes-1.3.8-1.x86_64.rpm)

### Windows

[Download Installer](https://download.helixnotes.com/releases/v1.3.8/HelixNotes_1.3.8_x64-setup.exe) (Windows 10/11)

### macOS (v1.3.6)

The macOS 1.3.8 release is delayed. Version 1.3.6 remains available below.

[Download .dmg (Apple Silicon)](https://download.helixnotes.com/releases/v1.3.6/HelixNotes_1.3.6_aarch64.dmg) (M-series Macs)

[Download .dmg (Intel)](https://download.helixnotes.com/releases/v1.3.6/HelixNotes_1.3.6_x64.dmg)

The Apple Silicon build is signed and notarized by Apple. The Intel build may still trigger a Gatekeeper warning. If macOS reports that the Intel build is damaged, run this once after installing or updating it:

```bash
xattr -cr /Applications/HelixNotes.app
```

### Android

[Download APK](https://download.helixnotes.com/releases/v1.3.8/HelixNotes_1.3.8_android.apk)

---

All releases: [gitlab.com/ArkHost/HelixNotes/-/releases](https://gitlab.com/ArkHost/HelixNotes/-/releases)

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

Full documentation: [helixnotes.com/docs](https://helixnotes.com/docs.html)

## Contributing translations

Want HelixNotes in your language? Copy the [English JSON catalog](src/lib/i18n/locales/en.json), translate its values, and attach the file to a GitLab issue using the **Translation** template. No build or programming experience is required.

See the [translation guide and example file](docs/translations/README.md) for placeholders, plural forms and submission instructions.

## Tech Stack

- **Frontend**: SvelteKit (Svelte 5) + TailwindCSS v4 + TipTap v3
- **Backend**: Rust (Tauri 2.0) + Tantivy (search) + Notify (file watcher)
- **Platforms**: Linux (AppImage), Windows, macOS, Android

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

```bash
pnpm tauri build
```

## Screenshots

![Editor](https://cdn.helixnotes.com/assets/screenshots/screenshot-1.png)
![Tasks calendar](https://cdn.helixnotes.com/assets/screenshots/screenshot-2.png)
![Graph view](https://cdn.helixnotes.com/assets/screenshots/screenshot-7.png)
![Daily notes](https://cdn.helixnotes.com/assets/screenshots/screenshot-4.png)
![Themes](https://cdn.helixnotes.com/assets/screenshots/screenshot-6.png)

## License

[AGPL-3.0](https://gitlab.com/ArkHost/HelixNotes/-/blob/main/LICENSE)
