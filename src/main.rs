#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
            Ok(Box::<Notes>::default())
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

fn tree(
    ui: &mut egui::Ui,
    path: &Path,
    selected: Option<&Path>,
    clicked: &mut Option<PathBuf>,
    new_note: &mut Option<(PathBuf, bool)>,
) {
    egui::CollapsingHeader::new(name(path))
        .id_salt(path)
        .default_open(false)
        .show(ui, |ui| match entries(path) {
            Ok(items) => {
                if items.is_empty() {
                    ui.weak("Empty folder");
                }
                for item in items {
                    if item.directory {
                        tree(ui, &item.path, selected, clicked, new_note);
                    } else if ui
                        .selectable_label(selected == Some(item.path.as_path()), name(&item.path))
                        .on_hover_text(item.path.display().to_string())
                        .clicked()
                    {
                        *clicked = Some(item.path);
                    }
                }
            }
            Err(error) => {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("Cannot read folder: {error}"),
                );
            }
        })
        .header_response
        .context_menu(|ui| {
            if ui.button("New Note…").clicked() {
                *new_note = Some((path.to_path_buf(), false));
                ui.close_menu();
            }
            if ui.button("New Folder…").clicked() {
                *new_note = Some((path.to_path_buf(), true));
                ui.close_menu();
            }
        });
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

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
enum View {
    #[default]
    Markdown,
    Read,
    Live,
}

#[derive(Default)]
struct Notes {
    root: Option<PathBuf>,
    file: Option<PathBuf>,
    text: String,
    saved: String,
    status: String,
    view: View,
    live_editor: live::LiveEditor,
    markdown_cache: CommonMarkCache,
    new_note: Option<NewNote>,
    settings_open: bool,
    fonts: FontSettings,
}

impl Notes {
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
            ui.label("Changes apply immediately for this session.");
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

    fn open_folder(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open notes folder")
            .pick_folder()
            && self.may_leave()
        {
            self.root = Some(path);
            self.file = None;
            self.live_editor = live::LiveEditor::default();
            self.text.clear();
            self.saved.clear();
            self.status = "Click the folder arrow to browse your notes".into();
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
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.may_leave() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        if self.new_note.is_none()
            && !self.settings_open
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
        {
            self.save();
        }
        if self.new_note.is_none()
            && !self.settings_open
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O))
        {
            self.open_folder();
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.strong("Rust Note");
                ui.separator();
                ui.menu_button("File", |ui| {
                    if ui.button("Open Folder…    Ctrl+O").clicked() {
                        ui.close_menu();
                        self.open_folder();
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
                ui.strong("EXPLORER");
                ui.add_space(8.0);
                if let Some(root) = &self.root {
                    egui::ScrollArea::both().show(ui, |ui| {
                        tree(ui, root, self.file.as_deref(), &mut clicked, &mut new_note)
                    });
                } else {
                    ui.weak("Your notes live in a folder.");
                    ui.add_space(8.0);
                    if ui.button("Open Folder…").clicked() {
                        self.open_folder();
                    }
                }
            });
        if let Some(path) = clicked {
            self.open_file(path);
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
                ui.weak(path.display().to_string());
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
                        "Open a folder, expand it, and select a Markdown file to start writing.",
                    );
                    ui.add_space(20.0);
                    if ui.button("Open Folder…").clicked() {
                        self.open_folder();
                    }
                });
            }
        });
        self.new_note_dialog(ctx);
        self.settings_dialog(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
