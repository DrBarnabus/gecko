use std::sync::Arc;

use dear_imgui_rs::{ListClipper, StyleColor, Ui};
use gecko_core::diagnostics::{LogBuffer, LogEntry};
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
        }
    }

    fn matches(&self, entry: &LogEntry) -> bool {
        if !self.show_level[level_slot(entry.level)] {
            return false;
        }

        self.filter_lower.is_empty() || entry.filter_key.contains(&self.filter_lower)
    }

    #[tracing::instrument(skip_all)]
    pub fn render(&mut self, ui: &mut Ui) -> Option<String> {
        let mut copy_request: Option<String> = None;

        let buffer = self.buffer.clone();

        buffer.read(|inner| {
            ui.window("Console").build(|| {
                ui.set_next_item_width(240.0);
                let mut filter_changed = ui.input_text("Filter", &mut self.filter_text).build();

                let labels = ["Error", "Warn", "Info", "Debug", "Trace"];
                for (i, label) in labels.iter().enumerate() {
                    ui.same_line();

                    let mut v = self.show_level[i];
                    if ui.checkbox(label, &mut v) {
                        self.show_level[i] = v;
                        filter_changed = true;
                    }
                }

                ui.same_line();
                ui.checkbox("Auto-scroll", &mut self.autoscroll);

                ui.same_line();
                if ui.button("Clear") {
                    self.min_seq = inner.next_seq();
                    self.filtered.clear();
                    self.scanned_to = self.min_seq;
                }

                ui.separator();

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

                ui.child_window("ConsoleLines").size([0.0, 0.0]).build(ui, || {
                    let stick = self.autoscroll && ui.scroll_y() >= ui.scroll_max_y();

                    let clipper = ListClipper::new(self.filtered.len()).begin(ui).iter();
                    for i in clipper {
                        let seq = self.filtered[i];
                        let Some(entry) = inner.get(seq) else { continue };

                        let line = format!("{}##{}", entry.display, seq);
                        let color = ui.push_style_color(StyleColor::Text, level_color(entry.level));
                        ui.selectable_config(line).selected(false).build();
                        color.pop();

                        if let Some(_ctx) = ui.begin_popup_context_item()
                            && ui.menu_item("Copy Line")
                        {
                            copy_request = Some(entry.display.clone());
                        }
                    }

                    if stick {
                        ui.set_scroll_here_y(1.0);
                    }
                });
            });
        });

        copy_request
    }
}
