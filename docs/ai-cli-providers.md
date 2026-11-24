# AI providers via CLI

Desktop only. HelixNotes can use the AI CLIs installed on this computer instead of an API key:

| Provider | Executable | Sign in first with |
|---|---|---|
| Claude Code | `claude` | `claude` (then `/login`) |
| Codex | `codex` | `codex login` |
| Gemini CLI | `gemini` | `gemini` |

Requests use the CLI's own account and quota (for example a Claude or ChatGPT subscription).

## Setup

1. Install the CLI and sign in once in a terminal.
2. Settings → AI → choose **Claude Code**, **Codex** or **Gemini CLI**.
3. Optional: **CLI path**, if the executable is not found automatically. HelixNotes looks in
   `PATH`, then `~/.local/bin`, `~/.npm-global/bin`, `~/.bun/bin`, `~/.volta/bin`, nvm's Node
   folders, `/usr/local/bin` and `/opt/homebrew/bin`. A desktop launcher often has a shorter
   `PATH` than your terminal.
4. Optional: **Model**. Leave empty to use the CLI's default (for example `sonnet` or `opus`
   for Claude Code).
5. **Test connection** runs one short request.

## Using it

Select text in a note and open AI Actions, as with any other provider. The answer appears in
the same review panel; nothing changes in the note until you apply it. The usual checks
apply: media placeholders must survive, and the note must not have changed since the request.

## How a request runs

- One non-interactive run per request: `claude -p`, `codex exec` or `gemini -p`.
- The prompt is sent on stdin, never as a command-line argument.
- The CLI runs in an empty temporary directory, deleted afterwards, so it does not pick up
  project files such as `CLAUDE.md` or `AGENTS.md`.
- Tools are disabled where the CLI allows it. Claude Code runs with no built-in tools, no MCP
  servers and no hooks or settings files. Codex runs in its `read-only` sandbox. Gemini CLI
  runs in headless mode, where tools that need approval are refused.
- Only a successful exit with non-empty output can be applied. A non-zero exit shows the
  CLI's last error line.
- Text only: image selection is hidden for CLI providers.
- Time limit: 5 minutes. Output limit: 4 MiB. There is no streaming; the answer appears when
  the CLI finishes.

## Checking a CLI from the source tree

```bash
HELIXNOTES_CLI_PROVIDER=claude_cli cargo test --manifest-path src-tauri/Cargo.toml real_cli -- --ignored
```

Use `codex_cli` or `gemini_cli` for the other providers.
