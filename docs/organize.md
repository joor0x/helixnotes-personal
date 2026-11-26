# Organize with AI

Desktop only for now. Files the notes of one folder into the notebooks the vault already has,
using the AI provider chosen in **Settings › AI** (Ollama, OpenAI, Anthropic, any
OpenAI-compatible server, or a CLI provider).

## Using it

1. Right-click **Unfiled Notes** or a notebook in the sidebar and choose **Organize with AI**.
   The entry appears only when an AI provider is set.
2. The dialog names the provider and model the notes will be sent to. Press **Suggest**.
3. Review the list. Each note shows a suggested destination that you can change: a notebook,
   **Trash**, or **Leave here**.
4. **Apply** moves the notes. Nothing changes in the vault before that.

Notes are moved the same way as when you drag them, so links to them are rewritten, and
trashed notes go to the vault's Trash, where they can be restored.

## What is decided, and how

- Only the notes directly inside the folder are looked at, not the ones in its sub-notebooks.
- A note with no content (blank, only list markers, or only the heading that repeats its
  title) is suggested for the Trash without being sent to the AI. It is marked **Empty**.
- For every other note the AI picks an existing notebook, the Trash (when the note has no
  meaningful content), or leaves it where it is when no notebook fits. It cannot create
  notebooks; a notebook name that is not in the vault is ignored and the note stays.
- The habits folder and `Daily` are never offered as destinations.
- One run looks at up to 200 notes, most recently modified first. The dialog says when more
  are left; run it again after applying.

## What is sent to the provider

For each note: its title, its tags and the first 500 characters of its text. For the
notebooks: their paths and descriptions. To write a description, the titles of up to 10 notes
of that notebook are sent once. With Ollama on this computer nothing leaves it.

## The notebook guide

The AI is told what each notebook is for by a guide stored in the vault at
`.helixnotes/notebook-guide.json`:

```json
{
  "notebooks": {
    "Recipes": "Cooking recipes and meal ideas.",
    "Work/Projects": "Plans and meeting notes for work projects."
  }
}
```

- The first run asks the AI to describe every notebook and saves the answers. Later runs
  reuse the file and only ask about notebooks that are new or renamed, so a run normally
  spends no request on the guide. Removed notebooks are dropped from it.
- You can edit a description by hand to steer where notes go; your text is kept. Delete an
  entry, or the file, to have it written again.
- The file is per device: WebDAV and Git sync do not carry it. Tools that copy the whole
  vault folder, such as Syncthing, do.

## Local models

Notes are sent 12 at a time so a request fits the context window of a small model, and each
request may take up to 5 minutes. Answers are read leniently (code fences and extra text are
ignored); a note the model gave no usable answer for stays where it is and the dialog reports
how many. If no answer can be read at all, try a larger model.

## Checking against Ollama from the source tree

```bash
HELIXNOTES_OLLAMA_MODEL=gemma3:4b cargo test --manifest-path src-tauri/Cargo.toml real_ollama -- --ignored --nocapture
```
