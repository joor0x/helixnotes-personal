//! Images and files pasted into a Markdown file outside the vault. They are stored with
//! the file's project (its git repository, or else its folder) in an attachments folder
//! and linked with a relative path, so they travel with the project.

use crate::state::AppState;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_fs::FsExt;

/// Optional per-project settings file at the project root, e.g. `{"attachmentsFolder": "docs/img"}`.
pub const PROJECT_SETTINGS_FILE: &str = ".helixnotes.json";
pub const DEFAULT_FOLDER: &str = "assets";

/// The nearest ancestor holding `.git` (a directory, or a file in worktrees); else the note's folder.
pub fn project_root(note: &Path) -> PathBuf {
    let folder = note.parent().unwrap_or(note).to_path_buf();
    let home = dirs::home_dir();
    for ancestor in folder.ancestors() {
        if ancestor.join(".git").exists() || ancestor.join(PROJECT_SETTINGS_FILE).is_file() {
            return ancestor.to_path_buf();
        }
        if home.as_deref() == Some(ancestor) {
            break;
        }
    }
    folder
}

fn project_folder_setting(root: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(root.join(PROJECT_SETTINGS_FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value
        .get("attachmentsFolder")?
        .as_str()
        .map(|folder| folder.trim().to_string())
        .filter(|folder| !folder.is_empty())
}

/// A folder relative to the project root that cannot climb out of it.
pub fn validate_folder(folder: &str) -> Result<PathBuf, String> {
    let folder = folder.trim().trim_matches(['/', '\\']);
    if folder.is_empty() {
        return Ok(PathBuf::new());
    }
    let path = PathBuf::from(folder.replace('\\', "/"));
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err("The attachments folder must be a path inside the project".into());
    }
    Ok(path)
}

/// `target` relative to `from_dir`, with forward slashes (both canonical).
fn relative_to(from_dir: &Path, target: &Path) -> String {
    let from: Vec<_> = from_dir.components().collect();
    let to: Vec<_> = target.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".into(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|component| component.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

fn split_name(name: &str) -> (String, String) {
    let safe = crate::vault::operations::sanitize_filename(name);
    let safe = if safe.is_empty() || safe.starts_with('.') {
        format!("image{safe}")
    } else {
        safe
    };
    match safe.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => {
            (stem.into(), format!(".{ext}"))
        }
        _ => (safe, String::new()),
    }
}

/// Writes `data` into `dir` as `name`, or `name-2`, `name-3`… when it is taken.
fn write_unique(dir: &Path, name: &str, data: &[u8]) -> Result<PathBuf, String> {
    let (stem, ext) = split_name(name);
    let mut temporary = tempfile::Builder::new()
        .prefix(".helixnotes-asset-")
        .tempfile_in(dir)
        .map_err(|error| error.to_string())?;
    temporary.write_all(data).map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    for attempt in 1..10_000 {
        let filename = if attempt == 1 {
            format!("{stem}{ext}")
        } else {
            format!("{stem}-{attempt}{ext}")
        };
        let target = dir.join(&filename);
        match crate::vault::operations::persist_without_replacing(temporary, &target) {
            Ok(()) => return Ok(target),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                temporary = error.file;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("Could not find a free file name".into())
}

/// Saves into `<project>/<attachments folder>` and returns the link relative to the note.
pub fn save_next_to_note(
    note: &Path,
    default_folder: &str,
    name: &str,
    data: &[u8],
) -> Result<(String, PathBuf), String> {
    let root = project_root(note);
    let folder = project_folder_setting(&root).unwrap_or_else(|| default_folder.to_string());
    let dir = root.join(validate_folder(&folder)?);
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let dir = std::fs::canonicalize(&dir).map_err(|error| error.to_string())?;
    // A symlinked folder must not carry writes out of the project.
    if !dir.starts_with(&root) {
        return Err("The attachments folder must be inside the project".into());
    }
    let file = write_unique(&dir, name, data)?;
    let note_dir = note.parent().unwrap_or(&root);
    Ok((relative_to(note_dir, &file), dir))
}

#[tauri::command]
pub async fn save_external_attachment<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    note_path: String,
    name: String,
    data: Vec<u8>,
) -> Result<String, String> {
    let note = std::fs::canonicalize(&note_path).map_err(|error| error.to_string())?;
    crate::commands::ensure_scoped_path(&app, &note)?;
    if !note.is_file() || note.extension().and_then(|ext| ext.to_str()) != Some("md") {
        return Err("Note path must point to a Markdown file".into());
    }
    let (default_folder, vaults) = {
        let config = state.config.lock().map_err(|error| error.to_string())?;
        let vaults: Vec<String> = config
            .vaults
            .iter()
            .map(|vault| vault.path.clone())
            .chain(config.active_vault.clone())
            .collect();
        (config.external_attachments_folder.clone(), vaults)
    };
    if vaults
        .iter()
        .filter_map(|vault| std::fs::canonicalize(vault).ok())
        .any(|vault| note.starts_with(vault))
    {
        return Err("Notes inside a vault keep their attachments in the vault".into());
    }
    let _write_guard = state.note_save.lock().await;
    let (link, dir) = tauri::async_runtime::spawn_blocking(move || {
        save_next_to_note(&note, &default_folder, &name, &data)
    })
    .await
    .map_err(|error| error.to_string())??;
    let _ = app.fs_scope().allow_directory(&dir, true);
    app.asset_protocol_scope()
        .allow_directory(&dir, true)
        .map_err(|error| error.to_string())?;
    Ok(link)
}

/// Lets an opened external note show images from its project's attachments folder.
pub fn allow_project_assets<R: Runtime>(app: &AppHandle<R>, note: &Path) {
    let Ok(note) = std::fs::canonicalize(note) else {
        return;
    };
    let default_folder = app
        .try_state::<AppState>()
        .and_then(|state| {
            state
                .config
                .lock()
                .ok()
                .map(|c| c.external_attachments_folder.clone())
        })
        .unwrap_or_else(|| DEFAULT_FOLDER.into());
    let root = project_root(&note);
    let folder = project_folder_setting(&root).unwrap_or(default_folder);
    let Ok(folder) = validate_folder(&folder) else {
        return;
    };
    if let Ok(dir) = std::fs::canonicalize(root.join(folder)) {
        if dir.starts_with(&root) {
            let _ = app.fs_scope().allow_directory(&dir, true);
            let _ = app.asset_protocol_scope().allow_directory(&dir, true);
        }
    }
}

#[tauri::command]
pub fn set_attachment_settings(
    state: State<'_, AppState>,
    ask_image_name: bool,
    external_attachments_folder: String,
) -> Result<(), String> {
    let folder = external_attachments_folder.trim().to_string();
    validate_folder(&folder)?;
    let mut current = state.config.lock().map_err(|e| e.to_string())?;
    let mut config = current.clone();
    config.ask_image_name = ask_image_name;
    config.external_attachments_folder = if folder.is_empty() {
        DEFAULT_FOLDER.into()
    } else {
        folder
    };
    crate::commands::save_app_config_to(&config, &crate::commands::app_config_path()?)?;
    *current = config;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hn-ext-assets-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(dir).unwrap()
    }

    #[test]
    fn images_go_to_the_repository_attachments_folder_with_relative_links() {
        let repo = temp_dir("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        let note = repo.join("docs/instalacion.md");
        std::fs::write(&note, "# Instalación\n").unwrap();
        let (link, _) = save_next_to_note(&note, "assets", "captura.png", b"png").unwrap();
        assert_eq!(link, "../assets/captura.png");
        let (second, _) = save_next_to_note(&note, "assets", "captura.png", b"png2").unwrap();
        assert_eq!(second, "../assets/captura-2.png");
        assert_eq!(
            std::fs::read(repo.join("assets/captura-2.png")).unwrap(),
            b"png2"
        );
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn project_settings_file_overrides_the_folder() {
        let repo = temp_dir("settings");
        std::fs::write(
            repo.join(PROJECT_SETTINGS_FILE),
            r#"{"attachmentsFolder": "docs/img"}"#,
        )
        .unwrap();
        let note = repo.join("README.md");
        std::fs::write(&note, "x").unwrap();
        let (link, _) = save_next_to_note(&note, "assets", "a.png", b"x").unwrap();
        assert_eq!(link, "docs/img/a.png");
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn folders_cannot_leave_the_project() {
        assert!(validate_folder("../fuera").is_err());
        assert!(validate_folder("/etc").is_ok_and(|p| p == Path::new("etc")));
        assert!(validate_folder("docs/../../x").is_err());
        assert_eq!(
            validate_folder("docs/img/").unwrap(),
            PathBuf::from("docs/img")
        );
    }

    #[test]
    fn names_are_sanitized_and_keep_their_extension() {
        assert_eq!(
            split_name("mi:captura.png"),
            ("mi-captura".into(), ".png".into())
        );
        assert_eq!(split_name(".png"), ("image".into(), ".png".into()));
        assert_eq!(
            split_name("sin-extension"),
            ("sin-extension".into(), String::new())
        );
    }
}
