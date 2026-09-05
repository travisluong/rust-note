# Agent instructions

## Complete and commit work

- After completing each requested task, run the checks appropriate to the change and commit the completed work to this repository before sending the final response.
- For Rust code changes, run `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings`. Fix failures caused by your changes before committing.
- Review the diff and stage only files belonging to the task. Preserve unrelated user changes, and do not commit secrets or generated build artifacts.
- Use a concise, descriptive commit message. Do not create an empty commit when no files changed.
- Explicit user instructions to skip or defer a commit take precedence. If a commit is blocked, explain why and report the remaining uncommitted work.
- Report the commit hash and validation results in the final response. Do not push unless the user requests it.
