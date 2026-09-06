#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod explorer;
mod live;
use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn main() -> eframe::Result {
    eframe::run_native(
        "Rust Note",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1100.0, 720.0])
                .with_min_inner_size([640.0, 400.0]),
            ..Default::default()
        },
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            let app = Notes::restore(cc.storage, cc.egui_ctx.clone());
            cc.egui_ctx
                .style_mut(|style| set_font_size(style, app.fonts.ui_size));
            Ok(Box::new(app))
        }),
    )
}

struct Entry {
    path: PathBuf,
    directory: bool,
}

fn entries(path: &Path) -> io::Result<Vec<Entry>> {
    let mut items = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        // Do not follow symbolic links: a link can create a directory cycle.
        if kind.is_dir() || kind.is_file() {
            items.push(Entry {
                path: entry.path(),
                directory: kind.is_dir(),
            });
        }
    }
    items.sort_by_key(|e| (!e.directory, name(&e.path).to_lowercase()));
    Ok(items)
}

fn notebook_files(roots: &[PathBuf]) -> (Vec<PathBuf>, Vec<String>) {
    fn collect(path: &Path, files: &mut Vec<PathBuf>, errors: &mut Vec<String>) {
        match entries(path) {
            Ok(children) => {
                for child in children {
                    if child.directory {
                        collect(&child.path, files, errors);
                    } else {
                        files.push(child.path);
                    }
                }
            }
            Err(error) => errors.push(format!("Cannot read {}: {error}", path.display())),
        }
    }

    let mut files = Vec::new();
    let mut errors = Vec::new();
    for root in roots {
        collect(root, &mut files, &mut errors);
    }
    files.sort_by_key(|path| path.to_string_lossy().to_lowercase());
    (files, errors)
}

fn notebook_file_label(roots: &[PathBuf], path: &Path) -> String {
    roots
        .iter()
        .find_map(|root| path.strip_prefix(root).ok())
        .filter(|relative| !relative.as_os_str().is_empty())
        .map_or_else(|| name(path), |relative| relative.display().to_string())
}

fn name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn validate_name(filename: &str) -> Result<&str, String> {
    let filename = filename.trim();
    if filename.is_empty()
        || filename == "."
        || filename == ".."
        || filename.ends_with('.')
        || filename
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return Err("Enter a valid name without path separators or special characters.".into());
    }
    Ok(filename)
}

fn create_folder(parent: &Path, folder_name: &str) -> Result<PathBuf, String> {
    let path = parent.join(validate_name(folder_name)?);
    fs::create_dir(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            "A file or folder with that name already exists. Choose another name.".into()
        } else {
            format!("Could not create folder: {error}")
        }
    })?;
    Ok(path)
}

fn create_note(folder: &Path, filename: &str) -> Result<PathBuf, String> {
    let filename = validate_name(filename)?;
    let filename = if Path::new(filename).extension().is_none() {
        format!("{filename}.md")
    } else {
        filename.to_owned()
    };
    let path = folder.join(filename);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                "A file with that name already exists. Choose another name.".into()
            } else {
                format!("Could not create note: {error}")
            }
        })?;
    Ok(path)
}

fn moved_path(path: &Path, source: &Path, destination: &Path) -> PathBuf {
    path.strip_prefix(source)
        .map(|suffix| {
            if suffix.as_os_str().is_empty() {
                destination.to_owned()
            } else {
                destination.join(suffix)
            }
        })
        .unwrap_or_else(|_| path.to_owned())
}

fn move_entry(root: &Path, source: &Path, folder: &Path) -> Result<PathBuf, String> {
    let resolve =
        |path: &Path| fs::canonicalize(path).map_err(|e| format!("Cannot move item: {e}"));
    let real_root = resolve(root)?;
    let real_source = resolve(source)?;
    let real_folder = resolve(folder)?;
    if real_source == real_root
        || !real_source.starts_with(&real_root)
        || !real_folder.starts_with(&real_root)
    {
        return Err("Items must stay inside the active notebook.".into());
    }
    if !real_folder.is_dir() {
        return Err("Drop onto a folder.".into());
    }
    if real_folder.starts_with(&real_source) {
        return Err("A folder cannot be moved into itself or one of its subfolders.".into());
    }
    if real_source.parent() == Some(real_folder.as_path()) {
        return Err("This item is already in that folder.".into());
    }
    let filename = source.file_name().ok_or("Cannot move this item.")?;
    let destination = folder.join(filename);
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            return Err("An item with that name already exists in the destination folder.".into());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Cannot check destination: {error}")),
    }
    fs::rename(source, &destination).map_err(|error| format!("Could not move item: {error}"))?;
    Ok(destination)
}

struct NewNote {
    is_folder: bool,
    folder: PathBuf,
    filename: String,
    error: String,
    focus: bool,
}

#[derive(Default)]
struct GoToFile {
    query: String,
    selected: usize,
    focus: bool,
}

impl GoToFile {
    fn navigate(&mut self, count: usize, key: egui::Key) {
        if count == 0 {
            self.selected = 0;
            return;
        }
        match key {
            egui::Key::ArrowUp => self.selected = self.selected.saturating_sub(1),
            egui::Key::ArrowDown => {
                self.selected = (self.selected + 1).min(count.saturating_sub(1));
            }
            _ => {}
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct FontSettings {
    ui_size: f32,
    content_size: f32,
}

impl Default for FontSettings {
    fn default() -> Self {
        Self {
            ui_size: 14.0,
            content_size: 14.0,
        }
    }
}

fn set_font_size(style: &mut egui::Style, size: f32) {
    for (text_style, font) in &mut style.text_styles {
        font.size = match text_style {
            egui::TextStyle::Small => size * 10.0 / 14.0,
            egui::TextStyle::Heading => size * 20.0 / 14.0,
            _ => size,
        };
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
enum View {
    #[default]
    Markdown,
    Read,
    Live,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Session {
    #[serde(skip_serializing_if = "Option::is_none")]
    root: Option<PathBuf>, // Legacy single-folder sessions.
    roots: Vec<PathBuf>,
    notebooks: Vec<PathBuf>,
    file: Option<PathBuf>,
    view: View,
    fonts: FontSettings,
    zoom_factor: Option<f32>,
    autosave_enabled: bool,
}

const SESSION_KEY: &str = "rust-note-session-v1";
const AUTOSAVE_DELAY: Duration = Duration::from_secs(1);
const AUTOSAVE_RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Notes {
    egui_ctx: egui::Context,
    explorer: explorer::Explorer,
    roots: Vec<PathBuf>,
    notebooks: Vec<PathBuf>,
    file: Option<PathBuf>,
    text: String,
    saved: String,
    status: String,
    view: View,
    live_editor: live::LiveEditor,
    markdown_cache: CommonMarkCache,
    new_note: Option<NewNote>,
    go_to_file: Option<GoToFile>,
    settings_open: bool,
    notebooks_open: bool,
    fonts: FontSettings,
    autosave_enabled: bool,
    last_edit_at: Option<Instant>,
    autosave_retry_at: Option<Instant>,
}

impl Notes {
    fn restore(storage: Option<&dyn eframe::Storage>, egui_ctx: egui::Context) -> Self {
        let session: Session = storage
            .and_then(|s| eframe::get_value(s, SESSION_KEY))
            .unwrap_or_default();
        let zoom_factor = session
            .zoom_factor
            .filter(|zoom| zoom.is_finite() && *zoom > 0.0)
            .unwrap_or(1.0)
            .clamp(0.2, 5.0);
        egui_ctx.set_zoom_factor(zoom_factor);
        let mut app = Self {
            egui_ctx,
            view: session.view,
            fonts: session.fonts,
            autosave_enabled: session.autosave_enabled,
            ..Default::default()
        };
        app.fonts.ui_size = if app.fonts.ui_size.is_finite() {
            app.fonts.ui_size.clamp(10.0, 28.0)
        } else {
            14.0
        };
        app.fonts.content_size = if app.fonts.content_size.is_finite() {
            app.fonts.content_size.clamp(10.0, 40.0)
        } else {
            14.0
        };
        for path in session
            .notebooks
            .into_iter()
            .chain(session.roots.iter().cloned())
            .chain(session.root.clone())
        {
            if !app.notebooks.contains(&path) {
                app.notebooks.push(path);
            }
        }
        if let Some(path) = session.roots.first().cloned().or(session.root) {
            app.activate_notebook(path);
        }
        if let Some(file) = session.file {
            let folder_status = std::mem::take(&mut app.status);
            app.open_file(file);
            if !folder_status.is_empty() {
                app.status = if app.status.is_empty() {
                    folder_status
                } else {
                    format!("{folder_status}; {}", app.status)
                };
            }
            if let Some(file) = &app.file {
                for root in &app.roots {
                    if file.starts_with(root) {
                        app.explorer.reveal(root, file);
                    }
                }
            }
        }
        app
    }

    fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut done = false;
        let response = egui::Modal::new(egui::Id::new("settings")).show(ctx, |ui| {
            ui.set_min_width(360.0);
            ui.heading("Settings");
            ui.label("UI font size");
            let mut changed = ui
                .add(
                    egui::Slider::new(&mut self.fonts.ui_size, 10.0..=28.0)
                        .suffix(" pt")
                        .step_by(1.0),
                )
                .changed();
            ui.weak("Menus, sidebar, and dialogs");
            ui.add_space(12.0);
            ui.label("Content font size");
            changed |= ui
                .add(
                    egui::Slider::new(&mut self.fonts.content_size, 10.0..=40.0)
                        .suffix(" pt")
                        .step_by(1.0),
                )
                .changed();
            ui.weak("Markdown, Read mode, and Live Preview");
            ui.add_space(12.0);
            ui.label("Autosave");
            let autosave_changed = ui
                .checkbox(&mut self.autosave_enabled, "Save notes automatically")
                .changed();
            changed |= autosave_changed;
            ui.weak("Saves after 1 second of inactivity and before leaving a note.");
            if autosave_changed {
                self.autosave_retry_at = None;
                if self.autosave_enabled && self.dirty() && self.last_edit_at.is_none() {
                    self.last_edit_at = Some(Instant::now());
                }
            }
            ui.add_space(12.0);
            ui.label("Changes apply immediately and are remembered next time.");
            ui.horizontal(|ui| {
                if ui.button("Reset defaults").clicked() {
                    self.fonts = FontSettings::default();
                    self.autosave_enabled = false;
                    self.autosave_retry_at = None;
                    changed = true;
                }
                done = ui.button("Done").clicked();
            });
            if changed {
                ctx.style_mut(|style| set_font_size(style, self.fonts.ui_size));
                ctx.request_repaint();
            }
        });
        if done || response.should_close() {
            self.settings_open = false;
        }
    }

    fn new_note_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.new_note.take() else {
            return;
        };
        let mut create = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("new_note")).show(ctx, |ui| {
            ui.set_min_width(360.0);
            ui.heading(if draft.is_folder {
                "New Folder"
            } else {
                "New Note"
            });
            ui.label(format!("Create in {}", draft.folder.display()));
            ui.label(if draft.is_folder {
                "Folder name"
            } else {
                "File name"
            });
            let input = ui.add(egui::TextEdit::singleline(&mut draft.filename).hint_text(
                if draft.is_folder {
                    "New folder"
                } else {
                    "Untitled.md"
                },
            ));
            if draft.focus {
                input.request_focus();
                draft.focus = false;
            }
            if !draft.is_folder {
                ui.weak("Names without an extension get .md automatically.");
            }
            if !draft.error.is_empty() {
                ui.colored_label(egui::Color32::LIGHT_RED, &draft.error);
            }
            ui.horizontal(|ui| {
                create = ui.button("Create").clicked()
                    || (input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                cancel = ui.button("Cancel").clicked();
            });
        });
        if cancel || response.should_close() {
            return;
        }
        if create && draft.is_folder {
            match create_folder(&draft.folder, &draft.filename) {
                Ok(path) => {
                    self.status = format!("Folder created: {}", name(&path));
                    return;
                }
                Err(error) => draft.error = error,
            }
        } else if create && self.may_leave() {
            match create_note(&draft.folder, &draft.filename) {
                Ok(path) => {
                    self.file = Some(path);
                    self.text.clear();
                    self.saved.clear();
                    self.last_edit_at = None;
                    self.autosave_retry_at = None;
                    self.view = View::Markdown;
                    self.live_editor = live::LiveEditor::default();
                    self.status = "Note created".into();
                    return;
                }
                Err(error) => draft.error = error,
            }
        }
        self.new_note = Some(draft);
    }

    fn dirty(&self) -> bool {
        self.text != self.saved
    }

    fn note_edited(&mut self) {
        if self.dirty() {
            self.last_edit_at = Some(Instant::now());
            self.autosave_retry_at = None;
        } else {
            self.last_edit_at = None;
            self.autosave_retry_at = None;
        }
    }

    fn write_contents(&self) -> io::Result<()> {
        let Some(path) = &self.file else {
            return Ok(());
        };
        fs::write(path, &self.text)
    }

    fn mark_saved(&mut self, status: &str) {
        self.saved.clone_from(&self.text);
        self.last_edit_at = None;
        self.autosave_retry_at = None;
        self.status = status.into();
    }

    fn save(&mut self) -> bool {
        match self.write_contents() {
            Ok(()) => {
                self.mark_saved("Saved");
                true
            }
            Err(error) => {
                self.status = format!("Could not save: {error}");
                if self.autosave_enabled {
                    self.autosave_retry_at = Some(Instant::now() + AUTOSAVE_RETRY_DELAY);
                }
                false
            }
        }
    }

    fn autosave(&mut self, now: Instant) -> bool {
        match self.write_contents() {
            Ok(()) => {
                self.mark_saved("Autosaved");
                true
            }
            Err(error) => {
                self.autosave_retry_at = Some(now + AUTOSAVE_RETRY_DELAY);
                self.status = format!("Autosave failed: {error}");
                false
            }
        }
    }

    fn autosave_if_due_at(&mut self, now: Instant) -> Option<Duration> {
        if !self.autosave_enabled {
            return None;
        }
        if !self.dirty() {
            self.last_edit_at = None;
            self.autosave_retry_at = None;
            return None;
        }
        if let Some(retry_at) = self.autosave_retry_at
            && now < retry_at
        {
            return Some(retry_at - now);
        }
        let Some(last_edit_at) = self.last_edit_at else {
            self.last_edit_at = Some(now);
            return Some(AUTOSAVE_DELAY);
        };
        let Some(elapsed) = now.checked_duration_since(last_edit_at) else {
            return Some(AUTOSAVE_DELAY);
        };
        if elapsed < AUTOSAVE_DELAY {
            return Some(AUTOSAVE_DELAY - elapsed);
        }
        self.autosave(now);
        self.autosave_retry_at
            .map(|retry_at| retry_at.saturating_duration_since(now))
    }

    fn autosave_if_due(&mut self, ctx: &egui::Context) {
        if let Some(delay) = self.autosave_if_due_at(Instant::now()) {
            ctx.request_repaint_after(delay);
        }
    }

    fn may_leave(&mut self) -> bool {
        if !self.dirty() {
            return true;
        }
        if self.autosave_enabled {
            return self.autosave(Instant::now());
        }
        match rfd::MessageDialog::new()
            .set_title("Unsaved changes")
            .set_description("Save your changes before continuing?")
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show()
        {
            rfd::MessageDialogResult::Yes => self.save(),
            rfd::MessageDialogResult::No => true,
            _ => false,
        }
    }

    fn add_folder(&mut self, path: PathBuf) {
        if !self.notebooks.contains(&path) {
            self.notebooks.push(path.clone());
        }
        self.activate_notebook(path);
    }

    fn activate_notebook(&mut self, path: PathBuf) {
        if !path.is_dir() {
            self.status = format!("Notebook is unavailable: {}", path.display());
            return;
        }
        self.roots = vec![path.clone()];
        self.explorer = explorer::Explorer::default();
        self.explorer.open_root(path);
        self.status.clear();
    }

    fn remove_folder(&mut self, path: &Path) {
        self.notebooks.retain(|root| root != path);
        self.roots.retain(|root| root != path);
        self.explorer.retain_roots(&self.roots);
    }

    fn open_folder(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open folder as notebook")
            .pick_folder()
        {
            self.add_folder(path);
        }
    }

    fn notebooks_dialog(&mut self, ctx: &egui::Context) {
        if !self.notebooks_open {
            return;
        }
        let mut done = false;
        let mut remove = None;
        let response = egui::Modal::new(egui::Id::new("notebooks")).show(ctx, |ui| {
            ui.set_min_width(440.0);
            ui.heading("Manage Notebooks");
            if ui.button("Open folder as notebook…").clicked() {
                self.open_folder();
            }
            ui.weak("Removing a notebook keeps its files and any open note.");
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    if self.notebooks.is_empty() {
                        ui.label("No notebooks yet. Choose a folder to get started.");
                    }
                    for path in &self.notebooks {
                        ui.push_id(path, |ui| {
                            ui.horizontal(|ui| {
                                ui.strong(name(path));
                                if self.roots.contains(path) {
                                    ui.weak("Active");
                                }
                                if ui.button("Remove").clicked() {
                                    remove = Some(path.clone());
                                }
                            });
                            ui.label(path.display().to_string());
                            ui.separator();
                        });
                    }
                });
            done = ui.button("Done").clicked();
        });
        if let Some(path) = remove {
            self.remove_folder(&path);
        }
        if done || response.should_close() {
            self.notebooks_open = false;
        }
    }

    fn go_to_file_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.go_to_file.take() else {
            return;
        };
        let (files, errors) = notebook_files(&self.roots);
        let query = dialog.query.to_lowercase();
        let mut matches: Vec<(PathBuf, String)> = files
            .into_iter()
            .map(|path| {
                let label = notebook_file_label(&self.roots, &path);
                (path, label)
            })
            .filter(|(_, label)| query.is_empty() || label.to_lowercase().contains(&query))
            .collect();
        matches.sort_by_key(|(_, label)| label.to_lowercase());
        dialog.selected = dialog.selected.min(matches.len().saturating_sub(1));

        let mut accepted = false;
        let mut accepted_path = None;
        let response = egui::Modal::new(egui::Id::new("go_to_file")).show(ctx, |ui| {
            ui.set_min_width(520.0);
            ui.heading("Go to File");
            let input = ui.add(
                egui::TextEdit::singleline(&mut dialog.query)
                    .hint_text("Search files…")
                    .desired_width(f32::INFINITY),
            );
            if dialog.focus {
                input.request_focus();
                dialog.focus = false;
            }
            if input.changed() {
                dialog.selected = 0;
            }

            if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                dialog.navigate(matches.len(), egui::Key::ArrowUp);
            }
            if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                dialog.navigate(matches.len(), egui::Key::ArrowDown);
            }

            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (index, (_, label)) in matches.iter().enumerate() {
                        let row = ui.selectable_label(dialog.selected == index, label);
                        if row.clicked() {
                            dialog.selected = index;
                        }
                        if dialog.selected == index {
                            row.scroll_to_me(Some(egui::Align::Center));
                        }
                    }
                    if matches.is_empty() {
                        ui.weak(if self.roots.is_empty() {
                            "No notebook is open."
                        } else {
                            "No matching files."
                        });
                    }
                    for error in &errors {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                });
            ui.add_space(4.0);
            ui.weak("↑ ↓ to navigate  •  Enter to open  •  Esc to cancel");

            if ui.input(|i| i.key_pressed(egui::Key::Enter))
                && let Some((path, _)) = matches.get(dialog.selected)
            {
                accepted = true;
                accepted_path = Some(path.clone());
            }
        });

        if response.should_close() && !accepted {
            return;
        }
        if let Some(path) = accepted_path {
            if self.open_file(path.clone()) {
                for root in &self.roots {
                    if path.starts_with(root) {
                        self.explorer.reveal(root, &path);
                        break;
                    }
                }
            } else {
                self.go_to_file = Some(dialog);
            }
        } else {
            self.go_to_file = Some(dialog);
        }
    }

    fn move_item(&mut self, source: PathBuf, folder: PathBuf) {
        let Some(root) = self.roots.first() else {
            return;
        };
        match move_entry(root, &source, &folder) {
            Ok(destination) => {
                if let Some(file) = &mut self.file {
                    *file = moved_path(file, &source, &destination);
                }
                for notebook in &mut self.notebooks {
                    *notebook = moved_path(notebook, &source, &destination);
                }
                if let Some(draft) = &mut self.new_note {
                    draft.folder = moved_path(&draft.folder, &source, &destination);
                }
                self.explorer.item_moved(&source, &destination);
                self.status = format!("Moved {} to {}", name(&source), folder.display());
            }
            Err(error) => self.status = error,
        }
    }

    fn open_file(&mut self, path: PathBuf) -> bool {
        if self.file.as_ref() == Some(&path) {
            return true;
        }
        // Read first so an unreadable file never replaces the current document.
        match fs::read_to_string(&path) {
            Ok(text) => {
                if self.may_leave() {
                    self.saved = text.clone();
                    self.text = text;
                    self.file = Some(path);
                    self.last_edit_at = None;
                    self.autosave_retry_at = None;
                    self.live_editor = live::LiveEditor::default();
                    self.status.clear();
                    true
                } else {
                    false
                }
            }
            Err(error) => {
                self.status = format!("Cannot open {} as UTF-8 text: {error}", name(&path));
                false
            }
        }
    }
}

impl eframe::App for Notes {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(
            storage,
            SESSION_KEY,
            &Session {
                root: None,
                roots: self.roots.clone(),
                notebooks: self.notebooks.clone(),
                file: self.file.clone(),
                view: self.view,
                fonts: self.fonts.clone(),
                zoom_factor: Some(self.egui_ctx.zoom_factor()),
                autosave_enabled: self.autosave_enabled,
            },
        );
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.may_leave() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        if self.new_note.is_none()
            && !self.settings_open
            && !self.notebooks_open
            && self.go_to_file.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
        {
            self.save();
        }
        if self.new_note.is_none()
            && !self.settings_open
            && !self.notebooks_open
            && self.go_to_file.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O))
        {
            self.go_to_file = Some(GoToFile {
                focus: true,
                ..Default::default()
            });
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.strong(
                    self.roots
                        .first()
                        .map(|path| name(path))
                        .unwrap_or_else(|| "Rust Note".into()),
                );
                ui.separator();
                ui.menu_button("File", |ui| {
                    for (label, is_folder) in [("New File", false), ("New Folder", true)] {
                        if ui
                            .add_enabled(!self.roots.is_empty(), egui::Button::new(label))
                            .clicked()
                        {
                            if let Some(folder) = self.roots.first() {
                                self.new_note = Some(NewNote {
                                    is_folder,
                                    folder: folder.clone(),
                                    filename: String::new(),
                                    error: String::new(),
                                    focus: true,
                                });
                            }
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui.button("Go to File    Ctrl+O").clicked() {
                        ui.close_menu();
                        self.go_to_file = Some(GoToFile {
                            focus: true,
                            ..Default::default()
                        });
                    }
                    if ui.button("Manage Notebooks").clicked() {
                        ui.close_menu();
                        self.notebooks_open = true;
                    }
                    if ui
                        .add_enabled(self.file.is_some(), egui::Button::new("Save    Ctrl+S"))
                        .clicked()
                    {
                        ui.close_menu();
                        self.save();
                    }
                    ui.separator();
                    if ui.button("Settings…").clicked() {
                        ui.close_menu();
                        self.settings_open = true;
                    }
                });
                ui.menu_button("Notebooks", |ui| {
                    let mut selected = None;
                    if self.notebooks.is_empty() {
                        ui.weak("No notebooks added");
                    }
                    for path in &self.notebooks {
                        ui.push_id(path, |ui| {
                            if ui
                                .selectable_label(self.roots.contains(path), name(path))
                                .on_hover_text(path.display().to_string())
                                .clicked()
                            {
                                selected = Some(path.clone());
                                ui.close_menu();
                            }
                        });
                    }
                    if let Some(path) = selected {
                        self.activate_notebook(path);
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(self.file.is_some(), |ui| {
                        let previous = self.view;
                        egui::ComboBox::from_id_salt("view")
                            .selected_text(match self.view {
                                View::Markdown => "Markdown view",
                                View::Read => "Read mode",
                                View::Live => "Live Preview",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.view,
                                    View::Markdown,
                                    "Markdown view",
                                );
                                ui.selectable_value(&mut self.view, View::Read, "Read mode");
                                ui.selectable_value(&mut self.view, View::Live, "Live Preview");
                            });
                        if self.view != previous {
                            self.live_editor = live::LiveEditor::default();
                        }
                    });
                });
            });
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.small(&self.status);
                if self.file.is_some() {
                    ui.separator();
                    ui.small(format!(
                        "{} words  •  {}",
                        self.text.split_whitespace().count(),
                        if self.dirty() {
                            "Unsaved changes"
                        } else {
                            "Saved"
                        }
                    ));
                }
            });
        });
        let mut moved = None;
        let mut clicked = None;
        let mut new_note = None;
        egui::SidePanel::left("files")
            .resizable(true)
            .default_width(260.0)
            .width_range(160.0..=500.0)
            .show(ctx, |ui| {
                ui.add_space(12.0);
                if self.roots.is_empty() {
                    ui.weak("Choose a notebook from Notebooks.");
                } else {
                    let actions = self.explorer.show(
                        ui,
                        &self.roots,
                        self.new_note.is_none()
                            && self.go_to_file.is_none()
                            && !self.settings_open
                            && !self.notebooks_open,
                    );
                    moved = actions.move_entry;
                    clicked = actions.open_file;
                    new_note = actions.new_entry;
                }
            });
        if let Some((source, folder)) = moved {
            self.move_item(source, folder);
        }
        if let Some(path) = clicked {
            self.open_file(path.clone());
            if self.file.as_ref() != Some(&path) {
                self.explorer.selected.clone_from(&self.file);
            }
        }
        if let Some((folder, is_folder)) = new_note {
            self.new_note = Some(NewNote {
                is_folder,
                folder,
                filename: String::new(),
                error: String::new(),
                focus: true,
            });
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(path) = &self.file {
                ui.horizontal(|ui| {
                    ui.heading(format!(
                        "{}{}",
                        name(path),
                        if self.dirty() { " *" } else { "" }
                    ));
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt((path, self.view))
                    .show(ui, |ui| {
                        set_font_size(ui.style_mut(), self.fonts.content_size);
                        if self.view == View::Read {
                            CommonMarkViewer::new().show_mut(
                                ui,
                                &mut self.markdown_cache,
                                &mut self.text,
                            );
                        } else if self.view == View::Live {
                            if self.live_editor.show(ui, &mut self.text) {
                                self.note_edited();
                            }
                        } else {
                            let before = self.text.clone();
                            let output = egui::TextEdit::multiline(&mut self.text)
                                .id_salt("markdown_editor")
                                .font(egui::TextStyle::Monospace)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .frame(false)
                                .show(ui);
                            let mut state = output.state;
                            if let Some(mut range) = state.cursor.char_range() {
                                live::continue_task_list(&mut self.text, &before, &mut range);
                                state.cursor.set_char_range(Some(range));
                                state.store(ctx, output.response.id);
                            }
                            if self.text != before {
                                self.note_edited();
                            }
                        }
                    });
            } else {
                ui.add_space(100.0);
                ui.vertical_centered(|ui| {
                    ui.heading("A little space for your thoughts.");
                    ui.add_space(12.0);
                    ui.weak(
                        "Open a folder as a notebook, then select a Markdown file to start writing.",
                    );
                    ui.add_space(20.0);
                    if ui.button("Manage Notebooks").clicked() {
                        self.notebooks_open = true;
                    }
                });
            }
        });
        self.new_note_dialog(ctx);
        self.settings_dialog(ctx);
        self.notebooks_dialog(ctx);
        self.go_to_file_dialog(ctx);
        self.autosave_if_due(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::Storage;
    #[derive(Default)]
    struct MemoryStorage(std::collections::HashMap<String, String>);
    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.into(), value);
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn moves_files_and_folders_preserving_drafts_and_registered_paths() {
        let root = std::env::temp_dir().join(format!("rust-note-move-{}", std::process::id()));
        let source = root.join("source");
        let target = root.join("target");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::create_dir_all(&target).unwrap();
        let file = source.join("nested/note.md");
        fs::write(&file, "Saved").unwrap();
        let mut app = Notes::default();
        app.add_folder(source.clone());
        app.add_folder(root.clone());
        app.open_file(file.clone());
        app.text = "Unsaved draft".into();
        app.explorer.reveal(&root, &file);
        app.move_item(source.clone(), target.clone());
        let moved = target.join("source/nested/note.md");
        assert!(!source.exists());
        assert_eq!(app.file, Some(moved.clone()));
        assert_eq!(app.explorer.selected, Some(moved.clone()));
        assert!(app.notebooks.contains(&target.join("source")));
        assert_eq!(app.text, "Unsaved draft");
        assert!(app.dirty());
        assert_eq!(fs::read_to_string(&moved).unwrap(), "Saved");
        app.move_item(moved.clone(), root.clone());
        assert_eq!(app.file, Some(root.join("note.md")));
        assert!(!moved.exists());
        assert!(app.save());
        assert_eq!(
            fs::read_to_string(root.join("note.md")).unwrap(),
            "Unsaved draft"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_moves_preserve_sources_and_destinations() {
        let root =
            std::env::temp_dir().join(format!("rust-note-move-invalid-{}", std::process::id()));
        let source = root.join("source");
        let target = root.join("target");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::create_dir_all(&target).unwrap();
        let file = source.join("note.md");
        fs::write(&file, "Source").unwrap();
        fs::write(target.join("note.md"), "Destination").unwrap();
        assert!(move_entry(&root, &file, &target).is_err());
        assert!(move_entry(&root, &source, &source).is_err());
        assert!(move_entry(&root, &source, &source.join("nested")).is_err());
        assert!(move_entry(&root, &source, &root).is_err());
        assert!(move_entry(&root, &root, &target).is_err());
        assert!(move_entry(&root, &file, &root.join("missing")).is_err());
        assert!(move_entry(&root, &file, &std::env::temp_dir()).is_err());
        assert!(move_entry(&root, &file, &target.join("note.md")).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "Source");
        assert_eq!(
            fs::read_to_string(target.join("note.md")).unwrap(),
            "Destination"
        );
        assert!(source.join("nested").is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn zoom_persists_from_context_and_restores_at_startup() {
        let mut app = Notes::default();
        app.egui_ctx.set_zoom_factor(1.7);
        let _ = app.egui_ctx.run(Default::default(), |_| {});
        let mut storage = MemoryStorage::default();
        eframe::App::save(&mut app, &mut storage);
        let ctx = egui::Context::default();
        let _restored = Notes::restore(Some(&storage), ctx.clone());
        let _ = ctx.run(Default::default(), |_| {});
        assert_eq!(ctx.zoom_factor(), 1.7);
    }

    #[test]
    fn zoom_defaults_for_old_sessions_and_validates_saved_values() {
        let mut storage = MemoryStorage::default();
        storage.set_string(
            SESSION_KEY,
            "(view:Read,fonts:(ui_size:20.0,content_size:26.0))".into(),
        );
        let app = Notes::restore(Some(&storage), egui::Context::default());
        let _ = app.egui_ctx.run(Default::default(), |_| {});
        assert_eq!(app.egui_ctx.zoom_factor(), 1.0);
        assert_eq!(app.fonts.ui_size, 20.0);
        for (zoom, expected) in [
            (0.0, 1.0),
            (-1.0, 1.0),
            (f32::NAN, 1.0),
            (f32::INFINITY, 1.0),
            (0.1, 0.2),
            (10.0, 5.0),
        ] {
            eframe::set_value(
                &mut storage,
                SESSION_KEY,
                &Session {
                    zoom_factor: Some(zoom),
                    ..Default::default()
                },
            );
            let app = Notes::restore(Some(&storage), egui::Context::default());
            let _ = app.egui_ctx.run(Default::default(), |_| {});
            assert_eq!(app.egui_ctx.zoom_factor(), expected);
        }
    }

    #[test]
    fn notebooks_persist_and_removal_preserves_open_draft() {
        let root = std::env::temp_dir().join(format!("rust-note-multi-{}", std::process::id()));
        let other = root.join("other");
        fs::create_dir_all(&other).unwrap();
        let file = root.join("note.md");
        fs::write(&file, "Saved").unwrap();
        let mut app = Notes::default();
        app.add_folder(root.clone());
        app.open_file(file.clone());
        app.text = "Draft".into();
        app.add_folder(other.clone());
        app.add_folder(root.clone());
        assert_eq!(app.notebooks, vec![root.clone(), other.clone()]);
        assert_eq!(app.roots, vec![root.clone()]);
        let mut storage = MemoryStorage::default();
        eframe::App::save(&mut app, &mut storage);
        assert_eq!(
            Notes::restore(Some(&storage), egui::Context::default()).roots,
            app.roots
        );
        assert_eq!(
            Notes::restore(Some(&storage), egui::Context::default()).notebooks,
            app.notebooks
        );
        app.explorer.reveal(&root, &file);
        app.remove_folder(&root);
        assert!(app.roots.is_empty());
        assert_eq!(app.notebooks, vec![other.clone()]);
        assert!(app.explorer.selected.is_none());
        assert_eq!(app.file, Some(file.clone()));
        assert_eq!(app.text, "Draft");
        assert!(app.dirty());
        assert_eq!(fs::read_to_string(&file).unwrap(), "Saved");
        app.remove_folder(&other);
        eframe::App::save(&mut app, &mut storage);
        let restored = Notes::restore(Some(&storage), egui::Context::default());
        assert!(restored.roots.is_empty());
        assert_eq!(restored.file, Some(file));
        // The previous session schema must still restore its single root.
        eframe::set_value(
            &mut storage,
            SESSION_KEY,
            &Session {
                root: Some(root.clone()),
                ..Default::default()
            },
        );
        assert_eq!(
            Notes::restore(Some(&storage), egui::Context::default()).roots,
            vec![root.clone()]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_folders_migrate_and_unavailable_selection_preserves_sidebar() {
        let root = std::env::temp_dir();
        let missing = root.join(format!("rust-note-unavailable-{}", std::process::id()));
        let mut storage = MemoryStorage::default();
        eframe::set_value(
            &mut storage,
            SESSION_KEY,
            &Session {
                roots: vec![root.clone(), missing.clone(), root.clone()],
                ..Default::default()
            },
        );
        let mut app = Notes::restore(Some(&storage), egui::Context::default());
        assert_eq!(app.notebooks, vec![root.clone(), missing.clone()]);
        assert_eq!(app.roots, vec![root.clone()]);
        app.activate_notebook(missing.clone());
        assert_eq!(app.roots, vec![root.clone()]);
        assert!(app.status.contains("unavailable"));
        app.remove_folder(&root);
        eframe::App::save(&mut app, &mut storage);
        let restored = Notes::restore(Some(&storage), egui::Context::default());
        assert_eq!(restored.notebooks, vec![missing]);
        assert!(restored.roots.is_empty());
    }

    #[test]
    fn session_restores_preferences_and_last_document_from_disk() {
        let root = std::env::temp_dir().join(format!("rust-note-session-{}", std::process::id()));
        fs::create_dir_all(root.join("nested")).unwrap();
        let file = root.join("nested/note.md");
        fs::write(&file, "Saved content").unwrap();
        let mut app = Notes {
            roots: vec![root.clone()],
            file: Some(file.clone()),
            view: View::Live,
            fonts: FontSettings {
                ui_size: 20.0,
                content_size: 26.0,
            },
            text: "Unsaved draft".into(),
            autosave_enabled: true,
            ..Default::default()
        };
        let mut storage = MemoryStorage::default();
        eframe::App::save(&mut app, &mut storage);
        let restored = Notes::restore(Some(&storage), egui::Context::default());
        assert_eq!(restored.roots, vec![root.clone()]);
        assert_eq!(restored.file, Some(file.clone()));
        assert_eq!(restored.explorer.selected, Some(file));
        assert!(restored.view == View::Live);
        assert_eq!(restored.fonts.ui_size, 20.0);
        assert_eq!(restored.fonts.content_size, 26.0);
        assert!(restored.autosave_enabled);
        assert_eq!(restored.text, "Saved content");
        assert!(!restored.dirty());
        fs::remove_dir_all(&root).unwrap();
        let restored = Notes::restore(Some(&storage), egui::Context::default());
        assert!(restored.roots.is_empty());
        assert_eq!(restored.notebooks, vec![root.clone()]);
        assert!(restored.status.contains("unavailable"));
        storage.set_string(SESSION_KEY, "invalid data".into());
        assert_eq!(
            Notes::restore(Some(&storage), egui::Context::default())
                .fonts
                .ui_size,
            14.0
        );
    }

    #[test]
    fn autosave_waits_for_idle_period_then_writes_changes() {
        let root = std::env::temp_dir().join(format!("rust-note-autosave-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("note.md");
        fs::write(&file, "Saved").unwrap();
        let mut app = Notes::default();
        app.open_file(file.clone());
        app.autosave_enabled = true;
        app.text = "Edited".into();
        let edited_at = Instant::now();
        app.last_edit_at = Some(edited_at);

        assert_eq!(
            app.autosave_if_due_at(edited_at + AUTOSAVE_DELAY - Duration::from_millis(1)),
            Some(Duration::from_millis(1))
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), "Saved");
        assert!(app.dirty());

        assert_eq!(app.autosave_if_due_at(edited_at + AUTOSAVE_DELAY), None);
        assert_eq!(fs::read_to_string(&file).unwrap(), "Edited");
        assert!(!app.dirty());
        assert_eq!(app.status, "Autosaved");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn autosave_flushes_before_switching_notes() {
        let root =
            std::env::temp_dir().join(format!("rust-note-autosave-switch-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("first.md");
        let second = root.join("second.md");
        fs::write(&first, "First").unwrap();
        fs::write(&second, "Second").unwrap();
        let mut app = Notes::default();
        app.open_file(first.clone());
        app.autosave_enabled = true;
        app.text = "Edited first".into();
        app.last_edit_at = Some(Instant::now());

        app.open_file(second.clone());

        assert_eq!(fs::read_to_string(&first).unwrap(), "Edited first");
        assert_eq!(app.file, Some(second.clone()));
        assert_eq!(app.text, "Second");
        assert!(!app.dirty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_autosave_keeps_draft_dirty_and_retries_later() {
        let now = Instant::now();
        let mut app = Notes {
            file: Some(std::env::temp_dir().join(format!(
                "rust-note-autosave-missing-{}/note.md",
                std::process::id()
            ))),
            text: "Draft".into(),
            saved: "Saved".into(),
            autosave_enabled: true,
            last_edit_at: Some(now),
            ..Default::default()
        };

        assert_eq!(
            app.autosave_if_due_at(now + AUTOSAVE_DELAY),
            Some(AUTOSAVE_RETRY_DELAY)
        );
        assert!(app.dirty());
        assert!(app.status.starts_with("Autosave failed:"));
    }

    #[test]
    fn create_subfolder_rejects_collisions_and_invalid_paths() {
        let root =
            std::env::temp_dir().join(format!("rust-note-folder-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let folder = create_folder(&root, "Projects").unwrap();
        assert!(folder.is_dir());
        assert!(create_folder(&folder, "Nested").unwrap().is_dir());
        assert!(create_folder(&root, "Projects").is_err());
        fs::write(root.join("existing"), "Preserve me").unwrap();
        assert!(create_folder(&root, "existing").is_err());
        assert_eq!(
            fs::read_to_string(root.join("existing")).unwrap(),
            "Preserve me"
        );
        for invalid in ["", "  ", "..", "../escape", "a/b", "a\\b", "C:folder"] {
            assert!(create_folder(&root, invalid).is_err());
        }
        assert!(create_folder(&root.join("missing"), "child").is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn create_note_in_subfolder_without_overwriting_existing_files() {
        let root =
            std::env::temp_dir().join(format!("rust-note-create-test-{}", std::process::id()));
        let folder = root.join("nested");
        fs::create_dir_all(&folder).unwrap();
        let path = create_note(&folder, "My note").unwrap();
        assert_eq!(path, folder.join("My note.md"));
        fs::write(&path, "Keep this content").unwrap();
        assert!(create_note(&folder, "My note.md").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "Keep this content");
        for invalid in [
            "",
            "  ",
            ".",
            "..",
            "../escape",
            "sub/note",
            "sub\\note",
            "C:note",
            "note.",
        ] {
            assert!(create_note(&folder, invalid).is_err(), "{invalid}");
        }
        assert!(create_note(&root.join("missing"), "note").is_err());
        assert!(create_note(&folder, "Explicit.markdown").unwrap().is_file());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn browse_nested_folder_and_save_note() {
        let root = std::env::temp_dir().join(format!("rust-note-test-{}", std::process::id()));
        fs::create_dir_all(root.join("nested/deeper")).unwrap();
        fs::write(root.join("z.md"), "# Original").unwrap();
        fs::write(root.join("nested/deeper/note.md"), "# Deep note").unwrap();
        let items = entries(&root).unwrap();
        assert!(items[0].directory);
        assert_eq!(
            name(&entries(&root.join("nested/deeper")).unwrap()[0].path),
            "note.md"
        );
        let mut app = Notes::default();
        app.open_file(root.join("z.md"));
        assert_eq!(app.text, "# Original");
        app.text.push_str("\nEdited");
        assert!(app.dirty());
        assert!(app.save());
        assert!(!app.dirty());
        assert_eq!(
            fs::read_to_string(root.join("z.md")).unwrap(),
            "# Original\nEdited"
        );
        app.open_file(root.join("missing.md"));
        assert_eq!(app.text, "# Original\nEdited");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn go_to_file_finds_files_in_nested_notebook_folders() {
        let root =
            std::env::temp_dir().join(format!("rust-note-go-to-file-{}", std::process::id()));
        fs::create_dir_all(root.join("folder/nested")).unwrap();
        fs::write(root.join("folder/a.md"), "A").unwrap();
        fs::write(root.join("folder/nested/deep.md"), "Deep").unwrap();
        fs::write(root.join("root.md"), "Root").unwrap();

        let (files, errors) = notebook_files(std::slice::from_ref(&root));

        assert!(errors.is_empty());
        let labels = files
            .iter()
            .map(|path| notebook_file_label(std::slice::from_ref(&root), path))
            .collect::<Vec<_>>();
        let expected = [
            root.join("folder").join("a.md"),
            root.join("folder").join("nested").join("deep.md"),
            root.join("root.md"),
        ]
        .map(|path| notebook_file_label(std::slice::from_ref(&root), &path));
        assert_eq!(labels, expected);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn go_to_file_navigation_stays_within_matching_files() {
        let mut picker = GoToFile::default();

        picker.navigate(3, egui::Key::ArrowDown);
        assert_eq!(picker.selected, 1);
        picker.navigate(3, egui::Key::ArrowDown);
        picker.navigate(3, egui::Key::ArrowDown);
        assert_eq!(picker.selected, 2);
        picker.navigate(3, egui::Key::ArrowUp);
        assert_eq!(picker.selected, 1);
        picker.navigate(3, egui::Key::ArrowUp);
        picker.navigate(3, egui::Key::ArrowUp);
        assert_eq!(picker.selected, 0);

        picker.navigate(0, egui::Key::ArrowDown);
        assert_eq!(picker.selected, 0);
    }
}
