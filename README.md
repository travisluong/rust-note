# Rust Note

A native Rust Markdown editor built with egui/eframe.

## Run

Install the Rust toolchain and your platform's native build tools (on Windows, Visual Studio C++ Build Tools), then run:

```sh
cargo run
```

- Choose **File → Manage Notebooks** (Ctrl+O / Cmd+O), then **Open folder as notebook…** to select a folder. It is added to the saved notebook registry and opened in the sidebar. Duplicate paths are ignored.
- Choose a notebook from **Notebooks** to browse it in the sidebar. Hover over its name to see its full path. Switching notebooks preserves the current note and any unsaved draft.
- In **Manage Notebooks**, click **Remove** to unregister a notebook. Its files and any open draft are preserved. Removing the active notebook clears the sidebar.
- The menu bar shows the active notebook’s folder name, or **Rust Note** when none is open. The sidebar shows its contents directly, without a notebook root row. Click a subfolder arrow to expand it.
- Use **File → New File** or **File → New Folder** to create a file or folder directly in the active notebook. These actions are disabled when no notebook is open.
- Click a file to edit its Markdown source in the right pane.
- Drag a file or folder onto another sidebar folder to move it. During dragging, a drop target appears above the tree for moving back to the notebook root. Escape cancels the drag. Moves preserve open drafts and update their save paths; name conflicts and moving folders into themselves are rejected. Move errors appear in the status bar.
- Click an entry to focus the sidebar, then use **Up/Down** to navigate visible files and folders. Files open immediately. **Right** expands a folder (or enters its first child when already expanded); **Left** collapses it or selects its parent. Arrow keys in the editor continue to move the text cursor. Unsaved changes still prompt before switching files.
- Right-click any folder and choose **New Folder…**, enter a name, then click **Create** (or press Enter). The subfolder appears when its parent is expanded. Existing files and folders are preserved, and your current note stays open.
- Right-click any folder and choose **New Note…**, enter a file name, then click **Create** (or press Enter). Names without extensions get `.md`. The new note opens in the editor; existing files are never overwritten. **Cancel** or Escape dismisses the dialog.
- Use the upper-right view selector to choose **Markdown view**, **Read mode**, or **Live Preview**. Live Preview is a continuous editor: type, press Enter, select across paragraphs, and undo normally. Headings, emphasis, links, and code are styled as you type; Markdown markers reveal on the active line and hide elsewhere. There are no paragraph controls or Done buttons. All views share the same text and Save command. Live Preview keeps lists, tables, images, and code fences in their Markdown representation; use Read mode for full rendering.
- Choose **File → Save** (Ctrl+S / Cmd+S) to write changes.
- Choose **File → Settings…** to independently adjust UI and content font sizes. Content sizing applies to all three views. Changes apply immediately and persist across restarts; **Reset defaults** restores the original sizes.
- On reopening, the app restores its view mode, notebook registry, active notebook, last opened file, both font sizes, and window position and size. Preferences are stored in the platform's app-data directory by eframe. Notes are reloaded from disk; use Save to keep edits. Existing saved folders migrate into the notebook registry. Unavailable notebooks remain registered; selecting one reports the problem without replacing the current sidebar. Missing folders or files do not block startup.
- Switching documents and closing the window prompts to save unsaved changes.

The sidebar lists regular files and directories, with folders first. Symbolic links are skipped to avoid recursive cycles. Files must contain UTF-8 text. Read mode renders Markdown headings, emphasis, lists, links, and code blocks. Folder contents are refreshed as the tree is drawn.

## Check

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```
