# Git sync

Desktop only. Off by default, and set per vault in **Settings › Sync › Git**.

When Git sync is on, each sync commits what changed in the vault, merges the remote branch and pushes. HelixNotes runs the `git` program installed on the system, so your SSH keys, credential helper and global Git settings work as usual. If no remote is set, it only commits on this device, which gives the vault a local history.

## Working with Syncthing and other sync tools

Git sync is independent of WebDAV sync and of any folder-sync tool. You can run them together.

- **The repository is kept outside the vault.** Unless the vault already has a `.git` folder, HelixNotes keeps the repository in its own data folder (`<config dir>/helixnotes/git/<id>.git`, or the portable data folder) and uses the vault as the work tree. It writes no `.git` and no `.gitignore` into the vault, so Syncthing, Nextcloud or Dropbox never copy repository internals between devices. Each device keeps its own repository, and the notes can arrive by either route. If two devices already have the same files from Syncthing, the merge finds them identical and nothing conflicts.
- **Files left by sync tools are never committed.** These include `.stfolder`, `.stignore`, `.stversions/`, `*.sync-conflict-*`, Syncthing temporary files, Nextcloud/ownCloud client databases, Dropbox and Resilio metadata, `.DS_Store`, `Thumbs.db` and `desktop.ini`.
- **An existing repository is used as it is.** If the vault has its own `.git`, HelixNotes uses that repository. It changes only a marked block in `.git/info/exclude` and does not touch your `.gitignore`.
- **Syncs never overlap.** A Git sync waits for any WebDAV sync or backup to finish, and the other way round.

## Choosing what is synced

- **Folders.** Tick folders to sync only those. A ticked folder includes all its subfolders. With nothing ticked, the whole vault is synced. If you choose folders, notes at the vault root are left out.
- **Excluded files.** One pattern per line, in `.gitignore` syntax (`Private/`, `*.pdf`, `Drafts/**/*.tmp.md`). These patterns are applied last, so they can override everything above them.
- **HelixNotes data.** Attachments, notebook icons, tag styles and the daily template are synced. The search index, history, trash and per-device state stay local. To leave attachments out, add `.helixnotes/attachments/` to the exclusions.

If a file that was already committed becomes excluded, it is removed from the repository but not from disk. Other clones then delete their copy on their next pull. If those clones also sync the folder with Syncthing, that deletion can reach this device as well, so exclude files before you start using Git sync on a second device.

## Conflicts

A sync never stops on a conflict, and both versions are kept:

- If both sides changed a file, your version stays in place. The remote version is saved beside it as `Name (conflict <date>).md`, the same way WebDAV sync does it.
- If one side deleted a file and the other changed it, the changed version is kept.

If git cannot merge for another reason, such as an untracked local file in the way, the merge is aborted, nothing is changed and the error is shown. If the repository is in the middle of a manual merge or rebase, HelixNotes does not sync until you finish or abort it.

## When syncs run

You can sync from **Sync with Git now** or from the title-bar sync button, which runs every sync that is turned on for the vault. Automatic syncs can run:

- on a timer (15 min to 4 h)
- a minute after you edit a note
- when the vault opens

Fetch and push stop after three minutes. HelixNotes never asks for a password: if the remote needs one, set up an SSH key or a credential helper first.

Commits use your Git `user.name` and `user.email`. If those are not set, the author is `HelixNotes <helixnotes@localhost>`.
