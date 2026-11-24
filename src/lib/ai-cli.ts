// Local AI CLIs used as text-only providers (desktop only). Must match src-tauri/src/ai/cli.rs.
export const AI_CLI_PROVIDERS = [
  { id: 'claude_cli', label: 'Claude Code', binary: 'claude', modelHint: 'sonnet, opus' },
  { id: 'codex_cli', label: 'Codex', binary: 'codex', modelHint: 'gpt-5-codex' },
  { id: 'gemini_cli', label: 'Gemini CLI', binary: 'gemini', modelHint: 'gemini-2.5-pro' },
] as const;

export function isCliProvider(provider: string | null | undefined): boolean {
  return AI_CLI_PROVIDERS.some(p => p.id === provider);
}

export function cliProviderInfo(provider: string | null | undefined) {
  return AI_CLI_PROVIDERS.find(p => p.id === provider) ?? null;
}
