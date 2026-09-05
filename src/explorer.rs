use crate::{entries, name};
use eframe::egui;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

struct Row {
    path: PathBuf,
    directory: bool,
    depth: usize,
    error: Option<String>,
}

fn visible(path: &Path, depth: usize, expanded: &HashSet<PathBuf>, rows: &mut Vec<Row>) {
    let index = rows.len();
    rows.push(Row {
        path: path.to_owned(),
        directory: true,
        depth,
        error: None,
    });
    if expanded.contains(path) {
        match entries(path) {
            Ok(children) => {
                for child in children {
                    if child.directory {
                        visible(&child.path, depth + 1, expanded, rows);
                    } else {
                        rows.push(Row {
                            path: child.path,
                            directory: false,
                            depth: depth + 1,
                            error: None,
                        });
                    }
                }
            }
            Err(error) => rows[index].error = Some(error.to_string()),
        }
    }
}

#[derive(Default)]
pub struct Explorer {
    pub selected: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    focused: bool,
}

impl Explorer {
    fn navigate(&mut self, rows: &[Row], key: egui::Key) -> Option<PathBuf> {
        let index = rows
            .iter()
            .position(|r| Some(&r.path) == self.selected.as_ref());
        let mut target = index.unwrap_or(0);
        let row = &rows[target];
        match key {
            egui::Key::ArrowDown if index.is_some() => target = (target + 1).min(rows.len() - 1),
            egui::Key::ArrowUp => target = target.saturating_sub(1),
            egui::Key::ArrowRight if row.directory => {
                if !self.expanded.insert(row.path.clone())
                    && target + 1 < rows.len()
                    && rows[target + 1].depth > row.depth
                {
                    target += 1;
                }
            }
            egui::Key::ArrowLeft if !row.directory || !self.expanded.remove(&row.path) => {
                if let Some(parent) = rows[..target].iter().rposition(|r| r.depth < row.depth) {
                    target = parent;
                }
            }
            _ => {}
        }
        self.selected = Some(rows[target].path.clone());
        (!rows[target].directory).then(|| rows[target].path.clone())
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        root: &Path,
        enabled: bool,
    ) -> (Option<PathBuf>, Option<(PathBuf, bool)>) {
        let mut rows = Vec::new();
        visible(root, 0, &self.expanded, &mut rows);
        if ui.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !ui.max_rect().contains(p))
        }) {
            self.focused = false;
        }
        let mut clicked = None;
        let mut new_entry = None;
        let mut scroll = false;
        if self.focused && enabled {
            for key in [
                egui::Key::ArrowUp,
                egui::Key::ArrowDown,
                egui::Key::ArrowLeft,
                egui::Key::ArrowRight,
            ] {
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
                    clicked = self.navigate(&rows, key);
                    rows.clear();
                    visible(root, 0, &self.expanded, &mut rows);
                    scroll = true;
                }
            }
        }
        egui::ScrollArea::both().show(ui, |ui| {
            for row in &rows {
                ui.horizontal(|ui| {
                    ui.add_space(row.depth as f32 * 16.0);
                    let arrow = row.directory.then(|| {
                        let size = ui.text_style_height(&egui::TextStyle::Body);
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
                        let center = rect.center();
                        let radius = size * 0.28;
                        let points = if self.expanded.contains(&row.path) {
                            vec![
                                center + egui::vec2(-radius, -radius * 0.5),
                                center + egui::vec2(radius, -radius * 0.5),
                                center + egui::vec2(0.0, radius),
                            ]
                        } else {
                            vec![
                                center + egui::vec2(-radius * 0.5, -radius),
                                center + egui::vec2(radius, 0.0),
                                center + egui::vec2(-radius * 0.5, radius),
                            ]
                        };
                        ui.painter().add(egui::Shape::convex_polygon(
                            points,
                            ui.visuals().text_color(),
                            egui::Stroke::NONE,
                        ));
                        response
                    });
                    let mut response = ui.selectable_label(
                        self.selected.as_ref() == Some(&row.path),
                        name(&row.path),
                    );
                    if let Some(arrow) = arrow {
                        response = response.union(arrow);
                    }
                    if scroll && self.selected.as_ref() == Some(&row.path) {
                        response.scroll_to_me(None);
                    }
                    if enabled && (response.clicked() || response.secondary_clicked()) {
                        self.focused = true;
                        ui.memory_mut(|m| m.request_focus(ui.id().with("explorer_keyboard")));
                        self.selected = Some(row.path.clone());
                        if row.directory {
                            if response.clicked() && !self.expanded.remove(&row.path) {
                                self.expanded.insert(row.path.clone());
                            }
                        } else {
                            clicked = Some(row.path.clone());
                        }
                    }
                    if row.directory {
                        response.context_menu(|ui| {
                            for (label, folder) in [("New Note…", false), ("New Folder…", true)]
                            {
                                if ui.button(label).clicked() {
                                    new_entry = Some((row.path.clone(), folder));
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                });
                if let Some(error) = &row.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
            }
        });
        (clicked, new_entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_follows_visible_order_and_folder_hierarchy() {
        let root = std::env::temp_dir().join(format!("rust-note-nav-{}", std::process::id()));
        std::fs::create_dir_all(root.join("folder/nested")).unwrap();
        std::fs::write(root.join("folder/a.md"), "A").unwrap();
        std::fs::write(root.join("b.md"), "B").unwrap();
        let mut explorer = Explorer {
            selected: Some(root.clone()),
            ..Default::default()
        };
        let navigate = |ex: &mut Explorer, key| {
            let mut rows = Vec::new();
            visible(&root, 0, &ex.expanded, &mut rows);
            ex.navigate(&rows, key)
        };
        navigate(&mut explorer, egui::Key::ArrowRight);
        navigate(&mut explorer, egui::Key::ArrowDown);
        assert_eq!(explorer.selected, Some(root.join("folder")));
        navigate(&mut explorer, egui::Key::ArrowRight);
        navigate(&mut explorer, egui::Key::ArrowDown);
        assert_eq!(explorer.selected, Some(root.join("folder/nested")));
        assert_eq!(
            navigate(&mut explorer, egui::Key::ArrowDown),
            Some(root.join("folder/a.md"))
        );
        navigate(&mut explorer, egui::Key::ArrowLeft);
        assert_eq!(explorer.selected, Some(root.join("folder")));
        navigate(&mut explorer, egui::Key::ArrowLeft);
        assert_eq!(
            navigate(&mut explorer, egui::Key::ArrowDown),
            Some(root.join("b.md"))
        );
        assert_eq!(
            navigate(&mut explorer, egui::Key::ArrowDown),
            Some(root.join("b.md"))
        );
        navigate(&mut explorer, egui::Key::ArrowUp);
        assert_eq!(explorer.selected, Some(root.join("folder")));
        std::fs::remove_dir_all(root).unwrap();
    }
}
