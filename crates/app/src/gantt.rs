use std::collections::{HashMap, HashSet};

use ado_core::{
    GanttBarKind, IterationHint, IterationNode, WorkItemNode, dates_column_text, gantt_bar_display,
    iteration_hint_for_path, parse_iso_date,
};
use jiff::civil::Date;
use jiff::{ToSpan, Unit};

pub const ROW_HEIGHT: f32 = 40.0;
pub const DEFAULT_TREE_WIDTH: f32 = 480.0;
pub const MIN_TREE_WIDTH: f32 = 240.0;
pub const MIN_TIMELINE_WIDTH: f32 = 240.0;
pub const HARD_MIN_TREE_WIDTH: f32 = 80.0;
pub const SPLITTER_STEP: f32 = 16.0;
pub const HANDLE_WIDTH: f32 = 8.0;
pub const LINK_HANDLE: f32 = 10.0;
pub const NUDGE_IDLE_MS: u64 = 500;
pub const TIMELINE_HEADER_H: f32 = 44.0;
pub const BAR_LABEL_INSIDE_MIN: f32 = 72.0;
pub const TODAY_MARGIN_PX: f32 = 80.0;
pub const PLACEHOLDER_MIN_WIDTH: f32 = 48.0;
pub const MILESTONE_SIZE: f32 = 16.0;
pub const SUMMARY_BAR_H: f32 = 14.0;
pub const PLACEHOLDER_BAR_H: f32 = 24.0;
pub const LEAF_BAR_H: f32 = 28.0;
pub const DATES_COL_WIDTH: f32 = 200.0;
pub const ID_COL_WIDTH: f32 = 64.0;
pub const TYPE_COL_WIDTH: f32 = 88.0;
pub const ASSIGNEE_COL_WIDTH: f32 = 88.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GanttScale {
    Day,
    Week,
    #[default]
    Month,
    Quarter,
    Year,
}

impl GanttScale {
    pub const ALL: [Self; 5] = [
        Self::Day,
        Self::Week,
        Self::Month,
        Self::Quarter,
        Self::Year,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Day => "Day",
            Self::Week => "Week",
            Self::Month => "Month",
            Self::Quarter => "Quarter",
            Self::Year => "Year",
        }
    }

    pub fn pixels_per_day(self, zoom: f32) -> f32 {
        let base = match self {
            Self::Day => 48.0,
            Self::Week => 24.0,
            Self::Month => 12.0,
            Self::Quarter => 4.0,
            Self::Year => 1.5,
        };
        (base * zoom).clamp(0.4, 96.0)
    }

    pub fn step_days(self) -> i32 {
        match self {
            Self::Day => 1,
            Self::Week => 7,
            Self::Month => 30,
            Self::Quarter => 91,
            Self::Year => 365,
        }
    }

    pub fn range_title(self, origin: Date) -> String {
        match self {
            Self::Day => format!("{} {}", month_abbrev(origin), origin.day()),
            Self::Week => format!("Week of {} {}", month_abbrev(origin), origin.day()),
            Self::Month => format!("{} {}", month_abbrev(origin), origin.year()),
            Self::Quarter => {
                let q = ((origin.month() as i32 - 1) / 3) + 1;
                format!("{} Q{q}", origin.year())
            }
            Self::Year => origin.year().to_string(),
        }
    }

    pub fn tick_label(self, date: Date) -> String {
        match self {
            Self::Day => date.day().to_string(),
            Self::Week => format!("{} {}", month_abbrev(date), date.day()),
            Self::Month => month_abbrev(date).to_string(),
            Self::Quarter => {
                let q = ((date.month() as i32 - 1) / 3) + 1;
                format!("Q{q}")
            }
            Self::Year => date.year().to_string(),
        }
    }

    pub fn major_label(self, date: Date) -> String {
        match self {
            Self::Day | Self::Week => format!("{} {}", month_abbrev(date), date.year()),
            Self::Month | Self::Quarter | Self::Year => date.year().to_string(),
        }
    }

    pub fn shows_weekend_bands(self) -> bool {
        matches!(self, Self::Day)
    }

    /// Align `date` to the start of this scale's calendar period.
    pub fn floor_date(self, date: Date) -> Date {
        match self {
            Self::Day => date,
            Self::Week => {
                let offset = match date.weekday() {
                    jiff::civil::Weekday::Monday => 0,
                    jiff::civil::Weekday::Tuesday => 1,
                    jiff::civil::Weekday::Wednesday => 2,
                    jiff::civil::Weekday::Thursday => 3,
                    jiff::civil::Weekday::Friday => 4,
                    jiff::civil::Weekday::Saturday => 5,
                    jiff::civil::Weekday::Sunday => 6,
                };
                date.saturating_sub(offset.days())
            }
            Self::Month => Date::new(date.year(), date.month(), 1).unwrap_or(date),
            Self::Quarter => {
                let month = ((((date.month() as i32) - 1) / 3) * 3 + 1) as i8;
                Date::new(date.year(), month, 1).unwrap_or(date)
            }
            Self::Year => Date::new(date.year(), 1, 1).unwrap_or(date),
        }
    }

    pub fn next_date(self, date: Date) -> Date {
        match self {
            Self::Day => date.saturating_add(1.day()),
            Self::Week => date.saturating_add(7.days()),
            Self::Month => date.saturating_add(1.month()),
            Self::Quarter => date.saturating_add(3.months()),
            Self::Year => date.saturating_add(1.year()),
        }
    }
}

fn month_abbrev(date: Date) -> &'static str {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    MONTHS[(date.month() as u8).saturating_sub(1).min(11) as usize]
}

fn is_weekend(date: Date) -> bool {
    matches!(
        date.weekday(),
        jiff::civil::Weekday::Saturday | jiff::civil::Weekday::Sunday
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureKind {
    Move,
    ResizeStart,
    ResizeEnd,
    Link,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gesture {
    pub kind: GestureKind,
    pub id: i64,
    pub origin_x: f32,
    pub start: Date,
    pub target: Date,
    pub live_start: Date,
    pub live_target: Date,
    pub live_x: f32,
    pub had_start: bool,
    pub had_target: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingNudge {
    pub id: i64,
    pub start: Date,
    pub target: Date,
    pub original_start: Date,
    pub original_target: Date,
    pub original_rev: i64,
    pub had_start: bool,
    pub had_target: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleRow {
    pub id: i64,
    pub depth: u32,
    pub has_children: bool,
    pub expanded: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineTick {
    pub x: f32,
    pub label: String,
    pub major_label: String,
    pub major: bool,
    pub weekend: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BarLayout {
    pub id: i64,
    pub kind: GanttBarKind,
    pub x: f32,
    pub width: f32,
    pub y: f32,
    pub summary: bool,
    pub milestone: bool,
    pub label: String,
    pub title: String,
    pub color: u32,
    pub progress: Option<f32>,
    pub predecessors: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DependencyArrow {
    pub from_id: i64,
    pub to_id: i64,
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

#[derive(Debug, Clone)]
pub struct GanttView {
    pub scale: GanttScale,
    pub zoom: f32,
    pub origin: Date,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub tree_width: f32,
    pub viewport_h: f32,
    pub viewport_w: f32,
    pub timeline_origin_x: f32,
    pub timeline_width: f32,
    pub selected_id: Option<i64>,
    pub hovered_id: Option<i64>,
    pub collapsed: HashSet<i64>,
    pub gesture: Option<Gesture>,
    pub pending_nudge: Option<PendingNudge>,
    pub context_menu: Option<ContextTarget>,
    pub splitter_drag: Option<(f32, f32)>,
    pub schedule_drag: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextTarget {
    pub id: i64,
    pub from_bar: bool,
    pub x: f32,
    pub y: f32,
}

impl Default for GanttView {
    fn default() -> Self {
        Self {
            scale: GanttScale::Month,
            zoom: 1.0,
            origin: jiff::Zoned::now().date(),
            scroll_x: 0.0,
            scroll_y: 0.0,
            tree_width: DEFAULT_TREE_WIDTH,
            viewport_h: 720.0,
            viewport_w: 1280.0,
            timeline_origin_x: 0.0,
            timeline_width: 800.0,
            selected_id: None,
            hovered_id: None,
            collapsed: HashSet::new(),
            gesture: None,
            pending_nudge: None,
            context_menu: None,
            splitter_drag: None,
            schedule_drag: None,
        }
    }
}

impl GanttView {
    pub fn flatten(&self, nodes: &[WorkItemNode]) -> Vec<VisibleRow> {
        flatten_visible(nodes, &self.collapsed)
    }

    pub fn visible_window<'a>(&self, rows: &'a [VisibleRow], viewport_h: f32) -> &'a [VisibleRow] {
        if rows.is_empty() {
            return rows;
        }
        let start = ((self.scroll_y / ROW_HEIGHT).floor() as usize).min(rows.len());
        let count = ((viewport_h / ROW_HEIGHT).ceil() as usize).saturating_add(2);
        let end = (start + count).min(rows.len());
        &rows[start..end]
    }

    pub fn row_scroll_remainder(&self) -> f32 {
        self.scroll_y.rem_euclid(ROW_HEIGHT)
    }

    pub fn row_y(&self, window_offset: usize) -> f32 {
        window_offset as f32 * ROW_HEIGHT - self.row_scroll_remainder()
    }

    pub fn clamp_scroll(&mut self, row_count: usize, viewport_h: f32) {
        let content_h = (viewport_h - TIMELINE_HEADER_H).max(0.0);
        let max_y = (row_count as f32 * ROW_HEIGHT - content_h).max(0.0);
        self.scroll_y = self.scroll_y.clamp(0.0, max_y);
    }

    pub fn toggle_expand(&mut self, id: i64) {
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
        }
    }

    pub fn expand_all_below(&mut self, nodes: &[WorkItemNode], id: i64) {
        for descendant in descendants(nodes, id) {
            self.collapsed.remove(&descendant);
        }
        self.collapsed.remove(&id);
    }

    pub fn collapse_all_below(&mut self, nodes: &[WorkItemNode], id: i64) {
        self.collapsed.insert(id);
        for descendant in descendants(nodes, id) {
            self.collapsed.insert(descendant);
        }
    }

    pub fn zoom_at(&mut self, factor: f32, pointer_x: f32, timeline_origin_x: f32) {
        let old = self.scale.pixels_per_day(self.zoom);
        self.zoom = (self.zoom * factor).clamp(0.4, 4.0);
        let new = self.scale.pixels_per_day(self.zoom);
        if old > 0.0 {
            let world = (self.scroll_x + pointer_x - timeline_origin_x) / old;
            self.scroll_x = (world * new) - (pointer_x - timeline_origin_x);
        }
    }

    pub fn step_period(&mut self, direction: i32) {
        if direction == 0 {
            return;
        }
        let span = match self.scale {
            GanttScale::Day => direction.days(),
            GanttScale::Week => (direction * 7).days(),
            GanttScale::Month => direction.months(),
            GanttScale::Quarter => (direction * 3).months(),
            GanttScale::Year => direction.years(),
        };
        self.origin = self.origin.saturating_add(span);
    }

    /// Apply a wheel/trackpad delta whose signs already match content direction
    /// (positive X reveals later dates, positive Y reveals later rows).
    pub fn apply_wheel(&mut self, dx: f32, dy: f32, shift: bool) {
        if shift {
            self.scroll_x += dy + dx;
        } else {
            self.scroll_x += dx;
            self.scroll_y += dy;
        }
    }

    pub fn go_today(&mut self, today: Date) {
        self.origin = today;
        self.scroll_x = -TODAY_MARGIN_PX;
    }

    pub fn scroll_rows(&mut self, rows: i32, viewport_h: f32, row_count: usize) {
        self.scroll_y += rows as f32 * ROW_HEIGHT;
        self.clamp_scroll(row_count, viewport_h);
    }

    pub fn page_scroll(&mut self, direction: i32, viewport_h: f32, row_count: usize) {
        let page = (viewport_h - TIMELINE_HEADER_H - ROW_HEIGHT).max(ROW_HEIGHT);
        self.scroll_y += page * direction as f32;
        self.clamp_scroll(row_count, viewport_h);
    }

    /// Place the timeline origin on the earliest scheduled or rollup/iteration
    /// bar so loaded work is on-screen instead of sitting off to one side.
    pub fn reveal_content(&mut self, nodes: &[WorkItemNode], iterations: &[IterationNode]) {
        let mut earliest: Option<Date> = None;
        for node in nodes {
            let hint = iteration_hint_for_path(&node.iteration_path, iterations);
            let display = gantt_bar_display(node, nodes, hint.as_ref());
            if let Some(start) = display.start {
                earliest = Some(match earliest {
                    Some(current) if start < current => start,
                    Some(current) => current,
                    None => start,
                });
            }
        }
        if let Some(start) = earliest {
            self.origin = start;
            self.scroll_x = 0.0;
        }
    }

    pub fn max_tree_width(&self) -> f32 {
        (self.viewport_w - 6.0 - MIN_TIMELINE_WIDTH).max(HARD_MIN_TREE_WIDTH)
    }

    pub fn min_tree_width(&self) -> f32 {
        if self.viewport_w >= MIN_TREE_WIDTH + MIN_TIMELINE_WIDTH + 6.0 {
            MIN_TREE_WIDTH
        } else {
            (self.viewport_w - 6.0 - MIN_TIMELINE_WIDTH).max(HARD_MIN_TREE_WIDTH)
        }
    }

    pub fn show_type_column(&self) -> bool {
        self.tree_width >= 560.0
    }

    pub fn show_assignee_column(&self) -> bool {
        self.tree_width >= 480.0
    }

    pub fn show_dates_column(&self) -> bool {
        self.tree_width >= 340.0
    }

    pub fn set_tree_width(&mut self, width: f32) {
        let min = self.min_tree_width();
        let max = self.max_tree_width().max(min);
        self.tree_width = width.clamp(min, max);
        self.timeline_width = (self.viewport_w - self.tree_width - 6.0).max(MIN_TIMELINE_WIDTH);
    }

    /// Clamp against the live pane width so a stale `viewport_w` cannot let the
    /// tree eat the timeline. Do not inflate a narrow pane — the tree shrinks
    /// first so the timeline can keep `MIN_TIMELINE_WIDTH`.
    pub fn set_tree_width_for_pane(&mut self, width: f32, pane_w: f32) {
        self.viewport_w = pane_w.max(1.0);
        self.set_tree_width(width);
    }

    pub fn clamp_tree_width(&mut self) {
        self.set_tree_width(self.tree_width);
    }

    pub fn nudge_tree_width(&mut self, delta: f32) {
        self.set_tree_width(self.tree_width + delta);
    }

    pub fn reset_tree_width(&mut self) {
        self.set_tree_width(DEFAULT_TREE_WIDTH);
    }

    pub fn select_relative(&mut self, rows: &[VisibleRow], delta: i32) {
        if rows.is_empty() {
            return;
        }
        let current = self
            .selected_id
            .and_then(|id| rows.iter().position(|row| row.id == id))
            .unwrap_or(0);
        let next = (current as i32 + delta).clamp(0, rows.len() as i32 - 1) as usize;
        if self
            .pending_nudge
            .as_ref()
            .is_some_and(|pending| pending.id != rows[next].id)
        {
            self.pending_nudge = None;
        }
        self.selected_id = Some(rows[next].id);
        let y = next as f32 * ROW_HEIGHT;
        if y < self.scroll_y {
            self.scroll_y = y;
        } else if y + ROW_HEIGHT > self.scroll_y + self.viewport_h {
            self.scroll_y = (y + ROW_HEIGHT - self.viewport_h).max(0.0);
        }
    }

    pub fn x_for_date(&self, date: Date) -> f32 {
        let days = date
            .since((Unit::Day, self.origin))
            .map(|span| span.get_days())
            .unwrap_or(0);
        days as f32 * self.scale.pixels_per_day(self.zoom) - self.scroll_x
    }

    pub fn date_for_x(&self, x: f32) -> Date {
        let days = ((x + self.scroll_x) / self.scale.pixels_per_day(self.zoom)).round() as i32;
        self.origin.saturating_add(days.days())
    }

    pub fn layout_bars(
        &self,
        nodes: &[WorkItemNode],
        rows: &[VisibleRow],
        iterations: &[IterationNode],
        viewport_h: f32,
    ) -> Vec<BarLayout> {
        let window = self.visible_window(rows, viewport_h);
        let by_id: HashMap<i64, &WorkItemNode> = nodes.iter().map(|node| (node.id, node)).collect();
        window
            .iter()
            .enumerate()
            .filter_map(|(offset, row)| {
                let node = by_id.get(&row.id)?;
                let hint = iteration_hint_for_path(&node.iteration_path, iterations);
                let display = gantt_bar_display(node, nodes, hint.as_ref());
                let (Some(start), Some(target)) = (display.start, display.end) else {
                    return None;
                };
                let mut x = self.x_for_date(start);
                let end = ado_core::to_gantt_inclusive_target(target);
                let mut width = (self.x_for_date(end) - x).max(6.0);
                if display.milestone {
                    width = MILESTONE_SIZE;
                    x -= MILESTONE_SIZE / 2.0;
                } else if display.kind == GanttBarKind::Unscheduled {
                    width = width.max(PLACEHOLDER_MIN_WIDTH);
                }
                Some(BarLayout {
                    id: node.id,
                    kind: display.kind,
                    x,
                    width,
                    y: self.row_y(offset),
                    summary: row.has_children,
                    milestone: display.milestone,
                    label: dates_column_text(node, nodes, hint.as_ref()),
                    title: node.title.clone(),
                    color: parse_type_color(ado_core::color_for_type(&node.work_item_type)),
                    progress: child_completion(nodes, node.id)
                        .or_else(|| Some(state_progress(&node.state))),
                    predecessors: node.predecessors.clone(),
                })
            })
            .collect()
    }

    pub fn begin_gesture(
        &mut self,
        bar: &BarLayout,
        kind: GestureKind,
        origin_x: f32,
        nodes: &[WorkItemNode],
    ) -> Result<(), &'static str> {
        if kind == GestureKind::Link {
            self.selected_id = Some(bar.id);
            let today = jiff::Zoned::now().date();
            self.gesture = Some(Gesture {
                kind,
                id: bar.id,
                origin_x,
                start: today,
                target: today,
                live_start: today,
                live_target: today,
                live_x: origin_x,
                had_start: false,
                had_target: false,
            });
            return Ok(());
        }
        if bar.milestone && kind != GestureKind::Move {
            return Err("Milestones can be moved, not resized.");
        }
        let node = nodes
            .iter()
            .find(|node| node.id == bar.id)
            .ok_or("Work Item not found")?;
        if !node.has_date_fields {
            return Err("This Work Item has no Start Date and Target Date fields.");
        }
        if bar.kind == GanttBarKind::NoFields {
            return Err("This Work Item has no Start Date and Target Date fields.");
        }
        let had_start = parse_iso_date(node.start_date.as_deref()).is_some();
        let had_target = parse_iso_date(node.target_date.as_deref()).is_some();
        let display = gantt_bar_display(node, nodes, None);
        let start = display.start.unwrap_or(self.origin);
        let target = display
            .end
            .unwrap_or_else(|| start.saturating_add((ado_core::DEFAULT_BAR_DAYS - 1).days()));
        if bar.kind == GanttBarKind::Unscheduled && kind != GestureKind::Move {
            return Err("Unscheduled bars can be moved to set dates.");
        }
        self.selected_id = Some(bar.id);
        self.gesture = Some(Gesture {
            kind,
            id: bar.id,
            origin_x,
            start,
            target,
            live_start: start,
            live_target: target,
            live_x: origin_x,
            had_start,
            had_target,
        });
        Ok(())
    }

    pub fn update_gesture(&mut self, x: f32) {
        let Some(gesture) = &mut self.gesture else {
            return;
        };
        if gesture.kind == GestureKind::Link {
            gesture.live_x = x;
            return;
        }
        let delta_days =
            ((x - gesture.origin_x) / self.scale.pixels_per_day(self.zoom)).round() as i32;
        match gesture.kind {
            GestureKind::Move => {
                gesture.live_start = gesture.start.saturating_add(delta_days.days());
                gesture.live_target = gesture.target.saturating_add(delta_days.days());
            }
            GestureKind::Link => {
                gesture.live_x = x;
            }
            GestureKind::ResizeStart => {
                let next = gesture.start.saturating_add(delta_days.days());
                if next <= gesture.target {
                    gesture.live_start = next;
                }
            }
            GestureKind::ResizeEnd => {
                let next = gesture.target.saturating_add(delta_days.days());
                if next >= gesture.start {
                    gesture.live_target = next;
                }
            }
        }
    }

    pub fn cancel_gesture(&mut self) {
        self.gesture = None;
        self.pending_nudge = None;
    }

    pub fn finish_gesture(&mut self) -> Option<(i64, Date, Date)> {
        let gesture = self.gesture.take()?;
        if gesture.kind == GestureKind::Link {
            return None;
        }
        if gesture.live_start == gesture.start && gesture.live_target == gesture.target {
            return None;
        }
        Some((gesture.id, gesture.live_start, gesture.live_target))
    }

    pub fn take_link_gesture(&mut self) -> Option<i64> {
        let gesture = self.gesture.as_ref()?;
        if gesture.kind != GestureKind::Link {
            return None;
        }
        let id = gesture.id;
        self.gesture = None;
        Some(id)
    }

    pub fn gesture_patch_dates(gesture: &Gesture) -> (Option<Date>, Option<Date>) {
        if !gesture.had_start && !gesture.had_target && gesture.kind == GestureKind::Move {
            return (Some(gesture.live_start), Some(gesture.live_target));
        }
        let start = (gesture.had_start || gesture.kind == GestureKind::ResizeStart)
            .then_some(gesture.live_start);
        let target = (gesture.had_target || gesture.kind == GestureKind::ResizeEnd)
            .then_some(gesture.live_target);
        (start, target)
    }

    pub fn timeline_ticks(&self, width: f32) -> Vec<TimelineTick> {
        let ppd = self.scale.pixels_per_day(self.zoom);
        if ppd <= 0.0 || width <= 0.0 {
            return Vec::new();
        }
        let left = self.date_for_x(-ppd);
        let right = self.date_for_x(width + ppd);
        let mut date = self.scale.floor_date(left);
        let mut ticks = Vec::new();
        let mut last_major = String::new();
        let mut guard = 0;
        while date <= right && guard < 512 {
            let x = self.x_for_date(date);
            let major_label = self.scale.major_label(date);
            let major = major_label != last_major;
            if major {
                last_major = major_label.clone();
            }
            ticks.push(TimelineTick {
                x,
                label: self.scale.tick_label(date),
                major_label,
                major,
                weekend: is_weekend(date),
            });
            let next = self.scale.next_date(date);
            if next <= date {
                break;
            }
            date = next;
            guard += 1;
        }
        ticks
    }

    pub fn today_x(&self, today: Date) -> f32 {
        self.x_for_date(today)
    }

    /// Two-level header majors, with at most one sticky label at x = 0 so
    /// scrolled-past months/years do not stack on top of each other.
    pub fn header_major_labels(&self, width: f32) -> Vec<(f32, String)> {
        Self::sticky_header_labels(
            self.timeline_ticks(width)
                .into_iter()
                .filter(|tick| tick.major)
                .map(|tick| (tick.x, tick.major_label)),
        )
    }

    /// Month (or day/week) tick labels, with the in-view period stuck at x = 0
    /// when its gridline has scrolled off the left edge.
    pub fn header_tick_labels(&self, width: f32) -> Vec<(f32, String)> {
        Self::sticky_header_labels(
            self.timeline_ticks(width)
                .into_iter()
                .map(|tick| (tick.x, tick.label)),
        )
    }

    fn sticky_header_labels(ticks: impl IntoIterator<Item = (f32, String)>) -> Vec<(f32, String)> {
        const STICKY_MIN: f32 = 72.0;
        let mut sticky: Option<String> = None;
        let mut out = Vec::new();
        for (x, label) in ticks {
            if x <= 0.0 {
                sticky = Some(label);
            } else {
                out.push((x, label));
            }
        }
        if let Some(label) = sticky {
            let overlaps = out.first().is_some_and(|(x, _)| *x < STICKY_MIN);
            if !overlaps {
                out.insert(0, (0.0, label));
            }
        }
        out
    }

    /// Fit every laid-out bar into `width` pixels, matching monday.com Auto Fit.
    pub fn auto_fit(&mut self, nodes: &[WorkItemNode], iterations: &[IterationNode], width: f32) {
        let mut earliest: Option<Date> = None;
        let mut latest: Option<Date> = None;
        for node in nodes {
            let hint = iteration_hint_for_path(&node.iteration_path, iterations);
            let display = gantt_bar_display(node, nodes, hint.as_ref());
            if let Some(start) = display.start {
                earliest = Some(match earliest {
                    Some(current) if start < current => start,
                    Some(current) => current,
                    None => start,
                });
            }
            if let Some(end) = display.end {
                latest = Some(match latest {
                    Some(current) if end > current => end,
                    Some(current) => current,
                    None => end,
                });
            }
        }
        let today = jiff::Zoned::now().date();
        let Some(start) = earliest else {
            self.go_today(today);
            self.zoom = 1.0;
            return;
        };
        let start = if today < start { today } else { start };
        let end = match latest {
            Some(current) if today > current => today,
            Some(current) => current,
            None => today,
        };
        self.origin = start;
        self.scroll_x = 0.0;
        let days = end
            .since((Unit::Day, start))
            .map(|span| span.get_days())
            .unwrap_or(1)
            .max(7) as f32;
        let target_ppd = ((width * 0.88) / days).clamp(0.4, 96.0);
        let base = match self.scale {
            GanttScale::Day => 48.0,
            GanttScale::Week => 24.0,
            GanttScale::Month => 12.0,
            GanttScale::Quarter => 4.0,
            GanttScale::Year => 1.5,
        };
        self.zoom = (target_ppd / base).clamp(0.4, 4.0);
    }

    pub fn apply_nudge(
        &mut self,
        nodes: &[WorkItemNode],
        which: NudgeAxis,
        delta_days: i32,
    ) -> Result<PendingNudge, &'static str> {
        let id = self.selected_id.ok_or("Select a Gantt bar first.")?;
        let node = nodes
            .iter()
            .find(|node| node.id == id)
            .ok_or("Work Item not found")?;
        if !node.has_date_fields {
            return Err("This Work Item has no Start Date and Target Date fields.");
        }
        let had_start = parse_iso_date(node.start_date.as_deref()).is_some();
        let had_target = parse_iso_date(node.target_date.as_deref()).is_some();
        let display = gantt_bar_display(node, nodes, None);
        let start = display.start.ok_or("Unscheduled")?;
        let target = display.end.ok_or("Unscheduled")?;
        let mut pending = self
            .pending_nudge
            .clone()
            .filter(|pending| pending.id == id)
            .unwrap_or(PendingNudge {
                id,
                start,
                target,
                original_start: start,
                original_target: target,
                original_rev: node.rev,
                had_start,
                had_target,
            });
        match which {
            NudgeAxis::Both => {
                pending.start = pending.start.saturating_add(delta_days.days());
                pending.target = pending.target.saturating_add(delta_days.days());
            }
            NudgeAxis::Target => {
                let next = pending.target.saturating_add(delta_days.days());
                if next >= pending.start {
                    pending.target = next;
                }
            }
            NudgeAxis::Start => {
                let next = pending.start.saturating_add(delta_days.days());
                if next <= pending.target {
                    pending.start = next;
                }
            }
        }
        self.pending_nudge = Some(pending.clone());
        Ok(pending)
    }

    pub fn hit_test_bar(bars: &[BarLayout], x: f32, y: f32) -> Option<(BarLayout, GestureKind)> {
        let bar = bars
            .iter()
            .find(|bar| {
                y >= bar.y && y < bar.y + ROW_HEIGHT && x >= bar.x && x <= bar.x + bar.width
            })?
            .clone();
        let kind = if bar.milestone {
            GestureKind::Move
        } else if x <= bar.x + HANDLE_WIDTH {
            GestureKind::ResizeStart
        } else if x >= bar.x + bar.width - HANDLE_WIDTH {
            GestureKind::ResizeEnd
        } else {
            GestureKind::Move
        };
        Some((bar, kind))
    }

    pub fn layout_dependency_arrows(&self, bars: &[BarLayout]) -> Vec<DependencyArrow> {
        let by_id: HashMap<i64, &BarLayout> = bars.iter().map(|bar| (bar.id, bar)).collect();
        let mut out = Vec::new();
        for bar in bars {
            for pred_id in &bar.predecessors {
                let Some(pred) = by_id.get(pred_id) else {
                    continue;
                };
                out.push(DependencyArrow {
                    from_id: *pred_id,
                    to_id: bar.id,
                    x1: pred.x + pred.width,
                    y1: pred.y + ROW_HEIGHT / 2.0,
                    x2: bar.x,
                    y2: bar.y + ROW_HEIGHT / 2.0,
                });
            }
        }
        out
    }
}

fn parse_type_color(hex: &str) -> u32 {
    u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x89b4fa)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NudgeAxis {
    Both,
    Start,
    Target,
}

pub fn is_completed_state(state: &str) -> bool {
    matches!(
        state.to_ascii_lowercase().as_str(),
        "done" | "closed" | "resolved" | "completed" | "removed"
    )
}

pub fn state_progress(state: &str) -> f32 {
    if is_completed_state(state) {
        1.0
    } else if state.eq_ignore_ascii_case("active") {
        0.5
    } else {
        0.0
    }
}

/// Fraction of direct children complete, counting Active as half done.
pub fn child_completion(nodes: &[WorkItemNode], id: i64) -> Option<f32> {
    let children: Vec<_> = nodes
        .iter()
        .filter(|node| node.parent_id == Some(id))
        .collect();
    if children.is_empty() {
        return None;
    }
    let total: f32 = children
        .iter()
        .map(|node| state_progress(&node.state))
        .sum();
    Some(total / children.len() as f32)
}

pub fn flatten_visible(nodes: &[WorkItemNode], collapsed: &HashSet<i64>) -> Vec<VisibleRow> {
    let mut children: HashMap<Option<i64>, Vec<i64>> = HashMap::new();
    for node in nodes {
        children.entry(node.parent_id).or_default().push(node.id);
    }
    let mut out = Vec::new();
    fn walk(
        id: i64,
        depth: u32,
        children: &HashMap<Option<i64>, Vec<i64>>,
        collapsed: &HashSet<i64>,
        out: &mut Vec<VisibleRow>,
    ) {
        let kids = children.get(&Some(id)).cloned().unwrap_or_default();
        let expanded = !collapsed.contains(&id);
        out.push(VisibleRow {
            id,
            depth,
            has_children: !kids.is_empty(),
            expanded,
        });
        if expanded {
            for child in kids {
                walk(child, depth + 1, children, collapsed, out);
            }
        }
    }
    for id in children.get(&None).cloned().unwrap_or_default() {
        walk(id, 0, &children, collapsed, &mut out);
    }
    out
}

pub fn descendants(nodes: &[WorkItemNode], id: i64) -> Vec<i64> {
    let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
    for node in nodes {
        if let Some(parent) = node.parent_id {
            children.entry(parent).or_default().push(node.id);
        }
    }
    let mut out = Vec::new();
    fn walk(id: i64, children: &HashMap<i64, Vec<i64>>, out: &mut Vec<i64>) {
        if let Some(kids) = children.get(&id) {
            for child in kids {
                out.push(*child);
                walk(*child, children, out);
            }
        }
    }
    walk(id, &children, &mut out);
    out
}

pub fn iteration_for(node: &WorkItemNode, iterations: &[IterationNode]) -> Option<IterationHint> {
    iteration_hint_for_path(&node.iteration_path, iterations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::ToSpan;

    fn node(id: i64, parent: Option<i64>) -> WorkItemNode {
        WorkItemNode {
            id,
            rev: 1,
            title: format!("WI {id}"),
            work_item_type: "Feature".into(),
            state: "New".into(),
            assigned_to: None,
            parent_id: parent,
            area_path: "Shop".into(),
            iteration_path: "Shop\\Sprint 12".into(),
            start_date: Some("2026-07-06T00:00:00Z".into()),
            target_date: Some("2026-07-20T00:00:00Z".into()),
            has_date_fields: true,
            predecessors: Vec::new(),
            fields: serde_json::Map::new(),
        }
    }

    #[test]
    fn flatten_hides_collapsed_children() {
        let nodes = vec![node(1, None), node(2, Some(1)), node(3, Some(2))];
        let mut collapsed = HashSet::new();
        assert_eq!(flatten_visible(&nodes, &collapsed).len(), 3);
        collapsed.insert(1);
        assert_eq!(flatten_visible(&nodes, &collapsed).len(), 1);
    }

    #[test]
    fn five_thousand_rows_virtualize() {
        let nodes: Vec<_> = (0..5_000)
            .map(|i| {
                let mut item = node(i as i64 + 1, None);
                item.title = format!("Row {i}");
                item
            })
            .collect();
        let gantt = GanttView::default();
        let rows = gantt.flatten(&nodes);
        assert_eq!(rows.len(), 5_000);
        let window = gantt.visible_window(&rows, 720.0);
        assert!(window.len() < 40);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        assert!(!bars.is_empty());
        assert!(bars.len() < 40);
    }

    #[test]
    fn zoom_stays_anchored_to_pointer() {
        let mut gantt = GanttView {
            scroll_x: 120.0,
            ..Default::default()
        };
        gantt.zoom_at(2.0, 200.0, 0.0);
        assert!((gantt.zoom - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn hit_test_picks_resize_handles() {
        let bar = BarLayout {
            id: 1,
            kind: GanttBarKind::Scheduled,
            x: 100.0,
            width: 80.0,
            y: 0.0,
            summary: false,
            milestone: false,
            label: "bar".into(),
            title: "bar".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
        };
        let bars = [bar.clone()];
        assert_eq!(
            GanttView::hit_test_bar(&bars, 102.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::ResizeStart)
        );
        assert_eq!(
            GanttView::hit_test_bar(&bars, 176.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd)
        );
        assert_eq!(
            GanttView::hit_test_bar(&bars, 140.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::Move)
        );
        let milestone = BarLayout {
            milestone: true,
            width: MILESTONE_SIZE,
            ..bar
        };
        assert_eq!(
            GanttView::hit_test_bar(std::slice::from_ref(&milestone), 102.0, 4.0)
                .map(|(_, kind)| kind),
            Some(GestureKind::Move),
            "a milestone is smaller than two resize handles; the whole diamond is a move"
        );
        assert_eq!(
            GanttView::hit_test_bar(&[milestone], 114.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::Move)
        );
    }

    #[test]
    fn start_only_and_milestone_bars_can_begin_a_move() {
        let mut start_only = node(1, None);
        start_only.target_date = None;
        let mut milestone = node(2, None);
        milestone.start_date = None;
        let nodes = vec![start_only, milestone];
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let start_bar = bars.iter().find(|bar| bar.id == 1).expect("start-only");
        let mile_bar = bars.iter().find(|bar| bar.id == 2).expect("milestone");
        assert!(mile_bar.milestone);
        gantt
            .begin_gesture(start_bar, GestureKind::Move, 10.0, &nodes)
            .expect("start-only move");
        assert!(
            gantt
                .gesture
                .as_ref()
                .is_some_and(|g| g.had_start && !g.had_target)
        );
        gantt.cancel_gesture();
        gantt
            .begin_gesture(mile_bar, GestureKind::Move, 10.0, &nodes)
            .expect("milestone move");
        assert!(
            gantt
                .gesture
                .as_ref()
                .is_some_and(|g| !g.had_start && g.had_target)
        );
        gantt.selected_id = Some(1);
        assert!(gantt.apply_nudge(&nodes, NudgeAxis::Both, 1).is_ok());
    }

    #[test]
    fn header_ticks_and_bars_share_scroll_x_after_today_and_a_horizontal_nudge() {
        let today = Date::new(2026, 10, 30).unwrap();
        let mut item = node(1, None);
        item.start_date = Some("2026-10-30T00:00:00Z".into());
        item.target_date = Some("2026-11-13T00:00:00Z".into());
        let nodes = vec![item];
        let mut gantt = GanttView::default();
        gantt.go_today(today);
        gantt.scroll_x += 96.0;
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let ticks = gantt.timeline_ticks(800.0);
        let bar_x = bars[0].x;
        let from_date = gantt.x_for_date(today);
        assert!(
            (bar_x - from_date).abs() < 0.5,
            "bar.x {bar_x} must equal x_for_date {from_date}"
        );
        let on_screen = ticks
            .iter()
            .find(|tick| tick.x >= 0.0 && tick.x <= 800.0)
            .expect("visible tick");
        let decoded = gantt.x_for_date(gantt.date_for_x(on_screen.x));
        assert!(
            (decoded - on_screen.x).abs() < 0.5,
            "tick.x {} must match x_for_date(date_for_x) {decoded} so header and bars share scroll_x",
            on_screen.x
        );
    }

    #[test]
    fn nudge_does_not_reuse_pending_from_another_row() {
        let nodes = vec![node(1, None), node(2, None)];
        let mut gantt = GanttView {
            selected_id: Some(1),
            ..Default::default()
        };
        gantt.apply_nudge(&nodes, NudgeAxis::Both, 1).unwrap();
        assert_eq!(gantt.pending_nudge.as_ref().map(|p| p.id), Some(1));
        let rows = gantt.flatten(&nodes);
        gantt.select_relative(&rows, 1);
        assert!(gantt.pending_nudge.is_none());
        gantt.selected_id = Some(2);
        let pending = gantt.apply_nudge(&nodes, NudgeAxis::Both, 2).unwrap();
        assert_eq!(pending.id, 2);
        assert_eq!(gantt.pending_nudge.as_ref().map(|p| p.id), Some(2));
    }

    #[test]
    fn click_without_drag_does_not_finish_as_a_save() {
        let nodes = vec![node(1, None)];
        let mut gantt = GanttView {
            selected_id: Some(1),
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        gantt
            .begin_gesture(&bars[0], GestureKind::Move, 10.0, &nodes)
            .unwrap();
        assert!(gantt.finish_gesture().is_none());
    }

    #[test]
    fn timeline_ticks_cover_the_visible_width() {
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let ticks = gantt.timeline_ticks(400.0);
        assert!(ticks.len() >= 2);
        assert!(ticks.iter().any(|tick| tick.major));
        assert!(ticks.iter().any(|tick| {
            tick.label.contains("2026") || tick.label.contains("Jul") || tick.label.contains("7")
        }));
    }

    #[test]
    fn month_tick_labels_are_calendar_months_not_thirty_day_steps() {
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let labels: Vec<String> = gantt
            .timeline_ticks(800.0)
            .into_iter()
            .filter(|tick| tick.x >= -1.0 && tick.x <= 800.0)
            .map(|tick| tick.label)
            .collect();
        assert!(
            labels.len() >= 3,
            "expected several month ticks, got {labels:?}"
        );
        assert_eq!(
            &labels[..3],
            ["Jul", "Aug", "Sep"],
            "30-day steps produced Jul|Jul|Aug; calendar months must be Jul|Aug|Sep"
        );
        for pair in labels.windows(2) {
            assert_ne!(
                pair[0], pair[1],
                "a month must not repeat in consecutive tick labels: {labels:?}"
            );
        }
    }

    #[test]
    fn auto_fit_puts_scheduled_bars_on_screen() {
        let nodes = vec![node(1, None)];
        let mut gantt = GanttView {
            origin: Date::new(2025, 1, 1).unwrap(),
            zoom: 0.4,
            ..Default::default()
        };
        gantt.auto_fit(&nodes, &[], 800.0);
        assert_eq!(gantt.origin, Date::new(2026, 7, 6).unwrap());
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        assert!(!bars.is_empty());
        assert!(bars[0].x >= -1.0);
        assert!(bars[0].x + bars[0].width <= 800.0 + 1.0);
        assert!(!bars[0].title.is_empty());
    }

    #[test]
    fn reveal_content_moves_origin_to_the_earliest_bar() {
        let nodes = vec![node(1, None)];
        let mut gantt = GanttView {
            origin: Date::new(2026, 1, 1).unwrap(),
            ..Default::default()
        };
        gantt.reveal_content(&nodes, &[]);
        assert_eq!(gantt.origin, Date::new(2026, 7, 6).unwrap());
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        assert!(!bars.is_empty());
        assert!(bars.iter().all(|bar| bar.x >= -1.0));
    }

    #[test]
    fn unscheduled_iteration_still_gets_a_placeholder_bar() {
        let mut item = node(1, None);
        item.start_date = None;
        item.target_date = None;
        let iterations = vec![ado_core::IterationNode {
            path: "Shop\\Sprint 12".into(),
            name: "Sprint 12".into(),
            start_date: Some("2026-07-01T00:00:00Z".into()),
            finish_date: Some("2026-07-14T00:00:00Z".into()),
            children: Vec::new(),
        }];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(&[item.clone()]);
        let bars = gantt.layout_bars(&[item], &rows, &iterations, 720.0);
        assert_eq!(bars.len(), 1);
        assert!(!bars[0].summary);
        assert_eq!(bars[0].kind, GanttBarKind::Unscheduled);
        assert!(bars[0].width >= PLACEHOLDER_MIN_WIDTH);
    }

    #[test]
    fn header_major_labels_do_not_stack_at_the_origin() {
        let gantt = GanttView {
            origin: Date::new(2026, 1, 1).unwrap(),
            scale: GanttScale::Month,
            scroll_x: 12.0 * 30.0 * 8.0,
            ..Default::default()
        };
        let labels = gantt.header_major_labels(800.0);
        let at_zero = labels.iter().filter(|(x, _)| *x == 0.0).count();
        assert!(at_zero <= 1, "sticky majors must not stack: {labels:?}");
        assert!(
            labels.iter().any(|(_, label)| label.contains("2026")),
            "year label should remain visible: {labels:?}"
        );
    }

    #[test]
    fn weekend_bands_are_day_scale_only() {
        assert!(GanttScale::Day.shows_weekend_bands());
        assert!(!GanttScale::Month.shows_weekend_bands());
        let gantt = GanttView {
            origin: Date::new(2026, 7, 4).unwrap(),
            scale: GanttScale::Day,
            ..Default::default()
        };
        let ticks = gantt.timeline_ticks(400.0);
        assert!(ticks.iter().any(|tick| tick.weekend && tick.label == "4"));
    }

    #[test]
    fn row_y_keeps_bars_aligned_with_fractional_scroll() {
        let nodes = vec![node(1, None), node(2, None), node(3, None)];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scroll_y: 50.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        assert!(!bars.is_empty());
        assert!((bars[0].y - gantt.row_y(0)).abs() < f32::EPSILON);
        assert!(gantt.row_scroll_remainder() > 0.0);
    }

    #[test]
    fn short_bars_keep_a_title_for_an_outside_label() {
        let nodes = vec![node(1, None)];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        assert_eq!(bars.len(), 1);
        assert!(bars[0].width < BAR_LABEL_INSIDE_MIN);
        assert_eq!(bars[0].title, "WI 1");
    }

    #[test]
    fn auto_fit_without_dates_goes_to_today() {
        let mut item = node(1, None);
        item.start_date = None;
        item.target_date = None;
        let mut gantt = GanttView {
            origin: Date::new(2025, 1, 1).unwrap(),
            zoom: 0.4,
            ..Default::default()
        };
        gantt.auto_fit(&[item], &[], 800.0);
        assert_eq!(gantt.origin, jiff::Zoned::now().date());
        assert!((gantt.zoom - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn page_scroll_moves_by_more_than_one_row() {
        let mut gantt = GanttView {
            viewport_h: 200.0,
            ..Default::default()
        };
        gantt.page_scroll(1, 200.0, 40);
        assert!(gantt.scroll_y > ROW_HEIGHT);
    }

    #[test]
    fn clamp_scroll_stops_at_the_last_row() {
        let mut gantt = GanttView {
            viewport_h: 80.0,
            scroll_y: 10_000.0,
            ..Default::default()
        };
        gantt.clamp_scroll(3, 80.0);
        assert!(gantt.scroll_y < 200.0);
        assert!(gantt.scroll_y >= 0.0);
    }

    #[test]
    fn tree_width_clamps_to_leave_room_for_the_timeline() {
        let mut gantt = GanttView {
            viewport_w: 800.0,
            tree_width: DEFAULT_TREE_WIDTH,
            ..Default::default()
        };
        gantt.set_tree_width(10_000.0);
        assert!((gantt.tree_width - (800.0 - 6.0 - MIN_TIMELINE_WIDTH)).abs() < f32::EPSILON);
        assert!(gantt.timeline_width >= MIN_TIMELINE_WIDTH);
        gantt.nudge_tree_width(500.0);
        assert!(gantt.tree_width <= gantt.max_tree_width());
        gantt.set_tree_width(0.0);
        assert!((gantt.tree_width - MIN_TREE_WIDTH).abs() < f32::EPSILON);
    }

    #[test]
    fn narrow_pane_keeps_a_240px_timeline_by_shrinking_the_tree() {
        let mut gantt = GanttView {
            viewport_w: 700.0,
            tree_width: DEFAULT_TREE_WIDTH,
            ..Default::default()
        };
        gantt.set_tree_width_for_pane(DEFAULT_TREE_WIDTH, 700.0);
        assert!(
            gantt.timeline_width >= MIN_TIMELINE_WIDTH - 0.5,
            "timeline {} at 700px pane",
            gantt.timeline_width
        );
        assert!(
            gantt.tree_width <= 700.0 - 6.0 - MIN_TIMELINE_WIDTH + 0.5,
            "tree {} ate the timeline",
            gantt.tree_width
        );
        gantt.set_tree_width_for_pane(10_000.0, 400.0);
        assert!(gantt.timeline_width >= MIN_TIMELINE_WIDTH - 0.5);
        assert!(
            gantt.tree_width < MIN_TREE_WIDTH,
            "tree should shrink below 240 in a 400px pane, got {}",
            gantt.tree_width
        );
    }

    #[test]
    fn set_tree_width_for_pane_uses_the_live_width_not_a_stale_viewport() {
        let mut gantt = GanttView {
            viewport_w: 1280.0,
            tree_width: DEFAULT_TREE_WIDTH,
            ..Default::default()
        };
        gantt.set_tree_width_for_pane(10_000.0, 1024.0);
        assert!(
            (gantt.tree_width - (1024.0 - 6.0 - MIN_TIMELINE_WIDTH)).abs() < 0.5,
            "tree_width {} should clamp against the 1024px pane, not the stale 1280",
            gantt.tree_width
        );
        assert!(gantt.timeline_width >= MIN_TIMELINE_WIDTH - 0.5);
    }

    #[test]
    fn layout_bars_share_row_height_stride() {
        let nodes = crate::state::fake_hierarchy(8);
        let gantt = GanttView::default();
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        assert!(bars.len() >= 2);
        let stride = bars[1].y - bars[0].y;
        assert!(
            (stride - ROW_HEIGHT).abs() < f32::EPSILON,
            "bar stride {stride} should equal ROW_HEIGHT {ROW_HEIGHT}"
        );
        assert!(
            (bars[0].y).abs() < f32::EPSILON,
            "first bar y should be 0, not TIMELINE_HEADER_H"
        );
    }

    #[test]
    fn parent_bars_report_child_completion() {
        let mut nodes = crate::state::fake_hierarchy(3);
        nodes[1].parent_id = Some(1);
        nodes[2].parent_id = Some(1);
        nodes[1].state = "Closed".into();
        nodes[2].state = "New".into();
        assert_eq!(child_completion(&nodes, 1), Some(0.5));
        assert_eq!(child_completion(&nodes, 2), None);
        nodes[1].state = "Active".into();
        assert_eq!(child_completion(&nodes, 1), Some(0.25));
    }

    #[test]
    fn go_today_leaves_a_margin_from_the_splitter() {
        let mut gantt = GanttView::default();
        let today = Date::new(2026, 10, 30).unwrap();
        gantt.go_today(today);
        let x = gantt.today_x(today);
        assert!(
            (x - TODAY_MARGIN_PX).abs() < 0.5,
            "today marker at {x}, expected ~{TODAY_MARGIN_PX}px from the splitter"
        );
    }

    #[test]
    fn today_marker_sits_on_oct_30_not_the_oct_1_gridline() {
        let mut gantt = GanttView::default();
        let today = Date::new(2026, 10, 30).unwrap();
        let oct1 = Date::new(2026, 10, 1).unwrap();
        gantt.go_today(today);
        let today_x = gantt.today_x(today);
        let oct1_x = gantt.x_for_date(oct1);
        assert!(
            (today_x - oct1_x).abs() > 20.0,
            "Today at {today_x} must not sit on the Oct 1 gridline at {oct1_x}"
        );
        let oct_tick = gantt
            .timeline_ticks(1200.0)
            .into_iter()
            .find(|tick| tick.label == "Oct");
        if let Some(tick) = oct_tick {
            assert!(
                (tick.x - oct1_x).abs() < 1.0,
                "Oct header tick at {} should mark Oct 1 ({oct1_x})",
                tick.x
            );
        }
    }

    #[test]
    fn scroll_left_reveals_earlier_months() {
        let mut gantt = GanttView {
            origin: Date::new(2026, 8, 1).unwrap(),
            ..Default::default()
        };
        let aug = Date::new(2026, 8, 1).unwrap();
        let before = gantt.x_for_date(aug);
        gantt.apply_wheel(-120.0, 0.0, false);
        let after = gantt.x_for_date(aug);
        assert!(
            after > before,
            "scroll-left must move August 1 to the right (earlier months into view): {before} -> {after}"
        );
        let labels: Vec<_> = gantt
            .header_tick_labels(400.0)
            .into_iter()
            .map(|(_, label)| label)
            .collect();
        assert!(
            labels.iter().any(|label| label == "Jul" || label == "Aug"),
            "after scrolling left the header should still show calendar months, got {labels:?}"
        );
    }

    #[test]
    fn link_gesture_does_not_resize_or_move_dates() {
        let nodes = vec![node(1, None), node(2, None)];
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        gantt
            .begin_gesture(
                &bars[0],
                GestureKind::Link,
                bars[0].x + bars[0].width + 6.0,
                &nodes,
            )
            .expect("link");
        gantt.update_gesture(bars[1].x + 10.0);
        assert!(gantt.finish_gesture().is_none());
        gantt
            .begin_gesture(
                &bars[0],
                GestureKind::Link,
                bars[0].x + bars[0].width + 6.0,
                &nodes,
            )
            .expect("link again");
        assert_eq!(gantt.take_link_gesture(), Some(1));
        assert!(gantt.gesture.is_none());
    }

    #[test]
    fn start_only_move_patches_start_not_target() {
        let mut item = node(1, None);
        item.target_date = None;
        let nodes = vec![item];
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        gantt
            .begin_gesture(&bars[0], GestureKind::Move, 10.0, &nodes)
            .expect("start-only");
        let ppd = gantt.scale.pixels_per_day(gantt.zoom);
        gantt.update_gesture(10.0 + ppd * 3.0);
        let gesture = gantt.gesture.clone().expect("live");
        let (start, target) = GanttView::gesture_patch_dates(&gesture);
        assert!(start.is_some(), "start-only drag must write Start Date");
        assert!(
            target.is_none(),
            "start-only drag must not invent Target Date"
        );
        let original = parse_iso_date(nodes[0].start_date.as_deref()).unwrap();
        assert_eq!(start, Some(original.saturating_add(3.days())));
    }

    #[test]
    fn parent_layout_is_a_summary_bar() {
        let nodes = crate::state::fake_hierarchy(3);
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let parent = bars.iter().find(|bar| bar.id == 1).expect("parent bar");
        let leaf = bars.iter().find(|bar| bar.id == 2).expect("leaf bar");
        assert!(parent.summary, "parent with children must be a summary bar");
        assert!(!leaf.summary, "leaf must not look like a summary bar");
        assert!(parent.progress.is_some());
    }

    #[test]
    fn dependency_arrows_join_predecessor_to_successor() {
        let mut a = node(1, None);
        let mut b = node(2, None);
        b.start_date = Some("2026-07-21T00:00:00Z".into());
        b.target_date = Some("2026-08-04T00:00:00Z".into());
        b.predecessors = vec![1];
        a.title = "Pred".into();
        b.title = "Succ".into();
        let nodes = vec![a, b];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert_eq!(arrows.len(), 1);
        assert_eq!(arrows[0].from_id, 1);
        assert_eq!(arrows[0].to_id, 2);
        assert!(arrows[0].x2 >= arrows[0].x1);
    }

    #[test]
    fn select_relative_scrolls_down_into_view() {
        let rows: Vec<VisibleRow> = (0..40)
            .map(|i| VisibleRow {
                id: i + 1,
                depth: 0,
                has_children: false,
                expanded: true,
            })
            .collect();
        let mut gantt = GanttView {
            viewport_h: 72.0,
            ..Default::default()
        };
        gantt.selected_id = Some(1);
        gantt.select_relative(&rows, 5);
        assert_eq!(gantt.selected_id, Some(6));
        assert!(gantt.scroll_y > 0.0);
    }
}
