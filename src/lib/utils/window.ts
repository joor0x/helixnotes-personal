import { invoke } from '@tauri-apps/api/core';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';

const openNoteWindows = new Map<string, string>();

export async function openNoteWindow(vaultPath: string, notePath: string, noteTitle: string) {
	const existingLabel = openNoteWindows.get(notePath);
	if (existingLabel) {
		const existing = await WebviewWindow.getByLabel(existingLabel);
		if (existing) {
			await existing.show();
			await existing.unminimize();
			await existing.setFocus();
			return;
		}
		openNoteWindows.delete(notePath);
	}

	const label = `note-${crypto.randomUUID()}`;
	const url = `/?note=${encodeURIComponent(notePath)}&vault=${encodeURIComponent(vaultPath)}`;

	// Created in Rust so a portable install keeps the window's webview data next to the app.
	try {
		await invoke('open_note_window', { label, url, title: `${noteTitle} - HelixNotes` });
	} catch (e) {
		console.error('Failed to create note window:', e);
		return;
	}

	const noteWindow = await WebviewWindow.getByLabel(label);
	if (!noteWindow) return;
	openNoteWindows.set(notePath, label);
	noteWindow.once('tauri://destroyed', () => {
		openNoteWindows.delete(notePath);
	});
}
