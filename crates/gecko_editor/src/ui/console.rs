use std::sync::Arc;

use dear_imgui_rs::{ListClipper, StyleColor, StyleVar, TableColumnFlags, Ui};
use gecko_core::diagnostics::{LogBuffer, LogBufferInner, LogEntry};
use tracing::Level;

fn level_slot(level: Level) -> usize {
    match level {
        Level::ERROR => 0,
        Level::WARN => 1,
        Level::INFO => 2,
        Level::DEBUG => 3,
        Level::TRACE => 4,
    }
}

fn level_color(level: Level) -> [f32; 4] {
    match level {
        Level::ERROR => [1.0, 0.35, 0.35, 1.0],
        Level::WARN => [1.0, 0.80, 0.25, 1.0],
        Level::INFO => [0.65, 0.90, 1.00, 1.0],
        Level::DEBUG => [0.60, 0.60, 0.60, 1.0],
        Level::TRACE => [0.60, 0.60, 0.60, 1.0],
    }
}

pub struct Console {
    buffer: Arc<LogBuffer>,
    filtered: Vec<u64>,
    scanned_to: u64,
    min_seq: u64,
    filter_text: String,
    filter_lower: String,
    show_level: [bool; 5],
    autoscroll: bool,
    context_seq: Option<u64>,
}

impl Console {
    pub fn new(buffer: Arc<LogBuffer>) -> Self {
        Self {
            buffer,
            filtered: Vec::new(),
            scanned_to: 0,
            min_seq: 0,
            filter_text: String::new(),
            filter_lower: String::new(),
            show_level: [true, true, true, true, false], // All but TRACE
            autoscroll: true,
            context_seq: None,
        }
    }

    fn matches(&self, entry: &LogEntry) -> bool {
        if !self.show_level[level_slot(entry.level)] {
            return false;
        }

        self.filter_lower.is_empty() || entry.filter_key.contains(&self.filter_lower)
    }

    fn draw_toolbar(&mut self, ui: &Ui, inner: &LogBufferInner) -> bool {
        let _spacing = ui.push_style_var(StyleVar::ItemSpacing([12.0, 6.0]));

        let labels = ["Error", "Warn", "Info", "Debug", "Trace"];
        let mut filter_changed = false;

        if let Some(_toolbar) = ui.begin_table("ConsoleToolbar", 2) {
            ui.table_setup_column_stretch_weight("filter", TableColumnFlags::empty(), 1.0, None);
            ui.table_setup_column_fixed_width("controls", TableColumnFlags::empty(), 0.0, None);

            ui.table_next_column();
            for (i, label) in labels.iter().enumerate() {
                let mut v = self.show_level[i];
                if ui.checkbox(label, &mut v) {
                    self.show_level[i] = v;
                    filter_changed = true;
                }
                ui.same_line();
            }

            let filter_width = ui.content_region_avail()[0].min(320.0);
            ui.set_next_item_width(filter_width);
            filter_changed |= ui.input_text("##Filter", &mut self.filter_text).hint("Filter").build();

            ui.table_next_column();
            ui.checkbox("Auto-scroll", &mut self.autoscroll);

            ui.same_line();
            if ui.button("Clear") {
                self.min_seq = inner.next_seq();
                self.filtered.clear();
                self.scanned_to = self.min_seq;
            }
        }

        filter_changed
    }

    fn sync_filtered(&mut self, inner: &LogBufferInner, filter_changed: bool) {
        let cutoff = inner.head_seq().max(self.min_seq);
        if filter_changed {
            self.filter_lower = self.filter_text.to_lowercase();
            self.filtered.clear();
            self.scanned_to = cutoff;
        }

        let stale = self
            .filtered
            .iter()
            .position(|&s| s >= cutoff)
            .unwrap_or(self.filtered.len());

        self.filtered.drain(..stale);

        let from = self.scanned_to.max(cutoff);
        let new_matches: Vec<u64> = inner
            .entries_from(from)
            .filter(|e| self.matches(e))
            .map(|e| e.seq)
            .collect();
        self.filtered.extend(new_matches);
        self.scanned_to = inner.next_seq();
    }

    fn draw_lines(&mut self, ui: &Ui, inner: &LogBufferInner) -> Option<String> {
        let mut copy_request: Option<String> = None;

        ui.child_window("ConsoleLines").size([0.0, 0.0]).build(ui, || {
            let hover = ui.push_style_color(StyleColor::HeaderHovered, [1.0, 1.0, 1.0, 0.07]);
            let active = ui.push_style_color(StyleColor::HeaderActive, [1.0, 1.0, 1.0, 0.12]);

            let stick = self.autoscroll && ui.scroll_y() >= ui.scroll_max_y();

            let mut open_context_seq: Option<u64> = None;
            let clipper = ListClipper::new(self.filtered.len()).begin(ui).iter();
            for i in clipper {
                let seq = self.filtered[i];
                let Some(entry) = inner.get(seq) else { continue };

                let line = format!("{}##{}", entry.display, seq);

                let color = ui.push_style_color(StyleColor::Text, level_color(entry.level));
                ui.selectable_config(line)
                    .selected(self.context_seq == Some(seq))
                    .build();
                color.pop();

                if let Some(_ctx) = ui.begin_popup_context_item() {
                    open_context_seq = Some(seq);

                    if ui.menu_item("Copy Line") {
                        copy_request = Some(entry.display.clone());
                    }
                }
            }

            self.context_seq = open_context_seq;

            if stick {
                ui.set_scroll_here_y(1.0);
            }

            active.pop();
            hover.pop();
        });

        copy_request
    }

    #[tracing::instrument(skip_all)]
    pub fn render(&mut self, ui: &mut Ui) -> Option<String> {
        let buffer = self.buffer.clone();

        buffer.read(|inner| {
            ui.window("Console")
                .build(|| {
                    let filter_changed = self.draw_toolbar(ui, inner);

                    ui.separator();

                    self.sync_filtered(inner, filter_changed);

                    self.draw_lines(ui, inner)
                })
                .flatten()
        })
    }
}
