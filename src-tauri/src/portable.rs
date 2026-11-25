//! Portable mode: keep every piece of app state next to the executable.
//!
//! Portable mode is on when any of these holds (desktop only):
//! - the build was made with `HELIXNOTES_INSTALL_TYPE=portable`;
//! - a file named `portable` sits next to the executable (next to the `.AppImage` on Linux);
//! - `HELIXNOTES_PORTABLE_DIR` names the folder to use as the portable root.
//!
//! App state then lives in `<root>/data/`, and paths inside `<root>` are written to the config
//! relative to it, so a portable folder keeps working when it moves (e.g. a USB drive that gets
//! another letter).

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// Name of the marker file that switches a regular build to portable mode.
pub const MARKER_FILE: &str = "portable";
const DATA_DIR: &str = "data";

static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Folder that holds the executable and the `data/` folder, when portable mode is on.
pub fn root() -> Option<&'static Path> {
    ROOT.get_or_init(detect_root).as_deref()
}

pub fn is_active() -> bool {
    root().is_some()
}

/// `<root>/data`, where all app state lives in portable mode.
pub fn data_dir() -> Option<PathBuf> {
    root().map(|root| root.join(DATA_DIR))
}

/// Webview data (localStorage, cache) for every window, in portable mode.
pub fn webview_data_dir() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("webview"))
}

/// Saved window size and position, in portable mode. Absolute, so the window-state plugin's
/// join onto its own config folder yields this path unchanged.
pub fn window_state_file() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("window-state.json"))
}

/// Main window creation is deferred to `create_main_window` when portable, because a window
/// made from the config cannot take an absolute data directory.
#[cfg(desktop)]
pub fn defer_main_window<R: tauri::Runtime>(context: &mut tauri::Context<R>) {
    if !is_active() {
        return;
    }
    for window in &mut context.config_mut().app.windows {
        if window.label == "main" {
            window.create = false;
        }
    }
}

/// Builds the main window from its config, with its webview data inside the portable folder.
#[cfg(desktop)]
pub fn create_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let Some(data_directory) = webview_data_dir() else {
        return Ok(());
    };
    let Some(config) = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .cloned()
    else {
        return Ok(());
    };
    tauri::WebviewWindowBuilder::from_config(app, &config)?
        .data_directory(data_directory)
        .build()?;
    Ok(())
}

/// Opens a detached note window. Built here rather than from JS so that, in portable mode, it
/// shares the main window's webview data folder (JS can only pass a relative one).
/// Async: creating a window from a sync command deadlocks on Windows.
#[cfg(desktop)]
#[tauri::command]
pub async fn open_note_window<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    label: String,
    url: String,
    title: String,
) -> Result<(), String> {
    if !label.starts_with("note-") {
        return Err("Note window labels must start with \"note-\"".to_string());
    }
    if url.contains("://") {
        return Err("Note windows only open app pages".to_string());
    }
    let mut builder =
        tauri::WebviewWindowBuilder::new(&app, label, tauri::WebviewUrl::App(PathBuf::from(url)))
            .title(title)
            .inner_size(900.0, 700.0)
            .min_inner_size(500.0, 400.0)
            .decorations(false)
            .center();
    if let Some(data_directory) = webview_data_dir() {
        builder = builder.data_directory(data_directory);
    }
    builder.build().map(|_| ()).map_err(|e| e.to_string())
}

fn detect_root() -> Option<PathBuf> {
    if cfg!(mobile) {
        return None;
    }
    if let Some(dir) = std::env::var_os("HELIXNOTES_PORTABLE_DIR").filter(|d| !d.is_empty()) {
        return Some(absolute(PathBuf::from(dir)));
    }
    let base = executable_dir()?;
    let forced = option_env!("HELIXNOTES_INSTALL_TYPE") == Some("portable");
    (forced || base.join(MARKER_FILE).is_file()).then_some(base)
}

/// Folder the user sees the app in. Inside an AppImage the binary runs from a temporary mount,
/// so the AppImage file's own folder is used instead.
fn executable_dir() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        if let Some(appimage) = std::env::var_os("APPIMAGE").filter(|a| !a.is_empty()) {
            return PathBuf::from(appimage).parent().map(Path::to_path_buf);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    exe.parent().map(Path::to_path_buf)
}

fn absolute(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(&path))
        .unwrap_or(path)
}

/// Turns a path read from the config into an absolute one: relative paths are relative to the
/// portable root. Outside portable mode, or for absolute or empty paths, returns it unchanged.
pub fn resolve_stored(path: &str) -> String {
    match root() {
        Some(root) => resolve_stored_in(root, path),
        None => path.to_string(),
    }
}

/// Turns an absolute path into the form written to the config: relative to the portable root
/// when it lies inside it. Outside portable mode, returns it unchanged.
pub fn to_stored(path: &str) -> String {
    match root() {
        Some(root) => to_stored_in(root, path),
        None => path.to_string(),
    }
}

fn resolve_stored_in(root: &Path, path: &str) -> String {
    if path.is_empty() || Path::new(path).is_absolute() {
        return path.to_string();
    }
    let relative: PathBuf = Path::new(path)
        .components()
        .filter(|part| !matches!(part, Component::CurDir))
        .collect();
    if relative.as_os_str().is_empty() {
        // Joining an empty path would add a trailing separator.
        return root.to_string_lossy().into_owned();
    }
    root.join(relative).to_string_lossy().into_owned()
}

fn to_stored_in(root: &Path, path: &str) -> String {
    let candidate = Path::new(path);
    if path.is_empty() || !candidate.is_absolute() {
        return path.to_string();
    }
    match candidate.strip_prefix(root) {
        // Keep "." for the root itself so an empty string never stands for a real path.
        Ok(rest) if rest.as_os_str().is_empty() => ".".to_string(),
        // Forward slashes read back on every OS, so a portable folder moves between them too.
        Ok(rest) => rest
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => path.to_string(),
    }
}

/// Rewrites the paths a config stores, in place, with `convert`.
pub fn map_config_paths(config: &mut crate::types::AppConfig, convert: impl Fn(&str) -> String) {
    for vault in &mut config.vaults {
        vault.path = convert(&vault.path);
    }
    if let Some(active) = config.active_vault.as_mut() {
        *active = convert(active);
    }
    for folder in &mut config.search_folders {
        *folder = convert(folder);
    }
    config.external_attachments_folder = convert(&config.external_attachments_folder);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"E:\HelixNotes")
        } else {
            PathBuf::from("/media/usb/HelixNotes")
        }
    }

    #[test]
    fn paths_inside_the_root_are_stored_relative() {
        let root = root();
        let vault = root.join("Notes").join("Work");
        assert_eq!(to_stored_in(&root, &vault.to_string_lossy()), "Notes/Work");
        assert_eq!(to_stored_in(&root, &root.to_string_lossy()), ".");
    }

    #[test]
    fn paths_outside_the_root_stay_absolute() {
        let root = root();
        let outside = root.parent().unwrap().join("Other");
        let outside = outside.to_string_lossy();
        assert_eq!(to_stored_in(&root, &outside), outside);
        // A sibling that only shares a name prefix is not inside the root.
        let sibling = format!("{}2", root.to_string_lossy());
        assert_eq!(to_stored_in(&root, &sibling), sibling);
    }

    #[test]
    fn stored_paths_resolve_against_the_root() {
        let root = root();
        let expected = root.join("Notes").join("Work");
        assert_eq!(
            resolve_stored_in(&root, "Notes/Work"),
            expected.to_string_lossy()
        );
        assert_eq!(resolve_stored_in(&root, "."), root.to_string_lossy());
        assert_eq!(resolve_stored_in(&root, ""), "");
    }

    #[test]
    fn a_moved_root_finds_the_same_vault() {
        let old_root = root();
        let new_root = old_root.parent().unwrap().join("Moved");
        let vault = old_root.join("Notes");
        let stored = to_stored_in(&old_root, &vault.to_string_lossy());
        assert_eq!(
            resolve_stored_in(&new_root, &stored),
            new_root.join("Notes").to_string_lossy()
        );
    }
}
