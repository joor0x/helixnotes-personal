//! Text-only AI providers backed by locally installed, already authenticated CLIs:
//! Claude Code (`claude`), Codex (`codex`) and Gemini CLI (`gemini`).
//!
//! Each request runs the CLI once, non-interactively, inside an empty temporary
//! directory with tools disabled where the CLI allows it, so the agent cannot read
//! or edit the vault. The prompt goes through stdin (never argv) and only a
//! successful exit with non-empty output produces an applicable result.
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

pub const PROVIDERS: [&str; 3] = ["claude_cli", "codex_cli", "gemini_cli"];

const TIMEOUT: Duration = Duration::from_secs(300);
const MAX_OUTPUT: usize = 4 * 1024 * 1024;
const MAX_ERROR_DETAIL: usize = 400;

pub fn is_cli_provider(provider: &str) -> bool {
    PROVIDERS.contains(&provider)
}

fn binary_name(provider: &str) -> Option<&'static str> {
    match provider {
        "claude_cli" => Some("claude"),
        "codex_cli" => Some("codex"),
        "gemini_cli" => Some("gemini"),
        _ => None,
    }
}

fn display_name(provider: &str) -> &'static str {
    match provider {
        "claude_cli" => "Claude Code",
        "codex_cli" => "Codex",
        _ => "Gemini CLI",
    }
}

/// Directories where these CLIs are commonly installed but which a desktop launcher's
/// PATH often lacks (the app may not inherit the login shell's PATH).
fn fallback_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        for rel in [
            ".local/bin",
            ".npm-global/bin",
            ".bun/bin",
            ".volta/bin",
            ".cargo/bin",
        ] {
            dirs.push(home.join(rel));
        }
        for nvm in [".nvm/versions/node", ".config/nvm/versions/node"] {
            if let Ok(entries) = std::fs::read_dir(home.join(nvm)) {
                let mut versions: Vec<_> =
                    entries.flatten().map(|e| e.path().join("bin")).collect();
                // Prefer the newest installed Node version.
                versions.sort();
                versions.reverse();
                dirs.extend(versions);
            }
        }
    }
    dirs.extend(["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin"].map(PathBuf::from));
    dirs
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Resolve the CLI executable: an explicit absolute path from Settings, else PATH,
/// else the common install locations.
pub fn resolve_binary(provider: &str, custom: Option<&str>) -> Result<PathBuf, String> {
    let name = binary_name(provider).ok_or("Unsupported AI provider")?;
    if let Some(custom) = custom.map(str::trim).filter(|c| !c.is_empty()) {
        let path = PathBuf::from(custom);
        if !path.is_absolute() {
            return Err("The CLI path must be an absolute path".into());
        }
        if !is_executable(&path) {
            return Err(format!(
                "{} was not found at the configured path",
                display_name(provider)
            ));
        }
        return Ok(path);
    }
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let candidates = std::env::split_paths(&path_var).chain(fallback_dirs());
    for dir in candidates {
        for file in executable_names(name) {
            let candidate = dir.join(&file);
            if is_executable(&candidate) {
                return Ok(candidate);
            }
        }
    }
    Err(format!(
        "{} CLI (`{name}`) was not found. Install it, sign in once in a terminal, or set its path in Settings > AI.",
        display_name(provider)
    ))
}

fn executable_names(name: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![
            format!("{name}.cmd"),
            format!("{name}.exe"),
            name.to_string(),
        ]
    } else {
        vec![name.to_string()]
    }
}

/// Arguments for one non-interactive run. The prompt itself is always sent on stdin.
fn build_args(
    provider: &str,
    model: &str,
    system_prompt_file: &Path,
    output_file: &Path,
) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    let mut push = |a: &str| args.push(a.into());
    let model = model.trim();
    match provider {
        "claude_cli" => {
            for a in [
                "-p",
                "--output-format",
                "text",
                // No built-in tools, MCP servers, hooks or project settings: plain text in, text out.
                "--tools",
                "",
                "--strict-mcp-config",
                "--setting-sources",
                "",
                "--no-session-persistence",
            ] {
                push(a);
            }
            push("--system-prompt-file");
            args.push(system_prompt_file.into());
            if !model.is_empty() {
                args.push("--model".into());
                args.push(model.into());
            }
        }
        "codex_cli" => {
            for a in [
                "exec",
                "--sandbox",
                "read-only",
                "--skip-git-repo-check",
                "--ephemeral",
                "--color",
                "never",
            ] {
                push(a);
            }
            args.push("--output-last-message".into());
            args.push(output_file.into());
            if !model.is_empty() {
                args.push("--model".into());
                args.push(model.into());
            }
            // "-" reads the whole prompt from stdin.
            args.push("-".into());
        }
        _ => {
            // Gemini CLI appends stdin to the -p prompt.
            push("-p");
            push("Follow the instructions above.");
            if !model.is_empty() {
                args.push("--model".into());
                args.push(model.into());
            }
        }
    }
    args
}

/// Codex and Gemini have no system-prompt flag; the instructions lead the stdin prompt.
fn build_stdin(provider: &str, system_prompt: &str, user_message: &str) -> String {
    if provider == "claude_cli" {
        user_message.to_string()
    } else {
        format!("{system_prompt}\n\n---\n\n{user_message}")
    }
}

fn child_path(binary: &Path) -> std::ffi::OsString {
    // npm-installed CLIs start with `#!/usr/bin/env node`; keep the CLI's own directory
    // (where nvm also puts `node`) ahead of the inherited PATH.
    let mut dirs: Vec<PathBuf> = binary.parent().map(Path::to_path_buf).into_iter().collect();
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.extend(fallback_dirs());
    std::env::join_paths(dirs).unwrap_or_default()
}

fn error_detail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("Warning:"))
        .collect();
    // Codex logs a lot on stderr; its fatal error is an `ERROR: {json}` line.
    let detail = lines
        .iter()
        .rev()
        .find_map(|l| l.strip_prefix("ERROR:"))
        .map(|rest| {
            let rest = rest.trim();
            serde_json::from_str::<serde_json::Value>(rest)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
                .unwrap_or_else(|| rest.to_owned())
        })
        .or_else(|| lines.last().map(|l| (*l).to_owned()))
        .unwrap_or_default();
    if detail.chars().count() > MAX_ERROR_DETAIL {
        let cut: String = detail.chars().take(MAX_ERROR_DETAIL).collect();
        return format!("{cut}…");
    }
    detail
}

/// Run the CLI once and return its complete answer.
pub async fn run(
    provider: &str,
    custom_path: Option<&str>,
    model: &str,
    system_prompt: &str,
    user_message: &str,
) -> Result<String, String> {
    if cfg!(mobile) {
        return Err("AI CLI providers are only available on desktop".into());
    }
    let binary = resolve_binary(provider, custom_path)?;
    let workdir = tempfile::Builder::new()
        .prefix("helixnotes-ai-")
        .tempdir()
        .map_err(|_| "Could not create a temporary directory for the AI CLI")?;
    let system_file = workdir.path().join("system-prompt.txt");
    let output_file = workdir.path().join("last-message.txt");
    std::fs::write(&system_file, system_prompt)
        .map_err(|_| "Could not prepare the AI CLI request")?;

    let mut child = Command::new(&binary)
        .args(build_args(provider, model, &system_file, &output_file))
        .current_dir(workdir.path())
        .env("PATH", child_path(&binary))
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| format!("Could not start {}", display_name(provider)))?;

    let mut stdin = child.stdin.take().ok_or("Could not start the AI CLI")?;
    let mut stdout = child.stdout.take().ok_or("Could not start the AI CLI")?;
    let mut stderr = child.stderr.take().ok_or("Could not start the AI CLI")?;
    let input = build_stdin(provider, system_prompt, user_message);

    let work = async {
        let write = async move {
            // A CLI that exits early closes stdin; its exit status reports the real error.
            let _ = stdin.write_all(input.as_bytes()).await;
            drop(stdin);
        };
        let read_out = async {
            let mut buf = Vec::new();
            (&mut stdout)
                .take(MAX_OUTPUT as u64 + 1)
                .read_to_end(&mut buf)
                .await
                .map(|_| buf)
        };
        let read_err = async {
            // Keep only the tail: fatal errors are printed last, after any log noise.
            const KEEP: usize = 64 * 1024;
            let mut tail = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let n = stderr.read(&mut chunk).await?;
                if n == 0 {
                    break;
                }
                tail.extend_from_slice(&chunk[..n]);
                if tail.len() > 2 * KEEP {
                    tail.drain(..tail.len() - KEEP);
                }
            }
            Ok::<_, std::io::Error>(tail)
        };
        let ((), out, err) = tokio::join!(write, read_out, read_err);
        let status = child.wait().await;
        (status, out, err)
    };
    let (status, out, err) = tokio::time::timeout(TIMEOUT, work)
        .await
        .map_err(|_| format!("{} did not answer within 5 minutes", display_name(provider)))?;

    let status = status.map_err(|_| "The AI CLI could not be monitored")?;
    let out = out.map_err(|_| "Could not read the AI CLI output")?;
    let err = err.unwrap_or_default();
    if !status.success() {
        let detail = error_detail(&err);
        let name = display_name(provider);
        return Err(if detail.is_empty() {
            format!("{name} exited with an error. Check that it is signed in and the model ID is valid.")
        } else {
            format!("{name} exited with an error: {detail}")
        });
    }
    if out.len() > MAX_OUTPUT {
        return Err("AI response reached its output limit. Use a smaller selection.".into());
    }
    let text = if provider == "codex_cli" {
        // Codex prints progress on stdout; the final answer is written to this file.
        std::fs::read_to_string(&output_file).unwrap_or_default()
    } else {
        String::from_utf8(out).map_err(|_| "The AI CLI returned invalid text")?
    };
    let text = text.trim_end_matches(['\n', '\r']).to_string();
    if text.trim().is_empty() {
        return Err(format!(
            "{} returned an empty response. The result cannot be applied.",
            display_name(provider)
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(provider: &str, model: &str) -> Vec<String> {
        build_args(
            provider,
            model,
            Path::new("/tmp/sys"),
            Path::new("/tmp/out"),
        )
        .into_iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
    }

    #[test]
    fn claude_runs_print_mode_without_tools() {
        let a = args("claude_cli", "sonnet");
        assert_eq!(a[0], "-p");
        let tools = a.iter().position(|x| x == "--tools").unwrap();
        assert_eq!(a[tools + 1], "");
        assert!(a.contains(&"--strict-mcp-config".to_string()));
        assert!(a
            .windows(2)
            .any(|w| w == ["--system-prompt-file", "/tmp/sys"]));
        assert!(a.windows(2).any(|w| w == ["--model", "sonnet"]));
    }

    #[test]
    fn codex_is_read_only_and_reads_stdin() {
        let a = args("codex_cli", "");
        assert_eq!(a[0], "exec");
        assert!(a.windows(2).any(|w| w == ["--sandbox", "read-only"]));
        assert!(a
            .windows(2)
            .any(|w| w == ["--output-last-message", "/tmp/out"]));
        assert_eq!(a.last().unwrap(), "-");
        assert!(
            !a.contains(&"--model".to_string()),
            "empty model uses the CLI default"
        );
    }

    #[test]
    fn gemini_uses_headless_prompt() {
        let a = args("gemini_cli", "gemini-2.5-pro");
        assert_eq!(a[0], "-p");
        assert!(a.windows(2).any(|w| w == ["--model", "gemini-2.5-pro"]));
    }

    #[test]
    fn system_prompt_goes_to_stdin_only_without_a_flag() {
        assert_eq!(build_stdin("claude_cli", "SYS", "USER"), "USER");
        assert!(build_stdin("codex_cli", "SYS", "USER").starts_with("SYS"));
        assert!(build_stdin("gemini_cli", "SYS", "USER").ends_with("USER"));
    }

    #[test]
    fn relative_custom_path_is_rejected() {
        assert!(resolve_binary("claude_cli", Some("bin/claude")).is_err());
        assert!(resolve_binary("unknown", None).is_err());
    }

    #[test]
    fn error_detail_skips_warnings_and_is_bounded() {
        let detail = error_detail(b"Warning: sign-in expires\nnot logged in\n");
        assert_eq!(detail, "not logged in");
        let codex = br#"2026 ERROR codex_models_manager: noise
ERROR: {"type":"error","status":400,"error":{"message":"The 'gpt-5.5' model is not supported."}}
trailing instructions dump"#;
        assert_eq!(error_detail(codex), "The 'gpt-5.5' model is not supported.");
        let long = "x".repeat(5000);
        assert!(error_detail(long.as_bytes()).chars().count() <= MAX_ERROR_DETAIL + 1);
    }

    /// Manual check against a real, signed-in CLI:
    /// `HELIXNOTES_CLI_PROVIDER=claude_cli cargo test real_cli -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn real_cli() {
        let provider =
            std::env::var("HELIXNOTES_CLI_PROVIDER").unwrap_or_else(|_| "claude_cli".into());
        let out = run(
            &provider,
            None,
            "",
            "Return ONLY the resulting text - no explanations.",
            "Fix all grammar errors in this text:\n\nShe dont like apples.",
        )
        .await
        .unwrap();
        assert!(out.contains("doesn't") || out.contains("does not"), "{out}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runs_a_fake_cli_end_to_end() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("claude");
        // Echo stdin back, uppercased, to prove the prompt travels through stdin.
        std::fs::write(&script, "#!/bin/sh\ntr a-z A-Z\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let out = run("claude_cli", script.to_str(), "", "sys", "hello\n")
            .await
            .unwrap();
        assert_eq!(out, "HELLO");

        std::fs::write(&script, "#!/bin/sh\necho 'not logged in' >&2\nexit 3\n").unwrap();
        let err = run("claude_cli", script.to_str(), "", "sys", "hello")
            .await
            .unwrap_err();
        assert!(err.contains("not logged in"), "{err}");

        std::fs::write(&script, "#!/bin/sh\ncat >/dev/null\n").unwrap();
        let err = run("claude_cli", script.to_str(), "", "sys", "hello")
            .await
            .unwrap_err();
        assert!(err.contains("empty response"), "{err}");
    }
}
