# AI Chat in the PhotoCraft fork

Open **Window > AI Chat** in the desktop editor. The panel docks, floats and saves with the workspace. Its Settings button opens **Preferences > AI Chat**, where the model, active-image attachment, secure key saving and local history preferences are stored with the editor preferences.

Enter an OpenAI API key in the panel or preferences. When remembering keys is enabled, the shared plugin saves it in the OS credential store for this editor profile. The key and prompt draft are never written into editor preferences or image documents. This uses your OpenAI API account and its normal usage charges.

The assistant can inspect a bounded PNG of the current document composite and use PhotoCraft's command and UI APIs for layers, selections, masks, painting, text, filters, import and export. Edits use the normal command and undo paths and obey the editor's automation policy. Image capture runs off the UI thread and rejects a result if the active document changed while it was rendering. It captures the image, not the desktop or chat panel.

The panel uses the shared [Craft AI Chat Rust plugin](https://github.com/lakam99/craft-ai-chat), pinned to an immutable revision. Transport, chat rendering, batching, history and credential storage are shared with the other editor forks. Web builds retain their existing editor and do not include the native assistant transport.

PhotoCraft and the fork launcher check [lakam99/photocraft releases](https://github.com/lakam99/photocraft/releases). Install packages only after verifying their published SHA256 checksums.

## Automation

In a running desktop editor, `ui.menu.invoke` can open `window.panel.aiChat`. The normal `engine.execute` control method also accepts `ai.prompt` with `{ "text": "your request" }`, then `ai.send`, `ai.stop`, `ai.clear`, `ai.settings` or `ai.status` with an empty parameter object. Status includes registered chat widget IDs, labels and rectangles without password or draft values. Model and attachment options use `prefs.get` / `prefs.set` paths under `ai`.

The assistant's own tools cannot invoke chat controls or change AI preferences. Enter API keys through the password field; keys have no editor-preference or document field.
