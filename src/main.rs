#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod explorer;
mod live;
use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::{
    fs, io,
    path::{Path, PathBuf},
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

struct NewNote {
    is_folder: bool,
    folder: PathBuf,
    filename: String,
    error: String,
    focus: bool,
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
}

const SESSION_KEY: &str = "rust-note-session-v1";

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
    settings_open: bool,
    notebooks_open: bool,
    fonts: FontSettings,
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
            ui.label("Changes apply immediately and are remembered next time.");
            ui.horizontal(|ui| {
                if ui.button("Reset defaults").clicked() {
                    self.fonts = FontSettings::default();
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

    fn save(&mut self) -> bool {
        let Some(path) = &self.file else {
            return true;
        };
        match fs::write(path, &self.text) {
            Ok(()) => {
                self.saved.clone_from(&self.text);
                self.status = "Saved".into();
                true
            }
            Err(error) => {
                self.status = format!("Could not save: {error}");
                false
            }
        }
    }

    fn may_leave(&mut self) -> bool {
        if !self.dirty() {
            return true;
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

    fn open_file(&mut self, path: PathBuf) {
        if self.file.as_ref() == Some(&path) {
            return;
        }
        // Read first so an unreadable file never replaces the current document.
        match fs::read_to_string(&path) {
            Ok(text) => {
                if self.may_leave() {
                    self.saved = text.clone();
                    self.text = text;
                    self.file = Some(path);
                    self.live_editor = live::LiveEditor::default();
                    self.status.clear();
                }
            }
            Err(error) => {
                self.status = format!("Cannot open {} as UTF-8 text: {error}", name(&path))
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
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
        {
            self.save();
        }
        if self.new_note.is_none()
            && !self.settings_open
            && !self.notebooks_open
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O))
        {
            self.notebooks_open = true;
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
                    if ui.button("Manage Notebooks    Ctrl+O").clicked() {
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
                        self.new_note.is_none() && !self.settings_open && !self.notebooks_open,
                    );
                    clicked = actions.open_file;
                    new_note = actions.new_entry;
                }
            });
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
                            CommonMarkViewer::new().show(ui, &mut self.markdown_cache, &self.text);
                        } else if self.view == View::Live {
                            self.live_editor.show(ui, &mut self.text);
                        } else {
                            ui.add_sized(
                                [ui.available_width(), ui.available_height()],
                                egui::TextEdit::multiline(&mut self.text)
                                    .font(egui::TextStyle::Monospace)
                                    .code_editor()
                                    .desired_width(f32::INFINITY)
                                    .frame(false),
                            );
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
}
