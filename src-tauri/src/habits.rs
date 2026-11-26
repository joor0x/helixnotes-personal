//! Habits: an opt-in, per-vault tracker. Each note in the vault's habits folder is a habit and
//! its `entries` frontmatter lists the days it was done (the format of Obsidian's Habit
//! Tracker 21), so the data is plain Markdown and syncs like any other note.

use tauri::State;

use crate::state::AppState;
use crate::types::Habit;
use crate::vault::operations;

/// The active vault's habits. Empty when the vault has no habits folder set.
#[tauri::command]
pub async fn get_habits(state: State<'_, AppState>) -> Result<Vec<Habit>, String> {
    let (vault_path, folder) = {
        let config = state.config.lock().map_err(|error| error.to_string())?;
        let vault = crate::commands::active_vault_config(&config)?;
        (vault.path.clone(), vault.habits_folder.clone())
    };
    let Some(folder) = folder else {
        return Ok(Vec::new());
    };
    tauri::async_runtime::spawn_blocking(move || operations::list_habits(&vault_path, &folder))
        .await
        .map_err(|error| error.to_string())?
}

/// Mark or unmark a day (YYYY-MM-DD) of a habit note. Returns the note's days afterwards.
#[tauri::command]
pub async fn toggle_habit_entry(
    state: State<'_, AppState>,
    vault_path: String,
    path: String,
    date: String,
) -> Result<Vec<String>, String> {
    state.validate_vault(&vault_path)?;
    let guard = state.note_save.clone().lock_owned().await;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        operations::toggle_habit_entry(&vault_path, &path, &date)
    })
    .await
    .map_err(|error| error.to_string())?
}
