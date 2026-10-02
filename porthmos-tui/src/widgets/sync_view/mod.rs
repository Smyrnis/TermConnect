use std::sync::Arc;

use chrono::{Local, TimeZone};
use porthmos_core::{
    glob_match,
    sync::{SyncAction, SyncFacts, SyncItem, SyncPlan, SyncReason},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::widgets::{
    file_list::{format_size, truncate_name},
    filter_line::{self, FilterLine},
    history_view::printable,
};

#[derive(Default)]
pub struct SyncView {
    plan: Option<Arc<SyncPlan>>,
    ticked: Vec<bool>,
    actions: Vec<SyncAction>,
    visible: Vec<usize>,
    filter: Option<String>,
    editing_filter: bool,
    cursor: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub ticked: usize,
    pub uploads: usize,
    pub upload_bytes: u64,
    pub downloads: usize,
    pub download_bytes: u64,
    pub skipped: usize,
}

pub fn reason_text(reason: SyncReason) -> &'static str {
    match reason {
        SyncReason::OnlyLocal => "new locally",
        SyncReason::OnlyRemote => "new on remote",
        SyncReason::LocalNewer => "local is newer",
        SyncReason::RemoteNewer => "remote is newer",
        SyncReason::TargetNewer => "target is newer",
        SyncReason::SameTimeDifferentSize => "same time, other size",
        SyncReason::SizeDiffers => "size differs",
        SyncReason::SizeDiffersTimeUnknown => "size differs, no time",
        SyncReason::KindMismatch => "file vs folder",
    }
}

fn matches(item: &SyncItem, filter: &str) -> bool {
    let filter = filter.trim();
    if filter.is_empty() {
        return true;
    }
    let path = item.path.to_string_lossy();
    let reason = reason_text(item.reason);
    if filter.contains(['*', '?']) {
        glob_match(filter, &path) || glob_match(filter, reason)
    } else {
        let needle = filter.to_lowercase();
        path.to_lowercase().contains(&needle) || reason.starts_with(&needle) || reason.contains(&format!(" {needle}"))
    }
}

impl SyncView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load(&mut self, plan: Arc<SyncPlan>) {
        self.ticked = plan.items.iter().map(|item| item.ticked).collect();
        self.actions = plan.items.iter().map(|item| item.action).collect();
        self.plan = Some(plan);
        self.filter = None;
        self.editing_filter = false;
        self.cursor = 0;
        self.refilter(None);
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn has_plan(&self) -> bool {
        self.plan.is_some()
    }

    pub fn sync_id(&self) -> Option<u64> {
        self.plan.as_ref().map(|plan| plan.sync_id)
    }

    pub fn plan(&self) -> Option<&SyncPlan> {
        self.plan.as_deref()
    }

    pub fn total(&self) -> usize {
        self.ticked.len()
    }

    pub fn matched(&self) -> usize {
        self.visible.len()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    fn current(&self) -> Option<usize> {
        self.visible.get(self.cursor).copied()
    }

    pub fn toggle(&mut self) {
        let Some(index) = self.current() else {
            return;
        };
        if self.ticked[index] {
            self.ticked[index] = false;
        } else if self.actions[index] != SyncAction::Skip {
            self.ticked[index] = true;
        } else {
            self.flip_at(index);
        }
    }

    pub fn flip(&mut self) {
        if let Some(index) = self.current() {
            self.flip_at(index);
        }
    }

    fn flip_at(&mut self, index: usize) {
        let Some(plan) = self.plan.as_ref() else {
            return;
        };
        let item = &plan.items[index];
        if !item.flippable {
            return;
        }
        let next = item.action_after(self.actions[index], plan.options.direction);
        self.actions[index] = next;
        self.ticked[index] = next != SyncAction::Skip;
    }

    pub fn tick_all(&mut self) {
        let Some(plan) = self.plan.as_ref() else {
            return;
        };
        for index in self.visible.iter().copied() {
            let default_action = plan.items[index].action;
            if default_action != SyncAction::Skip {
                self.actions[index] = default_action;
                self.ticked[index] = true;
            }
        }
    }

    pub fn untick_all(&mut self) {
        for index in self.visible.clone() {
            self.ticked[index] = false;
        }
    }

    #[cfg(test)]
    pub fn choices(&self) -> Vec<(u32, SyncAction)> {
        let Some(plan) = self.plan.as_ref() else {
            return Vec::new();
        };
        plan.items
            .iter()
            .enumerate()
            .filter(|(index, _)| self.ticked[*index] && self.actions[*index] != SyncAction::Skip)
            .map(|(index, item)| (item.id, self.actions[index]))
            .collect()
    }

    pub fn visible_choices(&self) -> Vec<(u32, SyncAction)> {
        let Some(plan) = self.plan.as_ref() else {
            return Vec::new();
        };
        self.visible
            .iter()
            .copied()
            .filter(|index| self.ticked[*index] && self.actions[*index] != SyncAction::Skip)
            .map(|index| (plan.items[index].id, self.actions[index]))
            .collect()
    }

    pub fn summary(&self) -> Summary {
        let mut summary = Summary::default();
        let Some(plan) = self.plan.as_ref() else {
            return summary;
        };
        for (index, item) in plan.items.iter().enumerate() {
            let action = self.actions[index];
            if !self.ticked[index] || action == SyncAction::Skip {
                summary.skipped += 1;
                continue;
            }
            summary.ticked += 1;
            match action {
                SyncAction::Upload => {
                    summary.uploads += 1;
                    summary.upload_bytes += item.local.map_or(0, |facts| facts.size);
                }
                SyncAction::Download => {
                    summary.downloads += 1;
                    summary.download_bytes += item.remote.map_or(0, |facts| facts.size);
                }
                SyncAction::Skip => {}
            }
        }
        summary
    }

    fn refilter(&mut self, keep: Option<usize>) {
        let filter = self.filter.clone();
        self.visible = match self.plan.as_ref() {
            Some(plan) => plan
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| filter.as_deref().is_none_or(|text| matches(item, text)))
                .map(|(index, _)| index)
                .collect(),
            None => Vec::new(),
        };
        let kept = keep.and_then(|index| self.visible.iter().position(|visible| *visible == index));
        self.cursor = kept.unwrap_or_else(|| self.cursor.min(self.visible.len().saturating_sub(1)));
    }

    fn set_filter(&mut self, text: Option<String>) {
        let keep = self.current();
        self.filter = text.filter(|text| !text.is_empty());
        self.refilter(keep);
    }
}

impl FilterLine for SyncView {
    fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    fn editing_filter(&self) -> bool {
        self.editing_filter
    }

    fn start_filter(&mut self) {
        self.editing_filter = true;
    }

    fn finish_filter(&mut self) {
        self.editing_filter = false;
    }

    fn type_filter(&mut self, character: char) {
        let mut text = self.filter.clone().unwrap_or_default();
        text.push(character);
        self.set_filter(Some(text));
    }

    fn erase_filter(&mut self) {
        let mut text = self.filter.clone().unwrap_or_default();
        text.pop();
        self.set_filter(Some(text));
    }

    fn clear_filter(&mut self) {
        self.editing_filter = false;
        self.set_filter(None);
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() as isize - 1;
        self.cursor = (self.cursor as isize + delta).clamp(0, last) as usize;
    }
}

fn moment(facts: Option<SyncFacts>) -> String {
    facts
        .and_then(|facts| facts.modified)
        .and_then(|seconds| Local.timestamp_opt(i64::try_from(seconds).ok()?, 0).single())
        .map_or_else(|| "\u{2014}".to_string(), |time| time.format("%Y-%m-%d %H:%M").to_string())
}

const MARK_AND_ARROW_WIDTH: usize = 6;
const REASON_WIDTH: usize = 22;
const SIZE_WIDTH: usize = 7;
const TIME_WIDTH: usize = 16;
const TIME_COLUMNS_WIDTH: usize = 2 * (4 + TIME_WIDTH);
const WITHOUT_PATH_OR_TIMES: usize = MARK_AND_ARROW_WIDTH + REASON_WIDTH + 1 + 2 + SIZE_WIDTH;
const MIN_PATH_WIDTH_WITH_TIMES: usize = 12;

#[derive(Clone, Copy)]
struct RowLayout {
    path_width: usize,
    with_times: bool,
}

fn row_layout(inner_width: usize) -> RowLayout {
    let with_times = inner_width >= WITHOUT_PATH_OR_TIMES + TIME_COLUMNS_WIDTH + MIN_PATH_WIDTH_WITH_TIMES;
    let taken = WITHOUT_PATH_OR_TIMES + if with_times { TIME_COLUMNS_WIDTH } else { 0 };
    RowLayout { path_width: inner_width.saturating_sub(taken), with_times }
}

fn fitted_path(path: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let shown = truncate_name(&printable(path), width);
    let padding = width.saturating_sub(UnicodeWidthStr::width(shown.as_str()));
    format!("{shown}{}", " ".repeat(padding))
}

fn row_text(item: &SyncItem, ticked: bool, action: SyncAction, layout: RowLayout) -> String {
    let mark = if ticked { "[x]" } else { "[ ]" };
    let arrow = match action {
        SyncAction::Upload => '\u{2191}',
        SyncAction::Download => '\u{2193}',
        SyncAction::Skip => '\u{b7}',
    };
    let size = match action {
        SyncAction::Download => item.remote.or(item.local),
        _ => item.local.or(item.remote),
    }
    .map_or(0, |facts| facts.size);
    let mut row = format!(
        "{mark} {arrow} {:<REASON_WIDTH$} {}  {:>SIZE_WIDTH$}",
        reason_text(item.reason),
        fitted_path(&item.path.to_string_lossy(), layout.path_width),
        format_size(size, false),
    );
    if layout.with_times {
        row.push_str(&format!("  L {:<TIME_WIDTH$}  R {:<TIME_WIDTH$}", moment(item.local), moment(item.remote)));
    }
    row
}

fn row_style(item: &SyncItem, ticked: bool) -> Style {
    match (item.reason, ticked) {
        (SyncReason::KindMismatch, _) => Style::default().fg(Color::Red),
        (_, true) => Style::default(),
        (SyncReason::TargetNewer | SyncReason::SameTimeDifferentSize | SyncReason::SizeDiffersTimeUnknown, false) => {
            Style::default().fg(Color::Yellow)
        }
        (_, false) => Style::default().add_modifier(Modifier::DIM),
    }
}

fn summary_text(summary: &Summary) -> String {
    format!(
        "{} ticked \u{b7} \u{2191} {} ({}) \u{b7} \u{2193} {} ({}) \u{b7} {} skipped",
        summary.ticked,
        summary.uploads,
        format_size(summary.upload_bytes, false),
        summary.downloads,
        format_size(summary.download_bytes, false),
        summary.skipped,
    )
}

pub fn render_sync(frame: &mut Frame, area: Rect, view: &SyncView, title: &str) {
    let mut block = Block::default().title(format!("{title} ({})", view.total())).borders(Borders::ALL);
    let inner_width = usize::from(area.width.saturating_sub(2));
    let will_run = format!(" \u{b7} {} will run", view.visible_choices().len());
    let status_width = inner_width.saturating_sub(will_run.chars().count());
    let footer =
        match filter_line::status(view.editing_filter(), view.filter(), view.matched(), view.total(), status_width) {
            Some(status) => format!("{status}{will_run}"),
            None => summary_text(&view.summary()),
        };
    block = block.title_bottom(footer);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(plan) = view.plan.as_ref() else {
        return;
    };
    if view.matched() == 0 {
        frame.render_widget(Paragraph::new("No matches"), inner);
        return;
    }
    let layout = row_layout(inner_width);
    let items: Vec<ListItem> = view
        .visible
        .iter()
        .map(|index| {
            let item = &plan.items[*index];
            ListItem::new(row_text(item, view.ticked[*index], view.actions[*index], layout))
                .style(row_style(item, view.ticked[*index]))
        })
        .collect();
    let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default();
    state.select(Some(view.cursor()));
    frame.render_stateful_widget(list, inner, &mut state);
}

#[cfg(test)]
mod tests;
