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
- Click **Read mode** in the upper right to render the current text, including unsaved edits. Click **Markdown view** to return to editing. Switching views preserves your changes.
- Choose **File → Save** (Ctrl+S / Cmd+S) to write changes.
- Switching documents or folders and closing the window prompts to save unsaved changes.

The sidebar lists regular files and directories, with folders first. Symbolic links are skipped to avoid recursive cycles. Files must contain UTF-8 text. Read mode renders Markdown headings, emphasis, lists, links, and code blocks. This MVP does not create new files. Folder contents are refreshed as the tree is drawn.

## Check

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```
