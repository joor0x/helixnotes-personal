//! Note-wide task actions: check or uncheck every task, delete completed tasks, and archive
//! completed tasks under a "Done" heading at the end of the note. Plus the `hide-tasks` flag
//! that keeps a note's tasks (checklists, SOPs) out of the Tasks view.

use std::sync::LazyLock;

use serde::Deserialize;
use tauri::State;

use crate::state::AppState;
use crate::types::NoteContent;
use crate::vault::{frontmatter, operations};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkTaskAction {
    CheckAll,
    UncheckAll,
    DeleteCompleted,
    ArchiveCompleted,
}

static TASK_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^(\s*)[-*]\s\[([ xX])\]\s+\S").unwrap());
static HEADING_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^(#{1,6})\s+(.*?)\s*#*\s*$").unwrap());
static FENCE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^\s{0,3}(`{3,}|~{3,})").unwrap());
static WORD_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"[\p{L}·']+").unwrap());

/// The archive heading per language (primary subtag). Any of them is recognised as the
/// archive section, whatever the note's language.
const DONE_HEADINGS: &[(&str, &str)] = &[
    ("en", "Done"),
    ("es", "Hecho"),
    ("ca", "Fet"),
    ("fr", "Fait"),
    ("de", "Erledigt"),
    ("it", "Fatto"),
    ("nl", "Klaar"),
    ("pt", "Feito"),
    ("ru", "Готово"),
];

#[derive(Debug, Clone, Copy)]
struct LineInfo {
    in_code: bool,
    /// Indent width and completion of a task line.
    task: Option<(usize, bool)>,
    heading: Option<usize>,
}

fn indent_width(prefix: &str) -> usize {
    prefix.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

fn classify(lines: &[&str]) -> Vec<LineInfo> {
    let mut fence: Option<(char, usize)> = None;
    lines
        .iter()
        .map(|line| {
            if let Some((marker, len)) = fence {
                let closes = FENCE_RE.captures(line).is_some_and(|caps| {
                    let run = &caps[1];
                    run.starts_with(marker)
                        && run.len() >= len
                        && line
                            .trim_start()
                            .trim_start_matches(marker)
                            .trim()
                            .is_empty()
                });
                if closes {
                    fence = None;
                }
                return LineInfo {
                    in_code: true,
                    task: None,
                    heading: None,
                };
            }
            if let Some(caps) = FENCE_RE.captures(line) {
                let run = &caps[1];
                fence = Some((run.chars().next().unwrap(), run.len()));
                return LineInfo {
                    in_code: true,
                    task: None,
                    heading: None,
                };
            }
            let task = TASK_RE
                .captures(line)
                .map(|caps| (indent_width(&caps[1]), !caps[2].trim().is_empty()));
            let heading = HEADING_RE.captures(line).map(|caps| caps[1].len());
            LineInfo {
                in_code: false,
                task,
                heading,
            }
        })
        .collect()
}

fn is_done_heading(text: &str) -> bool {
    DONE_HEADINGS
        .iter()
        .any(|(_, label)| text.trim().to_lowercase() == label.to_lowercase())
}

/// Line range `[start, end)` of the archive section: from its heading to the next heading of
/// the same or a higher level.
fn done_section(lines: &[&str], info: &[LineInfo]) -> Option<(usize, usize)> {
    let start = (0..lines.len()).find(|&i| {
        !info[i].in_code
            && info[i].heading.is_some()
            && HEADING_RE
                .captures(lines[i])
                .is_some_and(|caps| is_done_heading(&caps[2]))
    })?;
    let level = info[start].heading.unwrap();
    let end = (start + 1..lines.len())
        .find(|&i| !info[i].in_code && info[i].heading.is_some_and(|l| l <= level))
        .unwrap_or(lines.len());
    Some((start, end))
}

/// End (exclusive) of the block that starts with the task at `start`: the task plus every
/// following line indented deeper than it (blank lines inside the block included).
fn block_end(lines: &[&str], info: &[LineInfo], start: usize) -> usize {
    let (indent, _) = info[start].task.unwrap();
    let mut end = start + 1;
    let mut last_content = start + 1;
    while end < lines.len() {
        let line = lines[end];
        if line.trim().is_empty() {
            end += 1;
            continue;
        }
        let width = indent_width(&line[..line.len() - line.trim_start().len()]);
        if width <= indent || (!info[end].in_code && info[end].heading.is_some()) {
            break;
        }
        end += 1;
        last_content = end;
    }
    last_content
}

fn is_recurring(line: &str) -> bool {
    crate::reminders::parse_repeat(line).is_some()
}

fn set_checkbox(line: &str, done: bool) -> String {
    let caps = TASK_RE.captures(line).unwrap();
    let mark = caps.get(2).unwrap();
    let mut out = line.to_string();
    out.replace_range(mark.start()..mark.end(), if done { "x" } else { " " });
    out
}

/// The note's language for the archive heading: its `lang` frontmatter, else a guess from the
/// body's common words, else `fallback` (the interface language).
pub fn archive_heading(raw: &str, body: &str, fallback: Option<&str>) -> &'static str {
    let primary = |code: &str| {
        code.split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    let from_code = |code: &str| {
        let code = primary(code);
        DONE_HEADINGS
            .iter()
            .find(|(lang, _)| *lang == code)
            .map(|(_, label)| *label)
    };
    frontmatter_language(raw)
        .and_then(|code| from_code(&code))
        .or_else(|| guess_language(body).and_then(from_code))
        .or_else(|| fallback.and_then(from_code))
        .unwrap_or("Done")
}

fn frontmatter_language(raw: &str) -> Option<String> {
    static LANG_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?m)^(?:lang|language)[ \t]*:[ \t]*["']?([A-Za-z]{2,3})(?:[-_][A-Za-z0-9]{2,8}){0,2}["']?[ \t]*(?:#.*)?$"#).unwrap()
    });
    let rest = raw.strip_prefix('\u{FEFF}').unwrap_or(raw);
    let rest = rest.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    LANG_RE
        .captures(&rest[..end])
        .map(|caps| caps[1].to_string())
}

/// Guess Catalan, Spanish or English from words that are common in one and rare in the others.
/// `None` when there is too little text to tell (a bare checklist).
fn guess_language(body: &str) -> Option<&'static str> {
    const CA: &[&str] = &[
        "i", "amb", "els", "és", "per", "perquè", "què", "això", "aquest", "aquesta", "dels",
        "pel", "molt", "fer", "tinc", "també", "són", "seu", "seva", "quan", "res", "abans",
        "després", "cal", "hem", "però", "mateix", "doncs",
    ];
    const ES: &[&str] = &[
        "y", "con", "los", "es", "para", "porque", "qué", "esto", "este", "esta", "muy", "hacer",
        "tengo", "también", "son", "su", "cuando", "donde", "nada", "antes", "después", "hay",
        "hemos", "pero", "mismo", "el", "las", "como", "lo",
    ];
    const EN: &[&str] = &[
        "the", "and", "with", "for", "to", "of", "is", "this", "that", "are", "have", "when",
        "where", "before", "after", "it", "you", "my", "on", "check", "from", "be",
    ];
    let mut score = [0usize; 3];
    for word in WORD_RE.find_iter(body).take(2000) {
        let word = word.as_str().to_lowercase();
        if word.contains('·') {
            score[0] += 3;
            continue;
        }
        if word.contains('ñ') {
            score[1] += 3;
        }
        for (index, list) in [CA, ES, EN].iter().enumerate() {
            if list.contains(&word.as_str()) {
                score[index] += 1;
            }
        }
    }
    let (best, &top) = score.iter().enumerate().max_by_key(|(_, s)| **s)?;
    let second = score
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != best)
        .map(|(_, s)| *s)
        .max()
        .unwrap_or(0);
    (top >= 3 && top > second * 3 / 2).then_some(["ca", "es", "en"][best])
}

/// Apply `action` to the note body. Returns the new body and how many tasks changed, or `None`
/// when nothing changed. Tasks inside code blocks are never touched; recurring tasks are never
/// deleted or archived; check/uncheck leave the archive section alone.
pub fn apply(body: &str, action: BulkTaskAction, heading: &str) -> Option<(String, usize)> {
    let lines: Vec<&str> = body.lines().collect();
    let info = classify(&lines);
    let done = done_section(&lines, &info);
    let in_done = |i: usize| done.is_some_and(|(start, end)| i >= start && i < end);

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut changed = 0;
    match action {
        BulkTaskAction::CheckAll | BulkTaskAction::UncheckAll => {
            let target = action == BulkTaskAction::CheckAll;
            for (i, line) in lines.iter().enumerate() {
                match info[i].task {
                    Some((_, completed)) if completed != target && !in_done(i) => {
                        out.push(set_checkbox(line, target));
                        changed += 1;
                    }
                    _ => out.push(line.to_string()),
                }
            }
        }
        BulkTaskAction::DeleteCompleted | BulkTaskAction::ArchiveCompleted => {
            let archive = action == BulkTaskAction::ArchiveCompleted;
            let mut moved: Vec<String> = Vec::new();
            let mut i = 0;
            while i < lines.len() {
                let removable = match info[i].task {
                    Some((_, true)) => !is_recurring(lines[i]) && !(archive && in_done(i)),
                    _ => false,
                };
                if !removable {
                    out.push(lines[i].to_string());
                    i += 1;
                    continue;
                }
                let end = block_end(&lines, &info, i);
                // A block with an open or recurring subtask stays, so nothing pending is
                // buried in the archive or lost.
                let keeps_pending = (i + 1..end).any(|j| match info[j].task {
                    Some((_, false)) => true,
                    Some((_, true)) => is_recurring(lines[j]),
                    None => false,
                });
                if keeps_pending {
                    out.push(lines[i].to_string());
                    i += 1;
                    continue;
                }
                changed += (i..end).filter(|&j| info[j].task.is_some()).count();
                if archive {
                    let base = lines[i].len() - lines[i].trim_start().len();
                    for line in &lines[i..end] {
                        let strip = base.min(line.len() - line.trim_start().len());
                        moved.push(line[strip..].to_string());
                    }
                }
                i = end;
            }
            if changed == 0 {
                return None;
            }
            if archive {
                out = insert_archived(out, moved, heading);
            }
            out = collapse_blank_runs(out);
            if !lines.first().is_some_and(|line| line.trim().is_empty()) {
                let lead = out.iter().take_while(|line| line.trim().is_empty()).count();
                out.drain(..lead);
            }
            while out.last().is_some_and(|line| line.trim().is_empty()) {
                out.pop();
            }
        }
    }
    if changed == 0 {
        return None;
    }
    let mut result = out.join("\n");
    if body.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    Some((result, changed))
}

/// Append `moved` at the end of the existing archive section, or add the section at the end.
fn insert_archived(mut out: Vec<String>, moved: Vec<String>, heading: &str) -> Vec<String> {
    let refs: Vec<&str> = out.iter().map(String::as_str).collect();
    let info = classify(&refs);
    match done_section(&refs, &info) {
        Some((start, end)) => {
            let mut at = end;
            while at > start + 1 && out[at - 1].trim().is_empty() {
                at -= 1;
            }
            let mut insert = Vec::new();
            if at == start + 1 {
                insert.push(String::new());
            }
            insert.extend(moved);
            if at < out.len() && end < out.len() {
                insert.push(String::new());
            }
            out.splice(at..at, insert);
        }
        None => {
            while out.last().is_some_and(|line| line.trim().is_empty()) {
                out.pop();
            }
            if !out.is_empty() {
                out.push(String::new());
            }
            out.push(format!("## {heading}"));
            out.push(String::new());
            out.extend(moved);
        }
    }
    out
}

/// Removing blocks can leave two blank lines where a task list was; keep one.
fn collapse_blank_runs(lines: Vec<String>) -> Vec<String> {
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let info = classify(&refs);
    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let blank = line.trim().is_empty() && !info[i].in_code;
        if blank
            && out
                .last()
                .is_some_and(|prev: &String| prev.trim().is_empty())
        {
            continue;
        }
        out.push(line.clone());
    }
    out
}

#[tauri::command]
pub async fn set_note_hide_tasks(
    state: State<'_, AppState>,
    vault_path: String,
    path: String,
    hide: bool,
    expected_raw: String,
) -> Result<(), String> {
    state.validate_vault(&vault_path)?;
    let guard = state.note_save.clone().lock_owned().await;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        operations::set_note_hide_tasks(&vault_path, &path, hide, &expected_raw)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Apply a note-wide task action. Returns how many tasks changed (0 leaves the note as it was).
#[tauri::command]
pub async fn bulk_note_tasks(
    state: State<'_, AppState>,
    vault_path: String,
    path: String,
    action: BulkTaskAction,
    expected_raw: String,
    fallback_language: Option<String>,
) -> Result<usize, String> {
    let search = state.vault_snapshot(&vault_path)?;
    let _write_guard = state.note_save.lock().await;
    let NoteContent {
        meta,
        content: body,
        raw,
        ..
    } = operations::read_vault_note(&vault_path, &path)?;
    if raw != expected_raw {
        return Err("Save conflict: note changed on disk. Reopen the note and try again.".into());
    }
    let heading = archive_heading(&raw, &body, fallback_language.as_deref());
    let Some((new_body, changed)) = apply(&body, action, heading) else {
        return Ok(0);
    };
    if operations::save_note(&vault_path, &path, &meta, &new_body, &raw)?.is_some() {
        crate::commands::index_note_bg(search, &path);
    }
    Ok(changed)
}

/// Whether a note's tasks are kept out of the Tasks view.
pub fn hides_tasks(raw: &str) -> bool {
    frontmatter::frontmatter_hides_tasks(raw)
}

#[cfg(test)]
mod tests {
    use super::{apply, archive_heading, guess_language, BulkTaskAction::*};

    #[test]
    fn check_and_uncheck_all_skip_code_blocks_and_the_archive() {
        let body = "# Lista\n\n- [ ] Llaves\n  - [x] Cartera\n* [ ] Móvil\n\n```\n- [ ] código\n```\n\n## Hecho\n\n- [x] Viejo\n";
        let (checked, n) = apply(body, CheckAll, "Hecho").unwrap();
        assert_eq!(n, 2);
        assert_eq!(
            checked,
            "# Lista\n\n- [x] Llaves\n  - [x] Cartera\n* [x] Móvil\n\n```\n- [ ] código\n```\n\n## Hecho\n\n- [x] Viejo\n"
        );
        let (unchecked, n) = apply(&checked, UncheckAll, "Hecho").unwrap();
        assert_eq!(n, 3);
        assert!(unchecked.contains("- [ ] Llaves\n  - [ ] Cartera\n* [ ] Móvil"));
        assert!(unchecked.contains("- [ ] código") && unchecked.ends_with("- [x] Viejo\n"));
        assert!(apply(&unchecked, UncheckAll, "Hecho").is_none());
    }

    #[test]
    fn archive_creates_the_section_and_keeps_recurring_and_pending_blocks() {
        let body = "# Semana\n\n- [x] Comprar pan\n  notas del pan\n- [ ] Llamar\n  - [x] Buscar número\n- [x] Padre\n  - [ ] Hijo abierto\n- [x] Gym repeat:weekly\n- [X] Enviar mail\n";
        let (out, n) = apply(body, ArchiveCompleted, "Hecho").unwrap();
        assert_eq!(n, 3);
        assert_eq!(
            out,
            "# Semana\n\n- [ ] Llamar\n- [x] Padre\n  - [ ] Hijo abierto\n- [x] Gym repeat:weekly\n\n## Hecho\n\n- [x] Comprar pan\n  notas del pan\n- [x] Buscar número\n- [X] Enviar mail\n"
        );
        assert!(apply(&out, ArchiveCompleted, "Hecho").is_none());
    }

    #[test]
    fn archive_appends_to_an_existing_section_in_any_language() {
        let body = "- [x] Nou\n- [ ] Obert\n\n## Done\n\n- [x] Old\n\n## Notes\n\ntext\n";
        let (out, n) = apply(body, ArchiveCompleted, "Fet").unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            out,
            "- [ ] Obert\n\n## Done\n\n- [x] Old\n- [x] Nou\n\n## Notes\n\ntext\n"
        );
        let empty = "- [x] A\n\n## Fet\n";
        assert_eq!(
            apply(empty, ArchiveCompleted, "Fet").unwrap().0,
            "## Fet\n\n- [x] A\n"
        );
    }

    #[test]
    fn delete_completed_removes_blocks_everywhere_but_keeps_recurring() {
        let body =
            "- [x] A\n  - [x] A1\n- [ ] B\n\n- [x] C repeat:daily\n\n## Hecho\n\n- [x] Old\n";
        let (out, n) = apply(body, DeleteCompleted, "Hecho").unwrap();
        assert_eq!(n, 3);
        assert_eq!(out, "- [ ] B\n\n- [x] C repeat:daily\n\n## Hecho\n");
    }

    #[test]
    fn heading_follows_frontmatter_then_body_then_interface_language() {
        let ca = "---\nlang: ca-ES\n---\n";
        assert_eq!(archive_heading(ca, "the and with for", Some("es")), "Fet");
        assert_eq!(
            archive_heading(
                "",
                "Cal tancar la casa i baixar les persianes amb molt de compte abans de marxar",
                Some("en")
            ),
            "Fet"
        );
        assert_eq!(
            archive_heading(
                "",
                "Hay que cerrar la casa y bajar las persianas con cuidado antes de salir",
                Some("en")
            ),
            "Hecho"
        );
        assert_eq!(
            archive_heading("", "- [ ] Esquís\n- [ ] Botes", Some("es")),
            "Hecho"
        );
        assert_eq!(archive_heading("", "- [ ] Esquís", None), "Done");
        assert_eq!(guess_language("Esquís Botes Pals"), None);
    }
}
