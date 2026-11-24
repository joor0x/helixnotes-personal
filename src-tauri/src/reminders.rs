//! Task due times, reminders and desktop reminder notifications.
//!
//! Markdown syntax (all times are device-local wall-clock times, no offset):
//! - `due:2026-11-30` or `due:2026-11-30T14:30`: when the task is due.
//! - `remind:2026-11-30T13:45`: when to notify, independent of the due date.
//! - `repeat:weekly` (see [`Repeat`]): completing the task creates its next occurrence.
//! - `#tag` in the task line, plus the note's frontmatter tags, for filtering.
use std::sync::LazyLock;

use chrono::{Datelike, Days, Months, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use regex::Regex;

pub const DATE_FORMAT: &str = "%Y-%m-%d";
pub const DATETIME_FORMAT: &str = "%Y-%m-%dT%H:%M";

static DUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bdue:(\d{4}-\d{2}-\d{2})(?:T(\d{2}:\d{2}))?\b").unwrap());
static REMIND_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bremind:(\d{4}-\d{2}-\d{2}T\d{2}:\d{2})\b").unwrap());
static REPEAT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\brepeat:(daily|weekly|monthly|yearly|weekdays|[1-9]\d{0,2}[dwmy])\b").unwrap()
});
static CHECKBOX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*[-*]\s)\[[xX ]\]").unwrap());
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

/// The task text without its `due:`, `remind:` and `repeat:` tokens.
pub fn strip_schedule(content: &str) -> String {
    let text = DUE_RE.replace_all(content, "");
    let text = REMIND_RE.replace_all(&text, "");
    REPEAT_RE.replace_all(&text, "").into_owned()
}

/// How often a task repeats. Occurrences follow the calendar: a monthly task due on the
/// 31st is due on the last day of shorter months, without drifting to the 28th.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Days(u32),
    Weeks(u32),
    Months(u32),
    Years(u32),
    /// Monday to Friday.
    Weekdays,
}

impl Repeat {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        let rule = match value.as_str() {
            "daily" => Self::Days(1),
            "weekly" => Self::Weeks(1),
            "monthly" => Self::Months(1),
            "yearly" => Self::Years(1),
            "weekdays" => Self::Weekdays,
            _ => {
                let (count, unit) = value.split_at(value.len().checked_sub(1)?);
                let count: u32 = count.parse().ok().filter(|n| (1..=999).contains(n))?;
                match unit {
                    "d" => Self::Days(count),
                    "w" => Self::Weeks(count),
                    "m" => Self::Months(count),
                    "y" => Self::Years(count),
                    _ => return None,
                }
            }
        };
        Some(rule)
    }

    /// The shortest spelling: `weekly`, not `1w`.
    pub fn canonical(self) -> String {
        match self {
            Self::Days(1) => "daily".into(),
            Self::Weeks(1) => "weekly".into(),
            Self::Months(1) => "monthly".into(),
            Self::Years(1) => "yearly".into(),
            Self::Weekdays => "weekdays".into(),
            Self::Days(n) => format!("{n}d"),
            Self::Weeks(n) => format!("{n}w"),
            Self::Months(n) => format!("{n}m"),
            Self::Years(n) => format!("{n}y"),
        }
    }

    /// `date` moved forward by `periods` occurrences.
    fn advance(self, date: NaiveDate, periods: u32) -> Option<NaiveDate> {
        match self {
            Self::Days(n) => date.checked_add_days(Days::new(u64::from(n) * u64::from(periods))),
            Self::Weeks(n) => {
                date.checked_add_days(Days::new(7 * u64::from(n) * u64::from(periods)))
            }
            Self::Months(n) => date.checked_add_months(Months::new(n.checked_mul(periods)?)),
            Self::Years(n) => {
                date.checked_add_months(Months::new(n.checked_mul(periods)?.checked_mul(12)?))
            }
            Self::Weekdays => {
                let mut date = date;
                for _ in 0..periods {
                    date = date.succ_opt()?;
                    while matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
                        date = date.succ_opt()?;
                    }
                }
                Some(date)
            }
        }
    }
}

pub fn parse_repeat(content: &str) -> Option<Repeat> {
    REPEAT_RE
        .captures(content)
        .and_then(|caps| Repeat::parse(&caps[1]))
}

/// Validate a repeat value (see [`Repeat::parse`]). Empty clears it.
pub fn validate_repeat(value: Option<&str>) -> Result<Option<String>, String> {
    match value.map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) => Repeat::parse(v)
            .map(|rule| Some(rule.canonical()))
            .ok_or_else(|| "Invalid repeat rule".into()),
    }
}

/// Upper bound on skipped occurrences, so a decades-old daily task stays cheap.
const MAX_SKIPPED_PERIODS: u32 = 20_000;

/// The next occurrence's `(due, remind)` values after completing a recurring task at `now`.
///
/// Both move by the same number of periods, the fewest that put them in the future: the due
/// date on or after today (or after `now` when it has a time) and the reminder after `now`,
/// so the new occurrence is not born overdue and its reminder does not fire at once.
/// A task without dates is due one period after today.
pub fn next_occurrence(
    due: Option<&str>,
    due_time: Option<&str>,
    remind: Option<&str>,
    rule: Repeat,
    now: NaiveDateTime,
) -> Option<(Option<String>, Option<String>)> {
    let today = now.date();
    let due_date = due.and_then(|d| NaiveDate::parse_from_str(d, DATE_FORMAT).ok());
    let due_time = due_time.and_then(|t| NaiveTime::parse_from_str(t, "%H:%M").ok());
    let remind = remind.and_then(|r| NaiveDateTime::parse_from_str(r, DATETIME_FORMAT).ok());
    if due_date.is_none() && remind.is_none() {
        let next = rule.advance(today, 1)?;
        return Some((Some(next.format(DATE_FORMAT).to_string()), None));
    }
    let mut last = None;
    for periods in 1..=MAX_SKIPPED_PERIODS {
        let next_due = match due_date {
            Some(date) => Some(rule.advance(date, periods)?),
            None => None,
        };
        let next_remind = match remind {
            Some(at) => Some(rule.advance(at.date(), periods)?.and_time(at.time())),
            None => None,
        };
        let due_ok = next_due.is_none_or(|date| match due_time {
            Some(time) => date.and_time(time) > now,
            None => date >= today,
        });
        let remind_ok = next_remind.is_none_or(|at| at > now);
        last = Some((next_due, next_remind));
        if due_ok && remind_ok {
            break;
        }
    }
    let (next_due, next_remind) = last?;
    let due = next_due.map(|date| match due_time {
        Some(time) => date.and_time(time).format(DATETIME_FORMAT).to_string(),
        None => date.format(DATE_FORMAT).to_string(),
    });
    let remind = next_remind.map(|at| at.format(DATETIME_FORMAT).to_string());
    Some((due, remind))
}

/// The open line of the next occurrence of the recurring task `line`, or `None` when the
/// line is not a recurring task. Tokens keep their place; only their values change.
pub fn next_occurrence_line(line: &str, now: NaiveDateTime) -> Option<String> {
    let checkbox = CHECKBOX_RE.find(line)?;
    let content = &line[checkbox.end()..];
    let rule = parse_repeat(content)?;
    let (due, due_time) = parse_due(content);
    let remind = parse_remind(content);
    let (next_due, next_remind) = next_occurrence(
        due.as_deref(),
        due_time.as_deref(),
        remind.as_deref(),
        rule,
        now,
    )?;
    let open = CHECKBOX_RE.replace(line, "${1}[ ]");
    let mut next = match &next_due {
        Some(value) if due.is_some() => DUE_RE
            .replace(&open, format!("due:{value}").as_str())
            .into_owned(),
        _ => open.into_owned(),
    };
    if let Some(value) = &next_remind {
        next = REMIND_RE
            .replace(&next, format!("remind:{value}").as_str())
            .into_owned();
    }
    if due.is_none() {
        if let Some(value) = next_due {
            next = format!("{} due:{value}", next.trim_end());
        }
    }
    Some(next)
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

/// Replace (or with `None`, remove) a `due:`, `remind:` or `repeat:` token on a task line.
pub fn set_token_on_line(line: &str, key: &str, value: Option<&str>) -> String {
    let shape = if key == "repeat" {
        r"(?i:daily|weekly|monthly|yearly|weekdays|[1-9]\d{0,2}[dwmy])"
    } else {
        r"\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2})?"
    };
    let token = Regex::new(&format!(r"(?:^|\s){}:{shape}\b", regex::escape(key))).unwrap();
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
                if let Some(repeat) = &task.repeat {
                    body.push_str(" · ↻ ");
                    body.push_str(repeat);
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

    fn at(value: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(value, DATETIME_FORMAT).unwrap()
    }

    #[test]
    fn parses_repeat_rules() {
        assert_eq!(parse_repeat("x repeat:daily"), Some(Repeat::Days(1)));
        assert_eq!(parse_repeat("x repeat:Weekly #a"), Some(Repeat::Weeks(1)));
        assert_eq!(parse_repeat("x repeat:2w"), Some(Repeat::Weeks(2)));
        assert_eq!(parse_repeat("x repeat:3m"), Some(Repeat::Months(3)));
        assert_eq!(parse_repeat("x repeat:weekdays"), Some(Repeat::Weekdays));
        assert_eq!(parse_repeat("x repeat:0d"), None);
        assert_eq!(parse_repeat("x repeat:often"), None);
        assert_eq!(parse_repeat("x norepeat:daily"), None);
        assert_eq!(Repeat::Weeks(1).canonical(), "weekly");
        assert_eq!(Repeat::Days(10).canonical(), "10d");
        assert_eq!(
            validate_repeat(Some("1W")).unwrap().as_deref(),
            Some("weekly")
        );
        assert_eq!(validate_repeat(Some("")).unwrap(), None);
        assert!(validate_repeat(Some("1000d")).is_err());
        assert_eq!(
            strip_schedule("Gym repeat:2d due:2026-11-30")
                .split_whitespace()
                .collect::<Vec<_>>(),
            ["Gym"]
        );
    }

    #[test]
    fn next_occurrence_moves_due_and_reminder_together() {
        let now = at("2026-11-30T15:00");
        assert_eq!(
            next_occurrence(
                Some("2026-11-30"),
                Some("14:30"),
                Some("2026-11-30T13:45"),
                Repeat::Weeks(1),
                now
            ),
            Some((
                Some("2026-12-07T14:30".into()),
                Some("2026-12-07T13:45".into())
            ))
        );
    }

    #[test]
    fn next_occurrence_skips_periods_already_past() {
        // Completed three days late: the daily task is due today, not on a missed day.
        let now = at("2026-12-03T08:00");
        assert_eq!(
            next_occurrence(Some("2026-11-30"), None, None, Repeat::Days(1), now),
            Some((Some("2026-12-03".into()), None))
        );
        // A reminder earlier today is already past, so the occurrence moves to tomorrow.
        assert_eq!(
            next_occurrence(
                Some("2026-11-30"),
                None,
                Some("2026-11-30T07:00"),
                Repeat::Days(1),
                now
            ),
            Some((Some("2026-12-04".into()), Some("2026-12-04T07:00".into())))
        );
    }

    #[test]
    fn next_occurrence_follows_the_calendar() {
        let now = at("2026-01-31T10:00");
        assert_eq!(
            next_occurrence(Some("2026-01-31"), None, None, Repeat::Months(1), now),
            Some((Some("2026-02-28".into()), None))
        );
        // Friday -> Monday.
        let friday = at("2026-11-27T10:00");
        assert_eq!(
            next_occurrence(Some("2026-11-27"), None, None, Repeat::Weekdays, friday),
            Some((Some("2026-11-30".into()), None))
        );
        assert_eq!(
            next_occurrence(
                Some("2028-02-29"),
                None,
                None,
                Repeat::Years(1),
                at("2028-02-29T10:00")
            ),
            Some((Some("2029-02-28".into()), None))
        );
        // No dates: due one period after today.
        assert_eq!(
            next_occurrence(None, None, None, Repeat::Weeks(2), now),
            Some((Some("2026-02-14".into()), None))
        );
        // Only a reminder.
        assert_eq!(
            next_occurrence(None, None, Some("2026-01-31T09:00"), Repeat::Days(1), now),
            Some((None, Some("2026-02-01T09:00".into())))
        );
    }

    #[test]
    fn builds_the_next_occurrence_line() {
        let now = at("2026-11-30T15:00");
        assert_eq!(
            next_occurrence_line(
                "  - [x] Pay rent !high due:2026-11-30 remind:2026-11-30T09:00 repeat:monthly #home",
                now
            )
            .as_deref(),
            Some("  - [ ] Pay rent !high due:2026-12-30 remind:2026-12-30T09:00 repeat:monthly #home")
        );
        assert_eq!(
            next_occurrence_line("- [ ] Water plants repeat:3d", now).as_deref(),
            Some("- [ ] Water plants repeat:3d due:2026-12-03")
        );
        assert_eq!(
            next_occurrence_line("- [x] One-off due:2026-11-30", now),
            None
        );
        assert_eq!(next_occurrence_line("Not a task repeat:daily", now), None);
    }

    #[test]
    fn sets_and_clears_repeat_tokens() {
        let line = "- [ ] Gym due:2026-11-30";
        let weekly = set_token_on_line(line, "repeat", Some("weekly"));
        assert_eq!(weekly, "- [ ] Gym due:2026-11-30 repeat:weekly");
        let biweekly = set_token_on_line(&weekly, "repeat", Some("2w"));
        assert_eq!(biweekly, "- [ ] Gym due:2026-11-30 repeat:2w");
        assert_eq!(set_token_on_line(&biweekly, "repeat", None), line);
    }
}
