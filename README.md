# Rust Note

A native Rust Markdown editor built with egui/eframe.

## Run

Install the Rust toolchain and your platform's native build tools (on Windows, Visual Studio C++ Build Tools), then run:

```sh
cargo run
```

- Choose **File → Open Folder** (Ctrl+O / Cmd+O).
- Click the folder arrow in the left sidebar to expand it. Subfolders expand recursively.
- Click a file to edit its Markdown source in the right pane.
- Right-click any folder and choose **New Folder…**, enter a name, then click **Create** (or press Enter). The subfolder appears when its parent is expanded. Existing files and folders are preserved, and your current note stays open.
- Right-click any folder and choose **New Note…**, enter a file name, then click **Create** (or press Enter). Names without extensions get `.md`. The new note opens in the editor; existing files are never overwritten. **Cancel** or Escape dismisses the dialog.
- Use the upper-right view selector to choose **Markdown view**, **Read mode**, or **Live Preview**. Live Preview is a continuous editor: type, press Enter, select across paragraphs, and undo normally. Headings, emphasis, links, and code are styled as you type; Markdown markers reveal on the active line and hide elsewhere. There are no paragraph controls or Done buttons. All views share the same text and Save command. Live Preview keeps lists, tables, images, and code fences in their Markdown representation; use Read mode for full rendering.
- Choose **File → Save** (Ctrl+S / Cmd+S) to write changes.
- Choose **File → Settings…** to independently adjust UI and content font sizes. Content sizing applies to both editing and read mode. Changes apply immediately for the current session; **Reset defaults** restores the original sizes.
- Switching documents or folders and closing the window prompts to save unsaved changes.

The sidebar lists regular files and directories, with folders first. Symbolic links are skipped to avoid recursive cycles. Files must contain UTF-8 text. Read mode renders Markdown headings, emphasis, lists, links, and code blocks. Folder contents are refreshed as the tree is drawn.

## Check

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```
