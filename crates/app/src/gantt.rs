use std::collections::{HashMap, HashSet};

use ado_core::{
    DEFAULT_BAR_DAYS, GanttBarKind, IterationHint, IterationNode, WorkItemNode, compact_date,
    dates_column_text, format_date_range, from_gantt_exclusive_end, gantt_bar_display,
    iteration_hint_for_path, parse_iso_date,
};
use jiff::civil::Date;
use jiff::{ToSpan, Unit};

pub const ROW_HEIGHT: f32 = 40.0;
pub const DEFAULT_TREE_WIDTH: f32 = 760.0;
pub const MIN_TREE_WIDTH: f32 = 240.0;
pub const MIN_TIMELINE_WIDTH: f32 = 240.0;
pub const HARD_MIN_TREE_WIDTH: f32 = 80.0;
pub const SPLITTER_WIDTH: f32 = 8.0;
pub const SPLITTER_STEP: f32 = 16.0;
/// Inner grip on each painted end. Hits stay inside the bar, not in the
/// link-handle gap, so a click on the bar's last pixels resizes.
pub const HANDLE_WIDTH: f32 = 8.0;
/// Selected tree/timeline row tint. Stronger than zebra stripes so the row reads.
pub const SELECTED_ROW_TINT: f32 = 0.65;
/// Left accent on the selected tree/timeline row.
pub const SELECTED_ACCENT_W: f32 = 3.0;
/// Year-scale bars shorter than this still paint a hittable body, not a sliver.
pub const YEAR_MIN_BAR_WIDTH: f32 = 16.0;
/// Floor so a zero-width day still paints on Month/Week/Quarter.
const MIN_BAR_WIDTH: f32 = 6.0;
/// Pixels per tree depth. A spacer child, not padding, so painted x matches depth.
pub const TREE_INDENT: f32 = 20.0;
/// Painted link connector. Smaller than the inner resize grip so the
/// circle is not mistaken for an edge handle.
pub const LINK_HANDLE: f32 = 8.0;
pub const LINK_HANDLE_HIT: f32 = 16.0;
/// Gap between the painted bar end and the outside link handle.
pub const LINK_HANDLE_GAP: f32 = 6.0;
/// Parallel dependency lanes stay at least this far apart (stroke is 2px).
pub const PARALLEL_LANE_MIN: f32 = 10.0;
/// Inbound/wrap verticals. 16px centre-to-centre so the 2px stroke still
/// reads as ≥8px after clip and antialiasing (live measure was 4–7px).
pub const PARALLEL_VERT_MIN: f32 = 16.0;
/// Parent-overflow `!` on the bar. The circle is larger than the glyph so a
/// 20px font still reads as a true 14px bang instead of a 10px mark.
pub const OVERFLOW_BADGE: f32 = 22.0;
/// Gap after the link-handle hit box before the overflow badge.
pub const OVERFLOW_BADGE_GAP: f32 = 4.0;
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
/// Start-only items are a short marker, not a fake wide bar.
pub const OPEN_ENDED_MARKER_WIDTH: f32 = 18.0;
/// ▸ sits on the right of a start-only marker; keep Move on the rest of it.
pub const OPEN_ENDED_LINK_HIT: f32 = 6.0;
pub const MILESTONE_SIZE: f32 = 16.0;
pub const SUMMARY_BAR_H: f32 = 14.0;
pub const PLACEHOLDER_BAR_H: f32 = 24.0;
pub const LEAF_BAR_H: f32 = 28.0;
pub const DATES_COL_WIDTH: f32 = 140.0;
/// Left gutter for unscheduled / no-field pills so they do not stack on Today.
pub const UNSCHEDULED_GUTTER_X: f32 = 10.0;
/// Right edge of that gutter (pill plus a few px of slack). A drop still
/// left of this has not chosen a date yet.
pub const UNSCHEDULED_GUTTER_RIGHT: f32 = UNSCHEDULED_GUTTER_X + MILESTONE_SIZE + 6.0;
pub const NO_FIELDS_HINT_W: f32 = 88.0;
pub const DETAILS_PILL_WIDTH: f32 = 36.0;
pub const ID_COL_WIDTH: f32 = 64.0;
pub const TYPE_COL_WIDTH: f32 = 88.0;
pub const ASSIGNEE_COL_WIDTH: f32 = 88.0;
pub const EXPANDER_SLOT: f32 = 18.0;
pub const TODAY_PILL_W: f32 = 44.0;
pub const TODAY_PILL_GAP: f32 = 6.0;
/// Keep year/month majors off the Today pill; Year "2027" must not crowd it.
const HEADER_TODAY_LABEL_GAP: f32 = 72.0;
const MAJOR_LABEL_ESTIMATE: f32 = 84.0;
const TICK_MIN_GAP: f32 = 18.0;
/// Keep edge tick/year labels off the clip.
const HEADER_EDGE_PAD: f32 = 10.0;
/// Extra inset so the first day/month number is not flush against the splitter.
const HEADER_LEFT_PAD: f32 = 10.0;

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

    pub fn button_label(self, _zoom: f32) -> String {
        self.label().to_string()
    }

    /// Scale names stay bare; zoom % lives on one shared control so Day/Week/…
    /// do not each invent a different percent.
    pub fn toolbar_label(self, zoom: f32, _selected: bool) -> String {
        self.button_label(zoom)
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
            Self::Month | Self::Year => month_abbrev(date).to_string(),
            Self::Quarter => {
                let q = ((date.month() as i32 - 1) / 3) + 1;
                format!("Q{q}")
            }
        }
    }

    /// Grid/header step. Year walks months so labels are not a single cramped year.
    fn tick_floor(self, date: Date) -> Date {
        match self {
            Self::Year => Date::new(date.year(), date.month(), 1).unwrap_or(date),
            other => other.floor_date(date),
        }
    }

    fn tick_next(self, date: Date) -> Date {
        match self {
            Self::Year => date.saturating_add(1.month()),
            other => other.next_date(date),
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

fn compact_date_span(start: Date, end: Date) -> String {
    if start >= end {
        compact_date(start)
    } else if start.year() == end.year() && start.month() == end.month() {
        format!("{} – {}", compact_date(start), end.day())
    } else {
        format!("{} – {}", compact_date(start), compact_date(end))
    }
}

fn quarter_title(date: Date) -> String {
    let q = ((date.month() as i32 - 1) / 3) + 1;
    format!("{} Q{q}", date.year())
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
    /// Park baked into `scroll_x`. Restored with the pose so a mid-ease
    /// unsnap subtracts this delta, not the destination's.
    pub year_gutter_snap: f32,
    /// After Today. Restored with the pose so a team switch or persist
    /// round-trip does not leave the previous camera's Today-follow on.
    pub keep_today: bool,
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
            year_gutter_snap: self.year_gutter_snap
                + (other.year_gutter_snap - self.year_gutter_snap) * t,
            keep_today: other.keep_today,
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
    pub date: Date,
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
    /// Gutter pill for unscheduled items (not a date on the axis).
    pub marker: bool,
    /// Start-only used to paint as a ▸ marker; kept for hit-test compat.
    pub open_ended: bool,
}

impl BarLayout {
    pub fn paint_h(&self) -> f32 {
        if self.milestone || self.marker {
            MILESTONE_SIZE
        } else if self.summary {
            SUMMARY_BAR_H
        } else if self.kind == GanttBarKind::Unscheduled || self.kind == GanttBarKind::NoFields {
            PLACEHOLDER_BAR_H
        } else {
            LEAF_BAR_H
        }
    }

    pub fn paint_top(&self) -> f32 {
        ((ROW_HEIGHT - self.paint_h()) / 2.0).max(2.0)
    }

    pub fn can_resize(&self) -> bool {
        self.kind != GanttBarKind::Unscheduled
            && self.kind != GanttBarKind::NoFields
            && !self.milestone
            && !self.marker
            && !self.open_ended
            // Strictly wider than two grips so a YEAR_MIN_BAR_WIDTH (16px)
            // bar keeps a Move body. `>= 16` would cover the whole bar.
            // Parents with their own dates resize too; rollup-only bars
            // (unscheduled / no-fields) never reach this path.
            && self.width > HANDLE_WIDTH * 2.0
    }

    /// Inner 8px of each end, split at the midpoint on bars shorter than 16px.
    pub fn inner_grip_width(&self) -> f32 {
        HANDLE_WIDTH.min(self.width / 2.0).max(1.0)
    }

    /// 8px rollup edges only when they leave a Move body in the center.
    /// Target-only diamonds (16px) and year-scale slivers stay all-Move.
    /// Parents with their own dates resize instead of showing a not-allowed edge.
    pub fn shows_rollup_edges(&self) -> bool {
        self.summary
            && !self.can_resize()
            && !self.milestone
            && !self.marker
            && self.width > HANDLE_WIDTH * 2.0
    }

    /// Classify a timeline-x on this painted bar. `None` is the move body.
    pub fn resize_kind_at(&self, x: f32) -> Option<GestureKind> {
        match self.hit_zone_at_window_x(x, self.x, self.x + self.width) {
            BarHitZone::ResizeStart => Some(GestureKind::ResizeStart),
            BarHitZone::ResizeEnd => Some(GestureKind::ResizeEnd),
            BarHitZone::Move | BarHitZone::RollupEdge | BarHitZone::Outside => None,
        }
    }

    /// Classify a pointer against the bar's painted left/right in the SAME
    /// coordinate space as `window_x` (window pixels, not layout-local x).
    pub fn hit_zone_at_window_x(
        &self,
        window_x: f32,
        painted_left: f32,
        painted_right: f32,
    ) -> BarHitZone {
        if window_x < painted_left || window_x > painted_right {
            return BarHitZone::Outside;
        }
        let width = (painted_right - painted_left).max(1.0);
        let grip = HANDLE_WIDTH.min(width / 2.0).max(1.0);
        let on_start = window_x <= painted_left + grip;
        let on_end = window_x >= painted_right - grip;
        if self.summary && !self.can_resize() {
            // Two 8px edges meet on a 16px bar; skip them so a short parent
            // (diamond, year sliver) still has a Move body.
            if self.shows_rollup_edges() && width > HANDLE_WIDTH * 2.0 && (on_start || on_end) {
                return BarHitZone::RollupEdge;
            }
            return BarHitZone::Move;
        }
        if !self.can_resize() {
            return BarHitZone::Move;
        }
        if on_start {
            BarHitZone::ResizeStart
        } else if on_end {
            BarHitZone::ResizeEnd
        } else {
            BarHitZone::Move
        }
    }
}

/// Hit classification for a pointer vs a painted bar, in one coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarHitZone {
    ResizeStart,
    ResizeEnd,
    Move,
    RollupEdge,
    Outside,
}

impl BarHitZone {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ResizeStart => "resize-start",
            Self::ResizeEnd => "resize-end",
            Self::Move => "move",
            Self::RollupEdge => "rollup-edge",
            Self::Outside => "outside",
        }
    }

    pub fn gesture_kind(self) -> Option<GestureKind> {
        match self {
            Self::ResizeStart => Some(GestureKind::ResizeStart),
            Self::ResizeEnd => Some(GestureKind::ResizeEnd),
            Self::Move => Some(GestureKind::Move),
            Self::RollupEdge | Self::Outside => None,
        }
    }
}

/// Inner start grip: the first `HANDLE_WIDTH` of the painted bar.
pub fn resize_start_handle(bar: &BarLayout) -> (f32, f32) {
    let width = bar.inner_grip_width();
    (bar.x, width)
}

/// Inner end grip: the last `HANDLE_WIDTH` of the painted bar.
pub fn resize_end_handle(bar: &BarLayout) -> (f32, f32) {
    let width = bar.inner_grip_width();
    (bar.x + bar.width - width, width)
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
    /// Head sits on the successor's right (gutter pills only). Finish-to-start
    /// links always enter the target start, so this is `false` for dated bars.
    pub inbound_right: bool,
}

impl DependencyArrow {
    const WRAP_STUB: f32 = 12.0;

    /// Wrap-around routing drops at the source stub, then runs on the
    /// successor row into a short inbound. Used for overlap/backward FS so
    /// the path does not detour left of the source start.
    pub fn wraps_around_successor(&self) -> bool {
        self.x2 > self.x_head + Self::WRAP_STUB
    }

    pub fn stub_start(&self) -> f32 {
        if self.wraps_around_successor() {
            if self.inbound_right {
                self.x_head + Self::WRAP_STUB
            } else {
                self.x_head - Self::WRAP_STUB
            }
        } else {
            self.x2
        }
    }

    /// Timeline x of the incoming ▶/◀. Always the successor edge (`x_head`),
    /// never the inbound elbow (`x2`) — bar-dodging can push `x2` far left.
    pub fn arrowhead_left(&self) -> f32 {
        if self.inbound_right {
            self.x_head - 4.0
        } else {
            self.x_head - 16.0
        }
    }

    /// Sit above the outgoing stub, off the predecessor bar and the line.
    const REMOVE_ABOVE: f32 = 18.0;

    pub fn remove_control_pos(&self) -> (f32, f32) {
        (self.x1, self.y1 - Self::REMOVE_ABOVE)
    }

    pub fn gutter_y(&self) -> f32 {
        self.y2 - ROW_HEIGHT / 2.0
    }

    /// Visual midpoint along the polyline (outgoing, lane, incoming, stub).
    pub fn midpoint(&self) -> (f32, f32) {
        let inbound = self.stub_start();
        let pts = if self.wraps_around_successor() {
            let gutter = self.gutter_y();
            vec![
                (self.x1, self.y1),
                (self.x1, self.lane_y),
                (self.x2, self.lane_y),
                (self.x2, gutter),
                (inbound, gutter),
                (inbound, self.y2),
                (self.x_head, self.y2),
            ]
        } else {
            vec![
                (self.x1, self.y1),
                (self.x1, self.lane_y),
                (self.x2, self.lane_y),
                (self.x2, self.y2),
                (self.x_head, self.y2),
            ]
        };
        let mut total = 0.0;
        let mut segs = Vec::new();
        for pair in pts.windows(2) {
            let dx = pair[1].0 - pair[0].0;
            let dy = pair[1].1 - pair[0].1;
            let len = (dx * dx + dy * dy).sqrt();
            segs.push(len);
            total += len;
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
    /// Pointer is on the bar (or its handles), not merely the selected row.
    /// Resize/link dots paint from this, so a selected tree row stays clean.
    pub bar_hovered_id: Option<i64>,
    pub selected_link: Option<(i64, i64)>,
    pub hovered_link: Option<(i64, i64)>,
    /// After a date drop, keep that bar's tooltip mounted without waiting for
    /// a new mouse-enter. Cleared when hover leaves the bar.
    pub pin_bar_tooltip: Option<i64>,
    /// Skip the remount hover-leave that fires right after a date drop so the
    /// pinned tooltip is not cleared while the pointer is still on the bar.
    pub pin_ignore_leave: bool,
    pub collapsed: HashSet<i64>,
    pub gesture: Option<Gesture>,
    pub pending_nudge: Option<PendingNudge>,
    pub context_menu: Option<ContextTarget>,
    pub splitter_drag: Option<(f32, f32)>,
    pub schedule_drag: Option<i64>,
    /// After mouse-up, ignore hover until the pointer moves so a bar drag
    /// does not leave a stuck row highlight under a stationary cursor.
    pub hover_locked: bool,
    /// Scroll delta applied by [`GanttView::snap_cramped_leading_column`]'s
    /// Year/Quarter gutter park. [`GanttView::unsnap_year_gutter`] subtracts
    /// this exact amount so Month→Year→Month does not drift.
    pub year_gutter_snap: f32,
    /// After [`GanttView::go_today`], toolbar scale/zoom keeps Today still
    /// instead of re-anchoring to a Year park at x=0.
    pub keep_today: bool,
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
            bar_hovered_id: None,
            selected_link: None,
            hovered_link: None,
            pin_bar_tooltip: None,
            pin_ignore_leave: false,
            collapsed: HashSet::new(),
            gesture: None,
            pending_nudge: None,
            context_menu: None,
            splitter_drag: None,
            schedule_drag: None,
            hover_locked: false,
            year_gutter_snap: 0.0,
            keep_today: false,
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

    /// Pointer-anchored zoom. Drops Today-follow so a later toolbar scale
    /// does not jump back to Today (toolbar +/− uses [`Self::zoom_step_keeping`]).
    pub fn zoom_at(&mut self, factor: f32, pointer_x: f32, timeline_origin_x: f32) {
        self.keep_today = false;
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

    /// Toolbar +/− multiplies zoom by this ratio so 60% → 75% → 94% instead of
    /// jumping 60 → 75 → 100.
    pub const ZOOM_RATIO: f32 = 1.25;

    pub fn next_zoom(zoom: f32, direction: i32) -> f32 {
        let raw = if direction >= 0 {
            zoom * Self::ZOOM_RATIO
        } else {
            zoom / Self::ZOOM_RATIO
        };
        ((raw * 10_000.0).round() / 10_000.0).clamp(0.4, 4.0)
    }

    pub fn zoom_step_at_center(&mut self, direction: i32) {
        let keep_x = self.timeline_width.max(1.0) * 0.5;
        self.zoom_step_keeping(direction, keep_x);
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
            .find(|(x, _)| *x > HEADER_LEFT_PAD + 1.0)
            .map(|(x, _)| x)
            .unwrap_or(0.0)
    }

    pub fn viewport_range_title(&self) -> String {
        let start = self.viewport_axis_start_date();
        let width = self.timeline_width.max(1.0);
        let end = self.date_for_x((width - 1.0).max(0.0));
        match self.scale {
            GanttScale::Month => {
                if start.month() != end.month() || start.year() != end.year() {
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
                } else {
                    self.scale.range_title(start)
                }
            }
            GanttScale::Day | GanttScale::Week => compact_date_span(start, end),
            GanttScale::Quarter => {
                let (start, end) = self.painted_quarter_range();
                let left = quarter_title(start);
                let right = quarter_title(end);
                if left == right {
                    left
                } else {
                    format!("{left} – {right}")
                }
            }
            GanttScale::Year => {
                if start.year() == end.year() {
                    start.year().to_string()
                } else {
                    format!("{} – {}", start.year(), end.year())
                }
            }
        }
    }

    /// First and last painted Quarter columns, matching `header_tick_labels`
    /// (48px gap, leftover dropped when cramped). `viewport_axis_start_date`
    /// uses `TICK_MIN_GAP` and would title a leftover Q2 the axis did not paint.
    fn painted_quarter_range(&self) -> (Date, Date) {
        let width = self.timeline_width.max(1.0);
        let labels = self.header_tick_labels(width);
        let ticks = self.timeline_ticks(width);
        let resolve = |x: f32, label: &str| -> Option<Date> {
            ticks
                .iter()
                .filter(|tick| tick.label == label)
                .min_by(|a, b| {
                    let da = (a.x.max(0.0) - x).abs();
                    let db = (b.x.max(0.0) - x).abs();
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|tick| tick.date)
        };
        let start = labels
            .first()
            .and_then(|(x, label)| resolve(*x, label))
            .unwrap_or_else(|| self.viewport_axis_start_date());
        let end = labels
            .last()
            .and_then(|(x, label)| resolve(*x, label))
            .unwrap_or_else(|| self.date_for_x((width - 1.0).max(0.0)));
        (start, end)
    }

    /// Date of the leftover sticky tick when it is painted; otherwise the first
    /// in-view tick. Must match `header_tick_labels` so the toolbar does not
    /// name a month the axis dropped to keep Day 24 (or Jul) readable.
    pub fn viewport_axis_start_date(&self) -> Date {
        let width = self.timeline_width.max(1.0);
        let ticks = self.timeline_ticks(width);
        let leftover = ticks
            .iter()
            .rev()
            .find(|tick| tick.x <= 0.0 && !tick.label.is_empty());
        let first_in_view = ticks
            .iter()
            .find(|tick| tick.x > 0.0 && !tick.label.is_empty());
        let room = first_in_view.is_none_or(|tick| tick.x >= TICK_MIN_GAP);
        if let Some(tick) = leftover.filter(|_| room) {
            tick.date
        } else if let Some(tick) = first_in_view.or(leftover) {
            tick.date
        } else {
            self.viewport_start_date()
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
        } else if x > width - TODAY_MARGIN_PX {
            self.scroll_x += x - width * 0.35;
        }
    }

    pub fn clear_hover(&mut self) {
        self.hovered_id = None;
        self.bar_hovered_id = None;
        self.hovered_link = None;
        self.pin_bar_tooltip = None;
        self.pin_ignore_leave = false;
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
        self.keep_today = false;
        let span = match self.scale {
            GanttScale::Day => direction.days(),
            GanttScale::Week => (direction * 7).days(),
            GanttScale::Month => direction.months(),
            GanttScale::Quarter => (direction * 3).months(),
            GanttScale::Year => direction.years(),
        };
        self.origin = self.origin.saturating_add(span);
    }

    /// Keep the viewport-centre date still when switching Day/Week/Month/…
    /// Zoom is one multiplier across scales; do not remap it per scale.
    pub fn set_scale(&mut self, scale: GanttScale) {
        let keep_x = self.timeline_width.max(1.0) * 0.5;
        self.set_scale_at(scale, keep_x);
    }

    /// Keep `keep` at its current screen x (or the centre if it is off-screen).
    pub fn set_scale_keeping(&mut self, scale: GanttScale, keep: Date) {
        let width = self.timeline_width.max(1.0);
        let raw = self.x_for_date(keep);
        let keep_x = if raw >= 0.0 && raw <= width {
            raw
        } else {
            width * 0.5
        };
        self.set_scale_at(scale, keep_x);
    }

    /// Change scale while holding the date under `keep_x` still. World-x
    /// (not a rounded civil date) so Month→Year→Month is invertible.
    /// Pass `keep_x = 0` to keep the scroll anchor (left-edge date).
    pub fn set_scale_at(&mut self, scale: GanttScale, keep_x: f32) {
        if self.scale == scale {
            return;
        }
        let old = self.scale.pixels_per_day(self.zoom);
        self.scale = scale;
        self.zoom = self.zoom.clamp(0.4, 4.0);
        let new = self.scale.pixels_per_day(self.zoom);
        if old > 0.0 {
            let world = (self.scroll_x + keep_x) / old;
            self.scroll_x = world * new - keep_x;
        }
    }

    /// Reverse [`snap_cramped_leading_column`]'s Year/Quarter gutter park so
    /// Month→Year→Month keeps the left-edge date. Always subtract, even on
    /// Month: a leaked snap after Today would otherwise clip the first column.
    pub fn unsnap_year_gutter(&mut self) {
        self.scroll_x -= self.year_gutter_snap;
        self.year_gutter_snap = 0.0;
    }

    /// Screen x a toolbar scale/zoom click should hold. After Today, keep
    /// Today still so Month does not re-anchor to a Year park at x=0.
    pub fn toolbar_scale_keep_x(&self, today: Date) -> f32 {
        if !self.keep_today {
            return 0.0;
        }
        let x = self.x_for_date(today);
        let width = self.timeline_width.max(1.0);
        if x >= 0.0 && x <= width { x } else { 0.0 }
    }

    /// Toolbar Day/Week/Month/… click. After Today, re-align so a Year park
    /// is not amplified into a clipped Month column. Otherwise hold the
    /// left-edge date (Month→Year→Month). Pull an on-screen Today back in
    /// on Day/Week/Month steps (Month→Week, Week→Day) and when the camera
    /// origin is still Today (pointer-zoom then Month).
    pub fn set_scale_from_toolbar(&mut self, scale: GanttScale, today: Date) {
        if self.keep_today {
            self.set_scale_at(scale, 0.0);
            self.go_today(today);
            return;
        }
        let width = self.timeline_width.max(1.0);
        let today_visible = (0.0..=width).contains(&self.x_for_date(today));
        let from = self.scale;
        self.set_scale_at(scale, 0.0);
        if !today_visible || from == scale {
            return;
        }
        let day_week_month =
            |s: GanttScale| matches!(s, GanttScale::Day | GanttScale::Week | GanttScale::Month);
        let fine_step = day_week_month(from) && day_week_month(scale);
        let today_is_origin = self.origin == today;
        if fine_step || (today_is_origin && day_week_month(scale)) {
            self.nudge_today_off_clip(today);
        }
    }

    /// Pull Today in from the clip so the Today pill and the nearby month
    /// tick stay readable after a scale-up (Month→Week, Week→Day).
    fn nudge_today_off_clip(&mut self, today: Date) {
        let width = self.timeline_width.max(1.0);
        let x = self.x_for_date(today);
        let margin = TODAY_MARGIN_PX.max(TODAY_PILL_W + HEADER_EDGE_PAD);
        if x < margin {
            self.scroll_x += x - margin;
        } else if x > width - margin {
            self.scroll_x += x - (width - margin);
        }
    }

    /// Undo the Year/Quarter gutter park, run `f`, then park again. Keeps
    /// `year_gutter_snap` in the current pixel space across scale/zoom/Today.
    pub fn retarget_year_gutter(&mut self, f: impl FnOnce(&mut Self)) {
        self.unsnap_year_gutter();
        f(self);
        self.snap_cramped_leading_column();
    }

    /// If the leftover column at x=0 is a sliver, align the period start to
    /// the left edge so Week/Day first columns stay readable. Year/Quarter
    /// also park the first month after the unscheduled gutter so diamonds
    /// do not sit on dated bars.
    pub fn snap_cramped_leading_column(&mut self) {
        if matches!(self.scale, GanttScale::Year | GanttScale::Quarter) {
            // Undo a previous park first so a second snap (Year at zoom
            // 0.4, month narrower than the gutter) does not walk left.
            self.unsnap_year_gutter();
            let gutter_right = UNSCHEDULED_GUTTER_RIGHT;
            // Park the first on-screen month after the unscheduled gutter.
            // Using date_for_x(0) (then stepping forward if that month is
            // already off the left) so a second snap is a no-op instead of
            // walking one month further left.
            let mut month_start = GanttScale::Month.floor_date(self.date_for_x(0.0));
            let mut x = self.x_for_date(month_start);
            if x < 0.0 {
                month_start = month_start.saturating_add(1.month());
                x = self.x_for_date(month_start);
            }
            if x < gutter_right {
                let delta = x - gutter_right;
                self.scroll_x += delta;
                self.year_gutter_snap = delta;
            }
            return;
        }
        if !matches!(self.scale, GanttScale::Week | GanttScale::Day) {
            return;
        }
        let start = self.scale.floor_date(self.date_for_x(0.0));
        let x = self.x_for_date(start);
        if x >= -0.5 {
            return;
        }
        // Visible remainder of the leftover column, not how far the period
        // start is off-screen. A week that starts far left still shows a
        // sliver at x=0 even when `-x` is larger than the snap threshold.
        let full = self.period_width(start).max(1.0);
        let leftover = full + x;
        let min = (full * 0.4).max(40.0);
        if leftover < min {
            let mut delta = x;
            let width = self.timeline_width.max(1.0);
            let today = jiff::Zoned::now().date();
            let today_x = self.x_for_date(today);
            if (0.0..=width).contains(&today_x) {
                // scroll_x += delta (delta < 0) moves Today right. Do not
                // snap the leftover column so far that Today/Nov clip.
                let max_right = (width - TODAY_MARGIN_PX).max(TODAY_MARGIN_PX);
                let room = (max_right - today_x).max(0.0);
                if -delta > room {
                    delta = -room;
                }
            }
            if delta < -0.5 {
                self.scroll_x += delta;
            }
        }
    }

    pub fn zoom_step_keeping(&mut self, direction: i32, keep_x: f32) {
        let old = self.scale.pixels_per_day(self.zoom);
        let next = Self::next_zoom(self.zoom, direction);
        if (next - self.zoom).abs() < f32::EPSILON {
            return;
        }
        self.zoom = next;
        let new = self.scale.pixels_per_day(self.zoom);
        if old > 0.0 {
            let world = (self.scroll_x + keep_x) / old;
            self.scroll_x = world * new - keep_x;
        }
    }

    pub fn zoom_percent_label(&self) -> String {
        format!("{:.0}%", self.zoom * 100.0)
    }

    pub fn filler_row_count(window_len: usize, content_h: f32) -> usize {
        ((content_h / ROW_HEIGHT).ceil() as usize).saturating_sub(window_len)
    }

    /// Apply a wheel/trackpad delta whose signs already match content direction
    /// (positive X reveals later dates, positive Y reveals later rows).
    pub fn apply_wheel(&mut self, dx: f32, dy: f32, shift: bool) {
        self.keep_today = false;
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
        // New scroll does not include a Year gutter park; leave a stale
        // snap and the next unsnap shifts Today off by ~32px.
        self.year_gutter_snap = 0.0;
        self.keep_today = true;
        let x = self.x_for_date(today);
        let target = self.timeline_width.max(1.0) * TODAY_ALIGN;
        self.scroll_x = x - target;
    }

    /// Stay on Today when scheduled bars are in that viewport; otherwise Auto Fit
    /// the item range so a July backlog is not hidden behind a November camera.
    pub fn focus_work(
        &mut self,
        nodes: &[WorkItemNode],
        iterations: &[IterationNode],
        width: f32,
        today: Date,
    ) {
        let width = width.max(MIN_TIMELINE_WIDTH);
        let previous = self.snapshot();
        self.go_today(today);
        let rows = self.flatten(nodes);
        let bars = self.layout_bars(nodes, &rows, iterations, self.viewport_h.max(720.0));
        let visible = bars.iter().any(|bar| {
            bar.kind == GanttBarKind::Scheduled && bar.x < width && bar.x + bar.width > 0.0
        });
        if visible {
            return;
        }
        self.restore(previous);
        self.auto_fit(nodes, iterations, width);
    }

    pub fn snapshot(&self) -> SavedGanttView {
        SavedGanttView {
            origin: self.origin,
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            scale: self.scale,
            zoom: self.zoom,
            year_gutter_snap: self.year_gutter_snap,
            keep_today: self.keep_today,
        }
    }

    pub fn restore(&mut self, saved: SavedGanttView) {
        self.origin = saved.origin;
        self.scroll_x = saved.scroll_x;
        self.scroll_y = saved.scroll_y;
        self.scale = saved.scale;
        self.zoom = saved.zoom;
        self.year_gutter_snap = saved.year_gutter_snap;
        self.keep_today = saved.keep_today;
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

    pub fn select_row(&mut self, id: i64) {
        self.selected_id = Some(id);
        self.selected_link = None;
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
        self.select_row(rows[next].id);
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

    /// Inclusive Start/Target for an unscheduled-row drag. The pointer's right
    /// edge is the exclusive Gantt end (same as painted bars); convert it so
    /// Target Date does not include an extra day. A same-day drop keeps the
    /// default two-week bar so a tiny drag still schedules work.
    pub fn schedule_span_from_window_xs(&self, a: f32, b: f32) -> (Date, Date) {
        let start = self.drop_date(a.min(b));
        let exclusive_end = self.drop_date(a.max(b));
        if exclusive_end <= start {
            (start, start.saturating_add(13.days()))
        } else {
            (start, from_gantt_exclusive_end(exclusive_end))
        }
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
                let mut marker = false;
                let (start, target) = if let Some(gesture) = live {
                    (gesture.live_start, gesture.live_target)
                } else if display.kind == GanttBarKind::NoFields {
                    // Keep "No dates" readable on Year/Quarter. A 16px box
                    // collapses the label to "…".
                    let width = NO_FIELDS_HINT_W;
                    return Some(BarLayout {
                        id: node.id,
                        kind: display.kind,
                        x: UNSCHEDULED_GUTTER_X,
                        width,
                        y: self.row_y(offset),
                        summary: row.has_children,
                        milestone: false,
                        label: dates_column_text(node, nodes, hint.as_ref()),
                        title: node.title.clone(),
                        color: parse_type_color(ado_core::color_for_type(&node.work_item_type)),
                        progress: None,
                        predecessors: node.predecessors.clone(),
                        marker: false,
                        open_ended: false,
                    });
                } else if display.kind == GanttBarKind::Unscheduled {
                    // Gutter pill, not a fake iteration/rollup bar and not a
                    // stack of diamonds on the Today line.
                    marker = true;
                    let today = jiff::Zoned::now().date();
                    (today, today)
                } else if let (Some(start), Some(target)) = (display.start, display.end) {
                    (start, target)
                } else {
                    return None;
                };
                let mut x = self.x_for_date(start);
                let end = ado_core::to_gantt_inclusive_target(target);
                let mut width = (self.x_for_date(end) - x).max(MIN_BAR_WIDTH);
                if marker {
                    width = MILESTONE_SIZE;
                    x = UNSCHEDULED_GUTTER_X;
                } else if display.milestone {
                    width = MILESTONE_SIZE;
                    x -= MILESTONE_SIZE / 2.0;
                } else if display.kind == GanttBarKind::Unscheduled {
                    width = width.max(PLACEHOLDER_MIN_WIDTH);
                } else if matches!(self.scale, GanttScale::Year) {
                    width = width.max(YEAR_MIN_BAR_WIDTH);
                }
                let start_only = parse_iso_date(node.target_date.as_deref()).is_none()
                    && parse_iso_date(node.start_date.as_deref()).is_some()
                    && !display.milestone
                    && live.as_ref().is_none_or(|gesture| !gesture.had_target);
                let open_ended = start_only;
                let label = if let Some(gesture) = live {
                    format_date_range(Some(gesture.live_start), Some(gesture.live_target))
                } else {
                    dates_column_text(node, nodes, hint.as_ref())
                };
                if start_only && live.is_none() && x + width <= 0.0 {
                    // Fully left of the clip: pin a hittable sliver at x = 0.
                    // Peeking bars (`x < 0` but `x + width > 0`) keep real
                    // geometry so the visible fragment is not a fake start grip.
                    // The open-ended cue paints the continuation to the
                    // viewport edge so this sliver is not the only visual.
                    let visible = (x + width).max(HANDLE_WIDTH * 2.0).max(8.0);
                    width = visible.min(width.max(HANDLE_WIDTH * 2.0));
                    x = 0.0;
                }
                Some(BarLayout {
                    id: node.id,
                    kind: display.kind,
                    x,
                    width,
                    y: self.row_y(offset),
                    summary: row.has_children,
                    milestone: display.milestone,
                    label,
                    title: node.title.clone(),
                    color: parse_type_color(ado_core::color_for_type(&node.work_item_type)),
                    progress: child_completion(nodes, node.id)
                        .or_else(|| Some(state_progress(&node.state))),
                    predecessors: node.predecessors.clone(),
                    marker,
                    open_ended,
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
        self.selected_link = None;
        if kind == GestureKind::Link {
            self.selected_id = Some(bar.id);
            let node = nodes.iter().find(|node| node.id == bar.id);
            let had_start = node
                .and_then(|node| parse_iso_date(node.start_date.as_deref()))
                .is_some();
            let had_target = node
                .and_then(|node| parse_iso_date(node.target_date.as_deref()))
                .is_some();
            let (start, target) = node
                .map(|node| self.dates_from_painted_bar(bar, node, nodes, iterations))
                .unwrap_or_else(|| {
                    let today = jiff::Zoned::now().date();
                    (today, today)
                });
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
        if display.kind == GanttBarKind::Unscheduled {
            let today = jiff::Zoned::now().date();
            return (today, today.saturating_add((DEFAULT_BAR_DAYS - 1).days()));
        }
        let start = display
            .start
            .or(display.end)
            .unwrap_or_else(|| jiff::Zoned::now().date());
        let target = display
            .end
            .unwrap_or_else(|| start.saturating_add((DEFAULT_BAR_DAYS - 1).days()));
        (start, target.max(start))
    }

    pub fn update_gesture(&mut self, x: f32) {
        let snap_unscheduled = self.gesture.as_ref().is_some_and(|gesture| {
            gesture.kind == GestureKind::Move && !gesture.had_start && !gesture.had_target
        });
        if snap_unscheduled && (0.0..UNSCHEDULED_GUTTER_RIGHT).contains(&x) {
            // Still in the gutter strip: keep Today. date_for_x there is the
            // timeline origin, not a chosen day. Negative x is a real date
            // off the left edge, not the gutter.
            if let Some(gesture) = &mut self.gesture {
                gesture.live_x = x;
                gesture.live_start = gesture.start;
                gesture.live_target = gesture.target;
            }
            return;
        }
        let snapped = snap_unscheduled.then(|| self.date_for_x(x));
        let Some(gesture) = &mut self.gesture else {
            return;
        };
        gesture.live_x = x;
        let delta_days =
            ((x - gesture.origin_x) / self.scale.pixels_per_day(self.zoom)).round() as i32;
        match gesture.kind {
            GestureKind::Move => {
                if let Some(start) = snapped {
                    // Unscheduled gutter pills: the drop date is the date
                    // under the cursor, not Today + (cursor − gutter).
                    gesture.live_start = start;
                    gesture.live_target = start.saturating_add((DEFAULT_BAR_DAYS - 1).days());
                } else {
                    gesture.live_start = gesture.start.saturating_add(delta_days.days());
                    gesture.live_target = gesture.target.saturating_add(delta_days.days());
                }
            }
            GestureKind::Link => {}
            GestureKind::ResizeStart => {
                let next = gesture.start.saturating_add(delta_days.days());
                if !gesture.had_target {
                    gesture.live_start = next;
                    gesture.live_target =
                        next.saturating_add((DEFAULT_BAR_DAYS - 1).days()).max(next);
                } else if next <= gesture.target {
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
        let to_id = gesture.link_to.filter(|id| *id != from_id);
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

    pub fn clear_link_target(&mut self) {
        if let Some(gesture) = self.gesture.as_mut()
            && gesture.kind == GestureKind::Link
        {
            gesture.link_to = None;
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

    pub fn gridline_xs(&self, width: f32) -> Vec<f32> {
        let mut xs: Vec<f32> = self
            .timeline_ticks(width)
            .into_iter()
            .filter(|tick| tick.x >= 0.0 && tick.x <= width)
            .map(|tick| tick.x)
            .collect();
        if xs.first().is_none_or(|x| *x > 0.5) {
            xs.insert(0, 0.0);
        }
        xs.push((width - 1.0).max(0.0));
        xs
    }

    pub fn timeline_ticks(&self, width: f32) -> Vec<TimelineTick> {
        let ppd = self.scale.pixels_per_day(self.zoom);
        if ppd <= 0.0 || width <= 0.0 {
            return Vec::new();
        }
        let left = self.date_for_x(-ppd);
        let right = self.date_for_x(width + ppd.max(1.0));
        let mut date = self.scale.tick_floor(left);
        let mut ticks = Vec::new();
        let mut last_major = String::new();
        let mut guard = 0;
        while guard < 512 {
            let x = self.x_for_date(date);
            if x > width + 1.0 && date > right {
                break;
            }
            let major_label = self.scale.major_label(date);
            let major = major_label != last_major;
            if major {
                last_major = major_label.clone();
            }
            ticks.push(TimelineTick {
                x,
                date,
                label: self.scale.tick_label(date),
                major_label,
                major,
                weekend: is_weekend(date),
            });
            let next = self.scale.tick_next(date);
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
    /// Pin the in-view leftover month to x = 0; keep the next month on its
    /// own segment instead of sliding "Nov 2026" over October days.
    /// Dodge runs after sticky so a leftover "Nov 2026" at x = 0 cannot
    /// sit on the Today pill in Week view.
    pub fn header_major_labels_except(
        &self,
        width: f32,
        hide_x: Option<f32>,
    ) -> Vec<(f32, String)> {
        let ticks: Vec<(f32, String)> = self
            .timeline_ticks(width)
            .into_iter()
            .filter(|tick| tick.major)
            .map(|tick| (tick.x.max(0.0), tick.major_label))
            .collect();
        let mut labels = Self::sticky_header_labels(ticks, false, 72.0);
        if let Some(today) = hide_x {
            let pill = today_pill_left(today, width);
            labels = dodge_majors_off_today_pill(labels, pill, self.scale);
        }
        Self::fit_labels_to_width(labels, width)
    }

    /// Month (or day/week) tick labels, with the in-view period stuck at x = 0
    /// when its gridline has scrolled off the left edge.
    pub fn header_tick_labels(&self, width: f32) -> Vec<(f32, String)> {
        self.header_tick_labels_except(width, None)
    }

    /// Year ticks are months on the row *below* the Today pill. Thin every
    /// other month from the first painted tick so leftover Jul yields
    /// Jul, Sep, Nov, Jan, Mar — not calendar-odd names that skip Jan.
    /// Day/Week still shift the in-month number off the Today line so "2"
    /// stays readable. Year/Quarter ticks keep their column x (different
    /// row from the pill) so a Today dodge cannot stack them and drop Nov.
    pub fn header_tick_labels_except(&self, width: f32, hide_x: Option<f32>) -> Vec<(f32, String)> {
        const HIDE_PAD: f32 = 22.0;
        const SHIFT: f32 = 16.0;
        let mut ticks: Vec<(f32, String)> = self
            .timeline_ticks(width)
            .into_iter()
            .filter_map(|tick| {
                if tick.label.is_empty() {
                    None
                } else {
                    Some((tick.x, tick.label))
                }
            })
            .collect();
        if let Some(today) = hide_x
            && !matches!(self.scale, GanttScale::Year | GanttScale::Quarter)
            && let Some(index) = ticks
                .iter()
                .enumerate()
                .filter(|(_, (x, _))| (x - today).abs() < HIDE_PAD)
                .min_by(|(_, (a, _)), (_, (b, _))| {
                    (a - today)
                        .abs()
                        .partial_cmp(&(b - today).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(index, _)| index)
        {
            ticks[index].0 = today + SHIFT;
        }
        let min_gap = if matches!(self.scale, GanttScale::Quarter) {
            48.0
        } else {
            TICK_MIN_GAP
        };
        if matches!(self.scale, GanttScale::Year) {
            ticks = thin_year_month_labels(ticks);
        }
        let labels = Self::space_header_labels(
            Self::sticky_header_labels(ticks, false, min_gap),
            min_gap,
            true,
        );
        Self::fit_labels_to_width(labels, width)
    }

    fn fit_labels_to_width(mut labels: Vec<(f32, String)>, width: f32) -> Vec<(f32, String)> {
        if let Some((x, _)) = labels.first_mut()
            && *x < HEADER_LEFT_PAD
        {
            *x = HEADER_LEFT_PAD;
        }
        // Pinning the leftover month to the left pad can sit it on the
        // first in-view tick; drop the leftover so Year "Jul" stays.
        if labels.len() >= 2 {
            let need = header_label_px(&labels[0].1).max(TICK_MIN_GAP);
            if labels[1].0 - labels[0].0 < need {
                labels.remove(0);
            }
        }
        while let Some((x, label)) = labels.last() {
            let w = header_label_px(label);
            if *x + w <= width - HEADER_EDGE_PAD + 0.1 {
                break;
            }
            let shifted = (width - HEADER_EDGE_PAD - w).max(HEADER_EDGE_PAD);
            let collision = labels
                .len()
                .checked_sub(2)
                .and_then(|i| labels.get(i))
                .is_some_and(|(prev, prev_label)| {
                    shifted - *prev < header_label_px(prev_label).max(TICK_MIN_GAP)
                });
            if collision {
                labels.pop();
                continue;
            }
            if let Some((x, _)) = labels.last_mut() {
                *x = shifted;
            }
            break;
        }
        labels
    }

    fn sticky_header_labels(
        ticks: impl IntoIterator<Item = (f32, String)>,
        skip_sticky: bool,
        sticky_min: f32,
    ) -> Vec<(f32, String)> {
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
            let room = out.first().is_none_or(|(x, _)| *x >= sticky_min);
            if room {
                out.insert(0, (0.0, label));
            }
        }
        out
    }

    fn space_header_labels(
        labels: Vec<(f32, String)>,
        min_gap: f32,
        text_aware: bool,
    ) -> Vec<(f32, String)> {
        let mut labels = labels;
        labels.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut out: Vec<(f32, String)> = Vec::new();
        for (x, label) in labels {
            if let Some((prev_x, prev_label)) = out.last() {
                // Year month names sit on the row below Today. Keep "Nov"
                // even when "Oct" is 29px wide and the column is ~28px;
                // the Today pill is nudged instead of dropping the tick.
                let need = if text_aware {
                    header_label_px(prev_label).max(min_gap)
                } else {
                    min_gap
                };
                if x - prev_x < need {
                    continue;
                }
            }
            out.push((x, label));
        }
        out
    }

    /// Fit every laid-out bar into `width` pixels, matching monday.com Auto Fit.
    pub fn auto_fit(&mut self, nodes: &[WorkItemNode], iterations: &[IterationNode], width: f32) {
        self.keep_today = false;
        let mut earliest: Option<Date> = None;
        let mut latest: Option<Date> = None;
        for node in nodes {
            let hint = iteration_hint_for_path(&node.iteration_path, iterations);
            let display = gantt_bar_display(node, nodes, hint.as_ref());
            if display.kind == GanttBarKind::Unscheduled {
                // Diamonds sit at Today. Iteration/rollup hints are display
                // copy only and must not pull Auto Fit onto a ghost span.
                continue;
            }
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
                y >= bar.y && y < bar.y + ROW_HEIGHT && x >= bar.x && x <= bar.x + bar.width
            })?
            .clone();
        let kind = if bar.milestone || bar.marker {
            GestureKind::Move
        } else if bar.summary && !bar.can_resize() {
            match bar.hit_zone_at_window_x(x, bar.x, bar.x + bar.width) {
                BarHitZone::Move => GestureKind::Move,
                _ => return None,
            }
        } else if bar.open_ended {
            if x >= bar.x + bar.width - OPEN_ENDED_LINK_HIT {
                GestureKind::Link
            } else {
                GestureKind::Move
            }
        } else {
            bar.resize_kind_at(x).unwrap_or(GestureKind::Move)
        };
        Some((bar, kind))
    }

    /// Drop target for a link drag, including the bar's handle/edge padding
    /// and the rest of the row band so an edge release still arms the link.
    pub fn hit_test_link_target(bars: &[BarLayout], x: f32, y: f32) -> Option<i64> {
        const EDGE: f32 = HANDLE_WIDTH + LINK_HANDLE_HIT + 12.0;
        if let Some(bar) = bars.iter().find(|bar| {
            let width = if bar.milestone || bar.marker {
                MILESTONE_SIZE
            } else {
                bar.width.max(8.0)
            };
            y >= bar.y && y < bar.y + ROW_HEIGHT && x >= bar.x - EDGE && x <= bar.x + width + EDGE
        }) {
            return Some(bar.id);
        }
        bars.iter()
            .find(|bar| y >= bar.y && y < bar.y + ROW_HEIGHT)
            .map(|bar| bar.id)
    }

    pub fn layout_dependency_arrows(&self, bars: &[BarLayout]) -> Vec<DependencyArrow> {
        const STUB: f32 = 12.0;
        const INCOMING_FAN: f32 = 10.0;
        const LANE_SEP: f32 = PARALLEL_LANE_MIN;
        let by_id: HashMap<i64, &BarLayout> = bars.iter().map(|bar| (bar.id, bar)).collect();
        let mut out = Vec::new();
        for bar in bars {
            for pred_id in &bar.predecessors {
                let Some(pred) = by_id.get(pred_id) else {
                    continue;
                };
                let y1 = pred.y + ROW_HEIGHT / 2.0;
                let y2 = bar.y + ROW_HEIGHT / 2.0;
                let pred_end = if pred.milestone || pred.marker {
                    pred.x + MILESTONE_SIZE
                } else {
                    pred.x + pred.width
                };
                // Gutter pills live at the left edge. Attach to their right
                // so the inbound is not a cramped stub on top of "No dates".
                let succ_gutter = bar_is_gutter(bar);
                let succ_right = if bar.milestone || bar.marker {
                    bar.x + MILESTONE_SIZE
                } else {
                    bar.x + bar.width
                };
                let pred_left = pred.x;
                let x_out = pred_end + STUB;
                // Finish-to-start always enters the successor start. Gutter
                // pills keep inbound_right so the head is not a cramped stub
                // on "No dates". Overlap/backward wraps at the source stub
                // and runs back on the successor row into that start — never
                // flip the head to the bar's right end.
                let inbound_right = succ_gutter;
                let x_head = if inbound_right { succ_right } else { bar.x };
                let wrap_around = !succ_gutter && pred_end > bar.x + STUB;
                let mut x_in = if wrap_around {
                    x_out
                } else if inbound_right {
                    x_head + STUB
                } else {
                    x_head - STUB
                };
                let lo = pred.y.min(bar.y);
                let hi = pred.y.max(bar.y);
                let intervening: Vec<&BarLayout> = bars
                    .iter()
                    .filter(|mid| {
                        mid.id != pred.id
                            && mid.id != bar.id
                            && mid.y > lo + 1.0
                            && mid.y < hi - 1.0
                    })
                    .collect();
                // Year/Quarter bars are slivers; their titles still paint at
                // full glyph width and would shove the inbound vertical
                // tens of days away (stray line at x≈532). Occupy the
                // body only at those zooms.
                let include_labels = !matches!(self.scale, GanttScale::Year | GanttScale::Quarter);
                let clip_right = self.timeline_width.max(1.0);
                if wrap_around {
                    x_in = clear_wrap_drop_x(
                        x_out,
                        &intervening,
                        false,
                        x_head,
                        pred_left + STUB,
                        clip_right,
                    );
                    x_in = x_in.max(pred_left + STUB);
                } else if !inbound_right && !intervening.is_empty() {
                    let x_vert = clear_vertical_x(
                        x_in,
                        &intervening,
                        bar.x,
                        true,
                        x_head,
                        include_labels,
                        Some(clip_right),
                    );
                    if (x_vert - x_in).abs() > 0.5 {
                        x_in = x_vert;
                    }
                    x_in = clamp_inbound_stub(
                        x_in,
                        x_head,
                        true,
                        &intervening,
                        include_labels,
                        Some(clip_right),
                    );
                }
                if !succ_gutter && x_in < pred_left {
                    // Dodging shoved the vertical left of the source. Drop at
                    // the source stub and keep the head on the start instead
                    // of overshooting to a right-edge inbound — unless that
                    // stub sits inside an intervening bar or its open-ended cue.
                    let fallback = x_out.max(pred_left + STUB);
                    let hits_intervening = intervening.iter().any(|mid| {
                        let (left, right) = bar_occupied_x(mid, include_labels, Some(clip_right));
                        x_hits_span(fallback, left, right)
                    });
                    if !hits_intervening {
                        x_in = fallback;
                    }
                }
                let same_row = (pred.y - bar.y).abs() < 1.0;
                // Stay in the predecessor row's gutter so the outgoing
                // vertical does not run through bars on rows in between.
                let lane_y = if same_row {
                    y1
                } else if bar.y > pred.y {
                    pred.y + ROW_HEIGHT - 1.0
                } else {
                    pred.y + 1.0
                };
                if !succ_gutter && !inbound_right && !wrap_around {
                    let clear = gutter_clear_x(bars);
                    if x_in < clear && x_head > clear + 4.0 {
                        x_in = clear;
                    }
                }
                out.push(DependencyArrow {
                    from_id: *pred_id,
                    to_id: bar.id,
                    x1: x_out,
                    y1,
                    x2: x_in,
                    y2,
                    x_head,
                    lane_y,
                    inbound_right,
                });
            }
        }
        let mut incoming: HashMap<i64, Vec<usize>> = HashMap::new();
        for (index, arrow) in out.iter().enumerate() {
            incoming.entry(arrow.to_id).or_default().push(index);
        }
        for indexes in incoming.values_mut() {
            indexes.sort_by_key(|&index| out[index].from_id);
            if indexes.len() < 2 {
                continue;
            }
            for (rank, &index) in indexes.iter().enumerate() {
                let shift = rank as f32 * INCOMING_FAN;
                let lane_shift = rank as f32 * LANE_SEP;
                if !out[index].wraps_around_successor() {
                    if out[index].inbound_right {
                        out[index].x_head += shift;
                        out[index].x2 += shift;
                    } else {
                        // Fan the inbound stub, not the arrowhead, so FS
                        // still terminates at the successor start.
                        out[index].x2 -= shift;
                    }
                }
                let dir = if out[index].y2 >= out[index].y1 {
                    -1.0
                } else {
                    1.0
                };
                out[index].lane_y += dir * lane_shift;
            }
        }
        let mut outgoing: HashMap<i64, Vec<usize>> = HashMap::new();
        for (index, arrow) in out.iter().enumerate() {
            outgoing.entry(arrow.from_id).or_default().push(index);
        }
        for indexes in outgoing.values_mut() {
            indexes.sort_by_key(|&index| out[index].to_id);
            if indexes.len() < 2 {
                continue;
            }
            for (rank, &index) in indexes.iter().enumerate() {
                if rank == 0 {
                    continue;
                }
                let x_shift = rank as f32 * INCOMING_FAN;
                let lane_shift = rank as f32 * LANE_SEP;
                out[index].x1 += x_shift;
                let dir = if out[index].y2 >= out[index].y1 {
                    -1.0
                } else {
                    1.0
                };
                out[index].lane_y += dir * lane_shift;
            }
        }
        separate_parallel_lanes(&mut out);
        let clip_right = (self.timeline_width - 10.0).max(24.0);
        let clip_left = gutter_clear_x(bars);
        for arrow in &mut out {
            clip_arrow_to_timeline(arrow, clip_left, clip_right);
        }
        // Clip first: two long verticals can both clamp to the same
        // `clip_right` (1–3px stack at the chart edge). Separate after.
        separate_parallel_verticals(&mut out, clip_left, clip_right);
        out
    }
}

fn arrow_horizontal_span(arrow: &DependencyArrow) -> (f32, f32) {
    (arrow.x1.min(arrow.x2), arrow.x1.max(arrow.x2))
}

fn x_spans_overlap(a: (f32, f32), b: (f32, f32)) -> bool {
    a.0 <= b.1 + 8.0 && b.0 <= a.1 + 8.0
}

/// Keep parallel elbows at least `PARALLEL_LANE_MIN` apart, including two
/// links that do not share a predecessor or successor (Persist cart →
/// Go-live vs Cart UI → spike).
fn separate_parallel_lanes(arrows: &mut [DependencyArrow]) {
    if arrows.len() < 2 {
        return;
    }
    for _ in 0..arrows.len() {
        let mut moved = false;
        for i in 0..arrows.len() {
            for j in i + 1..arrows.len() {
                let span_i = arrow_horizontal_span(&arrows[i]);
                let span_j = arrow_horizontal_span(&arrows[j]);
                if !x_spans_overlap(span_i, span_j) {
                    continue;
                }
                let dy = arrows[j].lane_y - arrows[i].lane_y;
                if dy.abs() >= PARALLEL_LANE_MIN {
                    // A horizontal run must also miss the other arrowhead.
                    let head_i = (arrows[i].lane_y - arrows[j].y2).abs();
                    let head_j = (arrows[j].lane_y - arrows[i].y2).abs();
                    if head_i >= PARALLEL_LANE_MIN && head_j >= PARALLEL_LANE_MIN {
                        continue;
                    }
                    if head_i < PARALLEL_LANE_MIN {
                        let push = PARALLEL_LANE_MIN - head_i;
                        if arrows[i].lane_y >= arrows[j].y2 {
                            arrows[i].lane_y += push;
                        } else {
                            arrows[i].lane_y -= push;
                        }
                        moved = true;
                    }
                    if head_j < PARALLEL_LANE_MIN {
                        let push = PARALLEL_LANE_MIN - head_j;
                        if arrows[j].lane_y >= arrows[i].y2 {
                            arrows[j].lane_y += push;
                        } else {
                            arrows[j].lane_y -= push;
                        }
                        moved = true;
                    }
                    continue;
                }
                let push = PARALLEL_LANE_MIN - dy.abs();
                if arrows[j].lane_y >= arrows[i].lane_y {
                    arrows[j].lane_y += push;
                } else {
                    arrows[i].lane_y += push;
                }
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VertKind {
    Outgoing,
    Inbound,
}

fn movable_verticals(arrow: &DependencyArrow) -> [(VertKind, f32); 2] {
    [
        (VertKind::Outgoing, arrow.x1),
        (VertKind::Inbound, arrow.x2),
    ]
}

fn wrap_floor(arrow: &DependencyArrow) -> f32 {
    arrow.x_head + DependencyArrow::WRAP_STUB + 1.0
}

/// Shift one vertical. Wrap x1/x2 stay coupled so the drop does not split
/// from the source stub. Do not clamp inbound to `x_head - 4` — that undoes
/// a right-edge push and restacks two clipped verticals. Do not let a
/// forward FS inbound cross `x_head + WRAP_STUB`, or the polyline flips
/// to wrap-around.
fn shift_vertical(
    arrow: &mut DependencyArrow,
    kind: VertKind,
    delta: f32,
    clip_left: f32,
    clip_right: f32,
) -> bool {
    let wrap = arrow.wraps_around_successor();
    let cur = match kind {
        VertKind::Outgoing => arrow.x1,
        VertKind::Inbound => arrow.x2,
    };
    let next = (cur + delta).clamp(clip_left, clip_right);
    if (next - cur).abs() < 0.5 {
        return false;
    }
    if wrap && next <= wrap_floor(arrow) {
        return false;
    }
    if !wrap && kind == VertKind::Inbound && next > arrow.x_head + DependencyArrow::WRAP_STUB {
        return false;
    }
    let applied = next - cur;
    match kind {
        VertKind::Outgoing => arrow.x1 = next,
        VertKind::Inbound => arrow.x2 = next,
    }
    if wrap {
        match kind {
            VertKind::Outgoing => {
                arrow.x2 = (arrow.x2 + applied).clamp(clip_left, clip_right);
            }
            VertKind::Inbound => {
                arrow.x1 = (arrow.x1 + applied).clamp(clip_left, clip_right);
            }
        }
    }
    true
}

/// Keep long inbound/wrap verticals from stacking 1–3px apart (Persist cart
/// → Go-live vs Cart UI → spike at x≈601). Separate ANY pair whose vertical
/// Xs are within `PARALLEL_VERT_MIN`, regardless of endpoints or Y span —
/// a Y-overlap gate missed this Month-view pair (different source/target).
fn separate_parallel_verticals(arrows: &mut [DependencyArrow], clip_left: f32, clip_right: f32) {
    if arrows.len() < 2 {
        return;
    }
    for _ in 0..arrows.len() * 6 {
        let mut moved = false;
        for i in 0..arrows.len() {
            for j in i + 1..arrows.len() {
                let verts_i = movable_verticals(&arrows[i]);
                let verts_j = movable_verticals(&arrows[j]);
                let frozen_i = arrows[i]
                    .wraps_around_successor()
                    .then(|| arrows[i].stub_start());
                let frozen_j = arrows[j]
                    .wraps_around_successor()
                    .then(|| arrows[j].stub_start());
                for (ki, xi) in verts_i {
                    for (kj, xj) in verts_j {
                        let dx = xj - xi;
                        if dx.abs() >= PARALLEL_VERT_MIN {
                            continue;
                        }
                        let push = (PARALLEL_VERT_MIN - dx.abs()).max(1.0);
                        let (right, right_k, left, left_k) = if xj >= xi {
                            (j, kj, i, ki)
                        } else {
                            (i, ki, j, kj)
                        };
                        if shift_vertical(&mut arrows[right], right_k, push, clip_left, clip_right)
                            || shift_vertical(
                                &mut arrows[left],
                                left_k,
                                -push,
                                clip_left,
                                clip_right,
                            )
                        {
                            moved = true;
                        }
                    }
                    if let Some(frozen) = frozen_j {
                        let dx = frozen - xi;
                        if dx.abs() < PARALLEL_VERT_MIN {
                            let push = (PARALLEL_VERT_MIN - dx.abs()).max(1.0);
                            let delta = if xi >= frozen { push } else { -push };
                            if shift_vertical(&mut arrows[i], ki, delta, clip_left, clip_right) {
                                moved = true;
                            }
                        }
                    }
                }
                if let Some(frozen) = frozen_i {
                    for (kj, xj) in verts_j {
                        let dx = xj - frozen;
                        if dx.abs() < PARALLEL_VERT_MIN {
                            let push = (PARALLEL_VERT_MIN - dx.abs()).max(1.0);
                            let delta = if xj >= frozen { push } else { -push };
                            if shift_vertical(&mut arrows[j], kj, delta, clip_left, clip_right) {
                                moved = true;
                            }
                        }
                    }
                }
            }
        }
        if !moved {
            break;
        }
    }
}

fn clip_arrow_to_timeline(arrow: &mut DependencyArrow, left: f32, right: f32) {
    let clamp_span = |x: f32| x.clamp(left, right);
    // Keep the head on the successor. Pulling it to `left` parks ▶ on the
    // chart edge and on top of gutter "No dates" pills.
    arrow.x_head = arrow.x_head.clamp(0.0, right);
    if arrow.x_head > left + 1.0 {
        arrow.x1 = clamp_span(arrow.x1);
        arrow.x2 = clamp_span(arrow.x2);
    } else {
        arrow.x1 = arrow.x1.clamp(0.0, right);
        arrow.x2 = arrow.x2.clamp(0.0, right);
    }
    const MIN_STUB: f32 = 10.0;
    // Only expand a collapsed inbound stub. Do not shrink wrap-around elbows.
    if arrow.inbound_right {
        if arrow.x2 < arrow.x_head + MIN_STUB {
            arrow.x2 = (arrow.x_head + MIN_STUB).min(right);
        }
        if arrow.x_head > right - MIN_STUB {
            arrow.x_head = (right - MIN_STUB).max(left);
            arrow.x2 = right;
        }
    } else if (arrow.x_head - arrow.x2).abs() < MIN_STUB {
        let floor = if arrow.x_head > left + MIN_STUB {
            left
        } else {
            0.0
        };
        arrow.x2 = (arrow.x_head - MIN_STUB).max(floor);
        if arrow.x_head < left + MIN_STUB && arrow.x_head > left + 1.0 {
            arrow.x_head = (left + MIN_STUB).min(right);
            arrow.x2 = left;
        }
    }
}

fn parse_type_color(hex: &str) -> u32 {
    u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x89b4fa)
}

pub(crate) fn header_label_px(label: &str) -> f32 {
    (label.chars().count() as f32 * 7.0 + 8.0).max(14.0)
}

/// Painted x of the "Today" pill, matching the timeline header.
pub(crate) fn today_pill_left(today_x: f32, width: f32) -> f32 {
    if today_x + TODAY_PILL_GAP + TODAY_PILL_W > width - 4.0 {
        (today_x - TODAY_PILL_W - 4.0).max(4.0)
    } else {
        (today_x + TODAY_PILL_GAP).max(4.0)
    }
}

/// Keep every other Year month, phased from the leftover (last x≤0) tick so
/// leftover Jul yields Jul, Sep, Nov, Jan, Mar. Thinning before sticky
/// means the next kept month is two months out and the leftover is not
/// dropped for sitting on the skipped in-view month.
fn thin_year_month_labels(labels: Vec<(f32, String)>) -> Vec<(f32, String)> {
    let phase = labels
        .iter()
        .enumerate()
        .rev()
        .find(|(_, (x, _))| *x <= 0.0)
        .map(|(index, _)| index)
        .unwrap_or(0);
    labels
        .into_iter()
        .enumerate()
        .filter(|(index, _)| index % 2 == phase % 2)
        .map(|(_, item)| item)
        .collect()
}

fn dodge_majors_off_today_pill(
    labels: Vec<(f32, String)>,
    pill_left: f32,
    scale: GanttScale,
) -> Vec<(f32, String)> {
    let pill_right = pill_left + TODAY_PILL_W;
    labels
        .into_iter()
        .map(|(mut x, label)| {
            let text_w = MAJOR_LABEL_ESTIMATE.max(header_label_px(&label));
            if x < pill_right + HEADER_TODAY_LABEL_GAP && x + text_w > pill_left {
                // Year leftover at x = 0 can stay when Today is in the middle.
                if !(matches!(scale, GanttScale::Year)
                    && x < HEADER_LEFT_PAD
                    && x + text_w <= pill_left + 1.0)
                {
                    x = pill_right + HEADER_TODAY_LABEL_GAP;
                }
            }
            (x, label)
        })
        .collect()
}

fn bar_is_gutter(bar: &BarLayout) -> bool {
    bar.marker || bar.kind == GanttBarKind::Unscheduled || bar.kind == GanttBarKind::NoFields
}

/// Right edge arrows must stay clear of, so wrap verticals miss gutter pills.
fn gutter_clear_x(bars: &[BarLayout]) -> f32 {
    let right = bars
        .iter()
        .filter(|bar| bar_is_gutter(bar))
        .map(|bar| bar.x + bar.width)
        .fold(0.0_f32, f32::max);
    if right <= 0.0 {
        10.0
    } else {
        (right + 8.0).max(UNSCHEDULED_GUTTER_RIGHT)
    }
}

/// Hover copy for a Gantt bar: title, dates, assignee.
/// Prefer the node's current dates so a just-committed drag does not keep
/// the pre-drag range after `bar.label` was captured for a GPUI tooltip.
pub fn bar_hover_tooltip(bar: &BarLayout, nodes: &[WorkItemNode]) -> String {
    let node = nodes.iter().find(|node| node.id == bar.id);
    let assignee = node
        .and_then(|node| node.assigned_to.as_ref())
        .map(|identity| identity.display_name.trim())
        .filter(|name| !name.is_empty())
        .unwrap_or("Unassigned");
    let dates = if bar.kind == GanttBarKind::NoFields {
        "No dates".to_string()
    } else if bar.marker || bar.kind == GanttBarKind::Unscheduled {
        "Unscheduled".to_string()
    } else if !bar.label.is_empty() {
        // Live gesture layouts write the in-flight range onto `bar.label`.
        bar.label.clone()
    } else if let Some(node) = node {
        let text = dates_column_text(node, nodes, None);
        if text.is_empty() {
            "No dates".into()
        } else {
            text
        }
    } else {
        "No dates".into()
    };
    format!("{} · {} · {assignee}", bar.title, dates)
}

/// True when the timeline has no dated bars — only gutter pills, or nothing.
pub fn timeline_is_empty_of_scheduled(bars: &[BarLayout]) -> bool {
    !bars
        .iter()
        .any(|bar| bar.kind == GanttBarKind::Scheduled && !bar.marker)
}

/// Child start before the parent, or child target after the parent.
pub fn parent_date_overflow(nodes: &[WorkItemNode], id: i64) -> bool {
    parent_overflow_span(nodes, id).is_some()
}

/// Hover copy for the amber `!` when a child falls outside its parent.
pub fn parent_overflow_tip(nodes: &[WorkItemNode], id: i64) -> Option<String> {
    let (parent_start, parent_end) = parent_overflow_span(nodes, id)?;
    let range = format_date_range(parent_start, parent_end);
    if range.is_empty() {
        Some("Outside parent's dates".into())
    } else {
        Some(format!("Outside parent's dates ({range})"))
    }
}

fn parent_overflow_span(nodes: &[WorkItemNode], id: i64) -> Option<(Option<Date>, Option<Date>)> {
    let node = nodes.iter().find(|node| node.id == id)?;
    let parent_id = node.parent_id?;
    let parent = nodes.iter().find(|node| node.id == parent_id)?;
    let child_start = parse_iso_date(node.start_date.as_deref());
    let child_end = parse_iso_date(node.target_date.as_deref());
    let parent_start = parse_iso_date(parent.start_date.as_deref());
    let parent_end = parse_iso_date(parent.target_date.as_deref());
    let overflow = matches!(
        (child_start, parent_start),
        (Some(child), Some(parent)) if child < parent
    ) || matches!(
        (child_end, parent_end),
        (Some(child), Some(parent)) if child > parent
    );
    overflow.then_some((parent_start, parent_end))
}

fn bar_title_px(title: &str) -> f32 {
    title.chars().count() as f32 * 8.0 + 16.0
}

/// Inside when the visible text area can hold the title; otherwise the label
/// floats to the right of the bar instead of truncating (monday.com). Measure
/// the inner width after grips/padding so Year slivers do not clip to "C".
pub fn bar_title_is_inside(bar: &BarLayout) -> bool {
    if bar.kind == GanttBarKind::Unscheduled
        || bar.kind == GanttBarKind::NoFields
        || bar.milestone
        || bar.marker
    {
        return false;
    }
    let clip_pad = (-bar.x).max(0.0);
    let visible_w = (bar.width - clip_pad).max(0.0);
    let grip = if bar.can_resize() || bar.summary {
        bar.inner_grip_width()
    } else {
        0.0
    };
    let h_pad = if bar.can_resize() || bar.summary {
        8.0
    } else {
        12.0
    };
    let cue = if bar.open_ended {
        OPEN_ENDED_LINK_HIT + 4.0
    } else {
        0.0
    };
    let inner = (visible_w - grip * 2.0 - h_pad - cue).max(0.0);
    if inner < BAR_LABEL_INSIDE_MIN {
        // Short Quarter/Year slivers ("Cart", "Checkout rewrite") must not
        // clip to "Ur" / "ut rewrite" inside the bar.
        return false;
    }
    inner >= bar_title_px(&bar.title)
}

/// Right edge of a bar including an external title, so arrows skip labels.
pub fn bar_label_right(bar: &BarLayout) -> f32 {
    let painted_w = if bar.milestone || bar.marker {
        MILESTONE_SIZE
    } else {
        bar.width.max(8.0)
    };
    let title_w = bar_title_px(&bar.title);
    if bar_title_is_inside(bar) {
        bar.x + painted_w
    } else {
        bar.x + painted_w + HANDLE_WIDTH + LINK_HANDLE_HIT + 16.0 + title_w
    }
}

/// Body, and optionally an external title, so a vertical drop misses the row.
/// Open-ended start-only bars paint a dashed cue to the clip, so routing
/// must treat that tail as occupied or the elbow runs through it.
fn bar_occupied_x(bar: &BarLayout, include_label: bool, cue_right: Option<f32>) -> (f32, f32) {
    if bar_is_gutter(bar) {
        return (bar.x - 2.0, bar.x + bar.width + 4.0);
    }
    let left = if bar.milestone || bar.marker {
        bar.x - 2.0
    } else {
        bar.x - HANDLE_WIDTH
    };
    let mut right = if include_label {
        bar_label_right(bar)
    } else if bar.milestone || bar.marker {
        bar.x + MILESTONE_SIZE + 4.0
    } else {
        bar.x + bar.width.max(8.0) + 4.0
    };
    if let Some(clip) = cue_right
        && bar.open_ended
        && !bar.milestone
        && !bar.marker
    {
        right = right.max(clip);
    }
    (left, right)
}

fn x_hits_span(x: f32, left: f32, right: f32) -> bool {
    x >= left - 1.0 && x <= right + 1.0
}

/// Keep an inbound vertical in a gap that misses intervening bars and labels,
/// without flipping to the other side of `x_head`.
fn clear_vertical_x(
    pref: f32,
    intervening: &[&BarLayout],
    succ_edge: f32,
    inbound_left: bool,
    x_head: f32,
    include_labels: bool,
    cue_right: Option<f32>,
) -> f32 {
    let blocked: Vec<(f32, f32)> = intervening
        .iter()
        .map(|bar| bar_occupied_x(bar, include_labels, cue_right))
        .collect();
    if blocked
        .iter()
        .all(|(left, right)| !x_hits_span(pref, *left, *right))
    {
        return pref;
    }
    let on_side = |x: f32| {
        if inbound_left {
            x <= x_head - 4.0
        } else {
            x >= x_head + 4.0
        }
    };
    if inbound_left {
        let intervening_right = blocked
            .iter()
            .map(|(_, right)| *right)
            .fold(f32::NEG_INFINITY, f32::max);
        let gap_left = intervening_right + 6.0;
        let gap_right = succ_edge - 4.0;
        if gap_right - gap_left >= 4.0 {
            let cand = pref.clamp(gap_left, gap_right);
            if on_side(cand)
                && blocked
                    .iter()
                    .all(|(left, right)| !x_hits_span(cand, *left, *right))
            {
                return cand;
            }
        }
        // Slide left past each occupied span. Do not merge disjoint bars
        // (a gutter pill plus an open-ended cue) into one blob that
        // dumps the vertical at the chart edge.
        let mut x = pref;
        for _ in 0..=blocked.len() {
            let hit_left = blocked
                .iter()
                .filter(|(left, right)| x_hits_span(x, *left, *right))
                .map(|(left, _)| *left)
                .fold(f32::INFINITY, f32::min);
            if hit_left == f32::INFINITY {
                return x;
            }
            let cand = hit_left - 8.0;
            if cand >= x || !on_side(cand) {
                return pref.min(x_head - 4.0);
            }
            x = cand;
        }
        return x;
    }
    let intervening_left = blocked
        .iter()
        .map(|(left, _)| *left)
        .fold(f32::INFINITY, f32::min);
    let gap_right = intervening_left - 6.0;
    let gap_left = succ_edge + 4.0;
    if gap_right - gap_left >= 4.0 {
        let cand = pref.clamp(gap_left, gap_right);
        if on_side(cand) {
            return cand;
        }
    }
    let around = blocked
        .iter()
        .map(|(_, right)| *right)
        .fold(f32::NEG_INFINITY, f32::max)
        + 8.0;
    if on_side(around) {
        return around;
    }
    pref.max(x_head + 4.0)
}

/// Slide a wrap-around drop just past intervening bars so the long vertical
/// runs in a gap, without sending the loop to the chart left edge. Re-test
/// each candidate against every occupied span: sliding past the source stub
/// can still land inside a later overlapping bar.
///
/// Open-ended dashed cues count as occupied, same as forward inbound. When
/// sliding right of a cue would leave the clip, try the gap just left of
/// that bar's core — but never left of `pred_min_x` / the wrap stub, or the
/// loop becomes the long left detour wrap exists to avoid.
fn clear_wrap_drop_x(
    pref: f32,
    intervening: &[&BarLayout],
    include_labels: bool,
    x_head: f32,
    pred_min_x: f32,
    clip_right: f32,
) -> f32 {
    let blocked: Vec<(f32, f32)> = intervening
        .iter()
        .map(|bar| bar_occupied_x(bar, include_labels, Some(clip_right)))
        .collect();
    let min_x = pred_min_x.max(x_head + DependencyArrow::WRAP_STUB + 1.0);
    let mut x = pref;
    for _ in 0..=blocked.len() {
        let hits: Vec<&(f32, f32)> = blocked
            .iter()
            .filter(|(left, right)| x_hits_span(x, *left, *right))
            .collect();
        if hits.is_empty() {
            return x;
        }
        let hit_right = hits
            .iter()
            .map(|(_, right)| *right)
            .fold(f32::NEG_INFINITY, f32::max);
        let hit_left = hits
            .iter()
            .map(|(left, _)| *left)
            .fold(f32::INFINITY, f32::min);
        let cand_right = hit_right + 6.0;
        if cand_right > x
            && cand_right > x_head + DependencyArrow::WRAP_STUB
            && cand_right <= clip_right
        {
            x = cand_right;
            continue;
        }
        let cand_left = hit_left - 8.0;
        if cand_left >= min_x && cand_left < x {
            x = cand_left;
            continue;
        }
        return pref.max(min_x);
    }
    x
}

/// Keep a 12px inbound stub when it stays in the clear lane; otherwise keep
/// the already-cleared `x_in` so the vertical does not recross a bar.
fn clamp_inbound_stub(
    x_in: f32,
    x_head: f32,
    inbound_left: bool,
    intervening: &[&BarLayout],
    include_labels: bool,
    cue_right: Option<f32>,
) -> f32 {
    const STUB: f32 = 12.0;
    let clamped = if inbound_left {
        x_in.min(x_head - STUB)
    } else {
        x_in.max(x_head + STUB)
    };
    if (clamped - x_in).abs() < 0.5 {
        return clamped;
    }
    if intervening.iter().any(|bar| {
        let (left, right) = bar_occupied_x(bar, include_labels, cue_right);
        x_hits_span(clamped, left, right)
    }) {
        x_in
    } else {
        clamped
    }
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

/// Index after `id` and its descendants (last among that subtree).
pub fn insert_index_after_subtree(nodes: &[WorkItemNode], id: i64) -> usize {
    let Some(index) = nodes.iter().position(|node| node.id == id) else {
        return 0;
    };
    match descendants(nodes, id).last() {
        Some(last_id) => nodes
            .iter()
            .position(|node| node.id == *last_id)
            .map(|index| index + 1)
            .unwrap_or(index + 1),
        None => index + 1,
    }
}

/// Index after `selected_id` and its descendants so a new sibling lands next
/// to the selection instead of nesting under it.
pub fn insert_index_for_sibling(nodes: &[WorkItemNode], selected_id: Option<i64>) -> usize {
    let Some(id) = selected_id else {
        return nodes.len();
    };
    insert_index_after_subtree(nodes, id)
}

/// Index at which a newly created child should land: directly under the parent
/// (first among siblings) so it is visible next to the row that spawned it.
pub fn insert_index_for_child(nodes: &[WorkItemNode], parent_id: Option<i64>) -> usize {
    let Some(pid) = parent_id else {
        return nodes.len();
    };
    let Some(parent_index) = nodes.iter().position(|node| node.id == pid) else {
        return 0;
    };
    parent_index + 1
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

    fn bar_end(bar: &BarLayout) -> f32 {
        if bar.milestone || bar.marker {
            bar.x + MILESTONE_SIZE
        } else {
            bar.x + bar.width
        }
    }

    fn assert_arrow_stays_on_screen(arrow: &DependencyArrow, timeline_width: f32) {
        for (name, x) in [("x_head", arrow.x_head), ("x1", arrow.x1), ("x2", arrow.x2)] {
            assert!(
                x >= 10.0 && x <= timeline_width - 8.0,
                "{name}={x} must stay on-screen (width={timeline_width})"
            );
        }
    }

    fn assert_exits_right_without_left_detour(
        arrow: &DependencyArrow,
        pred: &BarLayout,
        succ: &BarLayout,
        timeline_width: f32,
    ) {
        assert_arrow_stays_on_screen(arrow, timeline_width);
        let pred_end = bar_end(pred);
        let succ_right = bar_end(succ);
        assert!(
            (arrow.x1 - (pred_end + 12.0).clamp(10.0, (timeline_width - 10.0).max(24.0))).abs()
                < 2.0
                || arrow.x1 + 0.5 >= pred_end,
            "must exit to the right of the source, x1={} pred_end={pred_end}",
            arrow.x1
        );
        assert!(
            arrow.x2 + 0.5 >= pred.x,
            "the vertical must not detour left of the source start, x2={} pred.x={}",
            arrow.x2,
            pred.x
        );
        assert!(
            (arrow.x_head - succ.x).abs() < 3.0,
            "finish-to-start must enter the target start, x_head={} succ.x={} succ_right={succ_right}",
            arrow.x_head,
            succ.x
        );
        assert!(
            !arrow.inbound_right,
            "dated FS links must not enter from the right, x_head={} succ_right={succ_right}",
            arrow.x_head
        );
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
        assert_eq!(
            gantt.zoom_percent_label(),
            "125%",
            "percentage must track column width, got {}",
            gantt.zoom_percent_label()
        );
        gantt.zoom_at_left(0.8);
        assert!(
            (gantt.zoom - 1.0).abs() < 0.01,
            "− from 125% returns to 100%, got {}",
            gantt.zoom
        );
        gantt.zoom_at_left(0.8);
        assert!(gantt.zoom < 1.0, "− must go below 100%, got {}", gantt.zoom);
        assert_eq!(
            gantt.zoom_percent_label(),
            "80%",
            "− below 100% must show the column percentage, got {}",
            gantt.zoom_percent_label()
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
            marker: false,
            open_ended: false,
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
            None,
            "the start grip stays inside the painted bar"
        );
        assert_eq!(
            GanttView::hit_test_bar(&bars, 184.0, 4.0).map(|(_, kind)| kind),
            None,
            "the end grip stays inside the painted bar"
        );
        assert_eq!(
            GanttView::hit_test_bar(&bars, 100.0 + 80.0 - 3.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "3px inside the right end must resize, not move"
        );
        let (end_x, end_w) = resize_end_handle(&bar);
        assert!(
            (end_x - (bar.x + bar.width - HANDLE_WIDTH)).abs() < 0.1,
            "end grip must start 8px inside the painted bar, got {end_x}"
        );
        assert!(
            (end_w - HANDLE_WIDTH).abs() < 0.1,
            "end grip must be the inner 8px, got {end_w}"
        );
        assert!(
            end_x + end_w <= bar.x + bar.width + 0.1,
            "end grip must not spill into the link-handle gap"
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
            GanttView::hit_test_bar(std::slice::from_ref(&summary), 140.0, 4.0)
                .map(|(_, kind)| kind),
            Some(GestureKind::Move),
            "the body of a parent summary bar can still move"
        );
        assert_eq!(
            GanttView::hit_test_bar(std::slice::from_ref(&summary), 176.0, 4.0)
                .map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "a parent with its own dates resizes at the right edge"
        );
        assert_eq!(
            summary.hit_zone_at_window_x(176.0, summary.x, summary.x + summary.width),
            BarHitZone::ResizeEnd
        );
        assert_eq!(
            summary.hit_zone_at_window_x(177.0, summary.x, summary.x + summary.width),
            BarHitZone::ResizeEnd,
            "3px inside a summary end must resize, not sit under a link dot"
        );
        let diamond = BarLayout {
            summary: true,
            milestone: false,
            marker: false,
            width: MILESTONE_SIZE,
            ..summary.clone()
        };
        assert!(
            !diamond.shows_rollup_edges(),
            "a 16px parent diamond must not grow rollup edges that cover the bar"
        );
        assert_eq!(
            diamond.hit_zone_at_window_x(108.0, 100.0, 116.0),
            BarHitZone::Move,
            "the center of a 16px summary must still move"
        );
        assert_eq!(
            diamond.hit_zone_at_window_x(100.0, 100.0, 116.0),
            BarHitZone::Move,
            "a 16px summary edge must stay Move, not RollupEdge"
        );
        assert_eq!(
            GanttView::hit_test_bar(std::slice::from_ref(&diamond), 100.0, 4.0)
                .map(|(_, kind)| kind),
            Some(GestureKind::Move),
            "hit-test on a short parent must still pick Move"
        );
        let sliver = BarLayout {
            width: 6.0,
            ..diamond
        };
        assert!(!sliver.shows_rollup_edges());
        assert_eq!(
            sliver.hit_zone_at_window_x(103.0, 100.0, 106.0),
            BarHitZone::Move,
            "a year-scale 6px parent must still move"
        );
    }

    #[test]
    fn end_grip_wins_hit_test_over_move_on_a_wide_bar() {
        let bar = BarLayout {
            id: 1002,
            kind: GanttBarKind::Scheduled,
            x: 80.0,
            width: 920.0,
            y: 0.0,
            summary: false,
            milestone: false,
            label: "Cart".into(),
            title: "Cart".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        let (end_x, end_w) = resize_end_handle(&bar);
        let link_x = bar.x + bar.width + HANDLE_WIDTH;
        assert!(
            end_x < bar.x + bar.width - 1.0,
            "end grip must cover the bar's right interior"
        );
        assert!(
            end_x + end_w <= link_x,
            "end grip must stay inside the bar, grip {}..{} link {link_x}",
            end_x,
            end_x + end_w
        );
        assert_eq!(
            GanttView::hit_test_bar(std::slice::from_ref(&bar), bar.x + bar.width - 1.0, 4.0)
                .map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "the last pixel of a Week-scale bar must resize, not move"
        );
        assert_eq!(
            GanttView::hit_test_bar(
                std::slice::from_ref(&bar),
                bar.x + bar.width - HANDLE_WIDTH + 1.0,
                4.0
            )
            .map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd)
        );
        assert_eq!(
            GanttView::hit_test_bar(std::slice::from_ref(&bar), bar.x + bar.width / 2.0, 4.0)
                .map(|(_, kind)| kind),
            Some(GestureKind::Move),
            "the body of a wide bar must still move"
        );
        assert_eq!(
            GanttView::hit_test_bar(std::slice::from_ref(&bar), end_x + end_w + 2.0, 4.0)
                .map(|(_, kind)| kind),
            None,
            "the link-handle gap past the bar must not be a resize"
        );
        assert_eq!(
            bar.resize_kind_at(bar.x + bar.width - 3.0),
            Some(GestureKind::ResizeEnd),
            "3px inside the right end must be ResizeEnd"
        );
        // Painted window edges can sit far from layout.x (tree, gutter, label).
        // Classification must use those edges, not layout-local x.
        let painted_left = 412.0;
        let painted_right = painted_left + bar.width;
        for inset in [2.0, 3.0, 4.0, 6.0] {
            assert_eq!(
                bar.hit_zone_at_window_x(painted_left + inset, painted_left, painted_right),
                BarHitZone::ResizeStart,
                "{inset}px inside the painted start must resize, not move"
            );
            assert_eq!(
                bar.hit_zone_at_window_x(painted_right - inset, painted_left, painted_right),
                BarHitZone::ResizeEnd,
                "{inset}px inside the painted end must resize, not move"
            );
            assert_eq!(
                bar.resize_kind_at(painted_left + inset),
                None,
                "layout-local x must not be used as a window coordinate"
            );
        }
        assert_eq!(
            bar.hit_zone_at_window_x(painted_left + bar.width / 2.0, painted_left, painted_right),
            BarHitZone::Move
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
        assert!(
            bars[0].marker,
            "unscheduled items are a gutter pill, not an iteration ghost bar"
        );
        assert!((bars[0].width - MILESTONE_SIZE).abs() < 0.1);
    }

    #[test]
    fn start_only_item_paints_a_normal_scheduled_bar() {
        let mut item = node(1, None);
        item.target_date = None;
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(std::slice::from_ref(&item));
        let bars = gantt.layout_bars(std::slice::from_ref(&item), &rows, &[], 720.0);
        assert_eq!(bars.len(), 1);
        assert!(
            bars[0].open_ended,
            "start-only Cart UI-style items stay a default-length bar with an open-ended cue"
        );
        assert!(!bars[0].marker);
        assert_eq!(bars[0].kind, GanttBarKind::Scheduled);
        assert!(
            bars[0].width > OPEN_ENDED_MARKER_WIDTH + 1.0,
            "start-only must use the default-length bar, not a tiny ▸ box, width={}",
            bars[0].width
        );
        assert!(
            !bars[0].can_resize(),
            "open-ended bars keep Move/start-resize, not a fake end grip"
        );
    }

    #[test]
    fn start_only_open_ended_bar_leaves_room_for_a_right_edge_cue() {
        let mut item = node(1, None);
        item.target_date = None;
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(std::slice::from_ref(&item));
        let bars = gantt.layout_bars(std::slice::from_ref(&item), &rows, &[], 720.0);
        assert_eq!(bars.len(), 1);
        assert!(bars[0].open_ended);
        assert!(
            bars[0].x + bars[0].width < gantt.timeline_width - 8.0,
            "the default-length body must stop short of the clip so a continuation cue can paint, right={} width={}",
            bars[0].x + bars[0].width,
            gantt.timeline_width
        );
    }

    #[test]
    fn start_only_bar_with_off_screen_start_clamps_to_the_visible_edge() {
        let mut item = node(1, None);
        item.start_date = Some("2026-07-21T00:00:00Z".into());
        item.target_date = None;
        let gantt = GanttView {
            origin: Date::new(2026, 11, 1).unwrap(),
            scale: GanttScale::Week,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(std::slice::from_ref(&item));
        let bars = gantt.layout_bars(std::slice::from_ref(&item), &rows, &[], 720.0);
        assert_eq!(bars.len(), 1);
        assert!(
            bars[0].x >= 0.0,
            "an off-screen start-only bar must clamp to the visible edge, x={}",
            bars[0].x
        );
        assert!(
            bars[0].width >= HANDLE_WIDTH * 2.0,
            "clamped start-only must stay wide enough to hit, width={}",
            bars[0].width
        );
        assert!(
            bars[0].x + bars[0].width > 0.0,
            "the clamped fragment must paint inside the viewport"
        );
    }

    #[test]
    fn peeking_start_only_bar_keeps_real_geometry() {
        let mut item = node(1, None);
        item.start_date = Some("2026-07-06T00:00:00Z".into());
        item.target_date = None;
        let gantt = GanttView {
            origin: Date::new(2026, 7, 10).unwrap(),
            scale: GanttScale::Week,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(std::slice::from_ref(&item));
        let bars = gantt.layout_bars(std::slice::from_ref(&item), &rows, &[], 720.0);
        assert_eq!(bars.len(), 1);
        assert!(
            bars[0].x < 0.0,
            "a peeking start-only bar must keep its true start, x={}",
            bars[0].x
        );
        assert!(
            bars[0].x + bars[0].width > 0.0,
            "the visible fragment must still peek into the viewport"
        );
    }

    #[test]
    fn start_only_and_short_year_bars_keep_a_minimum_width() {
        let mut item = node(1, None);
        item.target_date = None;
        for scale in [GanttScale::Quarter, GanttScale::Year] {
            let gantt = GanttView {
                origin: Date::new(2026, 7, 1).unwrap(),
                scale,
                zoom: 0.2,
                timeline_width: 800.0,
                ..Default::default()
            };
            let rows = gantt.flatten(std::slice::from_ref(&item));
            let bars = gantt.layout_bars(std::slice::from_ref(&item), &rows, &[], 720.0);
            assert_eq!(bars.len(), 1, "{scale:?}");
            assert!(
                bars[0].open_ended,
                "{scale:?} start-only must keep an open-ended continuation cue"
            );
            let min = if scale == GanttScale::Year {
                YEAR_MIN_BAR_WIDTH
            } else {
                MIN_BAR_WIDTH
            };
            assert!(
                bars[0].width + 0.1 >= min,
                "{scale:?} bar must stay visible, width={}",
                bars[0].width
            );
        }
        let mut one_day = node(1, None);
        one_day.target_date = one_day.start_date.clone();
        let gantt = GanttView {
            origin: Date::new(2026, 1, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 0.2,
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(std::slice::from_ref(&one_day));
        let bars = gantt.layout_bars(std::slice::from_ref(&one_day), &rows, &[], 720.0);
        assert_eq!(bars.len(), 1);
        assert!(
            bars[0].width + 0.1 >= YEAR_MIN_BAR_WIDTH,
            "Year-view bars must keep a {YEAR_MIN_BAR_WIDTH}px body, width={}",
            bars[0].width
        );
        assert!(
            !bars[0].can_resize(),
            "a {YEAR_MIN_BAR_WIDTH}px Year bar must stay all-Move, not two grips with no body"
        );
        let left = bars[0].x;
        let right = bars[0].x + bars[0].width;
        assert_eq!(
            bars[0].hit_zone_at_window_x(left + 1.0, left, right),
            BarHitZone::Move,
            "the Year min-width start edge must move Start and Target together"
        );
        assert_eq!(
            bars[0].hit_zone_at_window_x((left + right) / 2.0, left, right),
            BarHitZone::Move
        );
    }

    #[test]
    fn open_ended_right_edge_starts_a_link() {
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
            marker: false,
            open_ended: true,
        };
        assert_eq!(
            GanttView::hit_test_bar(std::slice::from_ref(&bar), 176.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::Link),
            "the ▸ at the right of a start-only bar must start a link"
        );
        assert_eq!(
            GanttView::hit_test_bar(&[bar], 140.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::Move)
        );
    }

    #[test]
    fn open_ended_marker_center_is_a_move() {
        let bar = BarLayout {
            id: 1,
            kind: GanttBarKind::Scheduled,
            x: 100.0,
            width: OPEN_ENDED_MARKER_WIDTH,
            y: 0.0,
            summary: false,
            milestone: false,
            label: "bar".into(),
            title: "bar".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: true,
        };
        assert_eq!(
            GanttView::hit_test_bar(
                std::slice::from_ref(&bar),
                100.0 + OPEN_ENDED_MARKER_WIDTH / 2.0,
                4.0
            )
            .map(|(_, kind)| kind),
            Some(GestureKind::Move),
            "the body of a start-only marker must still move"
        );
        assert_eq!(
            GanttView::hit_test_bar(
                std::slice::from_ref(&bar),
                100.0 + OPEN_ENDED_MARKER_WIDTH - 2.0,
                4.0
            )
            .map(|(_, kind)| kind),
            Some(GestureKind::Link),
            "only the ▸ edge of a start-only marker starts a link"
        );
        assert_eq!(
            GanttView::hit_test_bar(&[bar], 100.0 + 2.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::Move),
            "the left of an 18px start-only marker must move, not resize"
        );
    }

    #[test]
    fn unscheduled_without_iteration_paints_a_today_marker() {
        let mut item = node(1, None);
        item.start_date = None;
        item.target_date = None;
        item.iteration_path.clear();
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(&[item.clone()]);
        let bars = gantt.layout_bars(&[item], &rows, &[], 720.0);
        assert_eq!(
            bars.len(),
            1,
            "a new unscheduled row must still paint a marker"
        );
        assert_eq!(bars[0].kind, GanttBarKind::Unscheduled);
        assert!(bars[0].marker, "no iteration span must not look scheduled");
        assert!((bars[0].width - MILESTONE_SIZE).abs() < 0.1);
        assert!(
            (bars[0].x - UNSCHEDULED_GUTTER_X).abs() < 0.1,
            "unscheduled pills must sit in the gutter, not on Today, x={}",
            bars[0].x
        );
    }

    #[test]
    fn unscheduled_marker_gesture_starts_at_today_not_origin() {
        let mut item = node(1, None);
        item.start_date = None;
        item.target_date = None;
        item.iteration_path.clear();
        let origin = Date::new(2026, 7, 1).unwrap();
        let mut gantt = GanttView {
            origin,
            ..Default::default()
        };
        let rows = gantt.flatten(&[item.clone()]);
        let bars = gantt.layout_bars(&[item.clone()], &rows, &[], 720.0);
        let bar = bars[0].clone();
        assert!(bar.marker);
        gantt
            .begin_gesture(&bar, GestureKind::Move, bar.x, &[item], &[])
            .expect("move");
        let today = jiff::Zoned::now().date();
        let gesture = gantt.gesture.expect("gesture");
        assert_eq!(gesture.start, today);
        assert_eq!(
            gesture.target,
            today.saturating_add((DEFAULT_BAR_DAYS - 1).days())
        );
        assert_ne!(
            gesture.start, origin,
            "dragging a Today marker must not jump to the timeline origin"
        );
    }

    #[test]
    fn unscheduled_marker_no_move_does_not_commit() {
        let mut item = node(1, None);
        item.start_date = None;
        item.target_date = None;
        item.iteration_path.clear();
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(&[item.clone()]);
        let bars = gantt.layout_bars(&[item.clone()], &rows, &[], 720.0);
        let bar = bars[0].clone();
        assert!(bar.marker);
        let today = jiff::Zoned::now().date();
        let today_x = gantt.x_for_date(today);
        gantt
            .begin_gesture(&bar, GestureKind::Move, today_x, &[item], &[])
            .expect("move");
        gantt.update_gesture(today_x);
        assert!(
            gantt.finish_gesture().is_none(),
            "a release with no movement must not commit dates"
        );
    }

    #[test]
    fn unscheduled_gutter_micro_drag_does_not_commit_origin_dates() {
        let mut item = node(1, None);
        item.start_date = None;
        item.target_date = None;
        item.iteration_path.clear();
        let origin = Date::new(2026, 7, 1).unwrap();
        let mut gantt = GanttView {
            origin,
            ..Default::default()
        };
        let rows = gantt.flatten(&[item.clone()]);
        let bars = gantt.layout_bars(&[item.clone()], &rows, &[], 720.0);
        let bar = bars[0].clone();
        let today = jiff::Zoned::now().date();
        let today_x = gantt.x_for_date(today);
        gantt
            .begin_gesture(&bar, GestureKind::Move, today_x, &[item], &[])
            .expect("move");
        gantt.update_gesture(UNSCHEDULED_GUTTER_X + 8.0);
        let gesture = gantt.gesture.clone().expect("live");
        assert_eq!(
            gesture.live_start, today,
            "a gutter x must keep Today, not snap to the origin date {}",
            gesture.live_start
        );
        assert_ne!(
            gesture.live_start, origin,
            "micro-dragging in the gutter must not adopt the timeline origin"
        );
        assert!(
            gantt.finish_gesture().is_none(),
            "releasing still in the gutter must not commit dates"
        );
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
    fn quarter_short_bars_place_titles_outside() {
        let cases = [
            (
                "Checkout rewrite",
                "2026-07-01T00:00:00Z",
                "2026-07-20T00:00:00Z",
            ),
            ("Cart", "2026-07-06T00:00:00Z", "2026-07-14T00:00:00Z"),
        ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Quarter,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        for (title, start, target) in cases {
            let mut item = node(1, None);
            item.title = title.into();
            item.start_date = Some(start.into());
            item.target_date = Some(target.into());
            let rows = gantt.flatten(&[item.clone()]);
            let bars = gantt.layout_bars(&[item], &rows, &[], 720.0);
            assert_eq!(bars.len(), 1, "{title}");
            assert!(
                !bar_title_is_inside(&bars[0]),
                "Quarter {title:?} at width {} must sit outside instead of clipping",
                bars[0].width
            );
        }
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
        assert!(
            !bar_title_is_inside(&bars[0]),
            "a title that does not fit the visible bar must go outside"
        );
    }

    #[test]
    fn year_sliver_titles_paint_outside_instead_of_clipping() {
        let cases = [
            ("Cart", "2026-07-06T00:00:00Z", "2026-08-14T00:00:00Z"),
            (
                "Persist cart",
                "2026-08-04T00:00:00Z",
                "2026-08-21T00:00:00Z",
            ),
            (
                "Unparented spike",
                "2026-07-21T00:00:00Z",
                "2026-08-04T00:00:00Z",
            ),
        ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        for (title, start, target) in cases {
            let mut item = node(1, None);
            item.title = title.into();
            item.start_date = Some(start.into());
            item.target_date = Some(target.into());
            let rows = gantt.flatten(&[item.clone()]);
            let bars = gantt.layout_bars(&[item], &rows, &[], 720.0);
            assert_eq!(bars.len(), 1, "{title}");
            assert!(
                !bar_title_is_inside(&bars[0]),
                "Year {title:?} at width {} must sit to the right of the bar, not clip inside",
                bars[0].width
            );
        }
    }

    #[test]
    fn auto_fit_unscheduled_iteration_goes_to_today() {
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
        let mut gantt = GanttView {
            origin: Date::new(2025, 1, 1).unwrap(),
            zoom: 0.4,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.auto_fit(std::slice::from_ref(&item), &iterations, 800.0);
        let today = jiff::Zoned::now().date();
        assert_eq!(
            gantt.origin, today,
            "unscheduled iteration hints must not frame Auto Fit away from Today diamonds"
        );
        let rows = gantt.flatten(std::slice::from_ref(&item));
        let bars = gantt.layout_bars(std::slice::from_ref(&item), &rows, &iterations, 720.0);
        assert!(
            bars.iter()
                .any(|bar| { bar.marker && bar.x < 800.0 && bar.x + bar.width > 0.0 }),
            "Today diamonds must land on screen, origin={} bars={bars:?}",
            gantt.origin
        );
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
    fn link_gesture_on_a_start_only_bar_keeps_its_dates() {
        let mut start_only = node(1, None);
        start_only.target_date = None;
        start_only.start_date = Some("2026-07-06T00:00:00Z".into());
        let nodes = vec![start_only];
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let bar = bars.iter().find(|bar| bar.id == 1).expect("start-only bar");
        gantt
            .begin_gesture(bar, GestureKind::Link, bar.x + bar.width + 6.0, &nodes, &[])
            .expect("link");
        let gesture = gantt.gesture.as_ref().expect("link gesture");
        assert!(
            gesture.had_start && !gesture.had_target,
            "a start-only link must remember the live start, not pretend both sides were empty"
        );
        assert_eq!(GanttView::gesture_patch_dates(gesture), (None, None));
        gantt.cancel_gesture();
        assert!(gantt.gesture.is_none());
    }

    #[test]
    fn schedule_span_from_window_xs_uses_the_dragged_days() {
        let gantt = GanttView {
            origin: Date::new(2026, 8, 1).unwrap(),
            scale: GanttScale::Day,
            zoom: 1.0,
            timeline_origin_x: 0.0,
            scroll_x: 0.0,
            ..Default::default()
        };
        let day = gantt.scale.pixels_per_day(gantt.zoom);
        let (start, target) = gantt.schedule_span_from_window_xs(day * 2.0, day * 6.0);
        assert_eq!(start, Date::new(2026, 8, 3).unwrap());
        assert_eq!(
            target,
            Date::new(2026, 8, 6).unwrap(),
            "a 4-day pointer span must save 4 inclusive days, not an extra exclusive day"
        );
        let (point_start, point_target) =
            gantt.schedule_span_from_window_xs(day * 2.0, day * 2.0 + 3.0);
        assert_eq!(point_start, Date::new(2026, 8, 3).unwrap());
        assert_eq!(
            point_target,
            Date::new(2026, 8, 16).unwrap(),
            "a same-day drop keeps the default two-week bar"
        );
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
        assert!(
            bar.marker,
            "unscheduled must paint at Today, not the sprint"
        );
        let origin_x = bar.x + bar.width / 2.0;
        gantt
            .begin_gesture(bar, GestureKind::Move, origin_x, &nodes, &iterations)
            .expect("placeholder move");
        let drop = Date::new(2026, 8, 20).unwrap();
        let drop_x = gantt.x_for_date(drop);
        gantt.update_gesture(drop_x);
        let gesture = gantt.gesture.clone().expect("live");
        let (start, target) = GanttView::gesture_patch_dates(&gesture);
        let start = start.expect("start");
        assert_eq!(
            start, drop,
            "dragging the gutter pill to Aug 20 must save Aug 20, not Today+(cursor−gutter) (got {start}–{target:?})"
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
            (arrows[0].x_head - succ.x).abs() < 3.0,
            "arrowhead must terminate at the target start edge, got {} vs {}",
            arrows[0].x_head,
            succ.x
        );
        assert!(
            arrows[0].x1 <= pred.x + pred.width + 13.0,
            "adjacent links must leave the predecessor bar, not its far-right title, got x1={} bar_right={}",
            arrows[0].x1,
            pred.x + pred.width
        );
        assert!(
            arrows[0].x1 < bar_label_right(pred)
                || bar_label_right(pred) <= pred.x + pred.width + 1.0,
            "adjacent routing must stay local instead of wrapping to the label edge {}",
            bar_label_right(pred)
        );
        assert!(
            arrows[0].lane_y >= pred.y.min(succ.y) + ROW_HEIGHT - 1.0,
            "horizontal run must sit in the gap between rows, got lane {}",
            arrows[0].lane_y
        );
    }

    #[test]
    fn link_remove_sits_on_the_predecessor_row() {
        let arrow = DependencyArrow {
            from_id: 1,
            to_id: 2,
            x1: 50.0,
            y1: 20.0,
            x2: 400.0,
            y2: 220.0,
            x_head: 412.0,
            lane_y: 200.0,
            inbound_right: false,
        };
        let (x, y) = arrow.remove_control_pos();
        assert!(
            (x - arrow.x1).abs() < 1.0,
            "× must sit on the outgoing stub, not on the predecessor bar, got x={x} x1={}",
            arrow.x1
        );
        assert!(
            y <= arrow.y1 - 12.0,
            "× must sit above the predecessor bar and the line, got y={y} y1={}",
            arrow.y1
        );
        assert!(
            (y - arrow.y2).abs() > ROW_HEIGHT,
            "× must not sit on the successor row, got y={y} y2={}",
            arrow.y2
        );
    }

    #[test]
    fn overlapping_incoming_links_are_fanned_apart() {
        let mut a = node(1, None);
        let mut b = node(2, None);
        let mut c = node(3, None);
        a.title = "Persist cart".into();
        b.title = "Go-live".into();
        b.start_date = None;
        b.target_date = Some("2026-08-15T00:00:00Z".into());
        c.title = "Unparented spike".into();
        c.start_date = Some("2026-07-21T00:00:00Z".into());
        c.target_date = Some("2026-08-04T00:00:00Z".into());
        c.predecessors = vec![1, 2];
        let nodes = vec![a, b, c];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert_eq!(arrows.len(), 2);
        let succ = bars.iter().find(|bar| bar.id == 3).unwrap();
        let dx = (arrows[0].x_head - arrows[1].x_head).abs();
        let dy = (arrows[0].lane_y - arrows[1].lane_y).abs();
        assert!(
            dx >= 8.0 || dy >= PARALLEL_LANE_MIN - 0.5,
            "two links into the same bar must not share one polyline, dx={dx} dy={dy}"
        );
        for arrow in &arrows {
            let pred = bars.iter().find(|bar| bar.id == arrow.from_id).unwrap();
            if pred.x > succ.x + 1.0 {
                assert_exits_right_without_left_detour(arrow, pred, succ, gantt.timeline_width);
            } else {
                assert!(
                    (arrow.x_head - succ.x).abs() < 3.0,
                    "inbound arrowhead must terminate at the bar start edge, x_head={} bar.x={}",
                    arrow.x_head,
                    succ.x
                );
            }
            let (rx, ry) = arrow.remove_control_pos();
            assert!(
                rx < pred.x - 1.0 || rx > pred.x + pred.width - 1.0,
                "× must not overlap the predecessor bar, ×x={rx} pred={}..{}",
                pred.x,
                pred.x + pred.width
            );
            assert!(
                ry < pred.y + 8.0,
                "× must sit above the predecessor bar, ×y={ry} pred.y={}",
                pred.y
            );
        }
    }

    #[test]
    fn two_links_from_persist_cart_do_not_share_a_vertical() {
        let mut pred = node(1003, None);
        pred.title = "Persist cart".into();
        pred.start_date = Some("2026-07-06T00:00:00Z".into());
        pred.target_date = Some("2026-07-20T00:00:00Z".into());
        let mut spike = node(1012, None);
        spike.title = "Unparented spike".into();
        spike.start_date = Some("2026-07-21T00:00:00Z".into());
        spike.target_date = Some("2026-08-04T00:00:00Z".into());
        spike.predecessors = vec![1003];
        let mut go_live = node(1013, None);
        go_live.title = "Go-live milestone".into();
        go_live.start_date = None;
        go_live.target_date = Some("2026-08-15T00:00:00Z".into());
        go_live.predecessors = vec![1003];
        let nodes = vec![pred, spike, go_live];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert_eq!(arrows.len(), 2);
        let dx = (arrows[0].x1 - arrows[1].x1).abs();
        let dy = (arrows[0].lane_y - arrows[1].lane_y).abs();
        assert!(
            dx >= 8.0 || dy >= PARALLEL_LANE_MIN,
            "Persist cart → spike and Persist cart → Go-live must not share one vertical, dx={dx} dy={dy} x1={},{} lane={},{}",
            arrows[0].x1,
            arrows[1].x1,
            arrows[0].lane_y,
            arrows[1].lane_y
        );
    }

    #[test]
    fn adjacent_row_parallel_lanes_are_at_least_ten_apart() {
        let a = DependencyArrow {
            from_id: 1003,
            to_id: 1013,
            x1: 200.0,
            y1: 20.0,
            x2: 420.0,
            y2: 180.0,
            x_head: 400.0,
            lane_y: 39.0,
            inbound_right: false,
        };
        let b = DependencyArrow {
            from_id: 1005,
            to_id: 1012,
            x1: 210.0,
            y1: 60.0,
            x2: 380.0,
            y2: 220.0,
            x_head: 360.0,
            lane_y: 41.0,
            inbound_right: false,
        };
        let mut arrows = vec![a, b];
        separate_parallel_lanes(&mut arrows);
        let dy = (arrows[0].lane_y - arrows[1].lane_y).abs();
        assert!(
            dy >= PARALLEL_LANE_MIN,
            "Persist cart → Go-live must not brush Cart UI → spike, dy={dy} lane={},{}",
            arrows[0].lane_y,
            arrows[1].lane_y
        );
    }

    fn closest_vertical_gap(a: &DependencyArrow, b: &DependencyArrow) -> f32 {
        let mut min_dx = f32::MAX;
        for xa in [a.x1, a.x2] {
            for xb in [b.x1, b.x2] {
                min_dx = min_dx.min((xa - xb).abs());
            }
        }
        min_dx
    }

    #[test]
    fn parallel_verticals_one_px_apart_are_pushed_to_ten() {
        let a = DependencyArrow {
            from_id: 1003,
            to_id: 1013,
            x1: 200.0,
            y1: 20.0,
            x2: 601.0,
            y2: 180.0,
            x_head: 640.0,
            lane_y: 39.0,
            inbound_right: false,
        };
        let b = DependencyArrow {
            from_id: 1005,
            to_id: 1012,
            x1: 210.0,
            y1: 60.0,
            x2: 602.0,
            y2: 220.0,
            x_head: 360.0,
            lane_y: 79.0,
            inbound_right: false,
        };
        let mut arrows = vec![a, b];
        separate_parallel_verticals(&mut arrows, 0.0, 800.0);
        let dx = closest_vertical_gap(&arrows[0], &arrows[1]);
        assert!(
            dx >= PARALLEL_VERT_MIN,
            "1px-stacked inbound verticals must separate, dx={dx} x2={},{}",
            arrows[0].x2,
            arrows[1].x2
        );
    }

    #[test]
    fn parallel_verticals_separate_even_when_y_spans_miss() {
        let a = DependencyArrow {
            from_id: 1003,
            to_id: 1013,
            x1: 200.0,
            y1: 20.0,
            x2: 600.0,
            y2: 40.0,
            x_head: 640.0,
            lane_y: 30.0,
            inbound_right: false,
        };
        let b = DependencyArrow {
            from_id: 1005,
            to_id: 1012,
            x1: 210.0,
            y1: 400.0,
            x2: 602.0,
            y2: 480.0,
            x_head: 360.0,
            lane_y: 440.0,
            inbound_right: false,
        };
        let mut arrows = vec![a, b];
        separate_parallel_verticals(&mut arrows, 0.0, 800.0);
        let dx = closest_vertical_gap(&arrows[0], &arrows[1]);
        assert!(
            dx >= PARALLEL_VERT_MIN,
            "disjoint-Y verticals at x≈600/602 must still separate, dx={dx} x2={},{}",
            arrows[0].x2,
            arrows[1].x2
        );
    }

    #[test]
    fn separating_verticals_does_not_flip_a_forward_link_to_wrap() {
        let a = DependencyArrow {
            from_id: 1003,
            to_id: 1013,
            x1: 200.0,
            y1: 20.0,
            x2: 628.0,
            y2: 180.0,
            x_head: 640.0,
            lane_y: 39.0,
            inbound_right: false,
        };
        let b = DependencyArrow {
            from_id: 1005,
            to_id: 1012,
            x1: 210.0,
            y1: 60.0,
            x2: 630.0,
            y2: 220.0,
            x_head: 360.0,
            lane_y: 79.0,
            inbound_right: false,
        };
        assert!(
            !a.wraps_around_successor(),
            "precondition: Persist cart → Go-live is a forward inbound"
        );
        let mut arrows = vec![a, b];
        separate_parallel_verticals(&mut arrows, 0.0, 800.0);
        assert!(
            !arrows[0].wraps_around_successor(),
            "pushing the inbound must not flip it to wrap-around, x2={} x_head={}",
            arrows[0].x2,
            arrows[0].x_head
        );
        let dx = closest_vertical_gap(&arrows[0], &arrows[1]);
        assert!(
            dx >= PARALLEL_VERT_MIN || !arrows[1].wraps_around_successor(),
            "the other arrow may move, but the forward link must stay forward, dx={dx} x2={},{}",
            arrows[0].x2,
            arrows[1].x2
        );
    }

    #[test]
    fn persist_cart_go_live_clears_the_cart_ui_spike_lane() {
        let mut persist = node(1003, Some(1002));
        persist.title = "Persist cart".into();
        persist.start_date = Some("2026-07-06T00:00:00Z".into());
        persist.target_date = Some("2026-07-20T00:00:00Z".into());
        persist.work_item_type = "User Story".into();
        let mut cart_ui = node(1005, Some(1003));
        cart_ui.title = "Cart UI".into();
        cart_ui.start_date = Some("2026-07-21T00:00:00Z".into());
        cart_ui.target_date = None;
        cart_ui.work_item_type = "Task".into();
        let mut spike = node(1012, None);
        spike.title = "Unparented spike".into();
        spike.start_date = Some("2026-07-21T00:00:00Z".into());
        spike.target_date = Some("2026-08-04T00:00:00Z".into());
        spike.predecessors = vec![1005];
        spike.work_item_type = "Task".into();
        let mut go_live = node(1013, Some(1001));
        go_live.title = "Go-live milestone".into();
        go_live.start_date = None;
        go_live.target_date = Some("2026-08-15T00:00:00Z".into());
        go_live.predecessors = vec![1003];
        let nodes = vec![
            node(1001, None),
            node(1002, Some(1001)),
            persist,
            cart_ui,
            go_live,
            spike,
        ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let persist_live = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("Persist cart → Go-live");
        let cart_spike = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1005 && arrow.to_id == 1012)
            .expect("Cart UI → spike");
        let dy = (persist_live.lane_y - cart_spike.lane_y).abs();
        let miss_head = (persist_live.lane_y - cart_spike.y2).abs() >= PARALLEL_LANE_MIN
            && (cart_spike.lane_y - persist_live.y2).abs() >= PARALLEL_LANE_MIN;
        assert!(
            dy >= PARALLEL_LANE_MIN && miss_head,
            "Persist cart → Go-live must sit ≥{PARALLEL_LANE_MIN}px off Cart UI → spike, dy={dy} lanes={},{} heads={},{}",
            persist_live.lane_y,
            cart_spike.lane_y,
            persist_live.y2,
            cart_spike.y2
        );
    }

    /// Mock `work-items.json` tree with the live links Persist cart → Go-live
    /// and Cart UI → Unparented spike. Parallel inbound/wrap verticals must
    /// stay at least 6px apart (the stacked x≈601 / 602–603 pair).
    fn mock_fixture_nodes() -> Vec<WorkItemNode> {
        let mut checkout = node(1001, None);
        checkout.title = "Checkout rewrite".into();
        checkout.work_item_type = "Epic".into();
        checkout.start_date = Some("2026-07-01T00:00:00Z".into());
        checkout.target_date = Some("2026-09-30T00:00:00Z".into());
        let mut cart = node(1002, Some(1001));
        cart.title = "Cart".into();
        cart.work_item_type = "Feature".into();
        cart.start_date = Some("2026-07-06T00:00:00Z".into());
        cart.target_date = Some("2026-08-14T00:00:00Z".into());
        let mut persist = node(1003, Some(1002));
        persist.title = "Persist cart".into();
        persist.work_item_type = "User Story".into();
        persist.state = "Active".into();
        persist.start_date = Some("2026-07-06T00:00:00Z".into());
        persist.target_date = Some("2026-07-20T00:00:00Z".into());
        let mut cart_ui = node(1005, Some(1003));
        cart_ui.title = "Cart UI".into();
        cart_ui.work_item_type = "Task".into();
        cart_ui.start_date = Some("2026-07-21T00:00:00Z".into());
        cart_ui.target_date = None;
        let mut spike = node(1012, None);
        spike.title = "Unparented spike".into();
        spike.work_item_type = "Task".into();
        spike.start_date = Some("2026-07-21T00:00:00Z".into());
        spike.target_date = Some("2026-08-04T00:00:00Z".into());
        spike.predecessors = vec![1005];
        let mut go_live = node(1013, Some(1001));
        go_live.title = "Go-live milestone".into();
        go_live.work_item_type = "Feature".into();
        go_live.start_date = None;
        go_live.target_date = Some("2026-08-15T00:00:00Z".into());
        go_live.predecessors = vec![1003];
        let mut grooming = node(1014, Some(1002));
        grooming.title = "Backlog grooming".into();
        grooming.work_item_type = "Task".into();
        grooming.start_date = None;
        grooming.target_date = None;
        let mut growth = node(1020, None);
        growth.title = "Growth experiments".into();
        growth.work_item_type = "Epic".into();
        growth.start_date = Some("2026-08-01T00:00:00Z".into());
        growth.target_date = Some("2026-09-15T00:00:00Z".into());
        vec![
            checkout, cart, persist, cart_ui, spike, go_live, grooming, growth,
        ]
    }

    fn assert_mock_fixture_parallel_verticals(scale: GanttScale, timeline_width: f32) {
        let nodes = mock_fixture_nodes();
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale,
            timeline_width,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let persist_live = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("Persist cart → Go-live");
        let cart_spike = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1005 && arrow.to_id == 1012)
            .expect("Cart UI → spike");
        let dx = closest_vertical_gap(persist_live, cart_spike);
        assert!(
            dx >= 8.0,
            "{:?}: Persist cart → Go-live vertical must sit ≥8px from Cart UI → spike, dx={dx} persist x1={} x2={} spike x1={} x2={} lanes={},{}",
            scale,
            persist_live.x1,
            persist_live.x2,
            cart_spike.x1,
            cart_spike.x2,
            persist_live.lane_y,
            cart_spike.lane_y
        );
    }

    #[test]
    fn mock_fixture_parallel_verticals_are_at_least_eight_apart() {
        for width in [512.0, 800.0, 1200.0] {
            assert_mock_fixture_parallel_verticals(GanttScale::Month, width);
            assert_mock_fixture_parallel_verticals(GanttScale::Week, width);
        }
    }

    #[test]
    fn newly_created_link_offsets_against_existing_verticals() {
        let mut nodes = mock_fixture_nodes();
        if let Some(go_live) = nodes.iter_mut().find(|node| node.id == 1013) {
            go_live.predecessors.clear();
        }
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Week,
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let before = gantt.layout_dependency_arrows(&bars);
        assert!(
            before
                .iter()
                .any(|arrow| arrow.from_id == 1005 && arrow.to_id == 1012),
            "Cart UI → spike must already exist"
        );
        assert!(
            before
                .iter()
                .all(|arrow| !(arrow.from_id == 1003 && arrow.to_id == 1013)),
            "Persist cart → Go-live is the newly created link"
        );
        if let Some(go_live) = nodes.iter_mut().find(|node| node.id == 1013) {
            go_live.predecessors = vec![1003];
        }
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let persist_live = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("newly created Persist cart → Go-live");
        let cart_spike = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1005 && arrow.to_id == 1012)
            .expect("Cart UI → spike");
        let dx = closest_vertical_gap(persist_live, cart_spike);
        assert!(
            dx >= 8.0,
            "a newly created Persist cart → Go-live must sit ≥8px off Cart UI → spike, dx={dx} persist x1={} x2={} spike x1={} x2={}",
            persist_live.x1,
            persist_live.x2,
            cart_spike.x1,
            cart_spike.x2
        );
    }

    #[test]
    fn link_drop_at_a_bar_edge_still_hits() {
        let bar = BarLayout {
            id: 2,
            kind: GanttBarKind::Scheduled,
            x: 200.0,
            width: 80.0,
            y: 40.0,
            summary: false,
            milestone: false,
            label: "bar".into(),
            title: "bar".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        assert_eq!(
            GanttView::hit_test_link_target(std::slice::from_ref(&bar), 200.0, 44.0),
            Some(2),
            "the start edge of a target must still count as a drop"
        );
        assert_eq!(
            GanttView::hit_test_link_target(std::slice::from_ref(&bar), 280.0, 44.0),
            Some(2),
            "the end edge of a target must still count as a drop"
        );
        assert_eq!(
            GanttView::hit_test_link_target(std::slice::from_ref(&bar), 240.0, 44.0),
            Some(2)
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
        // The leftover year at the left edge can be a sliver; complete
        // years (January→January) stay equal width.
        let full: Vec<f32> = widths.iter().copied().skip(1).collect();
        let first_full = full[0];
        assert!(
            full.iter().all(|width| (width - first_full).abs() < 3.0),
            "complete year columns must be equal width, got leftover={} full={full:?}",
            widths[0]
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
                .all(|(x, _)| (x - today_x).abs() >= 14.0 || *x <= 1.0),
            "month label must not sit on the Today flag: {majors:?} today_x={today_x}"
        );
        assert_eq!(gantt.scale.tick_label(today), "2");
    }

    #[test]
    fn day_header_keeps_each_month_on_its_own_segment() {
        let origin = Date::new(2026, 10, 24).unwrap();
        let today = Date::new(2026, 11, 2).unwrap();
        let gantt = GanttView {
            origin,
            scale: GanttScale::Day,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let today_x = gantt.today_x(today);
        let majors = gantt.header_major_labels_except(800.0, Some(today_x));
        let oct = majors.iter().find(|(_, label)| label.contains("Oct"));
        let nov = majors.iter().find(|(_, label)| label.contains("Nov"));
        assert!(
            oct.is_some_and(|(x, _)| *x <= HEADER_LEFT_PAD + 0.1),
            "October days on the left need an Oct label at the left pad, got {majors:?}"
        );
        assert!(
            nov.is_some_and(|(x, _)| *x > today_x + TODAY_PILL_W && *x > 8.0),
            "Nov 2026 must sit to the right of the Today pill, got {majors:?} today_x={today_x}"
        );
    }

    #[test]
    fn day_ticks_keep_the_first_in_view_day_and_the_right_edge() {
        let gantt = GanttView {
            origin: Date::new(2026, 10, 24).unwrap(),
            scale: GanttScale::Day,
            zoom: 1.0,
            timeline_width: 800.0,
            scroll_x: -10.0,
            ..Default::default()
        };
        let labels = gantt.header_tick_labels_except(800.0, None);
        let texts: Vec<&str> = labels.iter().map(|(_, label)| label.as_str()).collect();
        assert!(
            texts.contains(&"24"),
            "Day 24 must stay on the axis, got {texts:?}"
        );
        if let Some((x, label)) = labels.last() {
            let w = (label.chars().count() as f32 * 7.0 + 8.0).max(14.0);
            assert!(
                *x + w <= 800.0 - HEADER_EDGE_PAD + 0.1,
                "rightmost tick {label} at {x} width {w} must sit inside the right pad"
            );
        }
    }

    #[test]
    fn scale_buttons_are_names_and_zoom_percent_is_shared() {
        for scale in GanttScale::ALL {
            assert_eq!(
                scale.button_label(0.5),
                scale.label(),
                "scale buttons must not each invent a percent, got {}",
                scale.button_label(0.5)
            );
            assert_eq!(
                scale.toolbar_label(0.5, true),
                scale.label(),
                "selected scales share the standalone zoom control"
            );
        }
    }

    #[test]
    fn focus_work_keeps_unscheduled_diamonds_on_today() {
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
        let mut gantt = GanttView {
            origin: Date::new(2026, 11, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let today = Date::new(2026, 11, 2).unwrap();
        gantt.focus_work(std::slice::from_ref(&item), &iterations, 800.0, today);
        let rows = gantt.flatten(std::slice::from_ref(&item));
        let bars = gantt.layout_bars(std::slice::from_ref(&item), &rows, &iterations, 720.0);
        assert!(
            bars.iter()
                .any(|bar| { bar.marker && bar.x < 800.0 && bar.x + bar.width > 0.0 }),
            "unscheduled-only load must keep Today diamonds on screen, origin={} scroll={}",
            gantt.origin,
            gantt.scroll_x
        );
    }

    #[test]
    fn focus_work_auto_fits_when_today_has_no_scheduled_bars() {
        let item = node(1, None);
        let mut gantt = GanttView {
            origin: Date::new(2026, 11, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let today = Date::new(2026, 11, 2).unwrap();
        gantt.focus_work(std::slice::from_ref(&item), &[], 800.0, today);
        let rows = gantt.flatten(std::slice::from_ref(&item));
        let bars = gantt.layout_bars(&[item], &rows, &[], 720.0);
        assert!(
            bars.iter().any(|bar| {
                bar.kind == GanttBarKind::Scheduled && bar.x < 800.0 && bar.x + bar.width > 0.0
            }),
            "July work must be visible when Today has no nearby bars, origin={} scroll={}",
            gantt.origin,
            gantt.scroll_x
        );
    }

    #[test]
    fn week_tick_labels_stay_inside_the_timeline() {
        let gantt = GanttView {
            origin: Date::new(2026, 10, 12).unwrap(),
            scale: GanttScale::Week,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let labels = gantt.header_tick_labels(800.0);
        assert!(!labels.is_empty());
        for (x, label) in &labels {
            let w = (label.chars().count() as f32 * 7.0 + 8.0).max(14.0);
            assert!(
                *x >= HEADER_EDGE_PAD - 0.1,
                "week tick {label:?} at {x} must sit inside the left pad"
            );
            assert!(
                *x + w <= 800.0 - HEADER_EDGE_PAD + 0.1,
                "week tick {label:?} at {x} width {w} must sit inside the right pad"
            );
        }
    }

    #[test]
    fn day_tick_labels_keep_a_readable_gap() {
        let origin = Date::new(2026, 10, 24).unwrap();
        let today = Date::new(2026, 11, 2).unwrap();
        let gantt = GanttView {
            origin,
            scale: GanttScale::Day,
            zoom: 0.4,
            timeline_width: 800.0,
            ..Default::default()
        };
        let today_x = gantt.today_x(today);
        let labels = gantt.header_tick_labels_except(800.0, Some(today_x));
        for pair in labels.windows(2) {
            assert!(
                pair[1].0 - pair[0].0 >= TICK_MIN_GAP - 0.1,
                "day numbers must not collide: {labels:?}"
            );
        }
    }

    #[test]
    fn day_range_title_spans_the_visible_axis() {
        let origin = Date::new(2026, 10, 24).unwrap();
        let gantt = GanttView {
            origin,
            scale: GanttScale::Day,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let title = gantt.viewport_range_title();
        let end = gantt.date_for_x(799.0);
        assert!(
            title.starts_with("Oct 24")
                && title.contains('–')
                && title.contains(&compact_date(end)),
            "Day toolbar must show the visible span, got {title:?} end={end}"
        );
        assert!(
            !title.eq("Oct 24"),
            "a single date must not stand in for the whole Day axis"
        );
    }

    #[test]
    fn day_range_title_uses_the_clipped_leftover_tick() {
        let mut gantt = GanttView {
            origin: Date::new(2026, 10, 24).unwrap(),
            scale: GanttScale::Day,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        // Half a day into Oct 23's leftover column: the sticky "23" sits at
        // x = 0 while date_for_x(0) still rounds to Oct 24.
        gantt.scroll_x = -20.0;
        let title = gantt.viewport_range_title();
        assert!(
            title.starts_with("Oct 23"),
            "toolbar must follow the clipped leftover tick, not date_for_x(0), got {title:?}"
        );
        assert_eq!(
            gantt.viewport_axis_start_date(),
            Date::new(2026, 10, 23).unwrap()
        );
        assert_eq!(gantt.date_for_x(0.0), Date::new(2026, 10, 24).unwrap());
    }

    #[test]
    fn week_range_title_matches_the_first_axis_tick() {
        let origin = Date::new(2026, 10, 12).unwrap();
        let gantt = GanttView {
            origin,
            scale: GanttScale::Week,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let title = gantt.viewport_range_title();
        let start = gantt.viewport_axis_start_date();
        assert!(
            title.starts_with(&compact_date(start)),
            "Week toolbar must follow the first tick ({start}), got {title:?}"
        );
        assert!(
            !title.starts_with("Week of"),
            "Week toolbar must show the visible span, not 'Week of …', got {title:?}"
        );
    }

    #[test]
    fn week_header_shifts_the_month_off_the_today_pill() {
        let today = Date::new(2026, 11, 2).unwrap();
        let centered_days =
            ((800.0 * TODAY_ALIGN) / GanttScale::Week.pixels_per_day(1.0)).round() as i32;
        for (origin, scroll_x) in [
            (today, 0.0),
            (today.saturating_sub(centered_days.days()), 0.0),
            (Date::new(2026, 11, 1).unwrap(), 0.0),
        ] {
            let gantt = GanttView {
                origin,
                scroll_x,
                scale: GanttScale::Week,
                zoom: 1.0,
                timeline_width: 800.0,
                ..Default::default()
            };
            let today_x = gantt.today_x(today);
            let majors = gantt.header_major_labels_except(800.0, Some(today_x));
            assert!(
                majors.iter().any(|(_, label)| label.contains("Nov")),
                "Week zoom must keep the Nov month label, origin={origin} got {majors:?}"
            );
            let pill_left = today_pill_left(today_x, 800.0);
            let pill_right = pill_left + TODAY_PILL_W;
            for (x, label) in &majors {
                let text_w = header_label_px(label);
                let overlaps = *x < pill_right && *x + text_w > pill_left;
                assert!(
                    !overlaps,
                    "Week label {label:?} at {x} (w={text_w}) must not overlap Today {pill_left}..{pill_right}, origin={origin} majors={majors:?}"
                );
            }
        }
    }

    #[test]
    fn zoom_steps_use_a_fixed_ratio() {
        assert!((GanttView::next_zoom(1.0, 1) - 1.25).abs() < 0.0001);
        assert!((GanttView::next_zoom(1.25, 1) - 1.5625).abs() < 0.0001);
        assert!((GanttView::next_zoom(1.0, -1) - 0.8).abs() < 0.0001);
        assert!((GanttView::next_zoom(0.6, 1) - 0.75).abs() < 0.0001);
        assert!((GanttView::next_zoom(0.75, 1) - 0.9375).abs() < 0.0001);
        assert_eq!(GanttView::next_zoom(0.4, -1), 0.4);
        assert_eq!(GanttView::next_zoom(4.0, 1), 4.0);
    }

    #[test]
    fn new_child_inserts_directly_under_parent() {
        let nodes = vec![
            node(1, None),
            node(2, Some(1)),
            node(3, Some(2)),
            node(4, Some(1)),
        ];
        assert_eq!(insert_index_for_child(&nodes, Some(1)), 1);
        assert_eq!(insert_index_for_child(&nodes, Some(2)), 2);
        assert_eq!(insert_index_for_child(&nodes, None), 4);
        assert_eq!(insert_index_for_child(&nodes, Some(4)), 4);
        assert_eq!(insert_index_for_sibling(&nodes, Some(2)), 3);
        assert_eq!(insert_index_for_sibling(&nodes, Some(1)), 4);
        assert_eq!(insert_index_for_sibling(&nodes, None), 4);
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
    fn far_apart_dependency_uses_a_short_elbow() {
        let mut a = node(1, None);
        let mut b = node(2, None);
        let mut c = node(3, None);
        a.parent_id = None;
        a.title = "Persist cart".into();
        b.parent_id = Some(1);
        b.title = "Go-live milestone".into();
        b.start_date = Some("2026-08-15T00:00:00Z".into());
        b.target_date = Some("2026-08-15T00:00:00Z".into());
        c.parent_id = None;
        c.title = "Unparented spike".into();
        c.start_date = Some("2026-07-21T00:00:00Z".into());
        c.target_date = Some("2026-08-04T00:00:00Z".into());
        c.predecessors = vec![1];
        let nodes = vec![a, b, c];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert_eq!(arrows.len(), 1);
        let pred = bars.iter().find(|bar| bar.id == 1).unwrap();
        let mid = bars.iter().find(|bar| bar.id == 2).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 3).unwrap();
        assert!(
            (succ.y - pred.y).abs() > ROW_HEIGHT + 1.0,
            "fixture must place the successor at least two rows below"
        );
        assert!(
            (arrows[0].x1 - (pred.x + pred.width + 12.0)).abs() < 0.5,
            "outgoing stub must leave the painted bar, got x1={} bar_right={}",
            arrows[0].x1,
            pred.x + pred.width
        );
        assert!(
            arrows[0].x2 <= succ.x - 12.0,
            "incoming stub must sit left of the successor, got x2={} succ.x={}",
            arrows[0].x2,
            succ.x
        );
        assert!(
            (arrows[0].x_head - succ.x).abs() < 3.0,
            "arrowhead must terminate at the target start edge, got {} vs {}",
            arrows[0].x_head,
            succ.x
        );
        let mid_label_left = mid.x;
        let mid_label_right = bar_label_right(mid);
        let crosses_label = arrows[0].x1 >= mid_label_left - 1.0
            && arrows[0].x1 <= mid_label_right + 1.0
            && arrows[0].lane_y >= mid.y
            && arrows[0].lane_y < mid.y + ROW_HEIGHT;
        assert!(
            !crosses_label,
            "the lane must not cross the Go-live label, x1={} lane_y={} mid_y={} label={mid_label_left}..{mid_label_right}",
            arrows[0].x1, arrows[0].lane_y, mid.y
        );
        assert!(
            arrows[0].x1.max(arrows[0].x2) < bar_label_right(mid) - 8.0 || arrows[0].x2 < 400.0,
            "must not loop out past in-between labels to the far right, x1={} x2={} mid_label={}",
            arrows[0].x1,
            arrows[0].x2,
            bar_label_right(mid)
        );
        assert!(
            !arrows[0].wraps_around_successor(),
            "next-day links must be a short elbow, not a wrap-around rectangle"
        );
        assert!(
            arrows[0].lane_y < mid.y,
            "the outgoing elbow must stay in the predecessor gutter, not drop through #{}, lane {} mid {}",
            mid.id,
            arrows[0].lane_y,
            mid.y
        );
        let (mid_left, mid_right) = (mid.x, bar_label_right(mid));
        assert!(
            arrows[0].x2 < mid_left - 1.0 || arrows[0].x2 > mid_right + 1.0,
            "the inbound vertical must not cross the Go-live label, x2={} label={mid_left}..{mid_right}",
            arrows[0].x2
        );
    }

    #[test]
    fn intervening_cart_ui_row_is_not_crossed_by_the_inbound_vertical() {
        let mut pred = node(1, None);
        pred.title = "Persist cart".into();
        let mut mid = node(2, Some(1));
        mid.title = "Cart UI".into();
        let mut succ = node(3, None);
        succ.title = "Unparented spike".into();
        succ.start_date = Some("2026-07-21T00:00:00Z".into());
        succ.target_date = Some("2026-08-04T00:00:00Z".into());
        succ.predecessors = vec![1];
        let nodes = vec![pred, mid, succ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert_eq!(arrows.len(), 1);
        let mid = bars.iter().find(|bar| bar.id == 2).unwrap();
        let (left, right) = (mid.x - HANDLE_WIDTH, bar_label_right(mid));
        assert!(
            arrows[0].x2 < left - 1.0 || arrows[0].x2 > right + 1.0,
            "the inbound vertical must skip the Cart UI row, x2={} cart={left}..{right}",
            arrows[0].x2
        );
        assert!(
            arrows[0].lane_y < mid.y || arrows[0].lane_y >= mid.y + ROW_HEIGHT,
            "the horizontal lane must not run through the Cart UI row body, lane {} mid {}",
            arrows[0].lane_y,
            mid.y
        );
    }

    #[test]
    fn backward_inbound_stays_on_the_right_of_the_head() {
        let mut pred = node(1, None);
        pred.title = "Persist cart".into();
        pred.start_date = Some("2026-08-20T00:00:00Z".into());
        pred.target_date = Some("2026-08-25T00:00:00Z".into());
        let mut mid = node(2, Some(1));
        mid.title = "Go-live milestone".into();
        mid.start_date = Some("2026-08-08T00:00:00Z".into());
        mid.target_date = Some("2026-08-20T00:00:00Z".into());
        let mut succ = node(3, None);
        succ.title = "Unparented spike".into();
        succ.start_date = Some("2026-07-21T00:00:00Z".into());
        succ.target_date = Some("2026-08-04T00:00:00Z".into());
        succ.predecessors = vec![1];
        let nodes = vec![pred, mid, succ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert_eq!(arrows.len(), 1);
        let pred = bars.iter().find(|bar| bar.id == 1).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 3).unwrap();
        let mid = bars.iter().find(|bar| bar.id == 2).unwrap();
        assert_exits_right_without_left_detour(&arrows[0], pred, succ, gantt.timeline_width);
        // Wrap routing occupies the bar body, not an outside title (Year
        // labels would shove the loop). The drop sits at the source stub,
        // right of Go-live's painted body.
        let body_left = mid.x - HANDLE_WIDTH;
        let body_right = mid.x + mid.width.max(8.0) + 4.0;
        assert!(
            arrows[0].x2 < body_left - 1.0 || arrows[0].x2 > body_right + 1.0,
            "the wrap drop must not sit on the intervening bar, x2={} mid={body_left}..{body_right}",
            arrows[0].x2
        );
    }

    #[test]
    fn moved_parent_link_loops_at_the_source_end_not_go_live() {
        // Mock tree after dragging Persist cart later and "Move children too":
        // children shift with the parent; Go-live (not a child) stays put.
        let mut epic = node(1001, None);
        epic.title = "Checkout rewrite".into();
        epic.start_date = Some("2026-07-01T00:00:00Z".into());
        epic.target_date = Some("2026-09-30T00:00:00Z".into());
        let mut cart = node(1002, Some(1001));
        cart.title = "Cart".into();
        cart.start_date = Some("2026-07-06T00:00:00Z".into());
        cart.target_date = Some("2026-08-14T00:00:00Z".into());
        let mut pred = node(1003, Some(1002));
        pred.title = "Persist cart".into();
        pred.start_date = Some("2026-08-04T00:00:00Z".into());
        pred.target_date = Some("2026-08-21T00:00:00Z".into());
        let mut child = node(1005, Some(1003));
        child.title = "Cart UI".into();
        child.start_date = Some("2026-08-19T00:00:00Z".into());
        child.target_date = None;
        let mut go_live = node(1013, Some(1001));
        go_live.title = "Go-live milestone".into();
        go_live.start_date = None;
        go_live.target_date = Some("2026-08-15T00:00:00Z".into());
        let mut succ = node(1012, None);
        succ.title = "Unparented spike".into();
        succ.start_date = Some("2026-07-21T00:00:00Z".into());
        succ.target_date = Some("2026-08-04T00:00:00Z".into());
        succ.predecessors = vec![1003];
        let nodes = vec![epic, cart, pred, child, go_live, succ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let arrow = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
            .expect("#1003→#1012");
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 1012).unwrap();
        assert!(
            pred.x > succ.x + 1.0,
            "fixture must place the moved parent after the successor start"
        );
        assert_exits_right_without_left_detour(arrow, pred, succ, gantt.timeline_width);
        let child = bars.iter().find(|bar| bar.id == 1005).unwrap();
        let go_live = bars.iter().find(|bar| bar.id == 1013).unwrap();
        let (child_left, child_right) = bar_occupied_x(child, false, Some(gantt.timeline_width));
        let (live_left, live_right) = bar_occupied_x(go_live, false, None);
        assert!(
            !x_hits_span(arrow.x2, child_left, child_right),
            "vertical x2={} must miss #1005 including its open-ended cue {child_left}..{child_right}",
            arrow.x2
        );
        assert!(
            !x_hits_span(arrow.x2, live_left, live_right),
            "vertical x2={} must miss #1013 occupied {live_left}..{live_right}",
            arrow.x2
        );
    }

    #[test]
    fn wrap_vertical_runs_in_a_gap_not_through_intervening_bars() {
        let mut pred = node(1003, Some(1002));
        pred.title = "Persist cart".into();
        pred.start_date = Some("2026-08-04T00:00:00Z".into());
        pred.target_date = Some("2026-08-21T00:00:00Z".into());
        let mut child = node(1005, Some(1003));
        child.title = "Cart UI".into();
        child.start_date = Some("2026-08-10T00:00:00Z".into());
        child.target_date = Some("2026-08-28T00:00:00Z".into());
        let mut succ = node(1012, None);
        succ.title = "Unparented spike".into();
        succ.start_date = Some("2026-07-21T00:00:00Z".into());
        succ.target_date = Some("2026-08-04T00:00:00Z".into());
        succ.predecessors = vec![1003];
        let nodes = vec![node(1001, None), node(1002, Some(1001)), pred, child, succ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let arrow = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
            .expect("#1003→#1012");
        let mid = bars.iter().find(|bar| bar.id == 1005).unwrap();
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let (left, right) = bar_occupied_x(mid, false, None);
        assert!(
            !x_hits_span(arrow.x2, left, right),
            "vertical x2={} must miss #1005 occupied {left}..{right}, pred_end={}",
            arrow.x2,
            pred.x + pred.width
        );
        assert!(
            (arrow.x2 - (bars.iter().find(|bar| bar.id == 1012).unwrap().x - 12.0)).abs() < 2.5
                || arrow.x2 + 0.5 >= pred.x + pred.width + 10.0,
            "must use the successor gap or a wrap past the source, x2={} pred_end={}",
            arrow.x2,
            pred.x + pred.width
        );
        assert!(
            arrow.x2 > 80.0,
            "must not detour to the chart left edge, x2={}",
            arrow.x2
        );
    }

    #[test]
    fn wrap_drop_slides_past_later_intervening_bars() {
        let pred = BarLayout {
            id: 1,
            kind: GanttBarKind::Scheduled,
            x: 300.0,
            width: 80.0,
            y: 0.0,
            summary: false,
            milestone: false,
            label: "pred".into(),
            title: "Pred".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        let mid1 = BarLayout {
            id: 2,
            kind: GanttBarKind::Scheduled,
            x: 360.0,
            width: 50.0,
            y: ROW_HEIGHT,
            summary: false,
            milestone: false,
            label: "mid1".into(),
            title: "Mid1".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        let mid2 = BarLayout {
            id: 3,
            kind: GanttBarKind::Scheduled,
            x: 418.0,
            width: 60.0,
            y: ROW_HEIGHT * 2.0,
            summary: false,
            milestone: false,
            label: "mid2".into(),
            title: "Mid2".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        let blocker = BarLayout {
            id: 5,
            kind: GanttBarKind::Scheduled,
            x: 40.0,
            width: 90.0,
            y: ROW_HEIGHT * 2.5,
            summary: false,
            milestone: false,
            label: "block".into(),
            title: "Block".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        let succ = BarLayout {
            id: 4,
            kind: GanttBarKind::Scheduled,
            x: 80.0,
            width: 80.0,
            y: ROW_HEIGHT * 3.0,
            summary: false,
            milestone: false,
            label: "succ".into(),
            title: "Succ".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: vec![1],
            marker: false,
            open_ended: false,
        };
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let arrows = gantt.layout_dependency_arrows(&[
            pred.clone(),
            mid1.clone(),
            mid2.clone(),
            blocker.clone(),
            succ,
        ]);
        assert_eq!(arrows.len(), 1);
        let stub = pred.x + pred.width + 12.0;
        let (left1, right1) = bar_occupied_x(&mid1, false, None);
        let (left2, right2) = bar_occupied_x(&mid2, false, None);
        assert!(
            x_hits_span(stub, left1, right1),
            "precondition: source stub {stub} must sit on mid1 {left1}..{right1}"
        );
        let naive = right1 + 6.0;
        assert!(
            x_hits_span(naive, left2, right2),
            "precondition: one-step slide {naive} must still hit mid2 {left2}..{right2}"
        );
        assert!(
            arrows[0].wraps_around_successor(),
            "must keep a local loop, x1={} x2={} x_head={}",
            arrows[0].x1,
            arrows[0].x2,
            arrows[0].x_head
        );
        assert!(
            !x_hits_span(arrows[0].x2, left1, right1),
            "wrap drop x2={} must miss mid1 {left1}..{right1}",
            arrows[0].x2
        );
        assert!(
            !x_hits_span(arrows[0].x2, left2, right2),
            "wrap drop x2={} must miss the later intervening bar {left2}..{right2}",
            arrows[0].x2
        );
        assert!(
            arrows[0].x2 > 80.0,
            "must not detour to the chart left edge, x2={}",
            arrows[0].x2
        );
    }

    #[test]
    fn wrap_drop_slides_left_of_an_open_ended_cue() {
        let pred = BarLayout {
            id: 1,
            kind: GanttBarKind::Scheduled,
            x: 300.0,
            width: 80.0,
            y: 0.0,
            summary: false,
            milestone: false,
            label: "pred".into(),
            title: "Pred".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        let mid = BarLayout {
            id: 2,
            kind: GanttBarKind::Scheduled,
            x: 340.0,
            width: 48.0,
            y: ROW_HEIGHT,
            summary: false,
            milestone: false,
            label: "mid".into(),
            title: "Cart UI".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: true,
        };
        let succ = BarLayout {
            id: 3,
            kind: GanttBarKind::Scheduled,
            x: 80.0,
            width: 80.0,
            y: ROW_HEIGHT * 2.0,
            summary: false,
            milestone: false,
            label: "succ".into(),
            title: "Succ".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: vec![1],
            marker: false,
            open_ended: false,
        };
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let arrows = gantt.layout_dependency_arrows(&[pred.clone(), mid.clone(), succ]);
        assert_eq!(arrows.len(), 1);
        let stub = pred.x + pred.width + 12.0;
        let (cue_left, cue_right) = bar_occupied_x(&mid, false, Some(gantt.timeline_width));
        assert!(
            x_hits_span(stub, cue_left, cue_right),
            "precondition: source stub {stub} sits in the open-ended cue {cue_left}..{cue_right}"
        );
        assert!(
            arrows[0].wraps_around_successor(),
            "must keep a local wrap, x2={} x_head={}",
            arrows[0].x2,
            arrows[0].x_head
        );
        assert!(
            !x_hits_span(arrows[0].x2, cue_left, cue_right),
            "wrap vertical x2={} must miss the open-ended cue {cue_left}..{cue_right}",
            arrows[0].x2
        );
        assert!(
            arrows[0].x2 + 0.5 >= pred.x,
            "must not detour left of the source, x2={} pred.x={}",
            arrows[0].x2,
            pred.x
        );
    }

    #[test]
    fn wrap_vertical_misses_gutter_no_dates_pills() {
        let pred = BarLayout {
            id: 1,
            kind: GanttBarKind::Scheduled,
            x: 300.0,
            width: 80.0,
            y: 0.0,
            summary: false,
            milestone: false,
            label: "pred".into(),
            title: "Pred".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        let pill = BarLayout {
            id: 2,
            kind: GanttBarKind::NoFields,
            x: UNSCHEDULED_GUTTER_X,
            width: NO_FIELDS_HINT_W,
            y: ROW_HEIGHT,
            summary: false,
            milestone: false,
            label: "No dates".into(),
            title: "No fields".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        let succ = BarLayout {
            id: 3,
            kind: GanttBarKind::Scheduled,
            x: 80.0,
            width: 80.0,
            y: ROW_HEIGHT * 2.0,
            summary: false,
            milestone: false,
            label: "succ".into(),
            title: "Succ".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: vec![1],
            marker: false,
            open_ended: false,
        };
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let arrows = gantt.layout_dependency_arrows(&[pred, pill.clone(), succ.clone()]);
        assert_eq!(arrows.len(), 1);
        let (left, right) = bar_occupied_x(&pill, false, None);
        assert!(
            !x_hits_span(arrows[0].x1, left, right) && !x_hits_span(arrows[0].x2, left, right),
            "connector x1={} x2={} must miss the No dates pill {left}..{right}",
            arrows[0].x1,
            arrows[0].x2
        );
        assert!(
            arrows[0].x2 >= gutter_clear_x(&[pill.clone(), succ.clone()]) - 0.5
                || arrows[0].x_head > 80.0,
            "inbound must not sit on the left edge, x2={} x_head={}",
            arrows[0].x2,
            arrows[0].x_head
        );
        assert!(
            arrows[0].inbound_right || arrows[0].x_head >= 70.0,
            "arrowhead must not park on the gutter, inbound_right={} x_head={}",
            arrows[0].inbound_right,
            arrows[0].x_head
        );
    }

    #[test]
    fn left_dot_resize_of_successor_keeps_the_backward_loop_short() {
        // After dragging #1012's start from Jul 21 back to Jul 6, #1003
        // (Jul 6→20) still ends after the successor starts. The inbound
        // column at succ.x-12 is a gap, but routing there is a long left
        // run on the predecessor gutter. Keep the wrap at the source stub.
        let mut pred = node(1003, Some(1002));
        pred.title = "Persist cart".into();
        pred.start_date = Some("2026-07-06T00:00:00Z".into());
        pred.target_date = Some("2026-07-20T00:00:00Z".into());
        let mut child = node(1005, Some(1003));
        child.title = "Cart UI".into();
        child.start_date = Some("2026-07-21T00:00:00Z".into());
        child.target_date = None;
        let mut unscheduled = node(1014, Some(1002));
        unscheduled.title = "Backlog grooming".into();
        unscheduled.start_date = None;
        unscheduled.target_date = None;
        let mut go_live = node(1013, Some(1001));
        go_live.title = "Go-live milestone".into();
        go_live.start_date = None;
        go_live.target_date = Some("2026-08-15T00:00:00Z".into());
        let mut succ = node(1012, None);
        succ.title = "Unparented spike".into();
        succ.start_date = Some("2026-07-06T00:00:00Z".into());
        succ.target_date = Some("2026-08-04T00:00:00Z".into());
        succ.predecessors = vec![1003];
        let nodes = vec![
            node(1001, None),
            node(1002, Some(1001)),
            pred,
            child,
            unscheduled,
            go_live,
            succ,
        ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let arrow = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
            .expect("#1003→#1012");
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 1012).unwrap();
        assert!(
            pred.x + pred.width > succ.x + 12.0,
            "fixture must overlap after the left-dot resize, pred_end={} succ.x={}",
            pred.x + pred.width,
            succ.x
        );
        assert_exits_right_without_left_detour(arrow, pred, succ, gantt.timeline_width);
        assert!(
            (arrow.x_head - succ.x).abs() < 0.5,
            "after a left-edge resize the link must still enter the start, x_head={} succ.x={}",
            arrow.x_head,
            succ.x
        );
        assert!(
            arrow.wraps_around_successor(),
            "left-edge overlap must keep the short backward loop, x2={} x_head={}",
            arrow.x2,
            arrow.x_head
        );
        let child = bars.iter().find(|bar| bar.id == 1005).unwrap();
        let (cue_left, cue_right) = bar_occupied_x(child, false, Some(gantt.timeline_width));
        assert!(
            !x_hits_span(arrow.x2, cue_left, cue_right),
            "wrap vertical x2={} must miss Cart UI's open-ended cue {cue_left}..{cue_right}",
            arrow.x2
        );
    }

    #[test]
    fn new_forward_link_to_go_live_does_not_detour_left_of_the_source() {
        let mut pred = node(1003, Some(1002));
        pred.title = "Persist cart".into();
        pred.start_date = Some("2026-07-06T00:00:00Z".into());
        pred.target_date = Some("2026-07-20T00:00:00Z".into());
        let mut child = node(1005, Some(1003));
        child.title = "Cart UI".into();
        child.start_date = Some("2026-07-21T00:00:00Z".into());
        child.target_date = None;
        let mut unscheduled = node(1014, Some(1002));
        unscheduled.title = "Backlog grooming".into();
        unscheduled.start_date = None;
        unscheduled.target_date = None;
        let mut go_live = node(1013, Some(1001));
        go_live.title = "Go-live milestone".into();
        go_live.start_date = None;
        go_live.target_date = Some("2026-08-15T00:00:00Z".into());
        go_live.predecessors = vec![1003];
        let mut spike = node(1012, None);
        spike.title = "Unparented spike".into();
        spike.start_date = Some("2026-07-21T00:00:00Z".into());
        spike.target_date = Some("2026-08-04T00:00:00Z".into());
        let nodes = vec![
            node(1001, None),
            node(1002, Some(1001)),
            pred,
            child,
            unscheduled,
            go_live,
            spike,
        ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let arrow = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("#1003→#1013");
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 1013).unwrap();
        assert_exits_right_without_left_detour(arrow, pred, succ, gantt.timeline_width);
        assert!(
            (arrow.x_head - succ.x).abs() < 0.5,
            "forward #1003→#1013 must end at the milestone start, not overshoot to the right, x_head={} succ.x={}",
            arrow.x_head,
            succ.x
        );
        assert!(
            !arrow.inbound_right,
            "forward milestone links must enter the start, not the right tip"
        );
        let cart = bars.iter().find(|bar| bar.id == 1005).unwrap();
        let (cart_left, cart_right) = bar_occupied_x(cart, true, Some(gantt.timeline_width));
        assert!(
            !x_hits_span(arrow.x2, cart_left, cart_right),
            "the inbound vertical must skip Cart UI including its open-ended cue, x2={} cart={cart_left}..{cart_right}",
            arrow.x2
        );
        assert!(
            arrow.lane_y < cart.y || arrow.lane_y >= cart.y + ROW_HEIGHT,
            "the horizontal lane must stay in a row gap, not through Cart UI, lane {} cart {}",
            arrow.lane_y,
            cart.y
        );
    }

    #[test]
    fn day_view_link_to_go_live_skips_the_cart_ui_cue() {
        let mut pred = node(1003, Some(1002));
        pred.title = "Persist cart".into();
        pred.start_date = Some("2026-07-06T00:00:00Z".into());
        pred.target_date = Some("2026-07-20T00:00:00Z".into());
        let mut child = node(1005, Some(1003));
        child.title = "Cart UI".into();
        child.start_date = Some("2026-07-21T00:00:00Z".into());
        child.target_date = None;
        let mut go_live = node(1013, Some(1001));
        go_live.title = "Go-live milestone".into();
        go_live.start_date = None;
        go_live.target_date = Some("2026-08-15T00:00:00Z".into());
        go_live.predecessors = vec![1003];
        let nodes = vec![
            node(1001, None),
            node(1002, Some(1001)),
            pred,
            child,
            go_live,
        ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Day,
            timeline_width: 2400.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let arrow = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("#1003→#1013");
        let cart = bars.iter().find(|bar| bar.id == 1005).unwrap();
        let (cart_left, cart_right) = bar_occupied_x(cart, true, Some(gantt.timeline_width));
        assert!(
            !x_hits_span(arrow.x2, cart_left, cart_right),
            "Day-view elbow must not drop through Cart UI, x2={} cart={cart_left}..{cart_right}",
            arrow.x2
        );
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 1013).unwrap();
        assert_exits_right_without_left_detour(arrow, pred, succ, gantt.timeline_width);
    }

    #[test]
    fn finish_to_start_arrow_endpoint_equals_target_start_forward() {
        let mut pred = node(1003, Some(1002));
        pred.title = "Persist cart".into();
        pred.start_date = Some("2026-07-06T00:00:00Z".into());
        pred.target_date = Some("2026-07-20T00:00:00Z".into());
        let mut go_live = node(1013, Some(1001));
        go_live.title = "Go-live milestone".into();
        go_live.start_date = None;
        go_live.target_date = Some("2026-08-15T00:00:00Z".into());
        go_live.predecessors = vec![1003];
        let nodes = vec![
            node(1001, None),
            node(1002, Some(1001)),
            pred,
            node(1005, Some(1003)),
            go_live,
        ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let arrow = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("#1003→#1013");
        let succ = bars.iter().find(|bar| bar.id == 1013).unwrap();
        assert!(
            (arrow.x_head - succ.x).abs() < 0.5,
            "forward arrow endpoint x must equal the target start x, x_head={} succ.x={}",
            arrow.x_head,
            succ.x
        );
    }

    #[test]
    fn finish_to_start_arrow_endpoint_equals_target_start_backward() {
        let mut pred = node(1003, Some(1002));
        pred.title = "Persist cart".into();
        pred.start_date = Some("2026-07-06T00:00:00Z".into());
        pred.target_date = Some("2026-07-20T00:00:00Z".into());
        let mut succ = node(1012, None);
        succ.title = "Unparented spike".into();
        succ.start_date = Some("2026-07-06T00:00:00Z".into());
        succ.target_date = Some("2026-08-04T00:00:00Z".into());
        succ.predecessors = vec![1003];
        let nodes = vec![node(1001, None), node(1002, Some(1001)), pred, succ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let arrow = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
            .expect("#1003→#1012");
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 1012).unwrap();
        assert!(
            pred.x + pred.width > succ.x + 12.0,
            "fixture must be a backward/overlap FS link, pred_end={} succ.x={}",
            pred.x + pred.width,
            succ.x
        );
        assert!(
            (arrow.x_head - succ.x).abs() < 0.5,
            "backward arrow endpoint x must equal the target start x, x_head={} succ.x={}",
            arrow.x_head,
            succ.x
        );
        assert!(
            arrow.wraps_around_successor(),
            "overlap must use the short backward loop, x1={} x2={} x_head={}",
            arrow.x1,
            arrow.x2,
            arrow.x_head
        );
    }

    #[test]
    fn backward_link_loops_locally_not_to_the_chart_left() {
        let mut pred = node(1003, Some(1002));
        pred.title = "Persist cart".into();
        pred.start_date = Some("2026-08-04T00:00:00Z".into());
        pred.target_date = Some("2026-08-21T00:00:00Z".into());
        let mut child = node(1005, Some(1003));
        child.title = "Cart UI".into();
        child.start_date = Some("2026-07-21T00:00:00Z".into());
        child.target_date = None;
        let mut unscheduled = node(1014, Some(1002));
        unscheduled.title = "Backlog grooming".into();
        unscheduled.start_date = None;
        unscheduled.target_date = None;
        let mut go_live = node(1013, Some(1001));
        go_live.title = "Go-live milestone".into();
        go_live.start_date = None;
        go_live.target_date = Some("2026-08-15T00:00:00Z".into());
        let mut succ = node(1012, None);
        succ.title = "Unparented spike".into();
        succ.start_date = Some("2026-07-21T00:00:00Z".into());
        succ.target_date = Some("2026-08-04T00:00:00Z".into());
        succ.predecessors = vec![1003];
        let nodes = vec![
            node(1001, None),
            node(1002, Some(1001)),
            pred,
            child,
            unscheduled,
            go_live,
            succ,
        ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let arrow = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
            .expect("#1003→#1012");
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 1012).unwrap();
        assert!(pred.x + pred.width > succ.x + 12.0);
        assert_exits_right_without_left_detour(arrow, pred, succ, gantt.timeline_width);
        let child = bars.iter().find(|bar| bar.id == 1005).unwrap();
        let go_live = bars.iter().find(|bar| bar.id == 1013).unwrap();
        let (child_left, child_right) = bar_occupied_x(child, false, None);
        let (live_left, live_right) = bar_occupied_x(go_live, false, None);
        assert!(
            !x_hits_span(arrow.x2, child_left, child_right),
            "vertical x2={} must miss #1005 occupied {child_left}..{child_right}",
            arrow.x2
        );
        assert!(
            !x_hits_span(arrow.x2, live_left, live_right),
            "vertical x2={} must miss #1013 occupied {live_left}..{live_right}",
            arrow.x2
        );
    }

    #[test]
    fn stub_clamp_does_not_recross_an_intervening_bar() {
        let pred = BarLayout {
            id: 1,
            kind: GanttBarKind::Scheduled,
            x: 20.0,
            width: 40.0,
            y: 0.0,
            summary: false,
            milestone: false,
            label: "pred".into(),
            title: "Pred".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        let mid = BarLayout {
            id: 2,
            kind: GanttBarKind::Scheduled,
            x: 100.0,
            width: 90.0,
            y: ROW_HEIGHT,
            summary: false,
            milestone: false,
            label: "mid".into(),
            title: "MMMMMMMMMMMMMMMM".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        let succ = BarLayout {
            id: 3,
            kind: GanttBarKind::Scheduled,
            x: 200.0,
            width: 80.0,
            y: ROW_HEIGHT * 2.0,
            summary: false,
            milestone: false,
            label: "succ".into(),
            title: "Succ".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: vec![1],
            marker: false,
            open_ended: false,
        };
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let arrows = gantt.layout_dependency_arrows(&[pred, mid.clone(), succ.clone()]);
        assert_eq!(arrows.len(), 1);
        let (left, right) = bar_occupied_x(&mid, true, None);
        assert!(
            arrows[0].x2 < left - 1.0 || arrows[0].x2 > right + 1.0,
            "forcing a 12px stub must not recross the intervening bar, x2={} mid={left}..{right} x_head={}",
            arrows[0].x2,
            arrows[0].x_head
        );
        assert!(
            (arrows[0].x_head - succ.x).abs() < 3.0,
            "the head must still sit on the successor start, x_head={} succ.x={}",
            arrows[0].x_head,
            succ.x
        );
        assert!(
            (arrows[0].x2 - arrows[0].x_head).abs() > 8.0,
            "precondition: dodging must leave the elbow away from the target, x2={} x_head={}",
            arrows[0].x2,
            arrows[0].x_head
        );
        assert!(
            (arrows[0].arrowhead_left() - (arrows[0].x_head - 16.0)).abs() < 0.5,
            "▶ must sit on the target edge, not the elbow, arrowhead={} x2={} x_head={}",
            arrows[0].arrowhead_left(),
            arrows[0].x2,
            arrows[0].x_head
        );
    }

    #[test]
    fn backward_dependency_routes_locally_not_to_the_left_edge() {
        let mut pred = node(1013, Some(1));
        pred.title = "Go-live milestone".into();
        pred.start_date = Some("2026-08-15T00:00:00Z".into());
        pred.target_date = Some("2026-08-15T00:00:00Z".into());
        let mut succ = node(1012, None);
        succ.title = "Unparented spike".into();
        succ.start_date = Some("2026-07-21T00:00:00Z".into());
        succ.target_date = Some("2026-08-04T00:00:00Z".into());
        succ.predecessors = vec![1013];
        let nodes = vec![node(1, None), pred, succ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert_eq!(arrows.len(), 1);
        let pred = bars.iter().find(|bar| bar.id == 1013).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 1012).unwrap();
        assert!(
            pred.x > succ.x + 1.0,
            "fixture must place the predecessor start after the successor start"
        );
        assert_exits_right_without_left_detour(&arrows[0], pred, succ, gantt.timeline_width);
        assert!(
            arrows[0].inbound_right && arrows[0].stub_start() > arrows[0].x_head + 8.0
                || !arrows[0].inbound_right && arrows[0].stub_start() < arrows[0].x_head - 8.0,
            "the inbound stub must sit outside the bar so the arrowhead reaches an edge, stub={} x_head={} inbound_right={}",
            arrows[0].stub_start(),
            arrows[0].x_head,
            arrows[0].inbound_right
        );
    }

    #[test]
    fn batch_move_recomputes_link_geometry_on_screen() {
        let mut pred = node(1005, Some(1));
        pred.title = "Cart UI".into();
        pred.start_date = Some("2026-07-21T00:00:00Z".into());
        pred.target_date = None;
        let mut succ = node(1012, None);
        succ.title = "Unparented spike".into();
        succ.start_date = Some("2026-07-21T00:00:00Z".into());
        succ.target_date = Some("2026-08-04T00:00:00Z".into());
        succ.predecessors = vec![1005];
        let mut nodes = vec![node(1, None), pred, succ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 360.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        if let Some(node) = nodes.iter_mut().find(|node| node.id == 1005) {
            node.start_date = Some("2026-10-01T00:00:00Z".into());
        }
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert_eq!(arrows.len(), 1, "the #1005→#1012 link must still layout");
        let pred_bar = bars.iter().find(|bar| bar.id == 1005).expect("pred");
        assert!(
            pred_bar.x > gantt.timeline_width,
            "fixture must place the shifted predecessor off the right edge, got x={}",
            pred_bar.x
        );
        assert!(
            arrows[0].x_head >= 10.0 && arrows[0].x_head <= gantt.timeline_width - 8.0,
            "after a batch move the arrowhead must stay on-screen, got x_head={} x1={} x2={} width={}",
            arrows[0].x_head,
            arrows[0].x1,
            arrows[0].x2,
            gantt.timeline_width
        );
    }

    #[test]
    fn backward_link_when_source_starts_after_target_keeps_head_on_screen() {
        let mut pred = node(1005, Some(1));
        pred.title = "Cart UI".into();
        pred.start_date = Some("2026-07-28T00:00:00Z".into());
        pred.target_date = None;
        let mut succ = node(1012, None);
        succ.title = "Unparented spike".into();
        succ.start_date = Some("2026-07-21T00:00:00Z".into());
        succ.target_date = Some("2026-08-04T00:00:00Z".into());
        succ.predecessors = vec![1005];
        let nodes = vec![node(1, None), pred, succ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert_eq!(arrows.len(), 1, "the #1005→#1012 link must still layout");
        let pred_bar = bars.iter().find(|bar| bar.id == 1005).expect("pred");
        let succ_bar = bars.iter().find(|bar| bar.id == 1012).expect("succ");
        assert!(
            pred_bar.x > succ_bar.x + 1.0,
            "fixture must place #1005 after #1012's start, pred.x={} succ.x={}",
            pred_bar.x,
            succ_bar.x
        );
        assert_exits_right_without_left_detour(
            &arrows[0],
            pred_bar,
            succ_bar,
            gantt.timeline_width,
        );
    }

    #[test]
    fn start_only_source_after_target_wraps_to_the_left_edge() {
        let mut parent = node(1003, None);
        parent.title = "Persist cart".into();
        parent.start_date = Some("2026-07-06T00:00:00Z".into());
        parent.target_date = Some("2026-08-20T00:00:00Z".into());
        let mut pred = node(1005, Some(1003));
        pred.title = "Cart UI".into();
        pred.start_date = Some("2026-07-28T00:00:00Z".into());
        pred.target_date = None;
        let mut succ = node(1012, None);
        succ.title = "Unparented spike".into();
        succ.start_date = Some("2026-07-21T00:00:00Z".into());
        succ.target_date = Some("2026-08-04T00:00:00Z".into());
        succ.predecessors = vec![1005];
        let nodes = vec![parent, pred, succ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert_eq!(arrows.len(), 1, "the #1005→#1012 link must still layout");
        let parent_bar = bars.iter().find(|bar| bar.id == 1003).expect("parent");
        let pred_bar = bars.iter().find(|bar| bar.id == 1005).expect("pred");
        let succ_bar = bars.iter().find(|bar| bar.id == 1012).expect("succ");
        assert!(
            pred_bar.x > succ_bar.x + 1.0,
            "fixture must place start-only #1005 after #1012's start"
        );
        assert_exits_right_without_left_detour(
            &arrows[0],
            pred_bar,
            succ_bar,
            gantt.timeline_width,
        );
        let parent_end = bar_end(parent_bar);
        assert!(
            (arrows[0].x1
                - (bar_end(pred_bar) + 12.0).clamp(10.0, (gantt.timeline_width - 10.0).max(24.0)))
            .abs()
                < 1.5,
            "the stub must leave #1005, not #1003, x1={} pred_end={} parent_end={parent_end}",
            arrows[0].x1,
            bar_end(pred_bar)
        );
        assert!(
            (arrows[0].x1 - (parent_end + 12.0)).abs() > 4.0
                || (parent_end - bar_end(pred_bar)).abs() < 2.0,
            "must not attach to Persist cart's end, x1={} parent_end={parent_end} pred_end={}",
            arrows[0].x1,
            bar_end(pred_bar)
        );
    }

    #[test]
    fn year_view_link_is_not_a_chart_width_loop() {
        let mut a = node(1, None);
        let mut c = node(3, None);
        a.title = "Persist cart".into();
        c.title = "Unparented spike".into();
        c.predecessors = vec![1];
        let nodes = vec![a, c];
        let gantt = GanttView {
            origin: Date::new(2026, 1, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert_eq!(arrows.len(), 1);
        let span = (arrows[0].x2 - arrows[0].x1).abs();
        assert!(
            span < 200.0,
            "Year links must stay a short elbow, got span {span} x1={} x2={}",
            arrows[0].x1,
            arrows[0].x2
        );
        let head_span = (arrows[0].x2 - arrows[0].x_head).abs();
        assert!(
            head_span < 40.0,
            "Year inbound stub must stay next to the head, x2={} x_head={}",
            arrows[0].x2,
            arrows[0].x_head
        );
    }

    #[test]
    fn year_view_inbound_vertical_ignores_wide_labels() {
        let mut parent = node(1003, None);
        parent.title = "Persist cart".into();
        parent.start_date = Some("2026-07-06T00:00:00Z".into());
        parent.target_date = Some("2026-07-20T00:00:00Z".into());
        let mut child = node(1004, Some(1003));
        child.title = "Cart UI with a very long label that fills the year chart".into();
        child.start_date = Some("2026-07-08T00:00:00Z".into());
        child.target_date = Some("2026-07-18T00:00:00Z".into());
        let mut succ = node(1012, None);
        succ.title = "Unparented spike".into();
        succ.start_date = Some("2026-07-21T00:00:00Z".into());
        succ.target_date = Some("2026-08-04T00:00:00Z".into());
        succ.predecessors = vec![1003];
        let nodes = vec![parent, child, succ];
        let gantt = GanttView {
            origin: Date::new(2026, 1, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert_eq!(arrows.len(), 1);
        let pred = bars.iter().find(|bar| bar.id == 1003).expect("pred");
        let target = bars.iter().find(|bar| bar.id == 1012).expect("succ");
        let x2 = arrows[0].x2;
        assert!(
            (x2 - arrows[0].x_head).abs() < 40.0,
            "Year inbound vertical must sit by the successor, not past a label, x2={x2} x_head={} pred={}..{} succ={}..{}",
            arrows[0].x_head,
            pred.x,
            pred.x + pred.width,
            target.x,
            target.x + target.width
        );
        assert!(
            x2 < 400.0,
            "Year inbound must not land at a stray mid-chart x, x2={x2}"
        );
    }

    #[test]
    fn year_view_unscheduled_is_a_gutter_pill() {
        let mut item = node(1, None);
        item.start_date = None;
        item.target_date = None;
        let gantt = GanttView {
            origin: Date::new(2026, 1, 1).unwrap(),
            scale: GanttScale::Year,
            selected_id: Some(1),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(std::slice::from_ref(&item));
        let bars = gantt.layout_bars(std::slice::from_ref(&item), &rows, &[], 720.0);
        let bar = bars.iter().find(|bar| bar.id == 1).expect("gutter pill");
        assert!(bar.marker);
        assert!(
            (bar.x - UNSCHEDULED_GUTTER_X).abs() < 0.1,
            "Year view must keep the unscheduled pill in the gutter, not on Today, x={}",
            bar.x
        );
    }

    #[test]
    fn no_date_fields_paints_a_gutter_hint() {
        let mut item = node(1, None);
        item.start_date = None;
        item.target_date = None;
        item.has_date_fields = false;
        item.work_item_type = "Product Backlog Item".into();
        for scale in [GanttScale::Month, GanttScale::Year, GanttScale::Quarter] {
            let gantt = GanttView {
                origin: Date::new(2026, 7, 1).unwrap(),
                scale,
                ..Default::default()
            };
            let rows = gantt.flatten(std::slice::from_ref(&item));
            let bars = gantt.layout_bars(std::slice::from_ref(&item), &rows, &[], 720.0);
            assert_eq!(bars.len(), 1);
            assert_eq!(bars[0].kind, GanttBarKind::NoFields);
            assert!(
                (bars[0].x - UNSCHEDULED_GUTTER_X).abs() < 0.1,
                "PBI rows without date fields must still show a gutter hint, x={}",
                bars[0].x
            );
            assert!(
                (bars[0].width - NO_FIELDS_HINT_W).abs() < 0.1,
                "{scale:?} No dates pill must stay wide enough to paint the words, width={}",
                bars[0].width
            );
        }
    }

    #[test]
    fn bar_hover_tooltip_lists_name_dates_and_assignee() {
        let mut item = node(1, None);
        item.title = "Cart".into();
        item.assigned_to = Some(ado_core::IdentityValue {
            display_name: "Ada Lovelace".into(),
            unique_name: "ada@contoso.com".into(),
            id: None,
        });
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(std::slice::from_ref(&item));
        let bars = gantt.layout_bars(std::slice::from_ref(&item), &rows, &[], 720.0);
        let tip = bar_hover_tooltip(&bars[0], std::slice::from_ref(&item));
        assert!(
            tip.contains("Cart") && tip.contains("Ada Lovelace") && tip.contains('→'),
            "bar tooltip must show name, dates, and assignee, got {tip:?}"
        );
        item.assigned_to = None;
        let tip = bar_hover_tooltip(&bars[0], std::slice::from_ref(&item));
        assert!(
            tip.contains("Unassigned"),
            "missing assignee must say Unassigned, got {tip:?}"
        );
    }

    #[test]
    fn clear_hover_drops_a_pinned_bar_tooltip() {
        let mut gantt = GanttView {
            pin_bar_tooltip: Some(2),
            pin_ignore_leave: true,
            hovered_id: Some(2),
            bar_hovered_id: Some(2),
            ..Default::default()
        };
        gantt.clear_hover();
        assert_eq!(gantt.pin_bar_tooltip, None);
        assert!(!gantt.pin_ignore_leave);
        assert_eq!(gantt.hovered_id, None);
        assert_eq!(gantt.bar_hovered_id, None);
    }

    #[test]
    fn live_resize_updates_the_bar_hover_tooltip_dates() {
        let mut item = node(1, None);
        item.title = "Cart".into();
        item.start_date = Some("2026-07-06T00:00:00Z".into());
        item.target_date = Some("2026-08-14T00:00:00Z".into());
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(std::slice::from_ref(&item));
        let bars = gantt.layout_bars(std::slice::from_ref(&item), &rows, &[], 720.0);
        gantt
            .begin_gesture(
                &bars[0],
                GestureKind::ResizeEnd,
                bars[0].x + bars[0].width,
                &[item.clone()],
                &[],
            )
            .expect("resize");
        let ppd = gantt.scale.pixels_per_day(gantt.zoom);
        gantt.update_gesture(bars[0].x + bars[0].width + ppd * 7.0);
        let live = gantt.layout_bars(std::slice::from_ref(&item), &rows, &[], 720.0);
        let tip = bar_hover_tooltip(&live[0], std::slice::from_ref(&item));
        assert!(
            tip.contains("Aug 21") || tip.contains("Aug 20"),
            "live resize must show the new Target Date in the tooltip, got {tip:?}"
        );
        assert!(
            !tip.contains("Aug 14"),
            "tooltip must not keep the pre-drag Target Date, got {tip:?}"
        );
    }

    #[test]
    fn unscheduled_bars_count_as_an_empty_timeline() {
        let mut dated = node(1, None);
        dated.start_date = None;
        dated.target_date = None;
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(std::slice::from_ref(&dated));
        let bars = gantt.layout_bars(std::slice::from_ref(&dated), &rows, &[], 720.0);
        assert!(timeline_is_empty_of_scheduled(&bars));
        let scheduled = node(2, None);
        let rows = gantt.flatten(std::slice::from_ref(&scheduled));
        let bars = gantt.layout_bars(std::slice::from_ref(&scheduled), &rows, &[], 720.0);
        assert!(!timeline_is_empty_of_scheduled(&bars));
    }

    #[test]
    fn quarter_and_year_titles_span_the_visible_range() {
        let gantt = GanttView {
            origin: Date::new(2026, 10, 1).unwrap(),
            scale: GanttScale::Quarter,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let title = gantt.viewport_range_title();
        assert!(
            title.contains('–'),
            "Quarter spanning two years must not collapse to Q1, got {title:?}"
        );
        assert!(
            !title.ends_with("Q1") || title.contains('–'),
            "a 2026–2027 Quarter span must not read only '2026 Q1', got {title:?}"
        );
        let mut year = gantt.clone();
        year.scale = GanttScale::Year;
        year.zoom = 1.0;
        year.origin = Date::new(2026, 7, 1).unwrap();
        year.scroll_x = 0.0;
        let year_title = year.viewport_range_title();
        let year_start = year.viewport_axis_start_date();
        let year_end = year.date_for_x(799.0);
        if year_end.year() != year_start.year() {
            assert!(
                year_title.contains('–')
                    && year_title.contains(&year_start.year().to_string())
                    && year_title.contains(&year_end.year().to_string()),
                "Year spanning {year_start}–{year_end} must name both years, got {year_title:?}"
            );
        } else {
            assert_eq!(year_title, year_start.year().to_string());
        }
    }

    #[test]
    fn quarter_title_matches_painted_columns() {
        // Mid-June: leftover Q2 is sticky at x=0 while Q3 sits ~44px in, under
        // the Quarter 48px gap, so the axis drops Q2. Title must follow Q3.
        let gantt = GanttView {
            origin: Date::new(2026, 6, 20).unwrap(),
            scale: GanttScale::Quarter,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let labels = gantt.header_tick_labels(800.0);
        let title = gantt.viewport_range_title();
        let first = labels
            .first()
            .map(|(_, label)| label.as_str())
            .unwrap_or("");
        assert!(
            first.starts_with('Q'),
            "Quarter axis must paint Q labels, got {labels:?}"
        );
        assert!(
            title.contains(first),
            "Quarter title {title:?} must name the first painted column {first:?}, labels={labels:?}"
        );
        assert!(
            !title.contains("Q2") || first == "Q2" || title.matches("Q2").count() == 1,
            "title must not claim leftover Q2 when the axis starts at {first}: {title} vs {labels:?}"
        );
        let q3 = Date::new(2026, 7, 1).unwrap();
        let q3_x = gantt.x_for_date(q3);
        if q3_x > 0.0 && q3_x < 48.0 {
            assert!(
                title.starts_with("2026 Q3"),
                "cramped leftover Q2 must not title the range, got {title:?} Q3_x={q3_x} labels={labels:?}"
            );
        }
    }

    #[test]
    fn year_major_labels_stay_inside_the_timeline() {
        let gantt = GanttView {
            origin: Date::new(2025, 1, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let labels = gantt.header_major_labels(800.0);
        assert!(!labels.is_empty(), "Year view must paint year majors");
        for (x, label) in &labels {
            let w = header_label_px(label);
            assert!(
                *x >= HEADER_EDGE_PAD - 0.1,
                "year label {label:?} at {x} must sit inside the left pad"
            );
            assert!(
                *x + w <= 800.0 - HEADER_EDGE_PAD + 0.1,
                "year label {label:?} at {x} width {w} must not clip a digit off the right edge"
            );
        }
    }

    #[test]
    fn set_scale_keeping_holds_the_focal_date() {
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 0.6,
            timeline_width: 800.0,
            ..Default::default()
        };
        let keep = Date::new(2026, 7, 21).unwrap();
        let before = gantt.x_for_date(keep);
        gantt.set_scale_keeping(GanttScale::Day, keep);
        let after = gantt.x_for_date(keep);
        assert!(
            (after - before).abs() < 1.0,
            "the focal date must stay at its screen x, got {after} (was {before})"
        );
        assert!(
            (gantt.zoom - 0.6).abs() < 0.01,
            "scale change must keep zoom"
        );
    }

    #[test]
    fn zoom_round_trip_restores_range_and_scale() {
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 0.6,
            timeline_width: 800.0,
            ..Default::default()
        };
        let origin = gantt.origin;
        let scroll = gantt.scroll_x;
        let title = gantt.viewport_range_title();
        gantt.zoom_step_keeping(1, 400.0);
        gantt.zoom_step_keeping(-1, 400.0);
        assert!(
            (gantt.zoom - 0.6).abs() < 0.0001,
            "zoom in then out must restore zoom, got {}",
            gantt.zoom
        );
        assert_eq!(gantt.scale, GanttScale::Month);
        assert_eq!(gantt.origin, origin);
        assert!(
            (gantt.scroll_x - scroll).abs() < 1.0,
            "zoom round trip must not pan, scroll {} vs {scroll}",
            gantt.scroll_x
        );
        assert_eq!(
            gantt.viewport_range_title(),
            title,
            "zoom round trip must not change the range title"
        );
    }

    #[test]
    fn month_year_month_restores_the_range() {
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 0.6,
            timeline_width: 800.0,
            ..Default::default()
        };
        let origin = gantt.origin;
        let scroll = gantt.scroll_x;
        let title = gantt.viewport_range_title();
        let keep_x = 400.0;
        let keep_date = gantt.date_for_x(keep_x);
        gantt.set_scale_at(GanttScale::Year, keep_x);
        gantt.set_scale_at(GanttScale::Month, keep_x);
        assert_eq!(gantt.scale, GanttScale::Month);
        assert_eq!(gantt.origin, origin);
        assert!(
            (gantt.scroll_x - scroll).abs() < 1.0,
            "Month→Year→Month must restore scroll, {} vs {scroll}",
            gantt.scroll_x
        );
        assert_eq!(
            gantt.viewport_range_title(),
            title,
            "Month→Year→Month must restore {title}, got {}",
            gantt.viewport_range_title()
        );
        gantt.set_scale_at(GanttScale::Quarter, keep_x);
        gantt.set_scale_at(GanttScale::Month, keep_x);
        assert_eq!(gantt.viewport_range_title(), title);
        let jul = Date::new(2026, 7, 1).unwrap();
        assert!(
            (gantt.x_for_date(jul) - 0.0).abs() < 2.0,
            "Jul must stay at the same screen x after a Quarter round trip, got {}",
            gantt.x_for_date(jul)
        );
        assert_eq!(gantt.date_for_x(keep_x), keep_date);
    }

    #[test]
    fn month_year_month_via_gutter_snap_keeps_the_left_edge() {
        let jul = Date::new(2026, 7, 1).unwrap();
        for scroll_x in [0.0, -9.0] {
            let mut gantt = GanttView {
                origin: jul,
                scale: GanttScale::Month,
                zoom: 1.0,
                scroll_x,
                timeline_width: 800.0,
                ..Default::default()
            };
            let jul_x = gantt.x_for_date(jul);
            let left = gantt.date_for_x(0.0);
            gantt.set_scale_at(GanttScale::Year, 0.0);
            gantt.snap_cramped_leading_column();
            gantt.unsnap_year_gutter();
            gantt.set_scale_at(GanttScale::Month, 0.0);
            gantt.snap_cramped_leading_column();
            assert!(
                (gantt.x_for_date(jul) - jul_x).abs() < 1.0,
                "Month→Year→Month must not shift Jul, scroll_x={scroll_x} before={jul_x} after={}",
                gantt.x_for_date(jul)
            );
            assert_eq!(
                gantt.date_for_x(0.0),
                left,
                "left-edge date must round-trip, scroll_x={scroll_x}"
            );
        }
    }

    #[test]
    fn quarter_today_label_does_not_sit_on_ticks() {
        let today = Date::new(2026, 11, 2).unwrap();
        let gantt = GanttView {
            origin: today.saturating_sub(40.days()),
            scale: GanttScale::Quarter,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let today_x = gantt.today_x(today);
        let ticks = gantt.header_tick_labels_except(800.0, Some(today_x));
        let majors = gantt.header_major_labels_except(800.0, Some(today_x));
        for (x, label) in &majors {
            assert!(
                (x - today_x).abs() >= 14.0 || *x <= 1.0,
                "Quarter major {label:?} at {x} must not sit on Today at {today_x}"
            );
        }
        assert!(
            ticks.iter().any(|(_, label)| label.starts_with('Q')),
            "Quarter ticks live on the row below Today and must stay, got {ticks:?}"
        );
        let pill_left = today_pill_left(today_x, 800.0);
        let pill_right = pill_left + TODAY_PILL_W;
        let stacked = ticks
            .iter()
            .filter(|(x, _)| (*x - (pill_right + TODAY_PILL_GAP)).abs() < 1.0)
            .count();
        assert!(
            stacked <= 1,
            "Quarter Today dodge must not stack Q labels on one x, got {ticks:?}"
        );
    }

    #[test]
    fn year_today_label_does_not_touch_year_majors() {
        let today = Date::new(2026, 11, 2).unwrap();
        let centered_days =
            ((800.0 * TODAY_ALIGN) / GanttScale::Year.pixels_per_day(0.4)).round() as i32;
        for (origin, timeline_width) in [
            (today.saturating_sub(20.days()), 800.0),
            // Centered Today, matching go_today / TODAY_ALIGN.
            (today.saturating_sub(centered_days.days()), 800.0),
        ] {
            let gantt = GanttView {
                origin,
                scale: GanttScale::Year,
                zoom: 0.4,
                timeline_width,
                ..Default::default()
            };
            let today_x = gantt.today_x(today);
            let pill_left = today_pill_left(today_x, timeline_width);
            let pill_right = pill_left + TODAY_PILL_W;
            let majors = gantt.header_major_labels_except(timeline_width, Some(today_x));
            let twenty_seven = majors.iter().find(|(_, label)| label == "2027");
            assert!(
                twenty_seven.is_some(),
                "Year view must still show 2027 instead of letting Today hide it, origin={origin} majors={majors:?}"
            );
            for (x, label) in &majors {
                let text_w = MAJOR_LABEL_ESTIMATE.max(header_label_px(label));
                let overlaps = *x < pill_right && *x + text_w > pill_left;
                assert!(
                    !overlaps,
                    "Year label {label:?} at {x} (w={text_w}) must not sit on Today {pill_left}..{pill_right}, origin={origin}"
                );
            }
        }
    }

    #[test]
    fn year_view_keeps_the_nov_tick_under_today() {
        let today = Date::new(2026, 11, 2).unwrap();
        for zoom in [1.0, 0.6] {
            let gantt = GanttView {
                origin: today.saturating_sub(40.days()),
                scale: GanttScale::Year,
                zoom,
                timeline_width: 800.0,
                ..Default::default()
            };
            let today_x = gantt.today_x(today);
            let ticks = gantt.header_tick_labels_except(800.0, Some(today_x));
            assert!(
                ticks.iter().any(|(_, label)| label == "Nov"),
                "Year ticks at zoom {zoom} must keep Nov next to Today, got {ticks:?}"
            );
            fn month_num(label: &str) -> Option<i32> {
                [
                    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
                    "Dec",
                ]
                .iter()
                .position(|name| *name == label)
                .map(|index| index as i32)
            }
            for pair in ticks.windows(2) {
                let Some(a) = month_num(&pair[0].1) else {
                    continue;
                };
                let Some(b) = month_num(&pair[1].1) else {
                    continue;
                };
                let step = (b - a + 12) % 12;
                assert_eq!(
                    step, 2,
                    "Year ticks must thin at a 2-month interval from the first painted month, got {ticks:?}"
                );
            }
            // Nov sits on the tick row under Today; do not require a horizontal
            // dodge that used to drop Jan (Jul, Sep, Nov, Mar).
            assert!(
                ticks.iter().any(|(_, label)| label == "Jan"),
                "Year ticks must keep Jan two months after Nov, got {ticks:?}"
            );
        }
    }

    #[test]
    fn year_ticks_from_leftover_jul_keep_a_two_month_step() {
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let ticks = gantt.header_tick_labels(800.0);
        let names: Vec<&str> = ticks.iter().map(|(_, label)| label.as_str()).collect();
        assert!(
            names.first() == Some(&"Jul"),
            "leftover Jul must be the first Year tick, got {names:?}"
        );
        let expected = ["Jul", "Sep", "Nov", "Jan", "Mar"];
        for (index, want) in expected.iter().enumerate() {
            assert_eq!(
                names.get(index),
                Some(want),
                "Year ticks from leftover Jul must stay on a 2-month step, got {names:?}"
            );
        }
    }

    #[test]
    fn quarter_q2_q3_are_not_jammed_at_the_left() {
        let today = Date::new(2026, 11, 2).unwrap();
        for (origin, zoom, width) in [
            (Date::new(2026, 1, 1).unwrap(), 1.0, 800.0),
            (today.saturating_sub(40.days()), 1.0, 800.0),
            (Date::new(2026, 7, 1).unwrap(), 0.4, 512.0),
        ] {
            let gantt = GanttView {
                origin,
                scale: GanttScale::Quarter,
                zoom,
                timeline_width: width,
                ..Default::default()
            };
            let today_x = gantt.today_x(today);
            let ticks = gantt.header_tick_labels_except(width, Some(today_x));
            let qs: Vec<(f32, &str)> = ticks
                .iter()
                .filter(|(_, label)| label.starts_with('Q'))
                .map(|(x, label)| (*x, label.as_str()))
                .collect();
            for pair in qs.windows(2) {
                assert!(
                    pair[1].0 - pair[0].0 >= 48.0 - 0.5,
                    "Quarter {0}@{1} and {2}@{3} must not jam (origin={origin} zoom={zoom}): {ticks:?}",
                    pair[0].1,
                    pair[0].0,
                    pair[1].1,
                    pair[1].0
                );
            }
            let left_cluster = qs.iter().filter(|(x, _)| *x < 40.0).count();
            assert!(
                left_cluster <= 1,
                "at most one quarter label may sit at the left edge, got {qs:?} origin={origin}"
            );
        }
    }

    #[test]
    fn switching_scale_keeps_the_zoom_percent_monotonic() {
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 0.6,
            timeline_width: 800.0,
            ..Default::default()
        };
        let month = gantt.zoom_percent_label();
        gantt.set_scale(GanttScale::Week);
        let week = gantt.zoom_percent_label();
        gantt.set_scale(GanttScale::Day);
        let day = gantt.zoom_percent_label();
        gantt.set_scale(GanttScale::Quarter);
        let quarter = gantt.zoom_percent_label();
        gantt.set_scale(GanttScale::Year);
        let year = gantt.zoom_percent_label();
        assert_eq!(month, "60%");
        assert_eq!(
            [
                month.as_str(),
                week.as_str(),
                day.as_str(),
                quarter.as_str(),
                year.as_str()
            ],
            ["60%", "60%", "60%", "60%", "60%"],
            "one zoom scale must stay monotonic across Day/Week/Month/Quarter/Year"
        );
    }

    #[test]
    fn month_ticks_are_not_cramped_together() {
        let gantt = GanttView {
            origin: Date::new(2026, 8, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 0.4,
            timeline_width: 400.0,
            ..Default::default()
        };
        let labels = gantt.header_tick_labels(400.0);
        for pair in labels.windows(2) {
            let (x0, l0) = &pair[0];
            let (x1, l1) = &pair[1];
            let need = (l0.chars().count() as f32 * 7.0 + 8.0).max(18.0);
            assert!(
                x1 - x0 >= need - 0.5,
                "ticks {l0:?}@{x0} and {l1:?}@{x1} are cramped (need {need})"
            );
        }
    }

    #[test]
    fn day_gridlines_reach_the_right_edge() {
        let gantt = GanttView {
            origin: Date::new(2026, 11, 2).unwrap(),
            scale: GanttScale::Day,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let ticks = gantt.timeline_ticks(800.0);
        let last = gantt
            .gridline_xs(800.0)
            .into_iter()
            .filter(|x| *x <= 800.0)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            800.0 - last < 2.0,
            "last Day gridline at {last} is too far from the right edge"
        );
        assert!(
            ticks.iter().any(|tick| tick.x >= 800.0),
            "ticks must extend to or past the right edge so the last column is not clipped, last in-view {last}"
        );
        let labels = gantt.header_tick_labels(800.0);
        if let Some((x, label)) = labels.first() {
            assert!(
                *x >= HEADER_LEFT_PAD - 0.1,
                "left-most Day tick {label:?} at {x} must sit inside the left pad"
            );
        }
    }

    #[test]
    fn child_outside_parent_dates_is_overflow() {
        let mut parent = node(1, None);
        parent.start_date = Some("2026-07-01T00:00:00Z".into());
        parent.target_date = Some("2026-07-31T00:00:00Z".into());
        let mut child = node(2, Some(1));
        child.start_date = Some("2026-07-20T00:00:00Z".into());
        child.target_date = Some("2026-08-15T00:00:00Z".into());
        assert!(parent_date_overflow(&[parent.clone(), child.clone()], 2));
        assert_eq!(
            parent_overflow_tip(&[parent.clone(), child.clone()], 2).as_deref(),
            Some("Outside parent's dates (Jul 1 → Jul 31)")
        );
        child.target_date = Some("2026-07-25T00:00:00Z".into());
        assert!(!parent_date_overflow(&[parent, child], 2));
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
            keep_today: true,
            ..Default::default()
        };
        let saved = gantt.snapshot();
        gantt.scroll_x = 0.0;
        gantt.scroll_y = 0.0;
        gantt.scale = GanttScale::Month;
        gantt.keep_today = false;
        gantt.restore(saved);
        assert_eq!(gantt.scroll_x, 120.0);
        assert_eq!(gantt.scroll_y, 80.0);
        assert_eq!(gantt.scale, GanttScale::Week);
        assert!((gantt.zoom - 1.5).abs() < f32::EPSILON);
        assert_eq!(gantt.origin, Date::new(2026, 7, 15).unwrap());
        assert!(
            gantt.keep_today,
            "snapshot must carry Today-follow so a team switch cannot leave the previous flag"
        );
        gantt.keep_today = false;
        let cleared = gantt.snapshot();
        gantt.keep_today = true;
        gantt.restore(cleared);
        assert!(
            !gantt.keep_today,
            "restore must drop Today-follow when the saved pose was not following Today"
        );
    }

    #[test]
    fn year_to_month_keeps_the_left_edge_date() {
        let origin = Date::new(2026, 7, 1).unwrap();
        let mut gantt = GanttView {
            origin,
            scale: GanttScale::Year,
            zoom: 0.4,
            timeline_width: 512.0,
            ..Default::default()
        };
        let left = gantt.date_for_x(0.0);
        gantt.set_scale_at(GanttScale::Month, 0.0);
        assert_eq!(
            gantt.date_for_x(0.0),
            left,
            "Year→Month must keep the scroll-anchor date, got {} (was {left})",
            gantt.date_for_x(0.0)
        );
        assert!(
            gantt.date_for_x(0.0) < Date::new(2027, 1, 1).unwrap(),
            "must not jump to 2027, got {}",
            gantt.date_for_x(0.0)
        );
    }

    #[test]
    fn week_snaps_a_cramped_first_column_and_keeps_1012_on_screen() {
        let mut item = node(1012, None);
        item.title = "Unparented spike".into();
        item.start_date = Some("2026-07-21T00:00:00Z".into());
        item.target_date = Some("2026-08-04T00:00:00Z".into());
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.set_scale_at(GanttScale::Week, 0.0);
        gantt.snap_cramped_leading_column();
        let leading = gantt.leading_period_width(800.0);
        let week_w = gantt.period_width(gantt.date_for_x(0.0));
        assert!(
            leading >= week_w * 0.4 - 1.0,
            "Week's first column must not be a sliver, leading={leading} week_w={week_w}"
        );
        let rows = gantt.flatten(std::slice::from_ref(&item));
        let bars = gantt.layout_bars(&[item], &rows, &[], 720.0);
        let bar = bars.iter().find(|bar| bar.id == 1012).expect("#1012");
        assert!(
            bar.x >= -0.5,
            "#1012 must not clip at the left edge, x={} width={}",
            bar.x,
            bar.width
        );
        assert!(
            bar.x + bar.width > 8.0,
            "#1012 must paint inside the Week viewport, x={} width={}",
            bar.x,
            bar.width
        );
    }

    #[test]
    fn week_snaps_when_the_visible_remainder_is_a_sliver() {
        let origin = Date::new(2026, 6, 29).unwrap();
        let mut gantt = GanttView {
            origin,
            scale: GanttScale::Week,
            zoom: 1.0,
            scroll_x: 120.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let start = gantt.scale.floor_date(gantt.date_for_x(0.0));
        let x = gantt.x_for_date(start);
        let full = gantt.period_width(start).max(1.0);
        let min = (full * 0.4).max(40.0);
        assert!(x < -0.5, "precondition: period start is off-screen, x={x}");
        assert!(
            -x >= min,
            "precondition: off-screen amount is not a small leftover, -x={} min={min}",
            -x
        );
        assert!(
            full + x < min,
            "precondition: visible remainder is a sliver, remainder={} min={min}",
            full + x
        );
        gantt.snap_cramped_leading_column();
        let after = gantt.x_for_date(start);
        assert!(
            after.abs() < 0.5,
            "must align the week start to the left edge, x={after}"
        );
    }

    #[test]
    fn year_ticks_are_months_and_not_cramped() {
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.snap_cramped_leading_column();
        let labels = gantt.header_tick_labels(800.0);
        assert!(
            labels.iter().any(|(_, label)| matches!(
                label.as_str(),
                "Jan"
                    | "Feb"
                    | "Mar"
                    | "Apr"
                    | "May"
                    | "Jun"
                    | "Jul"
                    | "Aug"
                    | "Sep"
                    | "Oct"
                    | "Nov"
                    | "Dec"
            )),
            "Year view must show month labels, got {labels:?}"
        );
        fn month_num(label: &str) -> Option<i32> {
            [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ]
            .iter()
            .position(|name| *name == label)
            .map(|index| index as i32)
        }
        for pair in labels.windows(2) {
            let (x0, l0) = &pair[0];
            let (x1, l1) = &pair[1];
            let need = header_label_px(l0);
            assert!(
                x1 - x0 >= need - 0.5,
                "Year ticks {l0:?}@{x0} and {l1:?}@{x1} are cramped (need {need})"
            );
            if let (Some(a), Some(b)) = (month_num(l0), month_num(l1)) {
                assert_eq!(
                    (b - a + 12) % 12,
                    2,
                    "Year ticks must stay on a 2-month step, got {labels:?}"
                );
            }
        }
    }

    #[test]
    fn year_snap_keeps_unscheduled_pills_off_dated_bars() {
        let mut scheduled = node(1, None);
        scheduled.start_date = Some("2026-07-06T00:00:00Z".into());
        scheduled.target_date = Some("2026-07-20T00:00:00Z".into());
        let mut open = node(2, None);
        open.start_date = None;
        open.target_date = None;
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.snap_cramped_leading_column();
        let nodes = vec![scheduled, open];
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let pill = bars.iter().find(|bar| bar.id == 2).expect("gutter pill");
        let dated = bars.iter().find(|bar| bar.id == 1).expect("dated bar");
        assert!(pill.marker);
        assert!(
            dated.x >= pill.x + pill.width + 1.0,
            "Year unscheduled diamond must not sit on a dated bar, pill={}..{} bar x={}",
            pill.x,
            pill.x + pill.width,
            dated.x
        );
        let scroll_after = gantt.scroll_x;
        gantt.snap_cramped_leading_column();
        assert!(
            (gantt.scroll_x - scroll_after).abs() < 0.5,
            "Year gutter snap must be idempotent, first={scroll_after} second={}",
            gantt.scroll_x
        );
    }

    #[test]
    fn year_gutter_snap_is_idempotent_when_a_month_fits_in_the_gutter() {
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 0.4,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.snap_cramped_leading_column();
        let first = gantt.scroll_x;
        gantt.snap_cramped_leading_column();
        assert!(
            (gantt.scroll_x - first).abs() < 0.5,
            "Year at zoom 0.4 must not walk on a second snap, first={first} second={}",
            gantt.scroll_x
        );
        gantt.retarget_year_gutter(|gantt| {
            gantt.set_scale_at(GanttScale::Year, 0.0);
        });
        assert!(
            (gantt.scroll_x - first).abs() < 0.5,
            "clicking Year while already on Year must not walk, first={first} after={}",
            gantt.scroll_x
        );
    }

    #[test]
    fn year_gutter_snap_retargets_after_zoom() {
        let jul = Date::new(2026, 7, 1).unwrap();
        let mut gantt = GanttView {
            origin: jul,
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.set_scale_at(GanttScale::Year, 0.0);
        gantt.snap_cramped_leading_column();
        let parked_before_zoom = gantt.year_gutter_snap;
        gantt.retarget_year_gutter(|gantt| {
            gantt.zoom_step_keeping(-1, 0.0);
        });
        let parked_after_zoom = gantt.year_gutter_snap;
        gantt.unsnap_year_gutter();
        let left = gantt.date_for_x(0.0);
        gantt.set_scale_at(GanttScale::Month, 0.0);
        assert_eq!(
            gantt.date_for_x(0.0),
            left,
            "unsnap after a Year zoom must use the post-zoom park, not {parked_before_zoom} vs {parked_after_zoom}"
        );
    }

    #[test]
    fn year_gutter_snap_round_trips_through_snapshot_and_mid_ease_restore() {
        let today = Date::new(2026, 11, 2).unwrap();
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 0.4,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.snap_cramped_leading_column();
        assert!(
            gantt.year_gutter_snap.abs() > 0.1,
            "precondition: Year must park, snap={}",
            gantt.year_gutter_snap
        );
        let from = gantt.snapshot();
        assert!(
            (from.year_gutter_snap - gantt.year_gutter_snap).abs() < 0.01,
            "snapshot must carry the park"
        );
        gantt.year_gutter_snap = 0.0;
        gantt.restore(from);
        assert!(
            (gantt.year_gutter_snap - from.year_gutter_snap).abs() < 0.01,
            "restore must put the park back"
        );
        gantt.retarget_year_gutter(|gantt| {
            gantt.go_today(today);
        });
        let to = gantt.snapshot();
        assert!(to.keep_today, "go_today snapshot must carry Today-follow");
        let mid = from.lerp(to, 0.5);
        assert!(
            mid.keep_today,
            "lerp snaps Today-follow to the destination pose"
        );
        gantt.restore(mid);
        assert!(
            (gantt.year_gutter_snap - mid.year_gutter_snap).abs() < 0.01,
            "mid-ease restore must keep the interpolated park, got {} want {}",
            gantt.year_gutter_snap,
            mid.year_gutter_snap
        );
        gantt.unsnap_year_gutter();
        let t = 1.0 - 0.5 * 0.5;
        let from_unparked = from.scroll_x - from.year_gutter_snap;
        let to_unparked = to.scroll_x - to.year_gutter_snap;
        let expect = from_unparked + (to_unparked - from_unparked) * t;
        assert!(
            (gantt.scroll_x - expect).abs() < 0.5,
            "unsnap of the interpolated pose must use the interpolated park, scroll {} want {expect}",
            gantt.scroll_x
        );
    }

    #[test]
    fn today_then_month_keeps_today_and_does_not_clip_the_left_edge() {
        let today = Date::new(2026, 11, 3).unwrap();
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 0.4,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.snap_cramped_leading_column();
        assert!(
            gantt.year_gutter_snap.abs() > 0.1,
            "precondition: Year must park, snap={}",
            gantt.year_gutter_snap
        );
        gantt.retarget_year_gutter(|gantt| {
            gantt.go_today(today);
        });
        assert!(
            gantt.keep_today,
            "go_today must mark the camera as following Today"
        );
        gantt.retarget_year_gutter(|gantt| {
            gantt.set_scale_from_toolbar(GanttScale::Month, today);
        });
        assert_eq!(gantt.scale, GanttScale::Month);
        assert!(
            gantt.year_gutter_snap.abs() < 0.01,
            "Month must not keep a Year park, snap={}",
            gantt.year_gutter_snap
        );
        let after = gantt.x_for_date(today);
        let aligned = gantt.timeline_width.max(1.0) * TODAY_ALIGN;
        assert!(
            (after - aligned).abs() < 2.0,
            "Today then Month must re-align Today, after={after} aligned={aligned}"
        );
        let left = gantt.date_for_x(0.0);
        assert!(
            left > today.saturating_sub(120.days()),
            "Month after Today must not re-anchor to a Year park, left={left} today={today}"
        );
        let nodes = vec![node(1, None)];
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        for bar in &bars {
            if bar.kind != GanttBarKind::Scheduled {
                continue;
            }
            let start = gantt.date_for_x(bar.x);
            assert!(
                start < left || bar.x >= -1.0,
                "a bar that starts on/after the left-edge date must not clip, x={} left={left}",
                bar.x
            );
        }
    }

    #[test]
    fn pointer_zoom_after_today_clears_keep_today() {
        let today = Date::new(2026, 11, 3).unwrap();
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 0.4,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.go_today(today);
        assert!(gantt.keep_today);
        gantt.zoom_at(1.25, 400.0, 0.0);
        assert!(
            !gantt.keep_today,
            "pointer zoom must drop Today-follow like pan"
        );
        gantt.set_scale_from_toolbar(GanttScale::Month, today);
        assert!(
            !gantt.keep_today,
            "toolbar scale after pointer zoom must not re-enter Today-follow"
        );
        let after = gantt.x_for_date(today);
        assert!(
            after >= 0.0 && after <= gantt.timeline_width,
            "pointer zoom then Month must keep Today on-screen, after={after}"
        );
    }

    #[test]
    fn month_to_week_keeps_today_and_november_on_screen() {
        let today = Date::new(2026, 11, 3).unwrap();
        let mut gantt = GanttView {
            origin: Date::new(2026, 9, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let today_x = gantt.x_for_date(today);
        assert!(
            today_x > gantt.timeline_width * 0.7,
            "precondition: Today must sit near the Month right edge, x={today_x}"
        );
        gantt.retarget_year_gutter(|gantt| {
            gantt.set_scale_from_toolbar(GanttScale::Week, today);
        });
        let week_x = gantt.x_for_date(today);
        assert!(
            week_x >= TODAY_MARGIN_PX - 1.0
                && week_x <= gantt.timeline_width - TODAY_MARGIN_PX + 1.0,
            "Month→Week must keep Today off the clip, x={week_x}"
        );
        let nov1 = Date::new(2026, 11, 1).unwrap();
        let nov_x = gantt.x_for_date(nov1);
        assert!(
            nov_x >= 0.0 && nov_x <= gantt.timeline_width,
            "Month→Week must keep Nov on-screen, x={nov_x}"
        );
        let labels = gantt.header_tick_labels(gantt.timeline_width);
        assert!(
            labels.iter().any(|(_, label)| label.contains("Nov")),
            "Week header must still name Nov, got {labels:?}"
        );
    }

    #[test]
    fn week_to_day_keeps_today_as_the_anchor() {
        let today = Date::new(2026, 11, 3).unwrap();
        let mut gantt = GanttView {
            origin: Date::new(2026, 9, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.retarget_year_gutter(|gantt| {
            gantt.set_scale_from_toolbar(GanttScale::Week, today);
        });
        gantt.retarget_year_gutter(|gantt| {
            gantt.set_scale_from_toolbar(GanttScale::Day, today);
        });
        assert_eq!(gantt.scale, GanttScale::Day);
        let day_x = gantt.x_for_date(today);
        assert!(
            day_x >= 0.0 && day_x <= gantt.timeline_width,
            "Week→Day must keep Today on-screen, x={day_x}"
        );
        let left = gantt.date_for_x(0.0);
        assert!(
            left >= Date::new(2026, 10, 15).unwrap(),
            "Week→Day must not jump to early October, left={left}"
        );
    }

    #[test]
    fn go_today_clears_a_stale_year_park_snap() {
        let today = Date::new(2026, 11, 3).unwrap();
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 0.4,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.snap_cramped_leading_column();
        let parked = gantt.year_gutter_snap;
        assert!(parked.abs() > 0.1, "precondition: Year must park");
        gantt.go_today(today);
        assert!(
            gantt.year_gutter_snap.abs() < 0.01,
            "go_today overwrites scroll and must drop the park, snap={}",
            gantt.year_gutter_snap
        );
        let before = gantt.scroll_x;
        gantt.unsnap_year_gutter();
        assert!(
            (gantt.scroll_x - before).abs() < 0.01,
            "unsnap after go_today must be a no-op, before={before} after={}",
            gantt.scroll_x
        );
    }

    #[test]
    fn flick_move_applies_release_x_without_prior_updates() {
        let nodes = vec![node(1, None)];
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let origin = bars[0].x + 12.0;
        gantt
            .begin_gesture(&bars[0], GestureKind::Move, origin, &nodes, &[])
            .expect("move");
        let ppd = gantt.scale.pixels_per_day(gantt.zoom);
        gantt.update_gesture(origin + ppd * 4.0);
        let finished = gantt.finish_gesture().expect("flick must commit");
        assert_eq!(
            finished.1,
            Date::new(2026, 7, 10).unwrap(),
            "a 4-day flick with no intermediate updates must still move the bar"
        );
    }
}
