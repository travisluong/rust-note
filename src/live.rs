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

fn char_index(source: &str, byte: usize) -> usize {
    source[..byte].chars().count()
}

fn cursor_after_task_marker(source: &str, cursor: egui::text::CCursor) -> egui::text::CCursor {
    let byte = source
        .char_indices()
        .nth(cursor.index)
        .map_or(source.len(), |(index, _)| index);
    for marker in task_markers(source) {
        if marker.visual.start <= byte && byte <= marker.visual.end {
            return egui::text::CCursor::new(char_index(source, marker.visual.end));
        }
    }
    cursor
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

fn task_prefix(line: &str) -> Option<String> {
    let marker_end = line.find(']')?;
    let marker = &line[..marker_end + 1];
    let marker_start = marker
        .char_indices()
        .rev()
        .find(|(_, ch)| matches!(ch, '-' | '*' | '+'))
        .map(|(index, _)| index)?;
    let prefix = &line[..marker_start];
    let bullet = &line[marker_start..marker_start + 1];
    let task = &line[marker_start + 1..];
    if !prefix.chars().all(char::is_whitespace)
        || !task.starts_with(" [")
        || !matches!(task.as_bytes().get(2), Some(b' ' | b'x' | b'X'))
        || task.as_bytes().get(3) != Some(&b']')
    {
        return None;
    }
    Some(format!("{prefix}{bullet} [ ] "))
}

fn byte_at_char(source: &str, index: usize) -> usize {
    source
        .char_indices()
        .nth(index)
        .map_or(source.len(), |(byte, _)| byte)
}

pub fn continue_task_list(
    text: &mut String,
    before: &str,
    cursor: &mut egui::text::CCursorRange,
) -> bool {
    if text.bytes().filter(|byte| *byte == b'\n').count()
        <= before.bytes().filter(|byte| *byte == b'\n').count()
    {
        return false;
    }
    let cursor_index = cursor.primary.index;
    let cursor_byte = byte_at_char(text, cursor_index);
    let line_start = text[..cursor_byte].rfind('\n').map_or(0, |byte| byte + 1);
    if line_start == 0 {
        return false;
    }
    let previous_start = text[..line_start - 1]
        .rfind('\n')
        .map_or(0, |byte| byte + 1);
    let previous_line = &text[previous_start..line_start - 1];
    let Some(prefix) = task_prefix(previous_line) else {
        return false;
    };
    let prefix_chars = prefix.chars().count();
    text.insert_str(line_start, &prefix);
    let primary = cursor.primary.index + prefix_chars;
    let secondary = cursor.secondary.index + prefix_chars;
    *cursor = egui::text::CCursorRange::two(
        egui::text::CCursor::new(primary),
        egui::text::CCursor::new(secondary),
    );
    true
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
    let mut task_hidden = vec![false; source.len()];
    for marker in task_markers(source) {
        hidden[marker.visual.clone()].fill(true);
        task_hidden[marker.visual].fill(true);
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
        if hidden[byte]
            && (!active.contains(&byte) || task_hidden[byte])
            && ch != '\n'
            && ch != '\r'
        {
            // Near-zero glyphs hide delimiters without breaking egui's source offsets.
            if task_hidden[byte] {
                // Compact fixed-width glyphs reserve room for the checkbox
                // without changing spacing between [ ], [x], and [X].
                format.font_id = FontId::monospace(size * 0.5);
                format.extra_letter_spacing = 0.0;
            } else {
                format.font_id.size = 0.01;
                format.extra_letter_spacing = 0.0;
            }
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
    pub fn show(&mut self, ui: &mut egui::Ui, text: &mut String) -> bool {
        let id = ui.id().with("continuous_live_editor");
        let before = text.clone();
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
            let char_index = char_index(text, marker.visual.start);
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
        let mut state = output.state;
        if let Some(mut range) = state.cursor.char_range() {
            continue_task_list(text, &before, &mut range);
            state.cursor.set_char_range(Some(range));
        }
        let next = state.cursor.char_range().map(|range| {
            let primary = cursor_after_task_marker(text, range.primary);
            let secondary = cursor_after_task_marker(text, range.secondary);
            if primary != range.primary || secondary != range.secondary {
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(primary, secondary)));
                ui.ctx().request_repaint();
            }
            (primary.index, secondary.index)
        });
        state.store(ui.ctx(), id);
        if next != self.selection || output.response.gained_focus() || output.response.lost_focus()
        {
            self.selection = next;
            ui.ctx().request_repaint();
        }
        text != &before
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

    #[test]
    fn cursor_skips_over_hidden_task_markers() {
        let source = "- [ ] task";
        for index in 0..=5 {
            assert_eq!(
                cursor_after_task_marker(source, egui::text::CCursor::new(index)).index,
                5
            );
        }
        assert_eq!(
            cursor_after_task_marker(source, egui::text::CCursor::new(6)).index,
            6
        );
    }

    #[test]
    fn toggling_task_markers_preserves_text_positions_and_wrapping() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            for prefix in ["-", "  -", "*", "+", "1."] {
                for active in [false, true] {
                    for width in [100.0, 500.0] {
                        let positions = [" ", "x", "X"].map(|state| {
                            let source =
                                format!("{prefix} [{state}] a task with enough words to wrap");
                            let active = if active { 0..source.len() } else { 0..0 };
                            let mut job = layout(&source, 16.0, Color32::WHITE, active);
                            assert_eq!(job.text, source);
                            job.wrap.max_width = width;
                            let galley = ctx.fonts(|fonts| fonts.layout_job(job));
                            let text_start = source.find(']').unwrap() + 2;
                            if prefix == "-" {
                                let text_left = galley
                                    .pos_from_cursor(
                                        &galley.from_ccursor(egui::text::CCursor::new(text_start)),
                                    )
                                    .left();
                                // Leave room for the 18-point checkbox, with
                                // less than 12 points before the task text.
                                assert!((18.0..30.0).contains(&text_left), "{text_left}");
                            }
                            (text_start..=source.chars().count())
                                .map(|index| {
                                    galley.pos_from_cursor(
                                        &galley.from_ccursor(egui::text::CCursor::new(index)),
                                    )
                                })
                                .collect::<Vec<_>>()
                        });
                        assert_eq!(positions[0], positions[1]);
                        assert_eq!(positions[0], positions[2]);
                    }
                }
            }
        });
    }

    #[test]
    fn enter_continues_task_list_with_matching_indentation_and_bullet() {
        let before = "  * [x] first";
        let mut text = "  * [x] first\nsecond".to_owned();
        let mut cursor = egui::text::CCursorRange::two(
            egui::text::CCursor::new(15),
            egui::text::CCursor::new(15),
        );

        assert!(continue_task_list(&mut text, before, &mut cursor));
        assert_eq!(text, "  * [x] first\n  * [ ] second");
        assert_eq!(cursor.primary.index, 23);
    }

    #[test]
    fn enter_does_not_continue_non_task_lines() {
        let before = "plain";
        let mut text = "plain\ntext".to_owned();
        let mut cursor =
            egui::text::CCursorRange::two(egui::text::CCursor::new(6), egui::text::CCursor::new(6));

        assert!(!continue_task_list(&mut text, before, &mut cursor));
        assert_eq!(text, "plain\ntext");
    }
}
