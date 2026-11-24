//! Extra folders (outside the vault) that global search also covers, such as project
//! documentation. Each folder gets its own persistent index, refreshed lazily while
//! searching; results carry the folder they come from and open in place.

use crate::search::SearchIndex;
use crate::state::AppState;
use crate::types::{SearchPage, SearchResult};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_fs::FsExt;

/// How stale a folder index may get before a search queues a re-scan (no watcher).
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const MAX_FOLDERS: usize = 20;
const MAX_PAGE_SIZE: usize = 100;
/// Deepest result merged across sources; later pages come back empty.
const MAX_WINDOW: usize = 1000;

pub struct FolderSearch {
    index: Arc<SearchIndex>,
    refreshed: Instant,
}

/// Restores the read scope of the configured folders, so their results can be opened.
pub fn allow_folders<R: Runtime>(app: &AppHandle<R>, folders: &[String]) {
    for folder in folders {
        if let Err(error) = app.fs_scope().allow_directory(folder, true) {
            log::warn!("Could not restore the search folder scope for {folder}: {error}");
        }
    }
}

fn folder_index(state: &AppState, folder: &str) -> Result<Arc<SearchIndex>, String> {
    let mut folders = state
        .folder_searches
        .lock()
        .map_err(|error| error.to_string())?;
    if let Some(entry) = folders.get_mut(folder) {
        if entry.refreshed.elapsed() >= REFRESH_INTERVAL {
            entry.index.queue_vault_refresh(folder)?;
            entry.refreshed = Instant::now();
        }
        return Ok(entry.index.clone());
    }
    if !Path::new(folder).is_dir() {
        return Err(format!("Search folder is not available: {folder}"));
    }
    let index = state.search_index_for_vault(folder)?;
    index.queue_vault_refresh(folder)?;
    folders.insert(
        folder.to_owned(),
        FolderSearch {
            index: index.clone(),
            refreshed: Instant::now(),
        },
    );
    Ok(index)
}

fn is_within(path: &str, root: &str) -> bool {
    Path::new(path).starts_with(root)
}

/// Searches the vault and every configured folder, merging them into one ranked page.
pub fn search_all(
    state: &AppState,
    vault: Arc<SearchIndex>,
    vault_path: Option<&str>,
    folders: &[String],
    query: &str,
    limit: usize,
    offset: usize,
) -> Result<SearchPage, String> {
    let limit = limit.clamp(1, MAX_PAGE_SIZE);
    let window = offset.saturating_add(limit).min(MAX_WINDOW);
    let mut sources = vec![(None, vault)];
    for folder in folders {
        // A folder inside the vault would only duplicate vault results.
        if vault_path.is_some_and(|vault| is_within(folder, vault)) {
            continue;
        }
        match folder_index(state, folder) {
            Ok(index) => sources.push((Some(folder.clone()), index)),
            Err(error) => log::warn!("Skipping search folder {folder}: {error}"),
        }
    }
    let mut pages = Vec::with_capacity(sources.len());
    for (source, index) in sources {
        pages.push((source, index.search_top(query, window)?));
    }
    Ok(merge_pages(pages, vault_path, limit, offset))
}

/// Ranks results from all sources by score (the vault wins ties), drops duplicates
/// (a folder may contain the vault) and cuts the requested page.
fn merge_pages(
    pages: Vec<(Option<String>, SearchPage)>,
    vault_path: Option<&str>,
    limit: usize,
    offset: usize,
) -> SearchPage {
    let mut total = 0usize;
    let mut results: Vec<SearchResult> = Vec::new();
    for (source, page) in pages {
        total += page.total;
        results.extend(page.results.into_iter().map(|mut result| {
            let in_vault = vault_path.is_some_and(|vault| is_within(&result.path, vault));
            result.source = if in_vault { None } else { source.clone() };
            result
        }));
    }
    results.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut seen = HashSet::new();
    results.retain(|result| {
        let fresh = seen.insert(result.path.clone());
        if !fresh {
            total = total.saturating_sub(1);
        }
        fresh
    });
    SearchPage {
        results: results.into_iter().skip(offset).take(limit).collect(),
        total,
    }
}

fn save_folders(state: &AppState, folders: Vec<String>) -> Result<Vec<String>, String> {
    let mut current = state.config.lock().map_err(|e| e.to_string())?;
    let mut config = current.clone();
    config.search_folders = folders;
    crate::commands::save_app_config_to(&config, &crate::commands::app_config_path()?)?;
    *current = config;
    Ok(current.search_folders.clone())
}

/// Adds a folder the user picked in the folder dialog (which granted its scope).
#[tauri::command]
pub fn add_search_folder<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<String>, String> {
    let dir = std::fs::canonicalize(&path).map_err(|error| error.to_string())?;
    if !dir.is_dir() {
        return Err("Search folder must be a directory".into());
    }
    if !app.fs_scope().is_allowed(&dir) {
        return Err("Folder was not selected by the user".into());
    }
    let folder = dir
        .to_str()
        .ok_or("Search folder path is not valid UTF-8")?
        .to_owned();
    let mut folders = {
        let config = state.config.lock().map_err(|e| e.to_string())?;
        let inside_vault = config
            .vaults
            .iter()
            .map(|vault| vault.path.as_str())
            .chain(config.active_vault.as_deref())
            .any(|vault| {
                let vault = std::fs::canonicalize(vault).unwrap_or_else(|_| PathBuf::from(vault));
                dir.starts_with(vault)
            });
        if inside_vault {
            return Err("This folder is already part of a vault".into());
        }
        config.search_folders.clone()
    };
    if folders.contains(&folder) {
        return Ok(folders);
    }
    if folders.len() >= MAX_FOLDERS {
        return Err(format!("At most {MAX_FOLDERS} search folders can be added"));
    }
    folders.push(folder.clone());
    let folders = save_folders(&state, folders)?;
    allow_folders(&app, std::slice::from_ref(&folder));
    // Build the index in the background so the first search does not wait for it.
    let handle = app.clone();
    std::thread::spawn(move || {
        if let Err(error) = folder_index(&handle.state::<AppState>(), &folder) {
            log::warn!("Could not index search folder {folder}: {error}");
        }
    });
    Ok(folders)
}

#[tauri::command]
pub fn remove_search_folder(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<String>, String> {
    let folders = {
        let config = state.config.lock().map_err(|e| e.to_string())?;
        config
            .search_folders
            .iter()
            .filter(|folder| **folder != path)
            .cloned()
            .collect()
    };
    let folders = save_folders(&state, folders)?;
    if let Ok(mut indexes) = state.folder_searches.lock() {
        indexes.remove(&path);
    }
    Ok(folders)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(path: &str, score: f32) -> SearchResult {
        SearchResult {
            path: path.into(),
            title: String::new(),
            snippet: String::new(),
            score,
            title_matches: Vec::new(),
            snippet_matches: Vec::new(),
            match_target: None,
            source: None,
        }
    }

    fn page(results: Vec<SearchResult>) -> SearchPage {
        SearchPage {
            total: results.len(),
            results,
        }
    }

    #[test]
    fn merges_sources_by_score_and_tags_folder_results() {
        let merged = merge_pages(
            vec![
                (
                    None,
                    page(vec![result("/v/a.md", 3.0), result("/v/b.md", 1.0)]),
                ),
                (Some("/docs".into()), page(vec![result("/docs/c.md", 2.0)])),
            ],
            Some("/v"),
            10,
            0,
        );
        let paths: Vec<_> = merged.results.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, ["/v/a.md", "/docs/c.md", "/v/b.md"]);
        assert_eq!(merged.results[1].source.as_deref(), Some("/docs"));
        assert_eq!(merged.results[0].source, None);
        assert_eq!(merged.total, 3);
    }

    #[test]
    fn folder_containing_the_vault_does_not_duplicate_or_relabel_vault_notes() {
        let merged = merge_pages(
            vec![
                (None, page(vec![result("/home/v/a.md", 1.0)])),
                (
                    Some("/home".into()),
                    page(vec![result("/home/v/a.md", 5.0), result("/home/x.md", 2.0)]),
                ),
            ],
            Some("/home/v"),
            10,
            0,
        );
        assert_eq!(merged.results.len(), 2);
        assert_eq!(merged.total, 2);
        assert_eq!(merged.results[0].path, "/home/v/a.md");
        assert_eq!(merged.results[0].source, None);
        assert_eq!(merged.results[1].source.as_deref(), Some("/home"));
    }

    #[test]
    fn pages_cut_the_merged_ranking() {
        let merged = merge_pages(
            vec![
                (
                    None,
                    page(vec![result("/v/a.md", 4.0), result("/v/b.md", 2.0)]),
                ),
                (
                    Some("/d".into()),
                    page(vec![result("/d/c.md", 3.0), result("/d/e.md", 1.0)]),
                ),
            ],
            Some("/v"),
            2,
            2,
        );
        let paths: Vec<_> = merged.results.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, ["/v/b.md", "/d/e.md"]);
        assert_eq!(merged.total, 4);
    }
}
