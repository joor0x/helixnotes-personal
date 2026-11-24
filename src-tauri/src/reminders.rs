//! Task due times, reminders and desktop reminder notifications.
//!
//! Markdown syntax (all times are device-local wall-clock times, no offset):
//! - `due:2026-11-30` or `due:2026-11-30T14:30`: when the task is due.
//! - `remind:2026-11-30T13:45`: when to notify, independent of the due date.
//! - `#tag` in the task line, plus the note's frontmatter tags, for filtering.
use std::sync::LazyLock;

use chrono::{NaiveDate, NaiveDateTime};
use regex::Regex;

pub const DATE_FORMAT: &str = "%Y-%m-%d";
pub const DATETIME_FORMAT: &str = "%Y-%m-%dT%H:%M";

static DUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bdue:(\d{4}-\d{2}-\d{2})(?:T(\d{2}:\d{2}))?\b").unwrap());
static REMIND_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bremind:(\d{4}-\d{2}-\d{2}T\d{2}:\d{2})\b").unwrap());
static TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)#([\p{L}\p{N}_][\p{L}\p{N}_/-]*)").unwrap());

/// `(date, time)` of the task's `due:` token.
pub fn parse_due(content: &str) -> (Option<String>, Option<String>) {
    match DUE_RE.captures(content) {
        Some(caps) => (
            Some(caps[1].to_string()),
            caps.get(2).map(|m| m.as_str().to_string()),
        ),
        None => (None, None),
    }
}

pub fn parse_remind(content: &str) -> Option<String> {
    REMIND_RE.captures(content).map(|caps| caps[1].to_string())
}

/// `#tags` written in the task line, without the `#`, in order and without duplicates.
pub fn inline_tags(content: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for caps in TAG_RE.captures_iter(content) {
        let tag = caps[1].trim_end_matches(['/', '-']).to_string();
        if !tag.is_empty() && !tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            tags.push(tag);
        }
    }
    tags
}

/// The task text without its `due:` and `remind:` tokens.
pub fn strip_schedule(content: &str) -> String {
    let text = DUE_RE.replace_all(content, "");
    REMIND_RE.replace_all(&text, "").into_owned()
}

/// Validate a due value: `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM`. Empty clears it.
pub fn validate_due(value: Option<&str>) -> Result<Option<String>, String> {
    match value.map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) if v.len() == 10 && NaiveDate::parse_from_str(v, DATE_FORMAT).is_ok() => {
            Ok(Some(v.to_string()))
        }
        Some(v) if v.len() == 16 && NaiveDateTime::parse_from_str(v, DATETIME_FORMAT).is_ok() => {
            Ok(Some(v.to_string()))
        }
        Some(_) => Err("Invalid due date".into()),
    }
}

/// Validate a reminder value: `YYYY-MM-DDTHH:MM`. Empty clears it.
pub fn validate_remind(value: Option<&str>) -> Result<Option<String>, String> {
    match value.map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) if v.len() == 16 && NaiveDateTime::parse_from_str(v, DATETIME_FORMAT).is_ok() => {
            Ok(Some(v.to_string()))
        }
        Some(_) => Err("Invalid reminder date".into()),
    }
}

/// Replace (or with `None`, remove) a `due:` or `remind:` token on a task line.
pub fn set_token_on_line(line: &str, key: &str, value: Option<&str>) -> String {
    let token = Regex::new(&format!(
        r"(?:^|\s){}:\d{{4}}-\d{{2}}-\d{{2}}(?:T\d{{2}}:\d{{2}})?\b",
        regex::escape(key)
    ))
    .unwrap();
    let stripped = token.replace_all(line, "");
    let stripped = stripped.trim_end();
    match value {
        Some(value) => format!("{stripped} {key}:{value}"),
        None => stripped.to_string(),
    }
}

#[cfg(desktop)]
pub use scheduler::spawn_scheduler;

#[cfg(desktop)]
mod scheduler {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use chrono::{Local, NaiveDateTime};
    use tauri::{AppHandle, Manager};
    use tauri_plugin_notification::NotificationExt;

    use super::DATETIME_FORMAT;
    use crate::state::AppState;
    use crate::types::TaskItem;

    const INTERVAL: Duration = Duration::from_secs(30);
    /// Reminders missed while the app was closed still fire if they are this recent.
    const GRACE: chrono::Duration = chrono::Duration::hours(24);
    /// Fired reminders are remembered this long so each one fires once.
    const RETENTION: chrono::Duration = chrono::Duration::days(7);

    #[derive(Default)]
    struct Scheduler {
        vault: Option<String>,
        /// path -> (mtime, size, tasks with a reminder)
        cache: HashMap<PathBuf, (SystemTime, u64, Vec<TaskItem>)>,
        fired: HashMap<String, String>,
    }

    fn fired_path() -> Option<PathBuf> {
        crate::commands::app_config_path()
            .ok()
            .and_then(|p| p.parent().map(|dir| dir.join("reminders-fired.json")))
    }

    fn load_fired() -> HashMap<String, String> {
        fired_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save_fired(fired: &HashMap<String, String>) {
        if let (Some(path), Ok(json)) = (fired_path(), serde_json::to_string(fired)) {
            let _ = std::fs::write(path, json);
        }
    }

    fn reminder_key(task: &TaskItem, remind: &str) -> String {
        format!("{}\u{0}{}\u{0}{}", task.note_path, remind, task.text)
    }

    impl Scheduler {
        fn refresh(&mut self, vault: &str) -> Vec<TaskItem> {
            if self.vault.as_deref() != Some(vault) {
                self.vault = Some(vault.to_string());
                self.cache.clear();
            }
            let files = crate::commands::vault_markdown_files(vault);
            let mut next = HashMap::with_capacity(files.len());
            for path in files {
                let Ok(meta) = std::fs::metadata(&path) else {
                    continue;
                };
                let (mtime, size) = (
                    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    meta.len(),
                );
                let entry = match self.cache.remove(&path) {
                    Some(cached) if cached.0 == mtime && cached.1 == size => cached,
                    _ => {
                        let tasks = std::fs::read_to_string(&path)
                            .map(|raw| {
                                let name = path.file_name().unwrap_or_default().to_string_lossy();
                                let (meta, body) =
                                    crate::vault::frontmatter::parse_note(&raw, &name);
                                crate::commands::parse_note_tasks(
                                    &path.to_string_lossy(),
                                    &meta.title,
                                    &body,
                                    &meta.tags,
                                )
                                .into_iter()
                                .filter(|t| t.remind.is_some())
                                .collect()
                            })
                            .unwrap_or_default();
                        (mtime, size, tasks)
                    }
                };
                next.insert(path, entry);
            }
            self.cache = next;
            self.cache
                .values()
                .flat_map(|(_, _, tasks)| tasks.iter().cloned())
                .collect()
        }

        fn tick(&mut self, app: &AppHandle) {
            let (enabled, vault) = {
                let state = app.state::<AppState>();
                let Ok(config) = state.config.lock() else {
                    return;
                };
                (config.reminder_notifications, config.active_vault.clone())
            };
            let Some(vault) = vault.filter(|_| enabled) else {
                return;
            };
            let now = Local::now().naive_local();
            let mut changed = false;
            for task in self.refresh(&vault) {
                let Some(remind) = task.remind.as_deref() else {
                    continue;
                };
                let Ok(at) = NaiveDateTime::parse_from_str(remind, DATETIME_FORMAT) else {
                    continue;
                };
                let key = reminder_key(&task, remind);
                if task.completed || at > now || at < now - GRACE || self.fired.contains_key(&key) {
                    continue;
                }
                let mut body = task.note_title.clone();
                if let Some(due) = &task.due {
                    body.push_str(" · ");
                    body.push_str(due);
                    if let Some(time) = &task.due_time {
                        body.push(' ');
                        body.push_str(time);
                    }
                }
                if let Err(error) = app
                    .notification()
                    .builder()
                    .title(&task.text)
                    .body(body)
                    .show()
                {
                    log::warn!("Could not show reminder notification: {error}");
                }
                self.fired.insert(key, remind.to_string());
                changed = true;
            }
            let cutoff = (now - RETENTION).format(DATETIME_FORMAT).to_string();
            let before = self.fired.len();
            self.fired.retain(|_, remind| *remind >= cutoff);
            if changed || self.fired.len() != before {
                save_fired(&self.fired);
            }
        }
    }

    /// Check reminders every 30 seconds while the app runs (including in the tray).
    pub fn spawn_scheduler(app: AppHandle) {
        std::thread::spawn(move || {
            let mut scheduler = Scheduler {
                fired: load_fired(),
                ..Default::default()
            };
            loop {
                std::thread::sleep(INTERVAL);
                scheduler.tick(&app);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_due_with_and_without_time() {
        assert_eq!(
            parse_due("Call !high due:2026-11-30T14:30 remind:2026-11-30T13:45"),
            (Some("2026-11-30".into()), Some("14:30".into()))
        );
        assert_eq!(
            parse_due("Report due:2026-12-02"),
            (Some("2026-12-02".into()), None)
        );
        assert_eq!(parse_due("bad due:2026-12-02Tlater"), (None, None));
        assert_eq!(parse_due("no date"), (None, None));
    }

    #[test]
    fn parses_reminders_only_with_a_time() {
        assert_eq!(
            parse_remind("x remind:2026-12-04T20:00"),
            Some("2026-12-04T20:00".into())
        );
        assert_eq!(parse_remind("x remind:2026-12-04"), None);
    }

    #[test]
    fn collects_inline_tags_without_duplicates() {
        assert_eq!(
            inline_tags("#work Call #client/acme and #Work, not a#b or # alone #año"),
            ["work", "client/acme", "año"]
        );
    }

    #[test]
    fn strips_schedule_tokens_from_text() {
        let text = strip_schedule("Call due:2026-11-30T14:30 bank remind:2026-11-30T13:45");
        assert_eq!(
            text.split_whitespace().collect::<Vec<_>>(),
            ["Call", "bank"]
        );
    }

    #[test]
    fn validates_values() {
        assert_eq!(
            validate_due(Some("2026-11-30")).unwrap().as_deref(),
            Some("2026-11-30")
        );
        assert_eq!(
            validate_due(Some("2026-11-30T14:30")).unwrap().as_deref(),
            Some("2026-11-30T14:30")
        );
        assert_eq!(validate_due(Some("")).unwrap(), None);
        assert!(validate_due(Some("2026-02-30")).is_err());
        assert!(validate_due(Some("2026-11-30T25:00")).is_err());
        assert!(validate_remind(Some("2026-11-30")).is_err());
        assert!(validate_remind(Some("2026-11-30T09:00")).unwrap().is_some());
    }

    #[test]
    fn sets_replaces_and_clears_tokens() {
        let line = "- [ ] Call due:2026-11-30 #work";
        let timed = set_token_on_line(line, "due", Some("2026-11-30T14:30"));
        assert_eq!(timed, "- [ ] Call #work due:2026-11-30T14:30");
        let reminded = set_token_on_line(&timed, "remind", Some("2026-11-30T13:45"));
        assert_eq!(
            reminded,
            "- [ ] Call #work due:2026-11-30T14:30 remind:2026-11-30T13:45"
        );
        let moved = set_token_on_line(&reminded, "due", Some("2026-12-01"));
        assert_eq!(
            moved,
            "- [ ] Call #work remind:2026-11-30T13:45 due:2026-12-01"
        );
        assert_eq!(
            set_token_on_line(&moved, "remind", None),
            "- [ ] Call #work due:2026-12-01"
        );
    }
}
