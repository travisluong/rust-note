use eframe::egui::{self, Color32, FontId, TextFormat, text::LayoutJob};
use pulldown_cmark::{Event, Options, Parser, Tag};
use std::ops::Range;

#[derive(Default)]
pub struct LiveEditor {
    selection: Option<(usize, usize)>,
}

struct TaskMarker {
    source: Range<usize>,
    visual: Range<usize>,
    checked: bool,
}

fn task_markers(source: &str) -> Vec<TaskMarker> {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    Parser::new_ext(source, options)
        .into_offset_iter()
        .filter_map(|(event, range)| {
            let Event::TaskListMarker(checked) = event else {
                return None;
            };
            let line_start = source[..range.start]
                .rfind('\n')
                .map_or(0, |index| index + 1);
            let dash = source[line_start..range.start]
                .rfind('-')
                .map_or(range.start, |index| line_start + index);
            Some(TaskMarker {
                source: range.clone(),
                visual: dash..range.end,
                checked,
            })
        })
        .collect()
}

fn active_lines(source: &str, selection: Option<(usize, usize)>) -> Range<usize> {
    let Some((a, b)) = selection else {
        return 0..0;
    };
    let byte = |index| {
        source
            .char_indices()
            .nth(index)
            .map_or(source.len(), |(i, _)| i)
    };
    let start = byte(a.min(b));
    let end = byte(a.max(b));
    let start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let end = source[end..].find('\n').map_or(source.len(), |i| end + i);
    start..end
}

// Layout retains the exact source and character count, so selection, clipboard,
// undo, and cursor movement all operate on the original Markdown document.
fn layout(source: &str, size: f32, color: Color32, active: Range<usize>) -> LayoutJob {
    let base = TextFormat {
        font_id: FontId::proportional(size),
        color,
        ..Default::default()
    };
    let mut formats = vec![base.clone(); source.len()];
    let mut hidden = vec![false; source.len()];
    for marker in task_markers(source) {
        hidden[marker.visual].fill(true);
    }
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                match &tag {
                    Tag::Heading { .. }
                    | Tag::Strong
                    | Tag::Emphasis
                    | Tag::Strikethrough
                    | Tag::Link { .. } => hidden[range.clone()].fill(true),
                    _ => {}
                }
                for format in &mut formats[range] {
                    match &tag {
                        Tag::Heading { level, .. } => {
                            format.font_id.size = size * (1.9 - (*level as u8 as f32 - 1.0) * 0.15);
                            format.color = Color32::WHITE;
                        }
                        Tag::Strong => {
                            format.color = Color32::WHITE;
                            format.extra_letter_spacing = 0.25;
                        }
                        Tag::Emphasis => format.italics = true,
                        Tag::Strikethrough => {
                            format.strikethrough = egui::Stroke::new(1.0_f32, color)
                        }
                        Tag::Link { .. } => {
                            format.color = Color32::from_rgb(120, 180, 250);
                            format.underline = egui::Stroke::new(1.0_f32, format.color);
                        }
                        Tag::CodeBlock(_) => {
                            format.font_id = FontId::monospace(size);
                            format.background = Color32::from_gray(35);
                        }
                        _ => {}
                    }
                }
            }
            Event::Text(_) => hidden[range].fill(false),
            Event::Code(_) => {
                hidden[range.clone()].fill(false);
                let raw = &source[range.clone()];
                let ticks = raw.bytes().take_while(|b| *b == b'`').count();
                hidden[range.start..range.start + ticks].fill(true);
                hidden[range.end - ticks..range.end].fill(true);
                for format in &mut formats[range] {
                    format.font_id = FontId::monospace(size);
                    format.background = Color32::from_gray(35);
                }
            }
            _ => {}
        }
    }
    let mut job = LayoutJob::default();
    for (byte, ch) in source.char_indices() {
        let mut format = formats[byte].clone();
        if hidden[byte] && !active.contains(&byte) && ch != '\n' && ch != '\r' {
            // Near-zero glyphs hide delimiters without breaking egui's source offsets.
            format.font_id.size = 0.01;
            format.extra_letter_spacing = 0.0;
            format.color = Color32::TRANSPARENT;
            format.background = Color32::TRANSPARENT;
            format.underline = egui::Stroke::NONE;
            format.strikethrough = egui::Stroke::NONE;
        }
        job.append(&source[byte..byte + ch.len_utf8()], 0.0, format);
    }
    job
}

impl LiveEditor {
    pub fn show(&mut self, ui: &mut egui::Ui, text: &mut String) {
        let id = ui.id().with("continuous_live_editor");
        let selection = if ui.memory(|m| m.has_focus(id)) {
            self.selection
        } else {
            None
        };
        let size = egui::TextStyle::Body.resolve(ui.style()).size;
        let color = ui.visuals().text_color();
        let mut layouter = |ui: &egui::Ui, source: &str, width: f32| {
            let mut job = layout(source, size, color, active_lines(source, selection));
            job.wrap.max_width = width;
            ui.fonts(|fonts| fonts.layout_job(job))
        };
        let output = egui::TextEdit::multiline(text)
            .id(id)
            .font(egui::TextStyle::Body)
            .frame(false)
            .desired_width(f32::INFINITY)
            .desired_rows(20)
            .hint_text("Write your note…")
            .layouter(&mut layouter)
            .show(ui);

        // Keep the Markdown source editable, but replace task-list markers with
        // native controls positioned over the corresponding rendered text.
        for marker in task_markers(text) {
            let char_index = text[..marker.visual.start].chars().count();
            let cursor = output.galley.pos_from_cursor(
                &output
                    .galley
                    .from_ccursor(egui::text::CCursor::new(char_index)),
            );
            let size = ui.spacing().interact_size.y;
            let rect = egui::Rect::from_min_size(
                output.galley_pos + cursor.min.to_vec2(),
                egui::vec2(size, size),
            );
            let mut checked = marker.checked;
            if ui
                .put(rect, egui::Checkbox::without_text(&mut checked))
                .clicked()
            {
                text.replace_range(marker.source, if checked { "[x]" } else { "[ ]" });
                ui.ctx().request_repaint();
            }
        }
        let next = output
            .cursor_range
            .map(|r| (r.primary.ccursor.index, r.secondary.ccursor.index));
        if next != self.selection || output.response.gained_focus() || output.response.lost_focus()
        {
            self.selection = next;
            ui.ctx().request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuous_editor_lays_out_formatted_text_without_mutating_it() {
        let ctx = egui::Context::default();
        let original = "# Heading\n\n**Bold** and *italic* 🦀\n\n`code`";
        let mut text = original.to_owned();
        let mut editor = LiveEditor::default();
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| editor.show(ui, &mut text));
            });
        }
        assert_eq!(text, original);
    }
    #[test]
    fn layout_preserves_exact_source_and_unicode_cursor_offsets() {
        for source in [
            "",
            "# Héllo\r\n\r\nA **note** 🦀\n",
            "- one\n  - nested\n",
            "```rust\nlet x = 1;\n```\n",
            "[link](https://example.com)",
        ] {
            let job = layout(source, 16.0, Color32::WHITE, 0..0);
            assert_eq!(job.text, source);
            assert_eq!(job.text.chars().count(), source.chars().count());
        }
        assert_eq!(active_lines("é\n**hi**\nend", Some((3, 4))), 3..9);
    }
    #[test]
    fn syntax_reveals_on_active_line_and_headings_are_styled() {
        let source = "# Title\n\n**bold**";
        let inactive = layout(source, 16.0, Color32::GRAY, 0..0);
        assert_eq!(inactive.sections[0].format.color, Color32::TRANSPARENT);
        assert!(inactive.sections[2].format.font_id.size > 16.0);
        let active = layout(source, 16.0, Color32::GRAY, 0..7);
        assert_ne!(active.sections[0].format.color, Color32::TRANSPARENT);
    }

    #[test]
    fn task_markers_hide_the_markdown_prefix_and_preserve_checkbox_state() {
        let markers = task_markers("- [ ] one\n  - [x] two");
        assert_eq!(markers.len(), 2);
        assert_eq!(
            &"- [ ] one\n  - [x] two"[markers[0].visual.clone()],
            "- [ ]"
        );
        assert!(!markers[0].checked);
        assert_eq!(
            &"- [ ] one\n  - [x] two"[markers[1].visual.clone()],
            "- [x]"
        );
        assert!(markers[1].checked);
    }
}
