// Git sync provider (opt-in, per vault).
//
// Model: the vault stays an ordinary folder. Every sync commits what changed, merges the remote
// branch and pushes, using the `git` program installed on the system, so the user's SSH keys,
// credential helpers and global settings apply unchanged.
//
// Living next to Syncthing, Nextcloud, Dropbox or HelixNotes' own WebDAV sync: unless the vault
// already holds a `.git` of its own, the repository is kept OUTSIDE the vault, in the app's data
// folder, with the vault as its work tree. No `.git` and no `.gitignore` are written into the
// vault, so a folder-sync tool never copies repository internals between devices, and every
// device keeps its own repository while the notes themselves may arrive by either route. Files
// those tools leave behind (`.stfolder`, `*.sync-conflict-*`, temporary files) are never
// committed.
//
// What is committed: the whole vault, or only the folders chosen in Settings, minus the user's
// exclusion patterns (gitignore syntax) and HelixNotes' local-only metadata. The rules live in
// the repository's own `info/exclude`, which is rewritten on every sync.
//
// Conflicts never stop a sync and never lose a side: when both sides changed a file, the local
// version stays in place and the remote one is written next to it as a conflict copy, the same
// way the WebDAV sync does.

use crate::state::AppState;
use crate::vault::operations::helixnotes_dir;
use crate::vault::watcher::changed_entry;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

/// How long a network command (fetch, push) may take before it is stopped. A remote asking for a
/// password nobody can type would otherwise keep the sync "running" forever.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(180);
/// Local commands are quick; this only guards against a stuck lock or a hung filesystem.
const LOCAL_TIMEOUT: Duration = Duration::from_secs(120);
const REMOTE_NAME: &str = "origin";
pub const DEFAULT_BRANCH: &str = "main";

/// Files left by folder-sync tools and operating systems. They are device-local by nature:
/// committing them would hand one device's sync state to every other clone.
const FOREIGN_SYNC_ARTIFACTS: &[&str] = &[
    // Syncthing
    ".stfolder",
    ".stfolder/",
    ".stignore",
    ".stversions/",
    "*.sync-conflict-*",
    ".syncthing.*.tmp",
    "~syncthing~*.tmp",
    // Nextcloud / ownCloud desktop clients
    ".sync_*.db*",
    "._sync_*.db*",
    ".owncloudsync.log*",
    ".nextcloudsync.log*",
    // Dropbox, Resilio, Google Drive, OneDrive
    ".dropbox",
    ".dropbox.attr",
    ".dropbox.cache/",
    ".sync/",
    "*.!sync",
    ".tmp.drivedownload/",
    ".tmp.driveupload/",
    // Operating systems and editors
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
    ".~lock.*#",
    "~$*",
];

pub struct GitSyncConfig {
    pub vault: String,
    /// Where the repository lives when the vault has no `.git` of its own.
    pub external_git_dir: PathBuf,
    /// Empty means commit locally only.
    pub remote: String,
    pub branch: String,
    /// Vault-relative folders, `/`-separated. Empty means the whole vault.
    pub folders: Vec<String>,
    /// Extra gitignore patterns, one per entry.
    pub exclude: Vec<String>,
}

#[derive(Default, Clone, Serialize, Debug, PartialEq, Eq)]
pub struct GitSyncSummary {
    /// Files in the commits made from local changes.
    pub committed: u32,
    /// Files the merge changed in the vault.
    pub pulled: u32,
    /// Files changed on both sides; the remote side was kept as a conflict copy.
    pub conflicts: u32,
    /// Files that were in the repository but are now excluded, removed from it (not from disk).
    pub untracked: u32,
    /// Whether anything was sent to the remote.
    pub pushed: bool,
}

#[derive(Clone, Serialize, Debug)]
pub struct GitInfo {
    /// `None` when git is not installed or not on PATH.
    pub version: Option<String>,
    /// The vault has its own `.git`, which is used as it is.
    pub repo_in_vault: bool,
}

// ───────────────────────── running git ─────────────────────────

struct Output {
    ok: bool,
    stdout: Vec<u8>,
    stderr: String,
}

impl Output {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim().to_string()
    }
}

fn base_command() -> Command {
    let mut command = Command::new("git");
    command
        // Fail instead of waiting for a password prompt nobody can see.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        // Messages we look at must not be translated.
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn run(mut command: Command, timeout: Duration) -> Result<Output, String> {
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "Git is not installed or is not on PATH".to_string()
        } else {
            format!("Could not run git: {error}")
        }
    })?;
    // Drain both pipes while waiting, or a chatty command would block on a full pipe.
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let out_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stdout.read_to_end(&mut buffer);
        buffer
    });
    let err_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stderr.read_to_end(&mut buffer);
        buffer
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Git did not finish within {} seconds and was stopped. If the remote needs a \
                     password, set up an SSH key or a credential helper first.",
                    timeout.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(format!("Could not wait for git: {error}")),
        }
    };
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default())
        .trim()
        .to_string();
    Ok(Output {
        ok: status.success(),
        stdout,
        stderr,
    })
}

pub fn git_version() -> Option<String> {
    let mut command = base_command();
    command.arg("--version");
    run(command, LOCAL_TIMEOUT)
        .ok()
        .filter(|output| output.ok)
        .map(|output| output.text())
}

pub fn vault_has_own_repo(vault: &Path) -> bool {
    vault.join(".git").exists()
}

pub fn info(vault: &str) -> GitInfo {
    GitInfo {
        version: git_version(),
        repo_in_vault: vault_has_own_repo(Path::new(vault)),
    }
}

struct Repo {
    work_tree: PathBuf,
    /// `None` when the vault's own `.git` is used.
    git_dir: Option<PathBuf>,
    /// Commit identity to use when git has none configured.
    fallback_identity: bool,
}

impl Repo {
    fn command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = base_command();
        // Vaults on shared, removable or synced drives often belong to another user (Syncthing
        // running as a service, NTFS on Linux), which git otherwise refuses. Only applies to
        // these invocations, on folders the user chose.
        command.args(["-c", "safe.directory=*"]);
        if self.fallback_identity {
            command.args([
                "-c",
                "user.name=HelixNotes",
                "-c",
                "user.email=helixnotes@localhost",
            ]);
        }
        match &self.git_dir {
            Some(git_dir) => {
                command
                    .arg("--git-dir")
                    .arg(git_dir)
                    .arg("--work-tree")
                    .arg(&self.work_tree);
            }
            None => {
                command.arg("-C").arg(&self.work_tree);
            }
        }
        command.current_dir(&self.work_tree).args(args);
        command
    }

    fn git<I, S>(&self, args: I) -> Result<Output, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        run(self.command(args), LOCAL_TIMEOUT)
    }

    fn git_network<I, S>(&self, args: I) -> Result<Output, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        run(self.command(args), NETWORK_TIMEOUT)
    }

    /// Runs git and turns a failure into an error carrying git's own message.
    fn check<I, S>(&self, what: &str, args: I) -> Result<Output, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.git(args)?;
        if output.ok {
            Ok(output)
        } else {
            Err(git_error(what, &output))
        }
    }

    fn git_path(&self, name: &str) -> Result<PathBuf, String> {
        let output = self.check("locate repository files", ["rev-parse", "--git-path", name])?;
        let path = PathBuf::from(output.text());
        Ok(if path.is_absolute() {
            path
        } else {
            self.work_tree.join(path)
        })
    }

    fn head(&self) -> Result<Option<String>, String> {
        let output = self.git(["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])?;
        Ok(output.ok.then(|| output.text()))
    }
}

fn git_error(what: &str, output: &Output) -> String {
    let detail = if output.stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        output.stderr.clone()
    };
    // Long hints bury the line that matters.
    let detail: String = detail
        .lines()
        .filter(|line| !line.starts_with("hint:"))
        .take(6)
        .collect::<Vec<_>>()
        .join("\n");
    format!("Git could not {what}: {detail}")
}

fn split_nul(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect()
}

// ───────────────────────── exclusion rules ─────────────────────────

/// Escapes a literal path segment for a gitignore pattern.
fn escape_segment(segment: &str) -> String {
    let mut escaped = String::with_capacity(segment.len());
    for (index, ch) in segment.chars().enumerate() {
        let leading = index == 0 && (ch == '#' || ch == '!');
        if leading || matches!(ch, '*' | '?' | '[' | ']' | '\\') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    // Trailing spaces are dropped by git unless escaped.
    let trimmed = escaped.trim_end_matches(' ');
    let spaces = escaped.len() - trimmed.len();
    let mut result = trimmed.to_string();
    for _ in 0..spaces {
        result.push_str("\\ ");
    }
    result
}

/// Cleans up a folder as typed or stored: `/`-separated, no empty, `.` or `..` parts.
/// Returns `None` for something that does not name a folder inside the vault.
pub fn normalize_folder(folder: &str) -> Option<String> {
    let parts: Vec<&str> = folder
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    if parts.is_empty() || parts.contains(&"..") {
        return None;
    }
    Some(parts.join("/"))
}

#[derive(Default)]
struct FolderNode {
    selected: bool,
    children: BTreeMap<String, FolderNode>,
}

fn emit_folder_rules(prefix: &str, node: &FolderNode, lines: &mut Vec<String>) {
    for (name, child) in &node.children {
        let path = format!("{prefix}/{}", escape_segment(name));
        lines.push(format!("!{path}/"));
        if !child.selected {
            // Keep the folder itself but only the chosen folders inside it.
            lines.push(format!("{path}/*"));
            emit_folder_rules(&path, child, lines);
        }
    }
}

/// The managed `info/exclude`: folder selection, HelixNotes' local-only data, other sync tools'
/// files, then the user's own patterns, which come last so they can override anything above.
pub fn exclude_rules(folders: &[String], user_patterns: &[String]) -> String {
    let mut lines = vec![
        "# Written by HelixNotes on every Git sync; edit the rules in Settings > Sync.".to_string(),
        String::new(),
    ];

    let mut root = FolderNode::default();
    for folder in folders.iter().filter_map(|folder| normalize_folder(folder)) {
        let mut node = &mut root;
        for part in folder.split('/') {
            node = node.children.entry(part.to_string()).or_default();
        }
        node.selected = true;
    }
    if !root.children.is_empty() {
        lines.push("# Only the chosen folders".to_string());
        lines.push("/*".to_string());
        lines.push("!/.helixnotes/".to_string());
        lines.push("!/.gitignore".to_string());
        emit_folder_rules("", &root, &mut lines);
        lines.push(String::new());
    }

    // Mirrors what the WebDAV sync carries: attachments, icons, tag styles and the daily
    // template travel; indexes, history, trash and per-device state stay local.
    lines.extend(
        [
            "# HelixNotes data that stays on this device",
            "/.helixnotes/*",
            "!/.helixnotes/attachments/",
            "!/.helixnotes/notebook_icons.json",
            "!/.helixnotes/tag_styles.json",
        ]
        .map(String::from),
    );
    let template = crate::vault::daily_template::RELATIVE_PATH;
    lines.push(format!("!/{}", template));
    lines.push(String::new());

    lines.push("# Files of other sync tools and of the operating system".to_string());
    lines.extend(
        FOREIGN_SYNC_ARTIFACTS
            .iter()
            .map(|pattern| pattern.to_string()),
    );

    let user: Vec<&str> = user_patterns
        .iter()
        .flat_map(|pattern| pattern.lines())
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.trim().is_empty())
        .collect();
    if !user.is_empty() {
        lines.push(String::new());
        lines.push("# Your exclusions".to_string());
        lines.extend(user.into_iter().map(String::from));
    }
    lines.push(String::new());
    lines.join("\n")
}

// ───────────────────────── repository setup ─────────────────────────

fn open_repo(cfg: &GitSyncConfig) -> Result<Repo, String> {
    let work_tree = PathBuf::from(&cfg.vault);
    if !work_tree.is_dir() {
        return Err(format!("The vault folder {} is not available", cfg.vault));
    }
    let in_vault = vault_has_own_repo(&work_tree);
    let mut repo = Repo {
        git_dir: (!in_vault).then(|| cfg.external_git_dir.clone()),
        work_tree,
        fallback_identity: false,
    };

    if !in_vault && !cfg.external_git_dir.join("HEAD").is_file() {
        std::fs::create_dir_all(&cfg.external_git_dir).map_err(|error| {
            format!(
                "Could not create the repository folder {}: {error}",
                cfg.external_git_dir.display()
            )
        })?;
        let branch = format!("--initial-branch={}", cfg.branch);
        repo.check(
            "create the repository",
            ["init", "--quiet", branch.as_str()],
        )?;
        for (key, value) in [
            // Folder-sync tools and Windows drives do not keep the executable bit.
            ("core.fileMode", "false"),
            ("core.quotePath", "false"),
            ("core.precomposeUnicode", "true"),
            ("core.bare", "false"),
        ] {
            repo.check("configure the repository", ["config", key, value])?;
        }
    }

    let has_identity = |key: &str| repo.git(["config", "--get", key]).is_ok_and(|o| o.ok);
    repo.fallback_identity = !(has_identity("user.name") && has_identity("user.email"));

    if repo.git_path("MERGE_HEAD")?.exists() || repo.git_path("rebase-merge")?.exists() {
        return Err(
            "The repository is in the middle of a merge or rebase. Finish or abort it with git \
             before syncing."
                .to_string(),
        );
    }
    Ok(repo)
}

const MANAGED_START: &str = "# >>> HelixNotes (managed block, rewritten on every Git sync)";
const MANAGED_END: &str = "# <<< HelixNotes";

/// Puts `rules` into `existing` as the managed block, keeping every line outside it, so a
/// repository the user set up keeps its own `info/exclude` entries.
fn with_managed_block(existing: &str, rules: &str) -> String {
    let mut kept = Vec::new();
    let mut inside = false;
    for line in existing.lines() {
        if line == MANAGED_START {
            inside = true;
        } else if line == MANAGED_END {
            inside = false;
        } else if !inside {
            kept.push(line);
        }
    }
    while kept.last().is_some_and(|line| line.trim().is_empty()) {
        kept.pop();
    }
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(MANAGED_START);
    out.push('\n');
    out.push_str(rules.trim_end());
    out.push('\n');
    out.push_str(MANAGED_END);
    out.push('\n');
    out
}

fn write_if_changed(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    if std::fs::read_to_string(path).ok().as_deref() != Some(content) {
        std::fs::write(path, content)
            .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    }
    Ok(())
}

/// Writes the rules where git applies them (`info/exclude`) and, on their own, to a file used
/// to find what they exclude. Returns that file.
fn write_exclude(repo: &Repo, cfg: &GitSyncConfig) -> Result<PathBuf, String> {
    let rules = exclude_rules(&cfg.folders, &cfg.exclude);
    let exclude = repo.git_path("info/exclude")?;
    let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
    write_if_changed(&exclude, &with_managed_block(&existing, &rules))?;
    let own = repo.git_path("info/helixnotes-exclude")?;
    write_if_changed(&own, &rules)?;
    Ok(own)
}

fn ensure_remote(repo: &Repo, url: &str) -> Result<(), String> {
    let current = repo.git(["remote", "get-url", REMOTE_NAME])?;
    if !current.ok {
        repo.check("add the remote", ["remote", "add", REMOTE_NAME, url])?;
    } else if current.text() != url {
        repo.check("change the remote", ["remote", "set-url", REMOTE_NAME, url])?;
    }
    Ok(())
}

/// Removes from the repository (not from disk) files that were committed before and are excluded
/// now, so excluding a file or folder takes effect for the remote too.
fn untrack_excluded(repo: &Repo, rules_file: &Path) -> Result<u32, String> {
    // Only HelixNotes' rules: a file the user force-added despite their own .gitignore stays.
    let mut from = std::ffi::OsString::from("--exclude-from=");
    from.push(rules_file);
    let listed = repo.check(
        "list excluded files",
        [
            OsStr::new("ls-files"),
            OsStr::new("-z"),
            OsStr::new("--cached"),
            OsStr::new("--ignored"),
            from.as_os_str(),
        ],
    )?;
    let paths = split_nul(&listed.stdout);
    for chunk in paths.chunks(200) {
        let mut args = vec!["rm", "--cached", "--quiet", "--ignore-unmatch", "--"];
        args.extend(chunk.iter().map(String::as_str));
        repo.check("stop tracking excluded files", args)?;
    }
    Ok(paths.len() as u32)
}

/// Stages everything (within the rules) and commits it. Returns how many files the commit had.
fn commit_all(repo: &Repo, message: &str) -> Result<u32, String> {
    repo.check("stage changes", ["add", "--all", "--", "."])?;
    let staged = repo.check(
        "list staged changes",
        ["diff", "--cached", "--name-only", "-z", "--no-renames"],
    )?;
    let count = split_nul(&staged.stdout).len() as u32;
    if count > 0 {
        repo.check(
            "commit",
            ["commit", "--quiet", "--no-verify", "-m", message],
        )?;
    }
    Ok(count)
}

fn commit_message(files: u32) -> String {
    let device = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .filter(|name| !name.trim().is_empty());
    let noun = if files == 1 { "file" } else { "files" };
    match device {
        Some(device) => format!("HelixNotes sync from {device}: {files} {noun}"),
        None => format!("HelixNotes sync: {files} {noun}"),
    }
}

// ───────────────────────── merging ─────────────────────────

/// A vault-relative path the sync changed, with the watcher's event kind for it.
type Changed = (String, &'static str);

/// One side of a conflicted path: the blob git staged for it, if that side has the file.
fn conflict_stages(repo: &Repo, path: &str) -> Result<(Option<String>, Option<String>), String> {
    let listed = repo.check("read a conflict", ["ls-files", "-u", "-z", "--", path])?;
    let (mut ours, mut theirs) = (None, None);
    for entry in split_nul(&listed.stdout) {
        // "<mode> <object> <stage>\t<path>"
        let Some((meta, _)) = entry.split_once('\t') else {
            continue;
        };
        let fields: Vec<&str> = meta.split(' ').collect();
        if fields.len() != 3 {
            continue;
        }
        match fields[2] {
            "2" => ours = Some(fields[1].to_string()),
            "3" => theirs = Some(fields[1].to_string()),
            _ => {}
        }
    }
    Ok((ours, theirs))
}

fn blob(repo: &Repo, object: &str) -> Result<Vec<u8>, String> {
    Ok(repo
        .check("read a file version", ["cat-file", "blob", object])?
        .stdout)
}

fn write_file(vault: &Path, relpath: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let path = vault.join(relpath);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&path, bytes)
        .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    Ok(path)
}

/// Resolves every conflicted path so that no version is lost: both changed → keep ours in place
/// and theirs as a conflict copy; one side deleted → keep the side that still has the file.
/// Returns the vault-relative paths written.
fn resolve_conflicts(repo: &Repo, conflicted: &[String]) -> Result<Vec<String>, String> {
    let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S").to_string();
    let mut written = Vec::new();
    for path in conflicted {
        let (ours, theirs) = conflict_stages(repo, path)?;
        match (ours, theirs) {
            (Some(ours), Some(theirs)) => {
                write_file(&repo.work_tree, path, &blob(repo, &ours)?)?;
                let copy = crate::sync::conflict_relpath(path, &stamp);
                let bytes = crate::sync::conflict_copy(&copy, blob(repo, &theirs)?, &stamp);
                write_file(&repo.work_tree, &copy, &bytes)?;
                repo.check(
                    "keep a conflict copy",
                    ["add", "--force", "--", copy.as_str()],
                )?;
                written.push(path.clone());
                written.push(copy);
            }
            (Some(kept), None) | (None, Some(kept)) => {
                write_file(&repo.work_tree, path, &blob(repo, &kept)?)?;
                written.push(path.clone());
            }
            (None, None) => {}
        }
        repo.check(
            "resolve a conflict",
            ["add", "--force", "--", path.as_str()],
        )?;
    }
    Ok(written)
}

/// Merges the fetched remote branch. Returns (files the merge changed, conflicts resolved).
fn merge_remote(repo: &Repo, branch: &str) -> Result<(Vec<Changed>, u32), String> {
    let remote_ref = format!("refs/remotes/{REMOTE_NAME}/{branch}");
    let remote_head = repo.git(["rev-parse", "--verify", "--quiet", &remote_ref])?;
    if !remote_head.ok {
        // The remote has no such branch yet; the push creates it.
        return Ok((Vec::new(), 0));
    }
    let before = repo.head()?;
    let message = format!("HelixNotes: merge {REMOTE_NAME}/{branch}");
    let merged = repo.git([
        "merge",
        "--no-edit",
        "--no-verify",
        "--allow-unrelated-histories",
        "-m",
        message.as_str(),
        remote_ref.as_str(),
    ])?;
    let mut conflicts = 0;
    if !merged.ok {
        let unmerged = repo.check(
            "list conflicts",
            ["diff", "--name-only", "--diff-filter=U", "-z"],
        )?;
        let conflicted = split_nul(&unmerged.stdout);
        if conflicted.is_empty() {
            // Not a content conflict (for example a local file in the way): leave things as
            // they were and report git's reason.
            let _ = repo.git(["merge", "--abort"]);
            return Err(git_error("merge the remote changes", &merged));
        }
        conflicts = conflicted.len() as u32;
        if let Err(error) = resolve_conflicts(repo, &conflicted) {
            let _ = repo.git(["merge", "--abort"]);
            return Err(error);
        }
        repo.check(
            "finish the merge",
            ["commit", "--quiet", "--no-verify", "--no-edit"],
        )?;
    }

    let after = repo.head()?;
    let mut changed = Vec::new();
    if after != before {
        match (before, after) {
            (Some(before), Some(after)) => {
                let diff = repo.check(
                    "list merged changes",
                    [
                        "diff",
                        "--name-status",
                        "-z",
                        "--no-renames",
                        &before,
                        &after,
                    ],
                )?;
                for pair in split_nul(&diff.stdout).chunks(2) {
                    if let [status, path] = pair {
                        let kind = match status.chars().next() {
                            Some('D') => "remove",
                            Some('A') => "create",
                            _ => "modify",
                        };
                        changed.push((path.clone(), kind));
                    }
                }
            }
            // Nothing was committed here before: everything the merge brought is new.
            (None, Some(after)) => {
                let listed = repo.check(
                    "list merged files",
                    ["ls-tree", "-r", "-z", "--name-only", &after],
                )?;
                changed.extend(split_nul(&listed.stdout).into_iter().map(|p| (p, "create")));
            }
            _ => {}
        }
    }
    Ok((changed, conflicts))
}

// ───────────────────────── the sync ─────────────────────────

fn emit_progress(app: &tauri::AppHandle, vault: &str, bookmark_id: &Option<String>, status: &str) {
    let _ = app.emit(
        "git-sync-progress",
        serde_json::json!({ "vault_path": vault, "bookmark_id": bookmark_id, "status": status }),
    );
}

/// The steps of a sync that do not need the app: used by `run_git_sync` and by the tests.
/// `lock` is held around every step that reads or writes the work tree, so a note being saved
/// is never half-committed or overwritten mid-save. Network steps run without it.
fn sync_repo<L>(
    cfg: &GitSyncConfig,
    lock: impl Fn() -> L,
    mut progress: impl FnMut(&str),
) -> Result<(GitSyncSummary, Vec<Changed>), String> {
    let mut summary = GitSyncSummary::default();
    let repo = open_repo(cfg)?;
    let rules_file = write_exclude(&repo, cfg)?;
    {
        let _guard = lock();
        summary.untracked = untrack_excluded(&repo, &rules_file)?;
        let pending = repo.check("check for changes", ["status", "--porcelain", "-z"])?;
        let count = split_nul(&pending.stdout).len() as u32;
        if count > 0 || summary.untracked > 0 {
            summary.committed += commit_all(&repo, &commit_message(count.max(1)))?;
        }
    }

    let mut changed = Vec::new();
    let remote = cfg.remote.trim();
    if !remote.is_empty() {
        ensure_remote(&repo, remote)?;
        progress("fetching");
        let fetched = repo.git_network(["fetch", "--quiet", "--prune", REMOTE_NAME])?;
        if !fetched.ok {
            return Err(git_error("fetch from the remote", &fetched));
        }
        {
            let _guard = lock();
            // A note saved while fetching is committed before the merge touches the work tree.
            let pending = repo.check("check for changes", ["status", "--porcelain", "-z"])?;
            let count = split_nul(&pending.stdout).len() as u32;
            if count > 0 {
                summary.committed += commit_all(&repo, &commit_message(count))?;
            }
            progress("merging");
            let (merged, conflicts) = merge_remote(&repo, &cfg.branch)?;
            summary.pulled = merged.len() as u32;
            summary.conflicts = conflicts;
            changed = merged;
        }

        let ahead = match repo.head()? {
            None => false,
            Some(_) => {
                let remote_ref = format!("refs/remotes/{REMOTE_NAME}/{}", cfg.branch);
                let exists = repo
                    .git(["rev-parse", "--verify", "--quiet", &remote_ref])?
                    .ok;
                if exists {
                    let range = format!("{remote_ref}..HEAD");
                    let count =
                        repo.check("compare with the remote", ["rev-list", "--count", &range])?;
                    count.text() != "0"
                } else {
                    true
                }
            }
        };
        if ahead {
            progress("pushing");
            let target = format!("HEAD:refs/heads/{}", cfg.branch);
            let pushed = repo.git_network(["push", "--quiet", REMOTE_NAME, target.as_str()])?;
            if !pushed.ok {
                return Err(git_error("push to the remote", &pushed));
            }
            // Keep the remote-tracking ref current so the next sync knows nothing is pending.
            let _ = repo.git(["fetch", "--quiet", REMOTE_NAME]);
            summary.pushed = true;
        }
    }
    Ok((summary, changed))
}

pub fn run_git_sync(
    app: tauri::AppHandle,
    bookmark_id: Option<String>,
    cfg: GitSyncConfig,
) -> Result<GitSyncSummary, String> {
    use std::sync::atomic::Ordering;

    let state = app.state::<AppState>();
    // Serializes with WebDAV sync, backups and other whole-vault work.
    let _maintenance = state.vault_maintenance.blocking_lock();
    let search = state.vault_snapshot(&cfg.vault)?;
    let vault = cfg.vault.clone();
    emit_progress(&app, &vault, &bookmark_id, "committing");

    let result = sync_repo(
        &cfg,
        || {
            let guard = state.note_save.blocking_lock();
            // Our own writes must not echo back as external edits.
            state.importing.store(true, Ordering::Release);
            ImportingGuard {
                _save: guard,
                flag: &state.importing,
            }
        },
        |status| emit_progress(&app, &vault, &bookmark_id, status),
    );
    let (summary, changed) = result?;

    let active = state
        .config
        .lock()
        .ok()
        .is_some_and(|config| config.active_vault.as_deref() == Some(vault.as_str()));
    let vault_root = Path::new(&vault);
    let metadata_root = helixnotes_dir(&vault);
    for (relpath, event_type) in changed {
        let path = vault_root.join(&relpath);
        let is_note = relpath.ends_with(".md") && !path.starts_with(&metadata_root);
        if is_note {
            if let Some(search) = &search {
                if let Err(error) = search.queue_index_note(&path.to_string_lossy()) {
                    log::error!("Could not queue a note from Git for search: {error}");
                }
            }
        }
        if !active {
            continue;
        }
        let event = if is_note {
            changed_entry(path, event_type, vault_root)
        } else {
            crate::types::FileEvent {
                vault_path: vault.clone(),
                event_type: event_type.to_string(),
                path: path.to_string_lossy().to_string(),
                note: None,
            }
        };
        let _ = app.emit("file-changed", event);
    }
    Ok(summary)
}

struct ImportingGuard<'a, G> {
    _save: G,
    flag: &'a std::sync::atomic::AtomicBool,
}

impl<G> Drop for ImportingGuard<'_, G> {
    fn drop(&mut self) {
        self.flag.store(false, std::sync::atomic::Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git_available() -> bool {
        git_version().is_some()
    }

    fn plain_git(dir: &Path, args: &[&str]) -> String {
        let mut command = base_command();
        command
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .current_dir(dir)
            .args(args);
        let output = run(command, LOCAL_TIMEOUT).unwrap();
        assert!(output.ok, "git {args:?} failed: {}", output.stderr);
        output.text()
    }

    struct Device {
        vault: tempfile::TempDir,
        data: tempfile::TempDir,
    }

    impl Device {
        fn new() -> Self {
            Device {
                vault: tempfile::tempdir().unwrap(),
                data: tempfile::tempdir().unwrap(),
            }
        }
        fn cfg(&self, remote: &Path) -> GitSyncConfig {
            GitSyncConfig {
                vault: self.vault.path().to_string_lossy().into_owned(),
                external_git_dir: self.data.path().join("repo"),
                remote: remote.to_string_lossy().into_owned(),
                branch: DEFAULT_BRANCH.to_string(),
                folders: Vec::new(),
                exclude: Vec::new(),
            }
        }
        fn write(&self, rel: &str, text: &str) {
            let path = self.vault.path().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        fn read(&self, rel: &str) -> Option<String> {
            fs::read_to_string(self.vault.path().join(rel)).ok()
        }
    }

    fn sync(cfg: &GitSyncConfig) -> GitSyncSummary {
        sync_repo(cfg, || (), |_| {}).unwrap().0
    }

    fn bare_remote() -> tempfile::TempDir {
        let remote = tempfile::tempdir().unwrap();
        plain_git(
            remote.path(),
            &["init", "--quiet", "--bare", "--initial-branch=main"],
        );
        remote
    }

    fn remote_files(remote: &Path) -> Vec<String> {
        let listed = plain_git(remote, &["ls-tree", "-r", "--name-only", "main"]);
        listed.lines().map(String::from).collect()
    }

    #[test]
    fn folder_rules_keep_only_chosen_folders_and_their_parents() {
        let rules = exclude_rules(&["Work/Projects".into(), "Personal".into()], &[]);
        let expected =
            "/*\n!/.helixnotes/\n!/.gitignore\n!/Personal/\n!/Work/\n/Work/*\n!/Work/Projects/\n";
        assert!(rules.contains(expected), "{rules}");
        assert!(!exclude_rules(&[], &[]).contains("\n/*\n"));
    }

    #[test]
    fn the_managed_block_keeps_the_users_own_exclude_lines() {
        let first = with_managed_block("# mine\n*.log\n", "a\nb\n");
        assert_eq!(
            first,
            format!("# mine\n*.log\n\n{MANAGED_START}\na\nb\n{MANAGED_END}\n")
        );
        let second = with_managed_block(&first, "c\n");
        assert_eq!(
            second,
            format!("# mine\n*.log\n\n{MANAGED_START}\nc\n{MANAGED_END}\n")
        );
        assert_eq!(
            with_managed_block("", "x"),
            format!("{MANAGED_START}\nx\n{MANAGED_END}\n")
        );
    }

    #[test]
    fn folder_names_are_escaped_and_bad_folders_dropped() {
        assert_eq!(escape_segment("#a*b "), "\\#a\\*b\\ ");
        assert_eq!(normalize_folder("/a\\b/./c/"), Some("a/b/c".into()));
        assert_eq!(normalize_folder("../x"), None);
        assert_eq!(normalize_folder("  "), Some("  ".into()));
        assert_eq!(normalize_folder("/"), None);
    }

    #[test]
    fn repository_stays_outside_the_vault_and_skips_foreign_sync_files() {
        if !git_available() {
            return;
        }
        let remote = bare_remote();
        let device = Device::new();
        device.write("Note.md", "hello");
        device.write(".stfolder/marker", "");
        device.write("Note.sync-conflict-20260101-000000-ABCDEF.md", "other");
        device.write(".helixnotes/search_index/x", "local");
        device.write(".helixnotes/attachments/pic.png", "png");
        let summary = sync(&device.cfg(remote.path()));
        assert!(summary.pushed);
        assert!(!device.vault.path().join(".git").exists());
        assert!(!device.vault.path().join(".gitignore").exists());
        assert_eq!(
            remote_files(remote.path()),
            vec![
                ".helixnotes/attachments/pic.png".to_string(),
                "Note.md".to_string()
            ]
        );
    }

    #[test]
    fn only_chosen_folders_and_no_excluded_files_are_committed() {
        if !git_available() {
            return;
        }
        let remote = bare_remote();
        let device = Device::new();
        device.write("Root.md", "root");
        device.write("Work/A.md", "a");
        device.write("Work/draft.tmp.md", "draft");
        device.write("Private/B.md", "b");
        let mut cfg = device.cfg(remote.path());
        cfg.folders = vec!["Work".into()];
        cfg.exclude = vec!["*.tmp.md".into()];
        sync(&cfg);
        assert_eq!(remote_files(remote.path()), vec!["Work/A.md".to_string()]);

        // Excluding something already committed removes it from the repository, not the disk.
        cfg.exclude = vec!["*.tmp.md".into(), "Work/A.md".into()];
        device.write("Work/C.md", "c");
        let summary = sync(&cfg);
        assert_eq!(summary.untracked, 1);
        assert_eq!(remote_files(remote.path()), vec!["Work/C.md".to_string()]);
        assert_eq!(device.read("Work/A.md").as_deref(), Some("a"));
    }

    #[test]
    fn two_devices_merge_and_keep_both_sides_of_a_conflict() {
        if !git_available() {
            return;
        }
        let remote = bare_remote();
        let (first, second) = (Device::new(), Device::new());
        first.write("Shared.md", "base");
        sync(&first.cfg(remote.path()));
        // The second device already has the same notes, as when Syncthing copied them first.
        second.write("Shared.md", "base");
        let summary = sync(&second.cfg(remote.path()));
        assert_eq!(summary.conflicts, 0);

        first.write("Shared.md", "from first");
        first.write("Only first.md", "1");
        sync(&first.cfg(remote.path()));
        second.write("Shared.md", "from second");
        let summary = sync(&second.cfg(remote.path()));
        assert_eq!(summary.conflicts, 1);
        assert!(summary.pushed);
        assert_eq!(second.read("Shared.md").as_deref(), Some("from second"));
        assert_eq!(second.read("Only first.md").as_deref(), Some("1"));
        let copies: Vec<_> = fs::read_dir(second.vault.path())
            .unwrap()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| name.starts_with("Shared (conflict "))
            .collect();
        assert_eq!(copies.len(), 1);
        assert_eq!(second.read(&copies[0]).as_deref(), Some("from first"));

        let summary = sync(&first.cfg(remote.path()));
        assert!(summary.pulled >= 2);
        assert_eq!(first.read("Shared.md").as_deref(), Some("from second"));
    }

    #[test]
    fn a_deleted_note_edited_elsewhere_is_kept() {
        if !git_available() {
            return;
        }
        let remote = bare_remote();
        let (first, second) = (Device::new(), Device::new());
        first.write("N.md", "v1");
        sync(&first.cfg(remote.path()));
        sync(&second.cfg(remote.path()));
        assert_eq!(second.read("N.md").as_deref(), Some("v1"));
        first.write("N.md", "v2");
        sync(&first.cfg(remote.path()));
        fs::remove_file(second.vault.path().join("N.md")).unwrap();
        sync(&second.cfg(remote.path()));
        assert_eq!(second.read("N.md").as_deref(), Some("v2"));
    }

    #[test]
    fn local_only_mode_commits_without_a_remote() {
        if !git_available() {
            return;
        }
        let device = Device::new();
        device.write("A.md", "a");
        let mut cfg = device.cfg(Path::new(""));
        cfg.remote = String::new();
        let summary = sync(&cfg);
        assert_eq!(summary.committed, 1);
        assert!(!summary.pushed);
        let log = plain_git(
            device.vault.path(),
            &[
                "--git-dir",
                &cfg.external_git_dir.to_string_lossy(),
                "log",
                "--format=%s",
            ],
        );
        assert!(log.starts_with("HelixNotes sync"), "{log}");
    }

    #[test]
    fn an_existing_repository_in_the_vault_is_used_as_it_is() {
        if !git_available() {
            return;
        }
        let device = Device::new();
        plain_git(
            device.vault.path(),
            &["init", "--quiet", "--initial-branch=main"],
        );
        device.write("A.md", "a");
        let mut cfg = device.cfg(Path::new(""));
        cfg.remote = String::new();
        device.write(".gitignore", "*.log\n");
        device.write("keep.log", "kept on purpose");
        plain_git(device.vault.path(), &["add", "--force", "keep.log"]);
        fs::write(device.vault.path().join(".git/info/exclude"), "# mine\n").unwrap();
        sync(&cfg);
        assert!(!cfg.external_git_dir.exists());
        let files = plain_git(device.vault.path(), &["ls-files"]);
        assert_eq!(files, ".gitignore\nA.md\nkeep.log");
        let exclude = fs::read_to_string(device.vault.path().join(".git/info/exclude")).unwrap();
        assert!(exclude.starts_with("# mine\n"), "{exclude}");
    }
}
