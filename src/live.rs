use eframe::egui::{self, Color32, FontId, TextFormat, text::LayoutJob};
use pulldown_cmark::{Event, Options, Parser, Tag};
use std::ops::Range;

#[derive(Default)]
pub struct LiveEditor {
    selection: Option<(usize, usize)>,
    parsed: Parsed,
}

#[derive(Default)]
struct Parsed {
    source: String,
    events: Vec<(Event<'static>, Range<usize>)>,
    tasks: Vec<TaskMarker>,
    layout: Option<(f32, Color32, Range<usize>, LayoutJob)>,
}

impl Parsed {
    fn update(&mut self, source: &str) {
        if self.source == source {
            return;
        }
        self.layout = None;
        self.source.clear();
        self.source.push_str(source);
        let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
        self.events = Parser::new_ext(source, options)
            .into_offset_iter()
            .map(|(event, range)| (event.into_static(), range))
            .collect();
        self.tasks = task_markers(source, &self.events);
    }
}

struct TaskMarker {
    source: Range<usize>,
    visual: Range<usize>,
    chars: Range<usize>,
    checked: bool,
}

fn cursor_after_task_marker(
    tasks: &[TaskMarker],
    cursor: egui::text::CCursor,
) -> egui::text::CCursor {
    for marker in tasks {
        if marker.chars.start <= cursor.index && cursor.index <= marker.chars.end {
            return egui::text::CCursor::new(marker.chars.end);
        }
    }
    cursor
}

fn task_markers(source: &str, events: &[(Event<'_>, Range<usize>)]) -> Vec<TaskMarker> {
    let mut previous_byte = 0;
    let mut previous_char = 0;
    events
        .iter()
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
            let start = previous_char + source[previous_byte..dash].chars().count();
            let end = start + source[dash..range.end].chars().count();
            previous_byte = range.end;
            previous_char = end;
            Some(TaskMarker {
                source: range.clone(),
                visual: dash..range.end,
                chars: start..end,
                checked: *checked,
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
#[cfg(test)]
fn layout(source: &str, size: f32, color: Color32, active: Range<usize>) -> LayoutJob {
    let mut parsed = Parsed::default();
    parsed.update(source);
    parsed.layout(size, color, active)
}

impl Parsed {
    fn layout(&mut self, size: f32, color: Color32, active: Range<usize>) -> LayoutJob {
        if let Some((cached_size, cached_color, cached_active, job)) = &self.layout
            && *cached_size == size
            && *cached_color == color
            && *cached_active == active
        {
            return job.clone();
        }
        let job = self.build_layout(size, color, active.clone());
        self.layout = Some((size, color, active, job.clone()));
        job
    }

    fn build_layout(&self, size: f32, color: Color32, active: Range<usize>) -> LayoutJob {
        let source = &self.source;
        let base = TextFormat {
            font_id: FontId::proportional(size),
            color,
            ..Default::default()
        };
        let mut boundaries = vec![0, source.len()];
        for (_, range) in &self.events {
            boundaries.extend([range.start, range.end]);
        }
        for marker in &self.tasks {
            boundaries.extend([marker.visual.start, marker.visual.end]);
        }
        for (event, range) in &self.events {
            if matches!(event, Event::Code(_)) {
                let ticks = source[range.clone()]
                    .bytes()
                    .take_while(|b| *b == b'`')
                    .count();
                boundaries.extend([range.start + ticks, range.end - ticks]);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        let span = |range: Range<usize>| {
            boundaries.binary_search(&range.start).unwrap()
                ..boundaries.binary_search(&range.end).unwrap()
        };
        let mut formats = vec![base; boundaries.len()];
        let mut hidden = vec![false; boundaries.len()];
        let mut task_hidden = vec![false; boundaries.len()];
        for marker in &self.tasks {
            hidden[span(marker.visual.clone())].fill(true);
            task_hidden[span(marker.visual.clone())].fill(true);
        }
        for (event, bytes) in &self.events {
            let range = span(bytes.clone());
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
                                format.font_id.size =
                                    size * (1.9 - (*level as u8 as f32 - 1.0) * 0.15);
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
                    let raw = &source[bytes.clone()];
                    let ticks = raw.bytes().take_while(|b| *b == b'`').count();
                    hidden[span(bytes.start..bytes.start + ticks)].fill(true);
                    hidden[span(bytes.end - ticks..bytes.end)].fill(true);
                    for format in &mut formats[range] {
                        format.font_id = FontId::monospace(size);
                        format.background = Color32::from_gray(35);
                    }
                }
                _ => {}
            }
        }
        let mut job = LayoutJob::default();
        let mut section = 0;
        for (byte, ch) in source.char_indices() {
            while boundaries[section + 1] <= byte {
                section += 1;
            }
            let mut format = formats[section].clone();
            if hidden[section]
                && (!active.contains(&byte) || task_hidden[section])
                && ch != '\n'
                && ch != '\r'
            {
                // Near-zero glyphs hide delimiters without breaking egui's source offsets.
                if task_hidden[section] {
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
            if let Some(last) = job.sections.last_mut()
                && last.format == format
            {
                last.byte_range.end = byte + ch.len_utf8();
            } else {
                job.sections.push(egui::text::LayoutSection {
                    leading_space: 0.0,
                    byte_range: byte..byte + ch.len_utf8(),
                    format,
                });
            }
        }
        job.text.clone_from(source);
        job
    }
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
            self.parsed.update(source);
            let mut job = self
                .parsed
                .layout(size, color, active_lines(source, selection));
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
        self.parsed.update(text);
        for marker in &self.parsed.tasks {
            let char_index = marker.chars.start;
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
            let response = ui
                .put(rect, egui::Checkbox::without_text(&mut checked))
                .on_hover_cursor(egui::CursorIcon::Default);
            if response.clicked() {
                text.replace_range(marker.source.clone(), if checked { "[x]" } else { "[ ]" });
                ui.ctx().request_repaint();
            }
        }
        let mut state = output.state;
        if let Some(mut range) = state.cursor.char_range() {
            continue_task_list(text, &before, &mut range);
            state.cursor.set_char_range(Some(range));
        }
        self.parsed.update(text);
        let next = state.cursor.char_range().map(|range| {
            let primary = cursor_after_task_marker(&self.parsed.tasks, range.primary);
            let secondary = cursor_after_task_marker(&self.parsed.tasks, range.secondary);
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
        assert!(
            inactive
                .sections
                .iter()
                .find(|section| section.byte_range.contains(&2))
                .unwrap()
                .format
                .font_id
                .size
                > 16.0
        );
        let active = layout(source, 16.0, Color32::GRAY, 0..7);
        assert_ne!(active.sections[0].format.color, Color32::TRANSPARENT);
    }

    #[test]
    fn task_markers_hide_the_markdown_prefix_and_preserve_checkbox_state() {
        let mut parsed = Parsed::default();
        parsed.update("- [ ] one\n  - [x] two");
        let markers = parsed.tasks;
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
        let mut parsed = Parsed::default();
        parsed.update(source);
        for index in 0..=5 {
            assert_eq!(
                cursor_after_task_marker(&parsed.tasks, egui::text::CCursor::new(index)).index,
                5
            );
        }
        assert_eq!(
            cursor_after_task_marker(&parsed.tasks, egui::text::CCursor::new(6)).index,
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

    #[test]
    fn large_preview_reuses_parse_and_groups_plain_text() {
        let source = "plain Unicode é 🦀 text ".repeat(10_000);
        let mut parsed = Parsed::default();
        parsed.update(&source);
        let events = parsed.events.as_ptr();
        parsed.update(&source);
        assert_eq!(parsed.events.as_ptr(), events);
        let job = parsed.layout(16.0, Color32::GRAY, 0..0);
        assert_eq!(job.text, source);
        assert_eq!(job.sections.len(), 1);
        assert_eq!(parsed.layout(16.0, Color32::GRAY, 0..0), job);

        parsed.update("**bold**");
        let hidden = parsed.layout(16.0, Color32::GRAY, 0..0);
        let visible = parsed.layout(16.0, Color32::GRAY, 0..8);
        assert_eq!(hidden.sections[0].format.color, Color32::TRANSPARENT);
        assert_ne!(visible.sections[0].format.color, Color32::TRANSPARENT);
        assert_eq!(
            parsed.layout(20.0, Color32::GRAY, 0..8).sections[0]
                .format
                .font_id
                .size,
            20.0
        );
        parsed.update("plain");
        assert_eq!(
            parsed.layout(16.0, Color32::RED, 0..0).sections[0]
                .format
                .color,
            Color32::RED
        );

        parsed.update("é 🦀\n- [ ] first\n- [x] second");
        for marker in &parsed.tasks {
            assert_eq!(
                marker.chars.start,
                parsed.source[..marker.visual.start].chars().count()
            );
            assert_eq!(
                marker.chars.end,
                parsed.source[..marker.visual.end].chars().count()
            );
        }
        assert!(!parsed.tasks[0].checked);
        parsed.update("é 🦀\n- [x] first\n- [x] second");
        assert!(parsed.tasks[0].checked);
        parsed.update("");
        assert!(parsed.tasks.is_empty());
        assert!(parsed.layout(16.0, Color32::GRAY, 0..0).text.is_empty());
    }
}
