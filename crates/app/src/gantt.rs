use std::collections::{HashMap, HashSet};

use ado_core::{
    GanttBarKind, IterationHint, IterationNode, WorkItemNode, compact_date, dates_column_text,
    gantt_bar_display, iteration_hint_for_path, parse_iso_date,
};
use jiff::civil::Date;
use jiff::{ToSpan, Unit};

pub const ROW_HEIGHT: f32 = 40.0;
pub const DEFAULT_TREE_WIDTH: f32 = 640.0;
pub const MIN_TREE_WIDTH: f32 = 240.0;
pub const MIN_TIMELINE_WIDTH: f32 = 240.0;
pub const HARD_MIN_TREE_WIDTH: f32 = 80.0;
pub const SPLITTER_WIDTH: f32 = 8.0;
pub const SPLITTER_STEP: f32 = 16.0;
pub const HANDLE_WIDTH: f32 = 8.0;
pub const LINK_HANDLE: f32 = 14.0;
pub const LINK_HANDLE_HIT: f32 = 18.0;
pub const NUDGE_IDLE_MS: u64 = 500;
pub const TIMELINE_HEADER_H: f32 = 44.0;
pub const BAR_LABEL_INSIDE_MIN: f32 = 72.0;
pub const TODAY_MARGIN_PX: f32 = 80.0;
/// Place the Today line in the middle of the timeline.
pub const TODAY_ALIGN: f32 = 0.5;
pub const SCALE_BUTTON_WIDTH: f32 = 132.0;
/// Toolbar zoom, Today, scale, period, Auto Fit, and team-switch camera easing.
pub const CAMERA_EASE_MS: u64 = 150;
pub const PLACEHOLDER_MIN_WIDTH: f32 = 48.0;
pub const MILESTONE_SIZE: f32 = 16.0;
pub const SUMMARY_BAR_H: f32 = 14.0;
pub const PLACEHOLDER_BAR_H: f32 = 24.0;
pub const LEAF_BAR_H: f32 = 28.0;
pub const DATES_COL_WIDTH: f32 = 140.0;
pub const DETAILS_PILL_WIDTH: f32 = 36.0;
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

    pub fn button_label(self, zoom: f32) -> String {
        let pct = (self.pixels_per_day(zoom) / self.base_pixels_per_day()) * 100.0;
        if (pct - 100.0).abs() < 0.5 {
            self.label().to_string()
        } else {
            format!("{} ({:.0}%)", self.label(), pct)
        }
    }

    pub fn base_pixels_per_day(self) -> f32 {
        match self {
            Self::Day => 48.0,
            Self::Week => 24.0,
            Self::Month => 12.0,
            Self::Quarter => 4.0,
            Self::Year => 1.5,
        }
    }

    pub fn pixels_per_day(self, zoom: f32) -> f32 {
        (self.base_pixels_per_day() * zoom).clamp(0.4, 96.0)
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
            Self::Day => compact_date(origin),
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
            Self::Year => String::new(),
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SavedGanttView {
    pub origin: Date,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub scale: GanttScale,
    pub zoom: f32,
}

impl SavedGanttView {
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = ease_out_quad(t.clamp(0.0, 1.0));
        Self {
            origin: lerp_date(self.origin, other.origin, t),
            scroll_x: self.scroll_x + (other.scroll_x - self.scroll_x) * t,
            scroll_y: self.scroll_y + (other.scroll_y - self.scroll_y) * t,
            scale: other.scale,
            zoom: self.zoom + (other.zoom - self.zoom) * t,
        }
    }
}

fn ease_out_quad(t: f32) -> f32 {
    1.0 - (1.0 - t) * (1.0 - t)
}

fn lerp_date(from: Date, to: Date, t: f32) -> Date {
    let days = to
        .since((Unit::Day, from))
        .map(|span| span.get_days())
        .unwrap_or(0) as f32;
    from.saturating_add(((days * t).round() as i32).days())
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
    pub link_to: Option<i64>,
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
    pub x_head: f32,
    pub lane_y: f32,
}

impl DependencyArrow {
    /// Visual midpoint along the polyline (outgoing, lane, incoming, stub).
    pub fn midpoint(&self) -> (f32, f32) {
        let pts = [
            (self.x1, self.y1),
            (self.x1, self.lane_y),
            (self.x2, self.lane_y),
            (self.x2, self.y2),
            (self.x_head, self.y2),
        ];
        let mut total = 0.0;
        let mut segs = [0.0; 4];
        for (i, pair) in pts.windows(2).enumerate() {
            let dx = pair[1].0 - pair[0].0;
            let dy = pair[1].1 - pair[0].1;
            segs[i] = (dx * dx + dy * dy).sqrt();
            total += segs[i];
        }
        let mut remain = total * 0.5;
        for (i, len) in segs.iter().enumerate() {
            if remain <= *len {
                let t = if *len > 0.0 { remain / *len } else { 0.0 };
                return (
                    pts[i].0 + (pts[i + 1].0 - pts[i].0) * t,
                    pts[i].1 + (pts[i + 1].1 - pts[i].1) * t,
                );
            }
            remain -= len;
        }
        (self.x_head, self.y2)
    }
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
    pub timeline_origin_y: f32,
    pub timeline_width: f32,
    pub selected_id: Option<i64>,
    pub hovered_id: Option<i64>,
    pub selected_link: Option<(i64, i64)>,
    pub hovered_link: Option<(i64, i64)>,
    pub collapsed: HashSet<i64>,
    pub gesture: Option<Gesture>,
    pub pending_nudge: Option<PendingNudge>,
    pub context_menu: Option<ContextTarget>,
    pub splitter_drag: Option<(f32, f32)>,
    pub schedule_drag: Option<i64>,
    /// After mouse-up, ignore hover until the pointer moves so a bar drag
    /// does not leave a stuck row highlight under a stationary cursor.
    pub hover_locked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextTarget {
    pub id: i64,
    pub from_bar: bool,
    pub x: f32,
    pub y: f32,
    pub link: Option<(i64, i64)>,
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
            timeline_origin_y: 140.0,
            timeline_width: 800.0,
            selected_id: None,
            hovered_id: None,
            selected_link: None,
            hovered_link: None,
            collapsed: HashSet::new(),
            gesture: None,
            pending_nudge: None,
            context_menu: None,
            splitter_drag: None,
            schedule_drag: None,
            hover_locked: false,
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

    pub fn first_visible_row_index(&self) -> usize {
        (self.scroll_y / ROW_HEIGHT).floor() as usize
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

    /// Keep the date under the visible timeline centre still while zooming.
    pub fn zoom_at_center(&mut self, factor: f32) {
        let pointer = self.timeline_width.max(1.0) * 0.5;
        self.zoom_at(factor, pointer, 0.0);
    }

    /// Toolbar +/−: keep the left-edge date still so month columns grow/shrink
    /// in place instead of the leftover fragment at x=0 collapsing.
    pub fn zoom_at_left(&mut self, factor: f32) {
        self.zoom_at(factor, 0.0, 0.0);
    }

    /// Nice +/− stops so 100% → 125% → 150% instead of 100 → 74 → …
    pub const ZOOM_STEPS: [f32; 9] = [0.4, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];

    pub fn next_zoom(zoom: f32, direction: i32) -> f32 {
        if direction >= 0 {
            Self::ZOOM_STEPS
                .into_iter()
                .find(|&step| step > zoom + 0.001)
                .unwrap_or(4.0)
        } else {
            Self::ZOOM_STEPS
                .into_iter()
                .rfind(|&step| step < zoom - 0.001)
                .unwrap_or(0.4)
        }
    }

    pub fn zoom_step_at_center(&mut self, direction: i32) {
        let next = Self::next_zoom(self.zoom, direction);
        if self.zoom > 0.0 {
            self.zoom_at_center(next / self.zoom);
        }
    }

    /// Pixel width of the scale period that contains `date` (a month at Month).
    pub fn period_width(&self, date: Date) -> f32 {
        let start = self.scale.floor_date(date);
        let end = self.scale.next_date(start);
        (self.x_for_date(end) - self.x_for_date(start)).abs()
    }

    /// Distance from the sticky left header to the next tick (the leftover
    /// column a screenshot measures).
    pub fn leading_period_width(&self, width: f32) -> f32 {
        self.header_tick_labels(width)
            .into_iter()
            .find(|(x, _)| *x > 1.0)
            .map(|(x, _)| x)
            .unwrap_or(0.0)
    }

    pub fn viewport_range_title(&self) -> String {
        let width = self.timeline_width.max(1.0);
        // Follow the first painted axis tick so a leftover previous month that
        // the sticky header skipped is not claimed in the toolbar.
        let start = self
            .header_tick_labels(width)
            .into_iter()
            .find(|(x, _)| *x >= 0.0)
            .map(|(x, _)| self.date_for_x(x))
            .unwrap_or_else(|| self.viewport_start_date());
        let end = self.date_for_x((width - 1.0).max(0.0));
        match self.scale {
            GanttScale::Month if start.month() != end.month() || start.year() != end.year() => {
                if start.year() == end.year() {
                    format!(
                        "{} – {} {}",
                        month_abbrev(start),
                        month_abbrev(end),
                        start.year()
                    )
                } else {
                    format!(
                        "{} {} – {} {}",
                        month_abbrev(start),
                        start.year(),
                        month_abbrev(end),
                        end.year()
                    )
                }
            }
            _ => self.scale.range_title(start),
        }
    }

    pub fn viewport_start_date(&self) -> Date {
        self.date_for_x(0.0)
    }

    pub fn viewport_center_date(&self) -> Date {
        self.date_for_x(self.timeline_width.max(1.0) * 0.5)
    }

    /// Keep a date on-screen after a picker or drag edit.
    pub fn reveal_date(&mut self, date: Date) {
        let x = self.x_for_date(date);
        let width = self.timeline_width.max(1.0);
        if x < 24.0 {
            self.scroll_x += x - width * 0.2;
        } else if x > width - 48.0 {
            self.scroll_x += x - width * 0.35;
        }
    }

    pub fn clear_hover(&mut self) {
        self.hovered_id = None;
        self.hovered_link = None;
    }

    pub fn pointer_in_canvas(&self, x: f32, y: f32) -> bool {
        let left = self.timeline_origin_x;
        let right = left + self.timeline_width.max(1.0);
        let top = self.timeline_origin_y + TIMELINE_HEADER_H;
        let bottom = self.timeline_origin_y + self.viewport_h;
        x >= left && x <= right && y >= top && y <= bottom
    }

    pub fn pointer_in_rows(&self, x: f32, y: f32) -> bool {
        let right = self.timeline_origin_x + self.timeline_width.max(1.0);
        let top = self.timeline_origin_y + TIMELINE_HEADER_H;
        let bottom = self.timeline_origin_y + self.viewport_h;
        x >= 0.0 && x <= right && y >= top && y <= bottom
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

    /// Keep the date under the viewport centre still when switching Day/Week/Month/…
    pub fn set_scale(&mut self, scale: GanttScale) {
        if self.scale == scale {
            return;
        }
        let width = self.timeline_width.max(1.0);
        let center = self.date_for_x(width * 0.5);
        self.scale = scale;
        self.origin = center;
        self.scroll_x = 0.0;
        let x = self.x_for_date(center);
        self.scroll_x = x - width * 0.5;
    }

    pub fn filler_row_count(window_len: usize, content_h: f32) -> usize {
        ((content_h / ROW_HEIGHT).ceil() as usize).saturating_sub(window_len)
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
        self.scroll_x = 0.0;
        let x = self.x_for_date(today);
        let target = self.timeline_width.max(1.0) * TODAY_ALIGN;
        self.scroll_x = x - target;
    }

    pub fn snapshot(&self) -> SavedGanttView {
        SavedGanttView {
            origin: self.origin,
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            scale: self.scale,
            zoom: self.zoom,
        }
    }

    pub fn restore(&mut self, saved: SavedGanttView) {
        self.origin = saved.origin;
        self.scroll_x = saved.scroll_x;
        self.scroll_y = saved.scroll_y;
        self.scale = saved.scale;
        self.zoom = saved.zoom;
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
        self.scroll_y = 0.0;
    }

    pub fn max_tree_width(&self) -> f32 {
        (self.viewport_w - SPLITTER_WIDTH - MIN_TIMELINE_WIDTH).max(HARD_MIN_TREE_WIDTH)
    }

    pub fn min_tree_width(&self) -> f32 {
        if self.viewport_w >= MIN_TREE_WIDTH + MIN_TIMELINE_WIDTH + SPLITTER_WIDTH {
            MIN_TREE_WIDTH
        } else {
            (self.viewport_w - SPLITTER_WIDTH - MIN_TIMELINE_WIDTH).max(HARD_MIN_TREE_WIDTH)
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
        self.timeline_width =
            (self.viewport_w - self.tree_width - SPLITTER_WIDTH).max(MIN_TIMELINE_WIDTH);
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
        let body_h = (self.viewport_h - TIMELINE_HEADER_H).max(0.0);
        if y < self.scroll_y {
            self.scroll_y = y;
        } else if y + ROW_HEIGHT > self.scroll_y + body_h {
            self.scroll_y = (y + ROW_HEIGHT - body_h).max(0.0);
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

    pub fn drop_date(&self, window_x: f32) -> Date {
        self.date_for_x(window_x - self.timeline_origin_x)
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
                let live = self
                    .gesture
                    .as_ref()
                    .filter(|gesture| gesture.id == node.id && gesture.kind != GestureKind::Link);
                let (start, target) = if let Some(gesture) = live {
                    (gesture.live_start, gesture.live_target)
                } else {
                    let (Some(start), Some(target)) = (display.start, display.end) else {
                        return None;
                    };
                    (start, target)
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
        iterations: &[IterationNode],
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
                link_to: None,
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
        let (start, target) = self.dates_from_painted_bar(bar, node, nodes, iterations);
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
            link_to: None,
            had_start,
            had_target,
        });
        Ok(())
    }

    fn dates_from_painted_bar(
        &self,
        _bar: &BarLayout,
        node: &WorkItemNode,
        nodes: &[WorkItemNode],
        iterations: &[IterationNode],
    ) -> (Date, Date) {
        let hint = iteration_hint_for_path(&node.iteration_path, iterations);
        let display = gantt_bar_display(node, nodes, hint.as_ref());
        let start = display.start.or(display.end).unwrap_or(self.origin);
        let target = display.end.unwrap_or(start);
        (start, target.max(start))
    }

    pub fn update_gesture(&mut self, x: f32) {
        let Some(gesture) = &mut self.gesture else {
            return;
        };
        gesture.live_x = x;
        let delta_days =
            ((x - gesture.origin_x) / self.scale.pixels_per_day(self.zoom)).round() as i32;
        match gesture.kind {
            GestureKind::Move => {
                gesture.live_start = gesture.start.saturating_add(delta_days.days());
                gesture.live_target = gesture.target.saturating_add(delta_days.days());
            }
            GestureKind::Link => {}
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
        self.clear_hover();
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

    pub fn take_link_gesture(&mut self) -> Option<(i64, Option<i64>)> {
        let gesture = self.gesture.as_ref()?;
        if gesture.kind != GestureKind::Link {
            return None;
        }
        let from_id = gesture.id;
        let to_id = gesture.link_to.or(self.hovered_id);
        self.gesture = None;
        Some((from_id, to_id))
    }

    pub fn set_link_target(&mut self, id: i64) {
        if let Some(gesture) = self.gesture.as_mut()
            && gesture.kind == GestureKind::Link
        {
            gesture.link_to = Some(id);
            self.hovered_id = Some(id);
        }
    }

    pub fn drag_preview_text(gesture: &Gesture) -> String {
        if gesture.kind == GestureKind::Link {
            return match gesture.link_to {
                Some(to) => format!("Link to #{to}"),
                None => "Drop on a bar to link".into(),
            };
        }
        if gesture.live_start == gesture.live_target {
            compact_date(gesture.live_start)
        } else {
            format!(
                "{} → {}",
                compact_date(gesture.live_start),
                compact_date(gesture.live_target)
            )
        }
    }

    pub fn gesture_patch_dates(gesture: &Gesture) -> (Option<Date>, Option<Date>) {
        if !gesture.had_start && !gesture.had_target && gesture.kind == GestureKind::Move {
            return (Some(gesture.live_start), Some(gesture.live_target));
        }
        match gesture.kind {
            GestureKind::ResizeStart => (Some(gesture.live_start), None),
            GestureKind::ResizeEnd => (None, Some(gesture.live_target)),
            GestureKind::Move => (
                gesture.had_start.then_some(gesture.live_start),
                gesture.had_target.then_some(gesture.live_target),
            ),
            GestureKind::Link => (None, None),
        }
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
        self.header_major_labels_except(width, None)
    }

    /// Skip a major label that would sit on the Today flag (Day zoom).
    /// Pin that month to x = 0 instead of dropping it, so "Nov 2026" stays
    /// visible when Today is the 2nd.
    pub fn header_major_labels_except(
        &self,
        width: f32,
        hide_x: Option<f32>,
    ) -> Vec<(f32, String)> {
        const HIDE_PAD: f32 = 80.0;
        let ticks: Vec<(f32, String)> = self
            .timeline_ticks(width)
            .into_iter()
            .filter(|tick| tick.major)
            .map(|tick| {
                let x = if hide_x.is_some_and(|today| (tick.x - today).abs() < HIDE_PAD) {
                    0.0
                } else {
                    tick.x
                };
                (x, tick.major_label)
            })
            .collect();
        Self::sticky_header_labels(ticks, false)
    }

    /// Month (or day/week) tick labels, with the in-view period stuck at x = 0
    /// when its gridline has scrolled off the left edge.
    pub fn header_tick_labels(&self, width: f32) -> Vec<(f32, String)> {
        self.header_tick_labels_except(width, None)
    }

    /// Skip empty Year ticks. Shift the Day number off the Today line so "2"
    /// stays readable instead of hiding it.
    pub fn header_tick_labels_except(&self, width: f32, hide_x: Option<f32>) -> Vec<(f32, String)> {
        const HIDE_PAD: f32 = 22.0;
        const SHIFT: f32 = 16.0;
        let ticks: Vec<(f32, String)> = self
            .timeline_ticks(width)
            .into_iter()
            .filter_map(|tick| {
                if tick.label.is_empty() {
                    return None;
                }
                if let Some(today) = hide_x.filter(|x| (tick.x - x).abs() < HIDE_PAD) {
                    return Some((today + SHIFT, tick.label));
                }
                Some((tick.x, tick.label))
            })
            .collect();
        Self::sticky_header_labels(ticks, false)
    }

    fn sticky_header_labels(
        ticks: impl IntoIterator<Item = (f32, String)>,
        skip_sticky: bool,
    ) -> Vec<(f32, String)> {
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
        if skip_sticky {
            return out;
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
        let Some(earliest) = earliest else {
            self.go_today(today);
            self.zoom = 1.0;
            return;
        };
        // Fit the item span only. Pulling Today into a July cluster in November
        // squeezed bars into slivers against the left edge.
        let end = latest.unwrap_or(earliest);
        self.origin = earliest.saturating_sub(2.days());
        self.scroll_x = 0.0;
        let days = end
            .since((Unit::Day, self.origin))
            .map(|span| span.get_days())
            .unwrap_or(1)
            .max(7) as f32
            + 4.0;
        let target_ppd = ((width * 0.78) / days).clamp(0.4, 96.0);
        // Pick the finest scale whose 40% zoom floor can still show the span.
        // Staying on Day at 40% leaves a Jul–Nov cluster hanging off the right.
        let chosen = GanttScale::ALL
            .into_iter()
            .find(|scale| days * scale.base_pixels_per_day() * 0.4 <= width)
            .unwrap_or(GanttScale::Year);
        self.scale = chosen;
        self.zoom = (target_ppd / chosen.base_pixels_per_day()).clamp(0.4, 4.0);
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
                y >= bar.y
                    && y < bar.y + ROW_HEIGHT
                    && x >= bar.x - HANDLE_WIDTH
                    && x <= bar.x + bar.width + HANDLE_WIDTH
            })?
            .clone();
        let kind = if bar.milestone || bar.summary {
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
        const STUB: f32 = 12.0;
        let by_id: HashMap<i64, &BarLayout> = bars.iter().map(|bar| (bar.id, bar)).collect();
        let mut out = Vec::new();
        for bar in bars {
            for pred_id in &bar.predecessors {
                let Some(pred) = by_id.get(pred_id) else {
                    continue;
                };
                let y1 = pred.y + ROW_HEIGHT / 2.0;
                let y2 = bar.y + ROW_HEIGHT / 2.0;
                let far_apart = (pred.y - bar.y).abs() > ROW_HEIGHT + 1.0;
                let x_out = pred.x + pred.width + STUB;
                let x_head = bar.x;
                if far_apart {
                    // Route to the right of both bars so the long vertical does
                    // not slice through intervening placeholder bars.
                    let wrap_x = pred.x.max(bar.x) + pred.width.max(bar.width) + STUB;
                    out.push(DependencyArrow {
                        from_id: *pred_id,
                        to_id: bar.id,
                        x1: x_out,
                        y1,
                        x2: wrap_x,
                        y2,
                        x_head,
                        lane_y: y1,
                    });
                } else {
                    let lane_y = if (pred.y - bar.y).abs() < 1.0 {
                        y1
                    } else {
                        pred.y.min(bar.y) + ROW_HEIGHT
                    };
                    out.push(DependencyArrow {
                        from_id: *pred_id,
                        to_id: bar.id,
                        x1: x_out,
                        y1,
                        x2: bar.x - STUB,
                        y2,
                        x_head,
                        lane_y,
                    });
                }
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
        let before = gantt.date_for_x(200.0);
        gantt.zoom_at(2.0, 200.0, 0.0);
        assert!((gantt.zoom - 2.0).abs() < f32::EPSILON);
        assert_eq!(
            gantt.date_for_x(200.0),
            before,
            "the date under the pointer must stay put"
        );
    }

    #[test]
    fn pointer_in_canvas_excludes_the_tree_and_toolbar() {
        let gantt = GanttView {
            timeline_origin_x: 480.0,
            timeline_origin_y: 140.0,
            timeline_width: 800.0,
            viewport_h: 660.0,
            ..Default::default()
        };
        assert!(gantt.pointer_in_canvas(500.0, 200.0));
        assert!(
            !gantt.pointer_in_canvas(120.0, 200.0),
            "the left grid is not the canvas"
        );
        assert!(
            !gantt.pointer_in_canvas(500.0, 40.0),
            "the toolbar is not the canvas"
        );
        assert!(
            gantt.pointer_in_rows(120.0, 200.0),
            "the left grid is part of the row hover area"
        );
        assert!(
            !gantt.pointer_in_rows(500.0, 40.0),
            "the toolbar is not a row"
        );
    }

    #[test]
    fn zoom_at_center_keeps_the_viewport_midpoint() {
        let mut gantt = GanttView {
            scroll_x: 120.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let before = gantt.date_for_x(400.0);
        gantt.zoom_at_center(2.0);
        assert_eq!(
            gantt.date_for_x(400.0),
            before,
            "wheel zoom off the timeline must keep the viewport centre"
        );
    }

    #[test]
    fn zoom_in_widens_month_columns_and_zoom_out_goes_below_100() {
        let oct1 = Date::new(2026, 10, 1).unwrap();
        let mut gantt = GanttView {
            origin: oct1,
            scroll_x: 0.0,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let before = gantt.period_width(oct1);
        let leading_before = gantt.leading_period_width(800.0);
        assert!(
            (before - 31.0 * 12.0).abs() < 0.5,
            "October at 100% is 31×12px, got {before}"
        );
        gantt.zoom_at_left(1.25);
        let after = gantt.period_width(oct1);
        let leading_after = gantt.leading_period_width(800.0);
        assert!(
            after > before,
            "+ must widen the month column: {before} -> {after}"
        );
        assert!(
            leading_after > leading_before,
            "+ must widen the leftover month at the left edge: {leading_before} -> {leading_after}"
        );
        assert!(
            gantt.scale.button_label(gantt.zoom).contains("125%"),
            "percentage must track column width, got {}",
            gantt.scale.button_label(gantt.zoom)
        );
        gantt.zoom_at_left(0.8);
        assert!(
            (gantt.zoom - 1.0).abs() < 0.01,
            "− from 125% returns to 100%, got {}",
            gantt.zoom
        );
        gantt.zoom_at_left(0.8);
        assert!(gantt.zoom < 1.0, "− must go below 100%, got {}", gantt.zoom);
        assert!(
            gantt.scale.button_label(gantt.zoom).contains("80%"),
            "− below 100% must show the column percentage, got {}",
            gantt.scale.button_label(gantt.zoom)
        );
        assert!(
            gantt.period_width(oct1) < before,
            "− below 100% must shrink month columns"
        );
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
        assert_eq!(
            GanttView::hit_test_bar(&bars, 96.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::ResizeStart),
            "an 8px hit area must extend outside the start edge"
        );
        assert_eq!(
            GanttView::hit_test_bar(&bars, 184.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "an 8px hit area must extend outside the end edge"
        );
        assert_eq!(
            GanttView::hit_test_bar(&bars, 91.0, 4.0).map(|(_, kind)| kind),
            None
        );
        let milestone = BarLayout {
            milestone: true,
            width: MILESTONE_SIZE,
            ..bar.clone()
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
        let summary = BarLayout {
            summary: true,
            ..bar
        };
        assert_eq!(
            GanttView::hit_test_bar(&[summary], 176.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::Move),
            "a parent summary bar has no resize handles"
        );
    }

    #[test]
    fn short_bar_gesture_uses_stored_dates_not_min_width() {
        let mut item = node(1, None);
        item.start_date = Some("2026-07-06T00:00:00Z".into());
        item.target_date = Some("2026-07-06T00:00:00Z".into());
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&[item.clone()]);
        let bars = gantt.layout_bars(&[item.clone()], &rows, &[], 720.0);
        assert!(
            bars[0].width >= 6.0,
            "the painted bar is padded, got {}",
            bars[0].width
        );
        gantt
            .begin_gesture(&bars[0], GestureKind::Move, 0.0, &[item], &[])
            .expect("move");
        let gesture = gantt.gesture.expect("gesture");
        assert_eq!(gesture.start.to_string(), "2026-07-06");
        assert_eq!(gesture.target.to_string(), "2026-07-06");
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
            .begin_gesture(start_bar, GestureKind::Move, 10.0, &nodes, &[])
            .expect("start-only move");
        assert!(
            gantt
                .gesture
                .as_ref()
                .is_some_and(|g| g.had_start && !g.had_target)
        );
        gantt.cancel_gesture();
        gantt
            .begin_gesture(mile_bar, GestureKind::Move, 10.0, &nodes, &[])
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
            .begin_gesture(&bars[0], GestureKind::Move, 10.0, &nodes, &[])
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
        assert_eq!(gantt.origin, Date::new(2026, 7, 4).unwrap());
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        assert!(!bars.is_empty());
        assert!(bars[0].x >= -1.0);
        assert!(bars[0].x + bars[0].width <= 800.0 + 1.0);
        assert!(
            bars[0].width > 40.0,
            "Auto Fit must not squeeze a two-week bar into a sliver, width={}",
            bars[0].width
        );
        assert!(!bars[0].title.is_empty());
    }

    #[test]
    fn auto_fit_switches_scale_when_day_cannot_fit() {
        let mut item = node(1, None);
        item.start_date = Some("2026-07-06T00:00:00Z".into());
        item.target_date = Some("2026-11-20T00:00:00Z".into());
        let nodes = vec![item];
        let mut gantt = GanttView {
            origin: Date::new(2026, 11, 1).unwrap(),
            scale: GanttScale::Day,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.auto_fit(&nodes, &[], 800.0);
        assert_ne!(
            gantt.scale,
            GanttScale::Day,
            "a Jul–Nov span must leave Day so 40% zoom is not the only lever, got {:?} @ {:.2}",
            gantt.scale,
            gantt.zoom
        );
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        assert!(!bars.is_empty());
        assert!(
            bars[0].x + bars[0].width <= 800.0 + 1.0,
            "fitted bar must stay on screen, x={} width={} scale={:?} zoom={}",
            bars[0].x,
            bars[0].width,
            gantt.scale,
            gantt.zoom
        );
    }

    #[test]
    fn auto_fit_ignores_today_outside_the_item_span() {
        let nodes = vec![node(1, None)];
        let mut gantt = GanttView {
            origin: Date::new(2025, 1, 1).unwrap(),
            zoom: 0.4,
            ..Default::default()
        };
        gantt.auto_fit(&nodes, &[], 800.0);
        let today = jiff::Zoned::now().date();
        let latest = Date::new(2026, 7, 20).unwrap();
        if today > latest {
            let today_x = gantt.x_for_date(today);
            assert!(
                today_x > 800.0,
                "Today outside the item span must not be pulled into the viewport, today_x={today_x}"
            );
        }
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        assert!(
            bars[0].width > 40.0,
            "July bars must stay readable when Today is months later, width={}",
            bars[0].width
        );
        assert!(bars[0].x >= -1.0);
        assert!(bars[0].x + bars[0].width <= 800.0 + 1.0);
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
        let today = jiff::Zoned::now().date();
        assert_eq!(gantt.origin, today);
        let today_x = gantt.today_x(today);
        let expected = gantt.timeline_width.max(1.0) * TODAY_ALIGN;
        assert!(
            (today_x - expected).abs() < 1.5,
            "Today line at {today_x} should sit near {expected}"
        );
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
        assert!(
            (gantt.tree_width - (800.0 - SPLITTER_WIDTH - MIN_TIMELINE_WIDTH)).abs() < f32::EPSILON
        );
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
            gantt.tree_width <= 700.0 - SPLITTER_WIDTH - MIN_TIMELINE_WIDTH + 0.5,
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
            (gantt.tree_width - (1024.0 - SPLITTER_WIDTH - MIN_TIMELINE_WIDTH)).abs() < 0.5,
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
    fn go_today_aligns_the_current_period_to_the_axis() {
        let mut gantt = GanttView::default();
        let today = Date::new(2026, 10, 30).unwrap();
        gantt.go_today(today);
        let today_x = gantt.today_x(today);
        let expected = gantt.timeline_width.max(1.0) * TODAY_ALIGN;
        assert!(
            (today_x - expected).abs() < 1.5,
            "Today line at {today_x} should sit near the timeline center, expected {expected}"
        );
        let oct1 = Date::new(2026, 10, 1).unwrap();
        assert!(
            (today_x - gantt.x_for_date(oct1)).abs() > 20.0,
            "Today must not sit on the month gridline"
        );
        let title = gantt.viewport_range_title();
        let axis = gantt.header_tick_labels(gantt.timeline_width.max(800.0));
        let first = axis.first().map(|(_, label)| label.as_str()).unwrap_or("");
        assert!(
            title.starts_with(first) || first.is_empty(),
            "header {title:?} must match the painted axis start {first:?}"
        );
    }

    #[test]
    fn go_today_in_early_november_titles_the_visible_start() {
        let mut gantt = GanttView::default();
        let today = Date::new(2026, 11, 2).unwrap();
        gantt.go_today(today);
        let title = gantt.viewport_range_title();
        let axis = gantt.header_tick_labels(gantt.timeline_width.max(800.0));
        let first = axis.first().map(|(_, label)| label.as_str()).unwrap_or("");
        assert!(
            title.starts_with(first) || first.is_empty(),
            "header {title:?} must match the painted axis start {first:?}, not a leftover month"
        );
        assert!(
            !title.starts_with("Sep") || first.starts_with("Sep"),
            "toolbar must not claim September when the axis starts later: {title} vs {axis:?}"
        );
    }

    #[test]
    fn range_title_follows_the_visible_start() {
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let start = gantt.viewport_start_date();
        let title = gantt.viewport_range_title();
        assert_eq!(start, Date::new(2026, 7, 1).unwrap());
        assert!(
            title.starts_with("Jul"),
            "header {title:?} must follow the visible start ({start}), not the centre"
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
                &[],
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
                &[],
            )
            .expect("link again");
        assert_eq!(gantt.take_link_gesture(), Some((1, None)));
        assert!(gantt.gesture.is_none());
    }

    #[test]
    fn placeholder_drop_uses_painted_dates_not_today_origin() {
        let mut item = node(1, None);
        item.start_date = None;
        item.target_date = None;
        item.title = "Backlog grooming".into();
        item.iteration_path = "Shop\\FY26\\Sprint 12".into();
        let nodes = vec![item];
        let iterations = vec![ado_core::IterationNode {
            path: "Shop\\FY26\\Sprint 12".into(),
            name: "Sprint 12".into(),
            start_date: Some("2026-08-10T00:00:00Z".into()),
            finish_date: Some("2026-08-21T00:00:00Z".into()),
            children: Vec::new(),
        }];
        let mut gantt = GanttView {
            origin: Date::new(2026, 10, 30).unwrap(),
            scroll_x: -TODAY_MARGIN_PX,
            timeline_origin_x: 486.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &iterations, 720.0);
        let bar = bars.iter().find(|bar| bar.id == 1).expect("placeholder");
        let drop = Date::new(2026, 8, 20).unwrap();
        let drop_x = gantt.x_for_date(drop);
        gantt
            .begin_gesture(bar, GestureKind::Move, drop_x, &nodes, &iterations)
            .expect("placeholder move");
        gantt.update_gesture(drop_x);
        let gesture = gantt.gesture.clone().expect("live");
        let (start, target) = GanttView::gesture_patch_dates(&gesture);
        let start = start.expect("start");
        assert_eq!(
            start.month(),
            8,
            "drop around Aug 20 must not save November (got {start}–{target:?})"
        );
        assert!(
            start >= Date::new(2026, 8, 1).unwrap() && start <= Date::new(2026, 8, 31).unwrap(),
            "placeholder drop around Aug 20 saved {start}–{target:?}"
        );
        assert_eq!(gantt.drop_date(gantt.timeline_origin_x + drop_x), drop);
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
            .begin_gesture(&bars[0], GestureKind::Move, 10.0, &nodes, &[])
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
        let pred = bars.iter().find(|bar| bar.id == 1).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 2).unwrap();
        assert!(
            arrows[0].x1 >= pred.x + pred.width + 8.0,
            "outgoing stub must leave the source bar, got x1 {} vs right {}",
            arrows[0].x1,
            pred.x + pred.width
        );
        assert!(
            arrows[0].x2 <= succ.x - 8.0,
            "incoming vertical must detour left of the target edge, got {} vs {}",
            arrows[0].x2,
            succ.x
        );
        assert!(
            (arrows[0].x_head - succ.x).abs() < 1.0,
            "arrowhead must sit on the target start, got {} vs {}",
            arrows[0].x_head,
            succ.x
        );
        assert!(
            arrows[0].x1 > arrows[0].x2,
            "adjacent bars must detour: outgoing stub {} should be to the right of incoming {}",
            arrows[0].x1,
            arrows[0].x2
        );
        assert!(
            arrows[0].lane_y >= pred.y.min(succ.y) + ROW_HEIGHT - 1.0,
            "horizontal run must sit in the gap between rows, got lane {}",
            arrows[0].lane_y
        );
    }

    #[test]
    fn link_remove_sits_at_the_polyline_midpoint() {
        let arrow = DependencyArrow {
            from_id: 1,
            to_id: 2,
            x1: 50.0,
            y1: 20.0,
            x2: 400.0,
            y2: 220.0,
            x_head: 412.0,
            lane_y: 200.0,
        };
        let mid = arrow.midpoint();
        let last_run = ((arrow.x1 + arrow.x2) * 0.5, arrow.lane_y);
        assert!(
            (mid.0 - last_run.0).abs() > 40.0,
            "× must follow path length, not the last horizontal; got {mid:?} vs {last_run:?}"
        );
        assert!(
            (mid.1 - arrow.lane_y).abs() < 1.0,
            "a long lane should own the midpoint, got {mid:?}"
        );
    }

    #[test]
    fn resize_end_moves_the_outgoing_dependency_stub() {
        let pred_node = node(1, None);
        let mut succ_node = node(2, None);
        succ_node.start_date = Some("2026-07-21T00:00:00Z".into());
        succ_node.target_date = Some("2026-08-04T00:00:00Z".into());
        succ_node.predecessors = vec![1];
        let nodes = vec![pred_node, succ_node];
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let before = gantt.layout_dependency_arrows(&bars);
        let old_x1 = before[0].x1;
        let pred = bars.iter().find(|bar| bar.id == 1).unwrap().clone();
        gantt
            .begin_gesture(
                &pred,
                GestureKind::ResizeEnd,
                pred.x + pred.width,
                &nodes,
                &[],
            )
            .expect("resize end");
        let ppd = gantt.scale.pixels_per_day(gantt.zoom);
        gantt.update_gesture(pred.x + pred.width + ppd * 5.0);
        let live = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let live_arrows = gantt.layout_dependency_arrows(&live);
        let live_pred = live.iter().find(|bar| bar.id == 1).unwrap();
        assert_eq!(live_arrows.len(), 1);
        assert!(
            (live_arrows[0].x1 - (live_pred.x + live_pred.width + 12.0)).abs() < 1.0,
            "outgoing stub must follow the live end edge, got {} vs {}",
            live_arrows[0].x1,
            live_pred.x + live_pred.width + 12.0
        );
        assert!(
            live_arrows[0].x1 > old_x1 + 4.0,
            "the stub must leave the old attachment point {old_x1}, got {}",
            live_arrows[0].x1
        );
    }

    #[test]
    fn resize_end_patches_target_only() {
        let nodes = vec![node(1, None)];
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let origin = bars[0].x + bars[0].width - 2.0;
        gantt
            .begin_gesture(&bars[0], GestureKind::ResizeEnd, origin, &nodes, &[])
            .expect("resize end");
        let ppd = gantt.scale.pixels_per_day(gantt.zoom);
        gantt.update_gesture(origin + ppd * 2.0);
        let gesture = gantt.gesture.clone().expect("live");
        let (start, target) = GanttView::gesture_patch_dates(&gesture);
        assert!(
            start.is_none(),
            "end-handle drag must not rewrite Start Date, got {start:?}"
        );
        assert_eq!(
            target,
            Some(Date::new(2026, 7, 22).unwrap()),
            "end-handle drag should move Target Date by the snapped day delta"
        );
    }

    #[test]
    fn live_drag_preview_snaps_the_bar_to_whole_days() {
        let nodes = vec![node(1, None)];
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let origin = bars[0].x + bars[0].width / 2.0;
        gantt
            .begin_gesture(&bars[0], GestureKind::Move, origin, &nodes, &[])
            .expect("move");
        let ppd = gantt.scale.pixels_per_day(gantt.zoom);
        gantt.update_gesture(origin + ppd * 2.4);
        let gesture = gantt.gesture.clone().expect("live");
        assert_eq!(
            gesture.live_start,
            Date::new(2026, 7, 8).unwrap(),
            "preview must snap 2.4 days to 2, got {}",
            gesture.live_start
        );
        let preview = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let live = preview.iter().find(|bar| bar.id == 1).unwrap();
        let expected_x = gantt.x_for_date(gesture.live_start);
        assert!(
            (live.x - expected_x).abs() < 1.0,
            "painted bar must follow the snapped preview, got {} vs {expected_x}",
            live.x
        );
        assert_eq!(
            GanttView::drag_preview_text(&gesture),
            format!(
                "{} → {}",
                compact_date(gesture.live_start),
                compact_date(gesture.live_target)
            )
        );
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
        let body_h = (gantt.viewport_h - TIMELINE_HEADER_H).max(0.0);
        let y = 5.0 * ROW_HEIGHT;
        assert!(
            (gantt.scroll_y - (y + ROW_HEIGHT - body_h).max(0.0)).abs() < 0.5,
            "PageDown must scroll by the body under the axis, not the full viewport_h"
        );
    }

    #[test]
    fn year_scale_keeps_the_centre_date_and_does_not_repeat_year_labels() {
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let before = gantt.viewport_center_date();
        gantt.set_scale(GanttScale::Year);
        assert_eq!(
            gantt.viewport_center_date(),
            before,
            "Month → Year must keep the centred date, not jump to Jan 1"
        );
        assert_ne!(gantt.origin, Date::new(2026, 1, 1).unwrap());
        let majors: Vec<_> = gantt
            .header_major_labels(800.0)
            .into_iter()
            .map(|(_, label)| label)
            .collect();
        let ticks: Vec<_> = gantt
            .header_tick_labels(800.0)
            .into_iter()
            .map(|(_, label)| label)
            .collect();
        assert!(
            majors.iter().any(|label| label == "2026"),
            "year header must keep the year on the major row: {majors:?}"
        );
        assert!(
            ticks.iter().all(|label| !majors.contains(label)),
            "year ticks must not repeat the major year labels: ticks={ticks:?} majors={majors:?}"
        );
        let xs: Vec<f32> = gantt
            .timeline_ticks(1200.0)
            .into_iter()
            .filter(|tick| tick.major)
            .map(|tick| tick.x)
            .collect();
        let widths: Vec<f32> = xs.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert!(widths.len() >= 2, "need at least two year columns: {xs:?}");
        let first = widths[0];
        assert!(
            widths.iter().all(|width| (width - first).abs() < 3.0),
            "year columns must be equal width, got {widths:?}"
        );
    }

    #[test]
    fn day_scale_keeps_the_july_centre() {
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let before = gantt.viewport_center_date();
        gantt.set_scale(GanttScale::Day);
        assert_eq!(gantt.viewport_center_date(), before);
        let title = gantt.scale.range_title(before);
        assert_eq!(title, compact_date(before));
        assert!(
            !title.contains(',') && !title.contains("2026"),
            "Day header must use compact month-day, got {title}"
        );
    }

    #[test]
    fn day_header_keeps_the_month_and_shifts_today_number() {
        let today = Date::new(2026, 11, 2).unwrap();
        let gantt = GanttView {
            origin: today,
            scale: GanttScale::Day,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let today_x = gantt.today_x(today);
        let labels = gantt.header_tick_labels_except(800.0, Some(today_x));
        assert!(
            labels.iter().any(|(_, label)| label == "2"),
            "today's day number must stay visible: {labels:?}"
        );
        assert!(
            labels
                .iter()
                .filter(|(_, label)| label == "2")
                .all(|(x, _)| (x - today_x).abs() >= 14.0),
            "the 2 must sit off the Today line: {labels:?} today_x={today_x}"
        );
        let majors = gantt.header_major_labels_except(800.0, Some(today_x));
        assert!(
            majors.iter().any(|(_, label)| label.contains("Nov")),
            "Day zoom must keep the Nov 2026 month label, got {majors:?}"
        );
        assert!(
            majors
                .iter()
                .filter(|(_, label)| label.contains("Nov"))
                .all(|(x, _)| (x - today_x).abs() >= 80.0 || *x <= 1.0),
            "month label must not sit on the Today flag: {majors:?} today_x={today_x}"
        );
        assert_eq!(gantt.scale.tick_label(today), "2");
    }

    #[test]
    fn zoom_steps_land_on_nice_percentages() {
        assert_eq!(GanttView::next_zoom(1.0, 1), 1.25);
        assert_eq!(GanttView::next_zoom(1.25, 1), 1.5);
        assert_eq!(GanttView::next_zoom(1.0, -1), 0.75);
        assert_eq!(GanttView::next_zoom(0.74, 1), 0.75);
    }

    #[test]
    fn auto_fit_toolbar_matches_the_painted_axis() {
        let mut item = node(1, None);
        item.start_date = Some("2026-07-01T00:00:00Z".into());
        item.target_date = Some("2026-10-15T00:00:00Z".into());
        let nodes = vec![item];
        let mut gantt = GanttView {
            origin: Date::new(2026, 1, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.auto_fit(&nodes, &[], 800.0);
        let title = gantt.viewport_range_title();
        let axis = gantt.header_tick_labels(800.0);
        let first = axis.first().map(|(_, label)| label.as_str()).unwrap_or("");
        assert!(
            title.starts_with(first) || first.is_empty(),
            "toolbar {title:?} must match the first axis tick {first:?} ({axis:?})"
        );
        assert!(
            !title.starts_with("Jun") || first.starts_with("Jun"),
            "toolbar must not say June when the axis starts in July: {title} vs {axis:?}"
        );
    }

    #[test]
    fn far_apart_dependency_wraps_to_the_right_of_both_bars() {
        let mut a = node(1, None);
        let mut b = node(2, None);
        let mut c = node(3, None);
        a.parent_id = None;
        b.parent_id = Some(1);
        c.parent_id = None;
        c.start_date = Some("2026-07-21T00:00:00Z".into());
        c.target_date = Some("2026-08-04T00:00:00Z".into());
        c.predecessors = vec![1];
        let nodes = vec![a, b, c];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert_eq!(arrows.len(), 1);
        let pred = bars.iter().find(|bar| bar.id == 1).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 3).unwrap();
        assert!(
            (succ.y - pred.y).abs() > ROW_HEIGHT + 1.0,
            "fixture must place the successor at least two rows below"
        );
        assert!(
            arrows[0].x2 >= pred.x + pred.width && arrows[0].x2 >= succ.x + succ.width,
            "the long vertical must sit to the right of both bars, got x2={} pred_right={} succ_right={}",
            arrows[0].x2,
            pred.x + pred.width,
            succ.x + succ.width
        );
    }

    #[test]
    fn filler_rows_cover_the_unused_viewport() {
        assert_eq!(GanttView::filler_row_count(2, 400.0), 8);
        assert_eq!(GanttView::filler_row_count(20, 400.0), 0);
    }

    #[test]
    fn first_visible_row_follows_scroll_y() {
        let gantt = GanttView {
            scroll_y: 50.0,
            ..Default::default()
        };
        assert_eq!(gantt.first_visible_row_index(), 1);
    }

    #[test]
    fn snapshot_restore_keeps_scroll_and_scale() {
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 15).unwrap(),
            scroll_x: 120.0,
            scroll_y: 80.0,
            scale: GanttScale::Week,
            zoom: 1.5,
            ..Default::default()
        };
        let saved = gantt.snapshot();
        gantt.scroll_x = 0.0;
        gantt.scroll_y = 0.0;
        gantt.scale = GanttScale::Month;
        gantt.restore(saved);
        assert_eq!(gantt.scroll_x, 120.0);
        assert_eq!(gantt.scroll_y, 80.0);
        assert_eq!(gantt.scale, GanttScale::Week);
        assert!((gantt.zoom - 1.5).abs() < f32::EPSILON);
        assert_eq!(gantt.origin, Date::new(2026, 7, 15).unwrap());
    }
}
