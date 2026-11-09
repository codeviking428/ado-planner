use std::collections::{HashMap, HashSet};

use ado_core::{
    ClashEnds, DEFAULT_BAR_DAYS, DependencyLink, GanttBarKind, HierarchyIndex, IterationHint,
    IterationNode, WorkItemNode, clash_ends, compact_date, dates_column_text,
    dates_column_text_indexed, format_date_range, from_gantt_exclusive_end, gantt_bar_display,
    gantt_bar_display_indexed, iteration_hint_for_path, parse_iso_date, retest_clashes,
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
/// Visual 4px edge pills sit in this plate. Hits stay inside the bar on a
/// resizable body; the end halo covers the last `RESIZE_HALO_INSIDE` pixels
/// so 2–6px inside the painted end resizes. Link from the outer handle
/// (`bar_right + LINK_HANDLE_GAP`). Each edge zone is capped at a third of
/// the bar so a 33px sliver still has a Move middle; inner link is dropped
/// when it would eat that third.
pub const HANDLE_WIDTH: f32 = 8.0;
/// Pointer hit for inner start/end resize. Wider than the 4px pill so a
/// press ~12px inside the edge still resizes instead of moving.
pub const RESIZE_HIT_WIDTH: f32 = 14.0;
/// Extra hit around the painted edge: 8px inside so 2–6px inside the
/// painted end is resize even when painted bounds are 1–2px off layout
/// (a 6px halo left a 4px dead band between the grip and the halo).
/// Start and end only step 1px past the painted edge so the inbound
/// link-start handle (connector at bar.x−10) and the outer end handle
/// (bar_right+10) are not stolen.
pub const RESIZE_HALO_OUTSIDE: f32 = 1.0;
pub const RESIZE_HALO_INSIDE: f32 = 8.0;
pub const RESIZE_HALO_END_OUTSIDE: f32 = 1.0;
/// Selected tree/timeline row tint. Stronger than zebra stripes so the row reads.
pub const SELECTED_ROW_TINT: f32 = 0.65;
/// Left accent on the selected tree/timeline row.
pub const SELECTED_ACCENT_W: f32 = 3.0;
/// Year-scale bars shorter than this still paint a hittable body, not a sliver.
/// Wider than `LEAF_BAR_H` so a floored leaf reads as a bar, not a circle.
pub const YEAR_MIN_BAR_WIDTH: f32 = 40.0;
/// Year/Quarter summary bars read as slivers at 14px; keep a thicker body.
pub const YEAR_MIN_BAR_H: f32 = 20.0;
/// Floor so a zero-width day still paints on Month/Week/Quarter.
const MIN_BAR_WIDTH: f32 = 6.0;
/// Pixels per tree depth. A spacer child, not padding, so painted x matches depth.
pub const TREE_INDENT: f32 = 20.0;
/// Painted link connector. Smaller than the inner resize grip so the
/// circle is not mistaken for an edge handle.
pub const LINK_HANDLE: f32 = 8.0;
pub const LINK_HANDLE_HIT: f32 = 16.0;
/// Inclusive 8px radius around the gap connector. `LINK_HANDLE_HIT` (16) is
/// half-open, so a press at center+8 (reviewer ~624) missed the plate.
pub const OUTER_LINK_HIT: f32 = 18.0;
/// Wider outbound plate on short bars (~63px Persist cart) so a grab in
/// the gap past the painted end still starts a link.
pub const SHORT_BAR_OUTER_LINK_HIT: f32 = 28.0;
/// Bars narrower than this use [`SHORT_BAR_OUTER_LINK_HIT`].
pub const SHORT_BAR_OUTER_LINK: f32 = 80.0;
/// Gap between the painted bar end and the outside link handle.
pub const LINK_HANDLE_GAP: f32 = 6.0;
/// Right-handle hit reaches this far into the bar so a press on the last
/// ~6px starts a link, not a resize (wide bars) or a no-op Move (slivers).
/// Kept at 6 so the 14px end-resize grip stays a unique zone just left of it.
pub const LINK_HANDLE_INSET: f32 = 6.0;
/// Parallel dependency lanes stay at least this far apart (stroke is 2px).
pub const PARALLEL_LANE_MIN: f32 = 10.0;
/// Inbound/wrap verticals. 16px centre-to-centre so the 2px stroke still
/// reads as ≥8px after clip and antialiasing (live measure was 4–7px).
pub const PARALLEL_VERT_MIN: f32 = 16.0;
/// Parent-overflow `!` on the bar. The circle is larger than the glyph so a
/// 20px font still reads as a true 14px bang instead of a 10px mark.
pub const OVERFLOW_BADGE: f32 = 22.0;
/// Clash `!` centred on the successor's top-left corner.
pub const CLASH_BADGE: f32 = 14.0;
/// Gap after the link-handle hit box before the overflow badge.
pub const OVERFLOW_BADGE_GAP: f32 = 4.0;
/// Handles + `!` + slack in front of an outside title, matching the painted
/// Year/Quarter chip (overflow-inclusive so Persist cart → Go-live clears
/// the `!` as well as the letters).
pub const OUTSIDE_LABEL_GAP: f32 =
    LINK_HANDLE_GAP + LINK_HANDLE_HIT + OVERFLOW_BADGE_GAP + OVERFLOW_BADGE + 6.0;
/// Outside title chip height, matching `render_gantt_bar_label`.
pub const OUTSIDE_LABEL_H: f32 = 16.0;
/// Minimum gap from a link handle's hit plate to an outside title.
pub const HANDLE_TITLE_CLEARANCE: f32 = 8.0;
/// Gap from a diamond to its outside title. The outbound plate is
/// `OUTER_LINK_HIT` wide, centred on the connector at
/// `LINK_HANDLE_GAP + LINK_HANDLE / 2` past the diamond.
pub const MILESTONE_LABEL_GAP: f32 =
    LINK_HANDLE_GAP + LINK_HANDLE / 2.0 + OUTER_LINK_HIT / 2.0 + HANDLE_TITLE_CLEARANCE;
/// Gap from a flipped outside title's right edge to the bar's left edge.
/// The inbound plate is `LINK_HANDLE_HIT` wide, centred on the connector at
/// `bar.x - LINK_HANDLE_GAP - LINK_HANDLE / 2`.
pub const OUTSIDE_TITLE_LEFT_GAP: f32 =
    LINK_HANDLE_GAP + LINK_HANDLE / 2.0 + LINK_HANDLE_HIT / 2.0 + HANDLE_TITLE_CLEARANCE;
/// Gap from an outside title's painted right edge to a long finish-to-start
/// outgoing vertical. Reviewer floor is 6px; 8px covers antialiasing.
const SOURCE_LABEL_CLEARANCE: f32 = 16.0;
/// Forward finish-to-start inbound (x2 → head). Sliding this to miss an
/// intervening open-ended cue parks a 100px+ run on the successor row
/// (Persist cart → Go-live under Cart UI / spike). Keep the drop next to
/// the diamond instead.
const MAX_FORWARD_INBOUND: f32 = 20.0;
pub const NUDGE_IDLE_MS: u64 = 500;
pub const TIMELINE_HEADER_H: f32 = 44.0;
pub const BAR_LABEL_INSIDE_MIN: f32 = 72.0;
pub const TODAY_MARGIN_PX: f32 = 80.0;
/// Place the Today line in the middle of the timeline.
pub const TODAY_ALIGN: f32 = 0.5;
pub const SCALE_BUTTON_WIDTH: f32 = 80.0;
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
/// Wide enough for "Kevin Mccloskey" at text-xs without clipping to "Kevin Mc…".
pub const ASSIGNEE_COL_WIDTH: f32 = 128.0;
pub const EXPANDER_SLOT: f32 = 18.0;
pub const TODAY_PILL_W: f32 = 44.0;
pub const TODAY_PILL_GAP: f32 = 6.0;
/// Keep outside-title chips off the Today line (line is 3px wide).
/// 13px from the line's left edge is 10px past its right edge, so a
/// Year Cart UI chip cannot graze the stroke by 1–2px.
const TODAY_LABEL_CLEARANCE: f32 = 13.0;
/// Do not park a chip more than this far from its natural beside-bar slot.
const MAX_LABEL_BAR_SHIFT: f32 = 12.0;
/// Keep year/month majors off the Today pill; Year "2027" must not crowd it.
const HEADER_TODAY_LABEL_GAP: f32 = 72.0;

/// Hover text on a finish-to-start dependency.
pub fn dependency_link_tooltip(from_title: &str, to_title: &str) -> String {
    format!("{from_title} → {to_title}, finish-to-start")
}

/// `Would loop: #12 → #34 → #56 → #12`, cut to five ids and `…`.
pub fn dependency_loop_message(ids: &[i64]) -> String {
    const MAX_IDS: usize = 5;
    let mut parts: Vec<String> = ids
        .iter()
        .take(MAX_IDS)
        .map(|id| format!("#{id}"))
        .collect();
    if ids.len() > MAX_IDS {
        parts.push("…".into());
    }
    format!("Would loop: {}", parts.join(" → "))
}

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

/// Month/Quarter/Year banners carry years, and a span can cross one, so the
/// title names it too: `Aug 12 – Sep 24, 2026`, `Dec 4, 2026 – Feb 8, 2027`.
fn dated_span(start: Date, end: Date) -> String {
    if start >= end {
        format!("{}, {}", compact_date(start), start.year())
    } else if start.year() != end.year() {
        format!(
            "{}, {} – {}, {}",
            compact_date(start),
            start.year(),
            compact_date(end),
            end.year()
        )
    } else {
        format!("{}, {}", compact_date_span(start, end), end.year())
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
    pub live_y: f32,
    pub link_to: Option<i64>,
    pub had_start: bool,
    pub had_target: bool,
    /// Pressed the inbound (start) connector. Preview drops from that
    /// handle; drop treats this bar as the successor (still FS).
    pub link_from_start: bool,
    /// Dependencies into and out of this bar, found once at press so each
    /// frame re-judges only these Clashes.
    pub links: Vec<DependencyLink>,
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
    /// Parent-overflow `!` is painted; outside titles use the wider gap.
    pub overflow_badge: bool,
    /// Widened to `YEAR_MIN_BAR_WIDTH`, so its painted edges are not its
    /// dates. Resize grips would not track the pointer; keep it all-Move.
    pub floored: bool,
}

impl BarLayout {
    pub fn paint_h(&self) -> f32 {
        if self.milestone || self.marker {
            MILESTONE_SIZE
        } else if self.summary {
            // Year start-only slivers are floored to `YEAR_MIN_BAR_WIDTH`.
            // Thicken those and ≤24px start-only slivers on any scale, not a
            // floored Persist cart or a 25–40px Month parent, so connector
            // lanes still have a gap under the 14px summary body.
            let sliver = self.width <= HANDLE_WIDTH * 2.0 + LINK_HANDLE_INSET + 2.5;
            if self.open_ended && (self.floored || sliver) {
                YEAR_MIN_BAR_H.max(SUMMARY_BAR_H)
            } else {
                SUMMARY_BAR_H
            }
        } else if self.kind == GanttBarKind::Unscheduled || self.kind == GanttBarKind::NoFields {
            PLACEHOLDER_BAR_H
        } else {
            LEAF_BAR_H.max(YEAR_MIN_BAR_H)
        }
    }

    pub fn paint_top(&self) -> f32 {
        ((ROW_HEIGHT - self.paint_h()) / 2.0).max(2.0)
    }

    /// Vertical center of the painted bar, where the link connector sits.
    pub fn paint_mid_y(&self) -> f32 {
        self.y + self.paint_top() + self.paint_h() / 2.0
    }

    /// Rubber-band horizontal: a lane under the painted bar, still inside
    /// the row gap so it does not jump onto the next bar.
    pub fn link_preview_lane_y(&self) -> f32 {
        let bar_bottom = self.y + self.paint_top() + self.paint_h();
        (bar_bottom + 8.0)
            .min(self.y + ROW_HEIGHT - 4.0)
            .max(bar_bottom + 4.0)
    }

    pub fn start_link_origin(&self) -> f32 {
        self.x - LINK_HANDLE_GAP - LINK_HANDLE / 2.0
    }

    pub fn end_link_origin(&self) -> f32 {
        let layout_right = if self.milestone || self.marker {
            self.x + MILESTONE_SIZE
        } else {
            self.x + self.width.max(8.0)
        };
        layout_right + LINK_HANDLE_GAP + LINK_HANDLE / 2.0
    }

    /// Outbound handle plate. Short bars grow this into the gap so a grab
    /// near the painted end (R100 ~653/659 on a 63px bar) still starts a
    /// link without eating the 2–6px-inside resize halo.
    pub fn outer_link_hit(&self) -> f32 {
        // Diamonds have no resize halo to reach past, and a wider plate
        // would run into the title at `MILESTONE_LABEL_GAP`.
        if !self.milestone && !self.marker && self.width.max(8.0) < SHORT_BAR_OUTER_LINK {
            SHORT_BAR_OUTER_LINK_HIT
        } else {
            OUTER_LINK_HIT
        }
    }

    /// Layout-x of the left edge of the outbound handle plate.
    pub fn end_link_hit_left(&self) -> f32 {
        let layout_right = if self.milestone || self.marker {
            self.x + MILESTONE_SIZE
        } else {
            self.x + self.width.max(8.0)
        };
        if self.outer_link_hit() > OUTER_LINK_HIT {
            layout_right + RESIZE_HALO_END_OUTSIDE + 1.0
        } else {
            self.end_link_origin() - OUTER_LINK_HIT / 2.0
        }
    }

    /// True when the link press is on the inbound connector, not the end.
    pub fn link_from_start_handle(&self, origin_x: f32) -> bool {
        origin_x < self.x + self.width.max(8.0) * 0.5
    }

    pub fn can_resize(&self) -> bool {
        self.kind != GanttBarKind::Unscheduled
            && self.kind != GanttBarKind::NoFields
            && !self.milestone
            && !self.marker
            && !self.open_ended
            && !self.floored
            // 16–24px Year/Quarter slivers stay all-Move. Two 8px grips plus
            // a 6px link sliver leave no hittable body on that width.
            // Parents with their own dates resize once they clear 24px.
            && self.width > HANDLE_WIDTH * 2.0 + LINK_HANDLE_INSET + 2.0
    }

    /// Dated bar whose last pixels start a link. Wide bars keep an inner
    /// end-resize grip just left of this sliver. Diamonds and start-only
    /// bars keep Move on that edge.
    pub fn link_handle_insets_into_bar(&self) -> bool {
        self.kind == GanttBarKind::Scheduled && !self.milestone && !self.open_ended && !self.marker
    }

    /// Start grip, end-resize grip, and inner link sliver along this bar.
    /// Outside-in at the right edge: link, then resize, then move. Each
    /// edge is capped at a third of the width so the middle always moves.
    pub fn edge_zones(&self) -> (f32, f32, f32) {
        self.edge_zones_for(self.width.max(1.0))
    }

    pub fn edge_zones_for(&self, width: f32) -> (f32, f32, f32) {
        let width = width.max(1.0);
        let wants_link = self.link_handle_insets_into_bar();
        let wants_resize = self.can_resize();
        let max_edge = (width / 3.0).max(1.0);
        if !wants_resize && wants_link {
            let link = LINK_HANDLE_INSET.min(width / 2.0).max(1.0);
            return (0.0, 0.0, link);
        }
        if wants_resize && !wants_link {
            let grip = RESIZE_HIT_WIDTH.min(max_edge).max(1.0);
            return (grip, grip, 0.0);
        }
        if !wants_resize {
            return (0.0, 0.0, 0.0);
        }
        let start = RESIZE_HIT_WIDTH.min(max_edge).max(1.0);
        let end = RESIZE_HIT_WIDTH.min(max_edge).max(1.0);
        // Inner link only when a full Move third remains after start+end+link.
        let link = if wants_link && width - start - end - max_edge >= LINK_HANDLE_INSET - 0.5 {
            LINK_HANDLE_INSET
        } else {
            0.0
        };
        (start, end, link)
    }

    /// End-halo depth, capped so two halos cannot eat a short bar's Move
    /// third. 6px on a 33px bar still leaves an 11px body.
    pub fn resize_halo_inside(&self) -> f32 {
        if !self.can_resize() {
            return RESIZE_HALO_INSIDE;
        }
        RESIZE_HALO_INSIDE.min((self.width.max(1.0) / 3.0).max(1.0))
    }

    /// Last pixels of a dated bar that start a link (0 if this bar has none).
    pub fn link_zone_width(&self) -> f32 {
        self.edge_zones().2
    }

    /// Shift the end resize grip left by this much so the last
    /// `link_zone_width` pixels can be the link sliver.
    pub fn resize_end_link_inset(&self) -> f32 {
        if self.can_resize() && self.link_handle_insets_into_bar() {
            self.link_zone_width()
        } else {
            0.0
        }
    }

    /// Inner grip on each painted end, split at the midpoint on bars
    /// shorter than 16px. Resizable dated bars shrink this with the link
    /// sliver so a ~64px Month bar still has start, resize, and link.
    pub fn inner_grip_width(&self) -> f32 {
        let (start, _, _) = self.edge_zones();
        if start > 0.0 {
            start
        } else {
            RESIZE_HIT_WIDTH.min(self.width / 2.0).max(1.0)
        }
    }

    /// 8px rollup edges only when they leave a Move body in the center.
    /// Same floor as `can_resize`: 16–24px Year/Quarter parents stay
    /// all-Move. Wider dated parents resize instead of a not-allowed edge.
    pub fn shows_rollup_edges(&self) -> bool {
        self.summary
            && !self.can_resize()
            && !self.milestone
            && !self.marker
            && !self.floored
            && self.width > HANDLE_WIDTH * 2.0 + LINK_HANDLE_INSET + 2.0
    }

    /// Classify a timeline-x on this painted bar. `None` is the move body.
    pub fn resize_kind_at(&self, x: f32) -> Option<GestureKind> {
        match self.hit_zone_at_window_x(x, self.x, self.x + self.width) {
            BarHitZone::ResizeStart => Some(GestureKind::ResizeStart),
            BarHitZone::ResizeEnd => Some(GestureKind::ResizeEnd),
            BarHitZone::Link | BarHitZone::Move | BarHitZone::RollupEdge | BarHitZone::Outside => {
                None
            }
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
        let (start, end_resize, link) = self.edge_zones_for(self.width.max(1.0));
        let halo_in = self.resize_halo_inside();
        if self.can_resize() {
            if window_x >= painted_left - RESIZE_HALO_OUTSIDE && window_x <= painted_left + halo_in
            {
                return BarHitZone::ResizeStart;
            }
            if window_x >= painted_right - halo_in
                && window_x <= painted_right + RESIZE_HALO_END_OUTSIDE
            {
                return BarHitZone::ResizeEnd;
            }
        }
        if window_x < painted_left || window_x > painted_right {
            return BarHitZone::Outside;
        }
        let on_link = link > 0.0 && window_x >= painted_right - link;
        let on_start = start > 0.0 && window_x <= painted_left + start;
        let on_end = end_resize > 0.0
            && window_x >= painted_right - end_resize - link
            && window_x < painted_right - link;
        // Last pixels of a resizable dated bar are the link sliver, even on
        // a summary. Non-resizable slivers keep Move here; the overlay
        // handle still starts a link on those last pixels.
        if self.can_resize() && self.link_handle_insets_into_bar() && on_link {
            return BarHitZone::Link;
        }
        if self.summary && !self.can_resize() {
            // Two 8px edges meet on a 16–24px bar; skip them so a short
            // parent (diamond, Year sliver) still has a Move body.
            if self.shows_rollup_edges() && (on_start || on_end) {
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
    Link,
    Move,
    RollupEdge,
    Outside,
}

impl BarHitZone {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ResizeStart => "resize-start",
            Self::ResizeEnd => "resize-end",
            Self::Link => "link",
            Self::Move => "move",
            Self::RollupEdge => "rollup-edge",
            Self::Outside => "outside",
        }
    }

    pub fn gesture_kind(self) -> Option<GestureKind> {
        match self {
            Self::ResizeStart => Some(GestureKind::ResizeStart),
            Self::ResizeEnd => Some(GestureKind::ResizeEnd),
            Self::Link => Some(GestureKind::Link),
            Self::Move => Some(GestureKind::Move),
            Self::RollupEdge | Self::Outside => None,
        }
    }
}

/// Inner start grip: the first `RESIZE_HIT_WIDTH` of the painted bar.
pub fn resize_start_handle(bar: &BarLayout) -> (f32, f32) {
    let width = bar.inner_grip_width();
    (bar.x, width)
}

/// Inner end grip: `RESIZE_HIT_WIDTH` ending `resize_end_link_inset` before
/// the painted right, so the last 6px of a dated bar can start a link.
pub fn resize_end_handle(bar: &BarLayout) -> (f32, f32) {
    let width = bar.inner_grip_width();
    let inset = bar.resize_end_link_inset();
    (bar.x + bar.width - width - inset, width)
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
    /// Overlap back-jog stubs (exit + inbound). Shorter than [`Self::WRAP_STUB`]
    /// so Week/Month U-turns sit closer to the source bar.
    const BACK_JOG_STUB: f32 = 8.0;

    /// Wrap-around routing drops at the source stub, then runs on the
    /// successor row into a short inbound. Used for a true backward FS
    /// (successor start left of the source) so the path does not detour
    /// across the chart. Overlap — source end past the target start, with
    /// a left inbound still on/after the source — is a back-jog that
    /// enters from the left, not a wrap through the target bar. A forward
    /// run whose inbound was shoved past `x_head` is not a wrap either.
    pub fn wraps_around_successor(&self) -> bool {
        self.x2 > self.x_head + Self::WRAP_STUB && self.x1 + 1.0 >= self.x_head
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
    /// Optional dates chip for a bar (tests / a later hover). Date drops
    /// do not pin this — the chip would sit beside the Moved toast.
    pub pin_bar_tooltip: Option<i64>,
    /// Highlight the finish-to-start chain whose last Target Date is latest.
    pub show_critical_path: bool,
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
            show_critical_path: false,
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

    /// Month/Quarter/Year name visible days, not whole periods: a ‹/› step
    /// is a quarter of the view, finer than a month, quarter or year column,
    /// so a period title would sit still for several clicks and then jump.
    /// The span starts at the first column the axis labels (a cramped
    /// leftover it dropped is not claimed) and ends on the last pixel.
    pub fn viewport_range_title(&self) -> String {
        match self.scale {
            GanttScale::Day | GanttScale::Week => {
                let (start, end) = self.painted_tick_range();
                compact_date_span(start, end)
            }
            GanttScale::Month | GanttScale::Quarter | GanttScale::Year => {
                let width = self.timeline_width.max(1.0);
                let axis_start = if matches!(self.scale, GanttScale::Quarter) {
                    self.painted_tick_range().0
                } else {
                    self.viewport_axis_start_date()
                };
                let start = axis_start.max(self.date_for_x(0.0));
                let end = self.date_for_x((width - 1.0).max(0.0));
                dated_span(start, end)
            }
        }
    }

    /// First and last painted tick dates, matching `header_tick_labels`
    /// (leftover dropped when cramped). `viewport_axis_start_date` /
    /// `date_for_x(width-1)` can name a leftover Jun 29 or a Jul 29 pixel
    /// the Day grid never painted.
    fn painted_tick_range(&self) -> (Date, Date) {
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
        // Ask the painted header rather than re-deriving room from raw ticks:
        // Year thinning can keep a leftover whose raw neighbour was dropped.
        let painted = leftover.is_some_and(|left| {
            first_in_view.is_none()
                || self
                    .header_tick_labels(width)
                    .first()
                    .is_some_and(|(x, label)| *x <= HEADER_LEFT_PAD && *label == left.label)
        });
        if let Some(tick) = leftover.filter(|_| painted) {
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

    /// Row / bar / link hover, not a live post-drop tooltip.
    pub fn clear_row_hover(&mut self) {
        self.hovered_id = None;
        self.bar_hovered_id = None;
        self.hovered_link = None;
    }

    pub fn clear_hover(&mut self) {
        self.clear_row_hover();
        self.pin_bar_tooltip = None;
    }

    /// Pointer landed on a tooltip-bearing control. Clears a pinned dates
    /// chip unless this hit is the same bar (so the chip can stay after a
    /// drop). Also drops hover on that bar so resize/link handles do not
    /// stay painted. Returns whether the pin changed. `id` 0 is a non-bar
    /// hit (link segment, arrowhead) and always unpins.
    pub fn note_hit_target(&mut self, id: i64) -> bool {
        match self.pin_bar_tooltip {
            Some(pin) if pin != id => {
                self.pin_bar_tooltip = None;
                if self.bar_hovered_id == Some(pin) {
                    self.bar_hovered_id = None;
                }
                if self.hovered_id == Some(pin) {
                    self.hovered_id = None;
                }
                true
            }
            _ => false,
        }
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

    /// ‹ / › pan, as days. A quarter of the visible span at the current
    /// scale and zoom so Day/Week/Month/Quarter/Year move the same fraction.
    /// Callers must not wrap the pan in [`retarget_year_gutter`]: the
    /// leading-column and gutter snaps would add up to a period to the step.
    pub fn pan_step_days(&self) -> i32 {
        let ppd = self.scale.pixels_per_day(self.zoom).max(0.4);
        let viewport_days = (self.timeline_width.max(MIN_TIMELINE_WIDTH) / ppd).max(1.0);
        (viewport_days * 0.25).round().max(1.0) as i32
    }

    pub fn step_period(&mut self, direction: i32) {
        if direction == 0 {
            return;
        }
        self.keep_today = false;
        let days = direction * self.pan_step_days();
        self.origin = self.origin.saturating_add(days.days());
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

    /// Pan so `date` sits at screen `x`. Toolbar Day/Week/Month derives
    /// scroll from one focus date (selection start, or viewport centre)
    /// after leftover-column snap so a round trip keeps the identical range.
    pub fn pin_date_at(&mut self, date: Date, x: f32) {
        let now = self.x_for_date(date);
        self.scroll_x += now - x;
    }

    /// Pan so `date` sits on the viewport centre.
    pub fn center_date(&mut self, date: Date) {
        let width = self.timeline_width.max(1.0);
        self.pin_date_at(date, width * 0.5);
    }

    fn day_week_month(scale: GanttScale) -> bool {
        matches!(
            scale,
            GanttScale::Day | GanttScale::Week | GanttScale::Month
        )
    }

    /// Screen x to hold `date` at. On-screen dates keep their current x;
    /// off-screen dates use the viewport centre.
    pub fn focus_keep_x(&self, date: Date) -> f32 {
        let width = self.timeline_width.max(1.0);
        let raw = self.x_for_date(date);
        if (0.0..=width).contains(&raw) {
            raw
        } else {
            width * 0.5
        }
    }

    /// Toolbar Day/Week/Month/… click, including leftover-column snap.
    /// Day/Week/Month keep one focus date (selection start, or viewport
    /// centre) at its screen x so Month→Week→Day→Month restores the
    /// identical range. Year/Quarter still hold the left-edge date.
    /// Re-clicking the current scale is a no-op: leftover snap and a
    /// selection pin must not pan. After Today, skip leftover/gutter snap
    /// so a selection cannot pull the camera off Today. An off-screen
    /// selection is ignored so it cannot pull the camera off the visible range.
    pub fn apply_toolbar_scale(&mut self, scale: GanttScale, today: Date, keep: Option<Date>) {
        if self.scale == scale {
            return;
        }
        let width = self.timeline_width.max(1.0);
        let keep = keep.filter(|date| (0.0..=width).contains(&self.x_for_date(*date)));
        if self.keep_today {
            self.set_scale_from_toolbar_keeping(scale, today, keep);
            return;
        }
        let from = self.scale;
        let focus = keep.unwrap_or_else(|| self.viewport_center_date());
        let keep_x = self.focus_keep_x(focus);
        let pin = Self::day_week_month(from) && Self::day_week_month(scale);
        self.retarget_year_gutter(|gantt| {
            gantt.set_scale_from_toolbar_keeping(scale, today, keep);
        });
        if pin && Self::day_week_month(self.scale) {
            self.pin_date_at(focus, keep_x);
        }
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
    /// is not amplified into a clipped Month column. Day/Week/Month keep the
    /// viewport centre (or `keep`) so the visible range does not jump.
    /// Year/Quarter still hold the left-edge date (Month→Year→Month).
    /// Prefer [`apply_toolbar_scale`] from the toolbar so leftover snap
    /// cannot drift the focus date.
    pub fn set_scale_from_toolbar(&mut self, scale: GanttScale, today: Date) {
        self.set_scale_from_toolbar_keeping(scale, today, None);
    }

    /// Like [`set_scale_from_toolbar`], but pin `keep` (the selection) when
    /// stepping Day/Week/Month. Does not undo leftover-column snap; the
    /// toolbar path uses [`apply_toolbar_scale`].
    pub fn set_scale_from_toolbar_keeping(
        &mut self,
        scale: GanttScale,
        today: Date,
        keep: Option<Date>,
    ) {
        if self.keep_today {
            self.set_scale_at(scale, 0.0);
            self.go_today(today);
            return;
        }
        let from = self.scale;
        let day_week_month =
            |s: GanttScale| matches!(s, GanttScale::Day | GanttScale::Week | GanttScale::Month);
        if day_week_month(from) && day_week_month(scale) {
            if let Some(keep) = keep {
                self.set_scale_keeping(scale, keep);
            } else {
                self.set_scale(scale);
            }
            return;
        }
        let width = self.timeline_width.max(1.0);
        let today_visible = (0.0..=width).contains(&self.x_for_date(today));
        self.set_scale_at(scale, 0.0);
        if !today_visible || from == scale {
            return;
        }
        let today_is_origin = self.origin == today;
        if today_is_origin && day_week_month(scale) {
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
        let index = HierarchyIndex::new(nodes);
        for node in nodes {
            let hint = iteration_hint_for_path(&node.iteration_path, iterations);
            let display = gantt_bar_display_indexed(node, nodes, &index, hint.as_ref());
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
        let index = HierarchyIndex::new(nodes);
        self.layout_bars_indexed(nodes, &index, rows, iterations, viewport_h)
    }

    /// [`GanttView::layout_bars`] with `index` built over `nodes`, so a frame
    /// costs the visible window rather than the whole Hierarchy.
    pub fn layout_bars_indexed(
        &self,
        nodes: &[WorkItemNode],
        index: &HierarchyIndex,
        rows: &[VisibleRow],
        iterations: &[IterationNode],
        viewport_h: f32,
    ) -> Vec<BarLayout> {
        let window = self.visible_window(rows, viewport_h);
        window
            .iter()
            .enumerate()
            .filter_map(|(offset, row)| {
                let node = index.get(nodes, row.id)?;
                let hint = iteration_hint_for_path(&node.iteration_path, iterations);
                let display = gantt_bar_display_indexed(node, nodes, index, hint.as_ref());
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
                        // Parents too: a summary bar is 14px, shorter than
                        // the "No dates" text, and gets rollup edges.
                        summary: false,
                        milestone: false,
                        label: dates_column_text_indexed(node, nodes, index, hint.as_ref()),
                        title: node.title.clone(),
                        color: parse_type_color(ado_core::color_for_type(&node.work_item_type)),
                        progress: None,
                        predecessors: node.predecessors.clone(),
                        marker: false,
                        open_ended: false,
                        overflow_badge: false,
                        floored: false,
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
                }
                let start_only = parse_iso_date(node.target_date.as_deref()).is_none()
                    && parse_iso_date(node.start_date.as_deref()).is_some()
                    && !display.milestone
                    && live.as_ref().is_none_or(|gesture| !gesture.had_target);
                let open_ended = start_only;
                let label = if let Some(gesture) = live {
                    format_date_range(Some(gesture.live_start), Some(gesture.live_target))
                } else {
                    dates_column_text_indexed(node, nodes, index, hint.as_ref())
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
                let floored = matches!(self.scale, GanttScale::Year)
                    && !marker
                    && !display.milestone
                    && width < YEAR_MIN_BAR_WIDTH;
                if floored {
                    width = YEAR_MIN_BAR_WIDTH;
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
                    progress: child_completion_indexed(nodes, index, node.id)
                        .or_else(|| Some(state_progress(&node.state))),
                    predecessors: node.predecessors.clone(),
                    marker,
                    open_ended,
                    overflow_badge: parent_overflow_span_indexed(nodes, index, node.id).is_some(),
                    floored,
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
                // Lane under the bar, not row center: the preview drops from
                // the handle to this y, so a press does not draw a reverse
                // vertical back through the source bar.
                live_y: bar.link_preview_lane_y(),
                link_to: None,
                had_start,
                had_target,
                link_from_start: bar.link_from_start_handle(origin_x),
                links: Vec::new(),
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
            live_y: bar.y + ROW_HEIGHT / 2.0,
            link_to: None,
            had_start,
            had_target,
            link_from_start: false,
            links: ado_core::links_touching(nodes, bar.id),
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

    /// `(source, drop_target, from_start_handle)`. A start-handle drag still
    /// stores FS: the source becomes the successor.
    pub fn take_link_gesture(&mut self) -> Option<(i64, Option<i64>, bool)> {
        let gesture = self.gesture.as_ref()?;
        if gesture.kind != GestureKind::Link {
            return None;
        }
        let from_id = gesture.id;
        let to_id = gesture.link_to.filter(|id| *id != from_id);
        let from_start = gesture.link_from_start;
        self.gesture = None;
        Some((from_id, to_id, from_start))
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
            let days = gesture
                .live_target
                .since((Unit::Day, gesture.live_start))
                .map(|span| span.get_days())
                .unwrap_or(0)
                .max(0)
                + 1;
            format!(
                "{} → {} · {days}d",
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

    /// Dates the Clash rule reads for `node`: its own fields, or what a drop
    /// of the live drag or resize would write.
    pub fn clash_ends_for(&self, node: &WorkItemNode) -> Option<ClashEnds> {
        let moved = self.gesture.as_ref().filter(|gesture| {
            gesture.id == node.id
                && gesture.kind != GestureKind::Link
                && (gesture.live_start != gesture.start || gesture.live_target != gesture.target)
        });
        match moved {
            Some(gesture) if node.has_date_fields => {
                let (start, target) = Self::gesture_patch_dates(gesture);
                ClashEnds::from_dates(
                    start.or_else(|| parse_iso_date(node.start_date.as_deref())),
                    target.or_else(|| parse_iso_date(node.target_date.as_deref())),
                )
            }
            _ => clash_ends(node),
        }
    }

    /// `loaded` with the live gesture's links re-judged on its tentative
    /// dates, plus a badge for each clashing successor among `bars`.
    pub fn clash_marks(
        &self,
        loaded: &HashSet<DependencyLink>,
        nodes: &[WorkItemNode],
        index: &HierarchyIndex,
        bars: &[BarLayout],
    ) -> ClashMarks {
        let ends = |id: i64| {
            index
                .get(nodes, id)
                .and_then(|node| self.clash_ends_for(node))
        };
        let mut links = loaded.clone();
        if let Some(gesture) = &self.gesture {
            retest_clashes(&mut links, &gesture.links, ends);
        }
        let mut badges = HashMap::new();
        for bar in bars {
            let Some(successor) = ends(bar.id) else {
                continue;
            };
            let clashes: Vec<ClashLine> = bar
                .predecessors
                .iter()
                .filter(|from| links.contains(&(**from, bar.id)))
                .filter_map(|&from| {
                    Some(ClashLine {
                        predecessor_id: from,
                        starts: successor.start,
                        predecessor_finishes: ends(from)?.target?,
                    })
                })
                .collect();
            if !clashes.is_empty() {
                badges.insert(
                    bar.id,
                    ScheduleBadge {
                        clashes,
                        failures: Vec::new(),
                    },
                );
            }
        }
        ClashMarks { links, badges }
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
            .filter(|tick| tick.major && tick.x < width)
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
    /// other month from a phase that keeps January so leftover Feb does
    /// not yield Feb, Apr, Jun… with no Jan.
    /// Day ticks sit on that same lower row (pill at y=2, ticks at y=22)
    /// and keep their column x. Week labels are `"Nov 2"` (~43px) at 24px
    /// per day, so they skip a tick that would sit on the Today line.
    pub fn header_tick_labels_except(&self, width: f32, hide_x: Option<f32>) -> Vec<(f32, String)> {
        let mut ticks: Vec<(f32, String)> = self
            .timeline_ticks(width)
            .into_iter()
            .filter_map(|tick| {
                // fit_labels_to_width slides the last label onto the clip,
                // so a tick past it would name an off-screen period.
                if tick.label.is_empty() || tick.x >= width {
                    None
                } else {
                    Some((tick.x, tick.label))
                }
            })
            .collect();
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
        let labels = Self::fit_labels_to_width(labels, width);
        if matches!(self.scale, GanttScale::Week)
            && let Some(today) = hide_x
        {
            return dodge_week_ticks_off_today(labels, today);
        }
        labels
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
            let room = out
                .first()
                .is_none_or(|(x, _)| leftover_label_has_room(*x, &label, sticky_min));
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

    /// Earliest/latest dates Auto Fit would use, or `None` when every item
    /// is unscheduled / has no fields. The toolbar disables itself then.
    pub fn scheduled_span(
        nodes: &[WorkItemNode],
        iterations: &[IterationNode],
    ) -> Option<(Date, Date)> {
        let mut earliest: Option<Date> = None;
        let mut latest: Option<Date> = None;
        for node in nodes {
            // Without its own dates a bar is a Rollup, iteration hint, or
            // "No dates" — all skipped below — so skip the O(N) Rollup walk.
            if !node.has_date_fields
                || (parse_iso_date(node.start_date.as_deref()).is_none()
                    && parse_iso_date(node.target_date.as_deref()).is_none())
            {
                continue;
            }
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
        let earliest = earliest?;
        Some((earliest, latest.unwrap_or(earliest)))
    }

    /// Pan so scheduled items start on screen. Scale and zoom stay put so
    /// Auto Fit does not change Month column width.
    pub fn auto_fit(&mut self, nodes: &[WorkItemNode], iterations: &[IterationNode], _width: f32) {
        self.keep_today = false;
        let today = jiff::Zoned::now().date();
        let Some((earliest, _)) = Self::scheduled_span(nodes, iterations) else {
            self.go_today(today);
            return;
        };
        // Fit the item span only. Pulling Today into a July cluster in November
        // squeezed bars into slivers against the left edge. Keep scale/zoom so
        // a Month camera does not suddenly change column width.
        self.origin = earliest.saturating_sub(2.days());
        self.scroll_x = 0.0;
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
            match bar.hit_zone_at_window_x(x, bar.x, bar.x + bar.width) {
                BarHitZone::ResizeStart => GestureKind::ResizeStart,
                BarHitZone::ResizeEnd => GestureKind::ResizeEnd,
                BarHitZone::Link => GestureKind::Link,
                _ => GestureKind::Move,
            }
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
        let timeline_w = self.timeline_width.max(1.0);
        let today = jiff::Zoned::now().date();
        let today_px = self.x_for_date(today);
        let today_x = (today_px >= 0.0 && today_px <= timeline_w).then_some(today_px);
        let mut out = Vec::new();
        for bar in bars {
            for pred_id in &bar.predecessors {
                let Some(pred) = by_id.get(pred_id) else {
                    continue;
                };
                // Gutter pills have no dates. A connector from/to "No dates"
                // looks scheduled and stacks on the left-edge hint.
                if bar_is_gutter(pred) || bar_is_gutter(bar) {
                    continue;
                }
                // Both ends past the clip would clamp to one edge and paint a
                // stray vertical stub. Edge chevrons already mark those rows.
                if bar_offscreen_edge(pred, timeline_w).is_some()
                    && bar_offscreen_edge(bar, timeline_w).is_some()
                {
                    continue;
                }
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
                // Finish-to-start always enters the successor start. Gutter
                // pills keep inbound_right so the head is not a cramped stub
                // on "No dates". Overlap (source end past target start) back-
                // jogs on the source lane and enters from the left. Wrap only
                // when a left inbound would sit left of the source *and* the
                // wrap drop would miss the target bar.
                let inbound_right = succ_gutter;
                let x_head = if inbound_right { succ_right } else { bar.x };
                let include_labels = !matches!(self.scale, GanttScale::Year | GanttScale::Quarter);
                let clip_right = self.timeline_width.max(1.0);
                let natural_in = x_head - STUB;
                // Overlap (source starts first, end past the target start) back-
                // jogs from the left at every scale. Wrap only when a left
                // inbound would sit left of the source — same-start / true
                // backward FS — so the drop is not pulled through the target.
                let overlap_back_jog =
                    !succ_gutter && !inbound_right && overlap_enters_from_the_left(pred, bar);
                let stub = if overlap_back_jog {
                    DependencyArrow::BACK_JOG_STUB
                } else {
                    STUB
                };
                let x_out = pred_end + stub;
                let wrap_around = !succ_gutter
                    && pred_end > bar.x + STUB
                    && natural_in < pred_left
                    && !overlap_back_jog;
                let mut x_in = if wrap_around {
                    x_out
                } else if inbound_right {
                    x_head + STUB
                } else {
                    x_head - stub
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
                if wrap_around {
                    // Day/Week/Month include outside titles so a Persist cart
                    // → spike wrap does not drop through the Go-live pill.
                    // Year/Quarter stay body-only (`include_labels` is false).
                    x_in = clear_wrap_drop_x(
                        x_out,
                        &intervening,
                        include_labels,
                        x_head,
                        pred_left + STUB,
                        clip_right,
                        today_x,
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
                        today_x,
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
                        today_x,
                    );
                }
                if !succ_gutter && x_in < pred_left && natural_in >= pred_left && !overlap_back_jog
                {
                    // Dodging shoved a forward inbound left of the source.
                    // Drop at the source stub and keep the head on the start
                    // instead of overshooting to a right-edge inbound — unless
                    // that stub sits inside an intervening bar or its cue.
                    // A true back-jog may sit slightly left of the source
                    // (`natural_in < pred_left`); do not pull that to x_out.
                    // Overlap must not fall back to persist_end+12: that drop
                    // runs through the successor body/cue (Week Cart UI).
                    let fallback = x_out.max(pred_left + STUB);
                    let hits_intervening = intervening.iter().any(|mid| {
                        let (left, right) =
                            bar_occupied_x(mid, include_labels, Some(clip_right), today_x);
                        x_hits_span(fallback, left, right)
                    });
                    let (succ_left, succ_right) =
                        bar_occupied_x(bar, include_labels, Some(clip_right), today_x);
                    let hits_successor = x_hits_span(fallback, succ_left, succ_right);
                    if !hits_intervening && !hits_successor {
                        x_in = fallback;
                    }
                }
                let same_row = (pred.y - bar.y).abs() < 1.0;
                let lane_y = if same_row {
                    y1
                } else {
                    arrow_lane_y(pred, bar)
                };
                if !succ_gutter && !inbound_right && !wrap_around {
                    let clear = gutter_clear_x(bars);
                    if x_in < clear && x_head > clear + 4.0 {
                        x_in = clear;
                    }
                    // A short inbound through an intervening bar (Cart UI
                    // body or its dashed tail) is worse than a longer run
                    // on the successor row. Drop before that bar starts.
                    if x_head - x_in > MAX_FORWARD_INBOUND {
                        let stub = x_head - STUB;
                        if !stub_recrosses_intervening(stub, &intervening, clip_right) {
                            x_in = stub;
                        }
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
                // Shared-source jogs (Persist cart → spike vs Persist cart →
                // Go-live) must clear PARALLEL_VERT_MIN, not the 10px inbound fan.
                let x_shift = rank as f32 * PARALLEL_VERT_MIN;
                let lane_shift = rank as f32 * LANE_SEP;
                out[index].x1 += x_shift;
                if out[index].wraps_around_successor() {
                    out[index].x2 += x_shift;
                }
                // Fan down when the source paints an outside title so the
                // second lane stays below the chip. Fan up lands in the
                // letters; clamp then collapses both runs onto min_lane.
                let outside_title = by_id
                    .get(&out[index].from_id)
                    .is_some_and(|pred| !bar_title_is_inside(pred) && !bar_is_gutter(pred));
                let dir = if out[index].y2 >= out[index].y1 && !outside_title {
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
        separate_shared_source_outgoing(&mut out, clip_left, clip_right);
        separate_shared_source_inbounds(&mut out, clip_left, clip_right);
        // MIN_STUB / clip can park a dodged inbound back onto an intervening
        // open-ended cue (Persist cart → Go-live on Cart UI at Month 512).
        let include_labels = !matches!(self.scale, GanttScale::Year | GanttScale::Quarter);
        dodge_outgoing_off_source_labels(
            &mut out, bars, clip_left, clip_right, timeline_w, today_x,
        );
        dodge_forward_inbounds_off_intervening(
            &mut out,
            bars,
            include_labels,
            clip_left,
            clip_right,
            today_x,
            timeline_w,
        );
        separate_parallel_verticals(&mut out, clip_left, clip_right);
        separate_shared_source_outgoing(&mut out, clip_left, clip_right);
        separate_shared_source_inbounds(&mut out, clip_left, clip_right);
        // Parallel-vertical pushes can slide a stub back under the source
        // title; re-apply per-arrow dodge after the last fan.
        dodge_outgoing_off_source_labels(
            &mut out, bars, clip_left, clip_right, timeline_w, today_x,
        );
        // Short inbound stubs (x_head-12) can land inside Cart UI's marker
        // when Go-live sits just past it. Slide to the handle gap — not
        // through the body, and not a long successor-row run.
        nudge_forward_inbounds_off_solid_bodies(&mut out, bars, clip_left, clip_right);
        // Nudging off Cart UI can land two shared-source inbounds on the
        // same x (live Persist cart → Go-live vs Persist cart → spike at
        // x≈224). Fan left after the last body dodge so a right push
        // into Cart UI cannot be collapsed back onto the spike drop.
        separate_parallel_verticals(&mut out, clip_left, clip_right);
        separate_shared_source_outgoing(&mut out, clip_left, clip_right);
        separate_shared_source_inbounds(&mut out, clip_left, clip_right);
        // Spreading can shove an inbound back onto Cart UI (Year sliver or
        // Month body at cart.x). `separate_parallel_verticals` after a nudge
        // pushes the right inbound onto Cart UI's left edge (x_head == cart.x
        // for the spike) and leaves a 2px hit-box gap vs Go-live. Last inbound
        // move is nudge, then left-only fan if the pair restacked — never a
        // right parallel push.
        nudge_forward_inbounds_off_solid_bodies(&mut out, bars, clip_left, clip_right);
        // Fan after the last body nudge. A 16px left slide lands on a Year
        // sliver (`YEAR_MIN_BAR_WIDTH`); a follow-up nudge pulls that drop
        // off the body. That nudge is sibling-aware so two past-sliver
        // inbounds do not restack onto one `past_body` x. No parallel
        // push after this — a right shift parks the spike on Cart UI.
        separate_shared_source_inbounds(&mut out, clip_left, clip_right);
        nudge_forward_inbounds_off_solid_bodies(&mut out, bars, clip_left, clip_right);
        dodge_outgoing_off_source_labels(
            &mut out, bars, clip_left, clip_right, timeline_w, today_x,
        );
        // Shared-source fan parks Persist cart → Go-live 16px right of the
        // spike stub, then inbound nudge pulls x2 left of Cart UI — a
        // short stub right and a 40px double-back. Collapse that run so
        // the outgoing sits on the inbound column (left of the spike,
        // still ≥8px from it and off the source chip).
        straighten_forward_backtracks(&mut out, bars, timeline_w, today_x);
        separate_shared_source_outgoing_off_labels(
            &mut out, bars, clip_left, clip_right, timeline_w, today_x,
        );
        // Later fans can park a wrap drop back on an intervening chip
        // (Day Persist cart → spike through the Go-live pill). Re-clear
        // after the last x-move, same occupancy Month uses for inbounds.
        dodge_wraps_off_intervening(
            &mut out,
            bars,
            include_labels,
            clip_left,
            clip_right,
            today_x,
        );
        dodge_verticals_off_intervening_pills(
            &mut out,
            bars,
            include_labels,
            clip_left,
            clip_right,
            today_x,
            timeline_w,
        );
        // Shared-source Y fan can lift the new Persist cart → Go-live run
        // into the outside chip; put it back ≥6px below, still in-row.
        clamp_lanes_off_source_titles(&mut out, bars);
        // Parallel-vertical fans can shove an overlap inbound 16px right
        // onto the successor (Week Persist cart → Cart UI at x≈884). Pull
        // those drops left of the target at every scale, then slide any
        // sibling inbound left so the pair does not restack.
        force_overlap_back_jog(
            &mut out,
            bars,
            include_labels,
            clip_left,
            clip_right,
            today_x,
        );
        separate_shared_source_inbounds(&mut out, clip_left, clip_right);
        force_overlap_back_jog(
            &mut out,
            bars,
            include_labels,
            clip_left,
            clip_right,
            today_x,
        );
        // Last: one obstacle-aware pick for each vertical. Earlier fans
        // and nudges flip-flop by scale; this pass rejects any x that
        // intersects a painted bar/milestone (or open-ended cue) in the
        // rows it spans. Try exit-right, then back-jog-left, then a
        // left-edge gutter.
        route_obstacle_aware_verticals(&mut out, bars, clip_left, clip_right, timeline_w);
        out
    }
}

/// Gap below the source bar/chip and above the next row, so a 2px stroke
/// can sit in the middle instead of 3px above Cart UI (the next row).
fn source_row_gap(pred: &BarLayout) -> (f32, f32) {
    let bar_bottom = pred.y + pred.paint_top() + pred.paint_h();
    let content_bottom = if bar_title_is_inside(pred) || bar_is_gutter(pred) {
        bar_bottom
    } else {
        let label_top = pred.y + ((ROW_HEIGHT - OUTSIDE_LABEL_H) / 2.0).max(2.0);
        bar_bottom.max(label_top + OUTSIDE_LABEL_H)
    };
    (content_bottom, pred.y + ROW_HEIGHT)
}

fn arrow_lane_y(pred: &BarLayout, succ: &BarLayout) -> f32 {
    const STROKE: f32 = 2.0;
    const CLEAR: f32 = 6.0;
    if succ.y > pred.y {
        let (gap_top, gap_bottom) = source_row_gap(pred);
        let min_lane = (gap_top + CLEAR).min(gap_bottom - STROKE - 1.0);
        let max_lane = (gap_bottom - STROKE - 1.0).max(min_lane);
        (min_lane + max_lane) / 2.0
    } else {
        let above_bar = pred.y + pred.paint_top() - STROKE;
        above_bar.max(pred.y + STROKE)
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

/// Slide a forward inbound (and its outgoing stub) left of intervening bars
/// after clip. Clip+MIN_STUB can otherwise drop Persist cart → Go-live through
/// the Cart UI cue at x≈clip.
fn dodge_forward_inbounds_off_intervening(
    arrows: &mut [DependencyArrow],
    bars: &[BarLayout],
    include_labels: bool,
    clip_left: f32,
    clip_right: f32,
    today_x: Option<f32>,
    timeline_w: f32,
) {
    let by_id: HashMap<i64, &BarLayout> = bars.iter().map(|bar| (bar.id, bar)).collect();
    for arrow in arrows.iter_mut() {
        if arrow.wraps_around_successor() {
            continue;
        }
        let Some(pred) = by_id.get(&arrow.from_id) else {
            continue;
        };
        let Some(succ) = by_id.get(&arrow.to_id) else {
            continue;
        };
        let lo = pred.y.min(succ.y);
        let hi = pred.y.max(succ.y);
        let intervening: Vec<&BarLayout> = bars
            .iter()
            .filter(|mid| {
                mid.id != pred.id && mid.id != succ.id && mid.y > lo + 1.0 && mid.y < hi - 1.0
            })
            .collect();
        if intervening.is_empty() {
            continue;
        }
        let blocked: Vec<(f32, f32)> = intervening
            .iter()
            .map(|bar| {
                bar_occupied_x_on(
                    bar,
                    include_labels,
                    Some(clip_right),
                    today_x,
                    Some(timeline_w),
                )
            })
            .collect();
        let slide_left = |x: f32| -> f32 {
            let mut x = x.clamp(clip_left, clip_right);
            for _ in 0..=blocked.len() {
                let hit_left = blocked
                    .iter()
                    .filter(|(left, right)| x_hits_span(x, *left, *right))
                    .map(|(left, _)| *left)
                    .fold(f32::INFINITY, f32::min);
                if hit_left == f32::INFINITY {
                    return x;
                }
                let cand = (hit_left - 8.0).max(clip_left);
                if cand >= x - 0.5 {
                    return x;
                }
                x = cand;
            }
            x
        };
        let next = slide_left(arrow.x2);
        if next > arrow.x_head + DependencyArrow::WRAP_STUB {
            continue;
        }
        // Keep a long successor-row inbound when the current stub still
        // sits inside an intervening bar (Cart UI body + dashed tail).
        // Skipping those slides is what parked Persist cart → Go-live
        // through Cart UI at x≈705.
        let already_clear = blocked
            .iter()
            .all(|(left, right)| !x_hits_span(arrow.x2, *left, *right));
        if already_clear && arrow.x_head - next > MAX_FORWARD_INBOUND {
            continue;
        }
        arrow.x2 = next;
    }
}

/// Slide a forward inbound out of an intervening painted bar (body or
/// open-ended dashed tail). Prefer a gap just past the bar when that still
/// enters the successor from the left; otherwise drop before the bar
/// starts. Outside titles stay ignored so Year/Quarter chips do not shove.
fn nudge_forward_inbounds_off_solid_bodies(
    arrows: &mut [DependencyArrow],
    bars: &[BarLayout],
    clip_left: f32,
    clip_right: f32,
) {
    let by_id: HashMap<i64, &BarLayout> = bars.iter().map(|bar| (bar.id, bar)).collect();
    for i in 0..arrows.len() {
        if arrows[i].wraps_around_successor() || arrows[i].inbound_right {
            continue;
        }
        let Some(pred) = by_id.get(&arrows[i].from_id) else {
            continue;
        };
        let Some(succ) = by_id.get(&arrows[i].to_id) else {
            continue;
        };
        let lo = pred.y.min(succ.y);
        let hi = pred.y.max(succ.y);
        let intervening: Vec<&BarLayout> = bars
            .iter()
            .filter(|mid| {
                mid.id != pred.id && mid.id != succ.id && mid.y > lo + 1.0 && mid.y < hi - 1.0
            })
            .collect();
        let mut x = arrows[i].x2;
        let from_id = arrows[i].from_id;
        let to_id = arrows[i].to_id;
        let x_head = arrows[i].x_head;
        // Re-scan so dropping before bar A cannot land inside bar B.
        for _ in 0..=intervening.len() {
            let mut moved = false;
            for mid in &intervening {
                // Body + open-ended cue (the painted Cart UI bar), not Year
                // outside titles — those would shove the drop tens of days.
                let (left, right) = bar_occupied_x(mid, false, Some(clip_right), None);
                if !x_hits_span(x, left, right) {
                    continue;
                }
                let (body_left, body_right) = bar_occupied_x(mid, false, None, None);
                let before = (left - 8.0).max(clip_left);
                // Occupancy already pads the body by 4px; +2 sits in the Year
                // sliver gap without walking onto Persist cart's outside chip.
                let past_body = (body_right + 2.0).min(clip_right);
                let next = if !x_hits_span(before, left, right) {
                    // Room left of the painted bar (Month Cart UI at x≈611).
                    before
                } else if past_body < x_head - 2.0 && !x_hits_span(past_body, body_left, body_right)
                {
                    // Year sliver: the unscheduled gutter clip sits on the body,
                    // but Go-live is just past it — use that gap. Do not park
                    // on another shared-source inbound already at past_body.
                    // `past_body - 16` is the sliver itself (`YEAR_MIN_BAR_WIDTH`);
                    // prefer a gap still right of the body when the successor
                    // head allows it.
                    let sibling_on_gap = arrows.iter().any(|other| {
                        other.from_id == from_id
                            && other.to_id != to_id
                            && !other.wraps_around_successor()
                            && !other.inbound_right
                            && (other.x2 - past_body).abs() < 1.0
                    });
                    if sibling_on_gap {
                        let left_alt = (past_body - PARALLEL_VERT_MIN).max(clip_left);
                        if !x_hits_span(left_alt, body_left, body_right) {
                            left_alt
                        } else {
                            let right_alt = past_body + PARALLEL_VERT_MIN;
                            if right_alt < x_head - 2.0
                                && !x_hits_span(right_alt, body_left, body_right)
                            {
                                right_alt
                            } else {
                                before
                            }
                        }
                    } else {
                        past_body
                    }
                } else {
                    before
                };
                if (next - x).abs() > 0.5 {
                    x = next;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        arrows[i].x2 = x;
    }
}

/// Shared-source outgoing stubs (Persist cart → spike vs Persist cart →
/// Go-live) can restack to 1–3px after clip even when lanes are apart.
/// Spread `x1` by `PARALLEL_VERT_MIN` without touching inbound `x2`.
/// Only push right: a leftward shove walks the new Persist cart → Go-live
/// stub back onto the source title and onto Persist cart → spike.
fn separate_shared_source_outgoing(
    arrows: &mut [DependencyArrow],
    clip_left: f32,
    clip_right: f32,
) {
    if arrows.len() < 2 {
        return;
    }
    let mut by_from: HashMap<i64, Vec<usize>> = HashMap::new();
    for (index, arrow) in arrows.iter().enumerate() {
        by_from.entry(arrow.from_id).or_default().push(index);
    }
    for indexes in by_from.values_mut() {
        if indexes.len() < 2 {
            continue;
        }
        indexes.sort_by_key(|&index| arrows[index].to_id);
        for _ in 0..indexes.len() * 4 {
            let mut moved = false;
            for pair in indexes.windows(2) {
                let a = pair[0];
                let b = pair[1];
                let dx = (arrows[a].x1 - arrows[b].x1).abs();
                if dx >= PARALLEL_VERT_MIN {
                    continue;
                }
                let (keep, slide) = if arrows[a].x1 <= arrows[b].x1 {
                    (a, b)
                } else {
                    (b, a)
                };
                let push = (arrows[keep].x1 + PARALLEL_VERT_MIN - arrows[slide].x1).max(1.0);
                if shift_vertical(
                    &mut arrows[slide],
                    VertKind::Outgoing,
                    push,
                    clip_left,
                    clip_right,
                ) {
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
    }
}

fn inbound_vert_x(arrow: &DependencyArrow) -> f32 {
    if arrow.wraps_around_successor() {
        arrow.stub_start()
    } else {
        arrow.x2
    }
}

/// Shared-source inbound verticals (live Persist cart → spike vs Persist
/// cart → Go-live) both park at Cart UI.x−16 after the body nudge, so they
/// read as one line. Keep the shorter stub (next to its head) and slide the
/// longer drop left by `PARALLEL_VERT_MIN` — left of Cart UI, not through it.
/// Wrap drops are frozen at `stub_start`; slide the non-wrap inbound away.
fn separate_shared_source_inbounds(
    arrows: &mut [DependencyArrow],
    clip_left: f32,
    clip_right: f32,
) {
    if arrows.len() < 2 {
        return;
    }
    let mut by_from: HashMap<i64, Vec<usize>> = HashMap::new();
    for (index, arrow) in arrows.iter().enumerate() {
        if arrow.inbound_right {
            continue;
        }
        by_from.entry(arrow.from_id).or_default().push(index);
    }
    for indexes in by_from.values_mut() {
        if indexes.len() < 2 {
            continue;
        }
        indexes.sort_by_key(|&index| arrows[index].to_id);
        for _ in 0..indexes.len() * 4 {
            let mut moved = false;
            for pair in indexes.windows(2) {
                let a = pair[0];
                let b = pair[1];
                let dx = (inbound_vert_x(&arrows[a]) - inbound_vert_x(&arrows[b])).abs();
                if dx >= PARALLEL_VERT_MIN {
                    continue;
                }
                let stub_a = (arrows[a].x_head - inbound_vert_x(&arrows[a])).abs();
                let stub_b = (arrows[b].x_head - inbound_vert_x(&arrows[b])).abs();
                let wrap_a = arrows[a].wraps_around_successor();
                let wrap_b = arrows[b].wraps_around_successor();
                let (keep, slide) = if wrap_a != wrap_b {
                    if wrap_a { (a, b) } else { (b, a) }
                } else if stub_a <= stub_b {
                    (a, b)
                } else {
                    (b, a)
                };
                if arrows[slide].wraps_around_successor() {
                    continue;
                }
                let target = inbound_vert_x(&arrows[keep]) - PARALLEL_VERT_MIN;
                let delta = target - arrows[slide].x2;
                if shift_vertical(
                    &mut arrows[slide],
                    VertKind::Inbound,
                    delta,
                    clip_left,
                    clip_right,
                ) {
                    moved = true;
                    continue;
                }
                if arrows[keep].wraps_around_successor() {
                    continue;
                }
                let keep_target = arrows[slide].x2 - PARALLEL_VERT_MIN;
                let keep_next = keep_target.clamp(clip_left, clip_right);
                if (keep_next - arrows[slide].x2).abs() < PARALLEL_VERT_MIN {
                    continue;
                }
                let keep_delta = keep_target - arrows[keep].x2;
                if shift_vertical(
                    &mut arrows[keep],
                    VertKind::Inbound,
                    keep_delta,
                    clip_left,
                    clip_right,
                ) {
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
    }
}

/// Pull a forward finish-to-start outgoing back onto its inbound column
/// when routing walked `x1` right of the bar then `x2` left of the handle.
/// A short leftward jog is acceptable when the only alternative is
/// crossing an intervening bar (open-ended Cart UI between Persist cart
/// and Go-live). Stubs that already cleared the source chip keep a long
/// right route.
fn straighten_forward_backtracks(
    arrows: &mut [DependencyArrow],
    bars: &[BarLayout],
    timeline_w: f32,
    today_x: Option<f32>,
) {
    let by_id: HashMap<i64, &BarLayout> = bars.iter().map(|bar| (bar.id, bar)).collect();
    for arrow in arrows.iter_mut() {
        if arrow.wraps_around_successor() || arrow.inbound_right {
            continue;
        }
        if arrow.x1 <= arrow.x2 + 1.0 {
            continue;
        }
        let Some(pred) = by_id.get(&arrow.from_id) else {
            continue;
        };
        let pred_end = if pred.milestone || pred.marker {
            pred.x + MILESTONE_SIZE
        } else {
            pred.x + pred.width
        };
        let natural = pred_end + DependencyArrow::WRAP_STUB;
        // Day-view Cart UI dodges pull x2 left of a natural stub (pred_end+12)
        // while the successor is far right. Collapsing that onto x2 exits
        // left of the source bar. Only a fan-parked stub (x1 past natural)
        // is a true double-back — inbound pulled left of Cart UI after the
        // shared-source jog sat 16px right of the spike.
        if arrow.x1 <= natural + 1.0 {
            continue;
        }
        if arrow.x_head - arrow.x2 <= MAX_FORWARD_INBOUND + DependencyArrow::WRAP_STUB {
            continue;
        }
        if let Some((_, right)) = bar_outside_title_span_clipped(pred, timeline_w.max(1.0), today_x)
            && arrow.x1 >= right + SOURCE_LABEL_CLEARANCE - 0.5
        {
            continue;
        }
        arrow.x1 = arrow.x2;
    }
}

#[cfg(test)]
fn bar_outside_title_span(bar: &BarLayout) -> Option<(f32, f32)> {
    bar_outside_title_span_clipped(bar, 10_000.0, None)
}

/// Occupancy of the painted outside chip. Same frame as `bar_outside_label_frame`
/// so a Persist cart → Go-live stub cannot sit 1–2px from the letters while
/// unit tests pass against a different span. Skip gutter pills (they never
/// paint an outside title) and honour Today so a flipped chip is occupied
/// on the side the letters actually sit.
fn bar_outside_title_span_clipped(
    bar: &BarLayout,
    timeline_w: f32,
    today_x: Option<f32>,
) -> Option<(f32, f32)> {
    if bar_is_gutter(bar) {
        return None;
    }
    let (left, width) =
        bar_outside_label_frame(bar, timeline_w, true, today_x, bar.overflow_badge)?;
    Some((left, left + width))
}

/// The outgoing vertical at `x1` must miss the source's own outside title
/// (Persist cart → Go-live through "Persist cart"). Intervening-only dodge
/// never sees the source row. Move each long finish-to-start stub on its
/// own so an already-past Persist cart → spike does not zero the shift and
/// leave Persist cart → Go-live under the chip.
fn dodge_outgoing_off_source_labels(
    arrows: &mut [DependencyArrow],
    bars: &[BarLayout],
    clip_left: f32,
    clip_right: f32,
    timeline_w: f32,
    today_x: Option<f32>,
) {
    let by_id: HashMap<i64, &BarLayout> = bars.iter().map(|bar| (bar.id, bar)).collect();
    for arrow in arrows.iter_mut() {
        let Some(pred) = by_id.get(&arrow.from_id) else {
            continue;
        };
        let Some((left, right)) =
            bar_outside_title_span_clipped(pred, timeline_w.max(1.0), today_x)
        else {
            continue;
        };
        let painted_w = if pred.milestone || pred.marker {
            MILESTONE_SIZE
        } else {
            pred.width.max(8.0)
        };
        // Adjacent FS (successor still on the source bar) stays on the 12px
        // stub. A successor under the *label* but past the bar (Go-live
        // under a zoomed-out Persist cart chip) still needs a dodge.
        if arrow.x_head <= pred.x + painted_w + 12.0 {
            continue;
        }
        // 6px pad so a stub 0–3px from the letters still counts; the handle
        // gap itself (stub at bar_right+12) is not the chip.
        let hit_left = left - 6.0;
        let on_chip = x_hits_span(arrow.x1, hit_left, right);
        let under_chip = x_spans_overlap(
            (arrow.x1.min(arrow.x2), arrow.x1.max(arrow.x2)),
            (left, right),
        );
        if !on_chip && !under_chip {
            continue;
        }
        let desired = right + SOURCE_LABEL_CLEARANCE;
        let wrap_limit = arrow.x_head - 1.0;
        // Capping at x_head-1 parks the stub on the letters when the
        // successor sits under the chip (Year Persist cart → Go-live).
        // Leave (or pull back into) the handle gap instead.
        if desired > wrap_limit {
            // Successor sits under the chip, so we cannot pass its right
            // edge. Park ≥8px left of the letters — not on the chip edge
            // and not a 6px stub that clips the glyph.
            let clear_left = left - SOURCE_LABEL_CLEARANCE;
            if arrow.x1 > clear_left && arrow.x1 < right + SOURCE_LABEL_CLEARANCE {
                arrow.x1 = clear_left.clamp(clip_left, clip_right);
            }
            continue;
        }
        let target = desired.clamp(clip_left, clip_right);
        if target <= arrow.x1 + 0.5 {
            continue;
        }
        if arrow.wraps_around_successor() && target <= wrap_floor(arrow) {
            continue;
        }
        let delta = target - arrow.x1;
        arrow.x1 = target;
        if arrow.wraps_around_successor() {
            arrow.x2 = (arrow.x2 + delta).clamp(clip_left, clip_right);
        }
    }
    separate_shared_source_outgoing_off_labels(
        arrows, bars, clip_left, clip_right, timeline_w, today_x,
    );
}

/// Last dodge can park Persist cart → Go-live 10px past the spike stub
/// (incoming-fan spacing). 8px hits then leave a 2px bounding-box gap.
/// Spread `x1` by `PARALLEL_VERT_MIN` without walking onto the source chip:
/// push right when that still misses the letters, otherwise push the left
/// stub further left.
fn separate_shared_source_outgoing_off_labels(
    arrows: &mut [DependencyArrow],
    bars: &[BarLayout],
    clip_left: f32,
    clip_right: f32,
    timeline_w: f32,
    today_x: Option<f32>,
) {
    if arrows.len() < 2 {
        return;
    }
    let by_id: HashMap<i64, &BarLayout> = bars.iter().map(|bar| (bar.id, bar)).collect();
    let mut by_from: HashMap<i64, Vec<usize>> = HashMap::new();
    for (index, arrow) in arrows.iter().enumerate() {
        by_from.entry(arrow.from_id).or_default().push(index);
    }
    for indexes in by_from.values_mut() {
        if indexes.len() < 2 {
            continue;
        }
        indexes.sort_by(|&a, &b| arrows[a].x1.total_cmp(&arrows[b].x1));
        for _ in 0..indexes.len() * 4 {
            let mut moved = false;
            for pair in indexes.windows(2) {
                let a = pair[0];
                let b = pair[1];
                let dx = (arrows[a].x1 - arrows[b].x1).abs();
                if dx >= PARALLEL_VERT_MIN {
                    continue;
                }
                let (left_i, right_i) = if arrows[a].x1 <= arrows[b].x1 {
                    (a, b)
                } else {
                    (b, a)
                };
                let chip = by_id.get(&arrows[right_i].from_id).and_then(|pred| {
                    bar_outside_title_span_clipped(pred, timeline_w.max(1.0), today_x)
                });
                let push = (arrows[left_i].x1 + PARALLEL_VERT_MIN - arrows[right_i].x1).max(1.0);
                let right_next = arrows[right_i].x1 + push;
                let hits_chip = chip.is_some_and(|(left, right)| {
                    x_hits_span(
                        right_next,
                        left - SOURCE_LABEL_CLEARANCE,
                        right + SOURCE_LABEL_CLEARANCE,
                    )
                });
                if !hits_chip
                    && shift_vertical(
                        &mut arrows[right_i],
                        VertKind::Outgoing,
                        push,
                        clip_left,
                        clip_right,
                    )
                {
                    moved = true;
                    continue;
                }
                let pull = (arrows[right_i].x1 - PARALLEL_VERT_MIN - arrows[left_i].x1).min(-1.0);
                if shift_vertical(
                    &mut arrows[left_i],
                    VertKind::Outgoing,
                    pull,
                    clip_left,
                    clip_right,
                ) {
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
    }
}

/// Keep a long outgoing horizontal in the source row gap after parallel-lane
/// pushes. Outside-title sources sit ≥6px below the chip; inside-title Day
/// bars use the same in-row band so Persist cart → Go-live cannot ride the
/// Go-live row. Shared-source re-spread below still only fans outside-title
/// sources — the inside-title gap is too tight to split two lanes.
fn clamp_lanes_off_source_titles(arrows: &mut [DependencyArrow], bars: &[BarLayout]) {
    const CLEAR: f32 = 6.0;
    const STROKE: f32 = 2.0;
    let by_id: HashMap<i64, &BarLayout> = bars.iter().map(|bar| (bar.id, bar)).collect();
    for arrow in arrows.iter_mut() {
        let Some(pred) = by_id.get(&arrow.from_id) else {
            continue;
        };
        if bar_is_gutter(pred) {
            continue;
        }
        if arrow.y2 <= arrow.y1 {
            continue;
        }
        let (gap_top, gap_bottom) = source_row_gap(pred);
        let clear = if bar_title_is_inside(pred) {
            1.0
        } else {
            CLEAR
        };
        let min_lane = (gap_top + clear).min(gap_bottom - STROKE - 1.0);
        let max_lane = (gap_bottom - STROKE - 1.0).max(min_lane);
        if bar_title_is_inside(pred) {
            // Parallel-lane pushes can park a Day-scale inside-title run on
            // a later row. Pull it back; do not snap to the midpoint or two
            // in-gap lanes collapse.
            arrow.lane_y = arrow.lane_y.clamp(min_lane, max_lane);
        } else {
            arrow.lane_y = ((min_lane + max_lane) / 2.0).clamp(min_lane, max_lane);
        }
        if let Some(succ) = by_id.get(&arrow.to_id)
            && arrow.lane_y >= succ.y
            && arrow.lane_y < succ.y + ROW_HEIGHT
        {
            arrow.lane_y = max_lane.min(succ.y - STROKE - 1.0).max(min_lane);
        }
    }
    let mut by_from: HashMap<i64, Vec<usize>> = HashMap::new();
    for (index, arrow) in arrows.iter().enumerate() {
        let Some(pred) = by_id.get(&arrow.from_id) else {
            continue;
        };
        if bar_title_is_inside(pred) || bar_is_gutter(pred) || arrow.y2 <= arrow.y1 {
            continue;
        }
        by_from.entry(arrow.from_id).or_default().push(index);
    }
    for indexes in by_from.values_mut() {
        if indexes.len() < 2 {
            continue;
        }
        indexes.sort_by_key(|&index| arrows[index].to_id);
        let pred = by_id[&arrows[indexes[0]].from_id];
        let (gap_top, gap_bottom) = source_row_gap(pred);
        let min_lane = (gap_top + CLEAR).min(gap_bottom - STROKE - 1.0);
        let max_lane = (gap_bottom - STROKE - 1.0).max(min_lane);
        let span = max_lane - min_lane;
        if span < 2.0 {
            continue;
        }
        let last = (indexes.len() - 1) as f32;
        for (rank, &index) in indexes.iter().enumerate() {
            arrows[index].lane_y = min_lane + span * rank as f32 / last;
        }
    }
}

fn clip_arrow_to_timeline(arrow: &mut DependencyArrow, left: f32, right: f32) {
    let clamp_span = |x: f32| x.clamp(left, right);
    // Keep the head on the successor. Do not pull ▶ off a diamond that sits
    // near the clip (Persist cart → Go-live) or onto a gutter "No dates".
    if arrow.x_head > left + 1.0 {
        arrow.x1 = clamp_span(arrow.x1);
        arrow.x2 = clamp_span(arrow.x2);
    } else {
        arrow.x1 = arrow.x1.clamp(0.0, right);
        arrow.x2 = arrow.x2.clamp(0.0, right);
    }
    const MIN_STUB: f32 = 10.0;
    // Only expand a collapsed inbound stub. Do not shrink wrap-around elbows.
    // Dated FS / milestone heads stay on the successor start — moving x_head
    // to the clip made Go-live's ▶ stop short of the diamond.
    if arrow.inbound_right {
        if arrow.x2 < arrow.x_head + MIN_STUB {
            arrow.x2 = (arrow.x_head + MIN_STUB).min(right);
        }
        if arrow.x_head > right - MIN_STUB {
            arrow.x2 = right;
        }
    } else if (arrow.x_head - arrow.x2).abs() < MIN_STUB {
        let floor = if arrow.x_head > left + MIN_STUB {
            left
        } else {
            0.0
        };
        arrow.x2 = (arrow.x_head - MIN_STUB).max(floor);
    }
}

fn parse_type_color(hex: &str) -> u32 {
    u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x89b4fa)
}

pub(crate) fn header_label_px(label: &str) -> f32 {
    (label.chars().count() as f32 * 7.0 + 8.0).max(14.0)
}

/// Whether the leftover label, painted at `HEADER_LEFT_PAD`, fits before the
/// first in-view tick at `next_x`. Too narrow, the leftover gives way: a
/// two-day "Jun" sliver must not push "Jul" out of the header.
fn leftover_label_has_room(next_x: f32, leftover: &str, min_gap: f32) -> bool {
    next_x >= min_gap.max(HEADER_LEFT_PAD + header_label_px(leftover))
}

/// Painted x of the "Today" pill, matching the timeline header.
pub(crate) fn today_pill_left(today_x: f32, width: f32) -> f32 {
    if today_x + TODAY_PILL_GAP + TODAY_PILL_W > width - 4.0 {
        (today_x - TODAY_PILL_W - 4.0).max(4.0)
    } else {
        (today_x + TODAY_PILL_GAP).max(4.0)
    }
}

/// Keep every other Year month. Prefer a phase that keeps January so
/// leftover Feb (go-today in November) does not paint Feb, Apr, Jun with
/// no Jan. Leftover Jul still yields Jul, Sep, Nov, Jan, Mar because Jul
/// and Jan share parity. Thinning before sticky means the next kept month
/// is two months out and the leftover is not dropped for sitting on the
/// skipped in-view month.
fn thin_year_month_labels(labels: Vec<(f32, String)>) -> Vec<(f32, String)> {
    let leftover_phase = labels
        .iter()
        .enumerate()
        .rev()
        .find(|(_, (x, _))| *x <= 0.0)
        .map(|(index, _)| index)
        .unwrap_or(0);
    let jan_phase = labels
        .iter()
        .enumerate()
        .find(|(_, (_, label))| label == "Jan")
        .map(|(index, _)| index);
    let phase = jan_phase.unwrap_or(leftover_phase);
    labels
        .into_iter()
        .enumerate()
        .filter(|(index, _)| index % 2 == phase % 2)
        .map(|(_, item)| item)
        .collect()
}

/// Week `"Nov 2"` labels are ~43px at 24px/day, so a Monday tick sits on
/// Tuesday's Today line. Drop ticks whose text box crosses the stroke.
fn dodge_week_ticks_off_today(labels: Vec<(f32, String)>, today_x: f32) -> Vec<(f32, String)> {
    const LINE_PAD: f32 = 6.0;
    labels
        .into_iter()
        .filter(|(x, label)| {
            let right = *x + header_label_px(label);
            right < today_x - LINE_PAD || *x > today_x + LINE_PAD
        })
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
    let mut tip = format!("{} · {} · {assignee}", bar.title, dates);
    if let Some(overflow) = parent_overflow_tip(nodes, bar.id) {
        let parent_name = node
            .and_then(|item| item.parent_id)
            .and_then(|parent_id| nodes.iter().find(|item| item.id == parent_id))
            .map(|item| item.title.trim())
            .filter(|name| !name.is_empty())
            .unwrap_or("its parent");
        tip.push_str(" · ! ");
        tip.push_str(&overflow);
        tip.push_str(". The ! means this item extends outside ");
        tip.push_str(parent_name);
        tip.push_str(" after that parent moved.");
    }
    tip
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
    child_outside_parent(node, parent)
}

fn parent_overflow_span_indexed(
    nodes: &[WorkItemNode],
    index: &HierarchyIndex,
    id: i64,
) -> Option<(Option<Date>, Option<Date>)> {
    let node = index.get(nodes, id)?;
    let parent = index.get(nodes, node.parent_id?)?;
    child_outside_parent(node, parent)
}

fn child_outside_parent(
    node: &WorkItemNode,
    parent: &WorkItemNode,
) -> Option<(Option<Date>, Option<Date>)> {
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

/// Painted outside-chip width. `text-xs` is ~10px/glyph plus `px_1` on the
/// chip; 8px/char sat the Persist cart → Go-live stub 0–3px from the letters
/// and 10px/char + 16 still left a 1–2px Month miss.
pub fn bar_title_paint_px(title: &str) -> f32 {
    title.chars().count() as f32 * 10.0 + 24.0
}

/// Gap from the painted bar (or diamond) to an outside title, matching
/// `render_gantt_bar_label`.
pub fn outside_title_right_gap(
    milestone: bool,
    show_link_handles: bool,
    overflow_badge: bool,
) -> f32 {
    if milestone {
        MILESTONE_LABEL_GAP
    } else if overflow_badge {
        OUTSIDE_LABEL_GAP
    } else if show_link_handles {
        // 16px handle hit starts 6px past the bar; +20 slack keeps the
        // elbow stub and the chip from sharing an edge.
        LINK_HANDLE_GAP + LINK_HANDLE_HIT + 20.0
    } else {
        10.0
    }
}

fn full_outside_title_fits(
    label_x: f32,
    estimated: f32,
    timeline_w: f32,
    today_x: Option<f32>,
) -> bool {
    if label_x < 0.0 || label_x + estimated > timeline_w - 4.0 {
        return false;
    }
    if let Some(today) = today_x
        && label_x < today
        && label_x + estimated > today - TODAY_LABEL_CLEARANCE
    {
        return false;
    }
    true
}

/// Overlay frame for a title that does not fit inside the bar. Always the
/// full glyph width — never a 40px "Pers…" clip. Prefer the right of the
/// bar; flip left when the full title cannot sit on the right (clip or
/// Today) if the left side can hold it. Never park far from the bar — if
/// both sides miss, keep the natural right slot (the Today line gaps
/// through the chip).
pub fn bar_outside_label_frame(
    bar: &BarLayout,
    timeline_w: f32,
    show_link_handles: bool,
    today_x: Option<f32>,
    overflow_badge: bool,
) -> Option<(f32, f32)> {
    if bar_is_gutter(bar) || bar_title_is_inside(bar) {
        return None;
    }
    if bar_offscreen_edge(bar, timeline_w).is_some() {
        return None;
    }
    let title_width = bar_title_paint_px(&bar.title);
    let painted_w = if bar.milestone || bar.marker {
        MILESTONE_SIZE
    } else {
        bar.width.max(8.0)
    };
    let bar_right = bar.x + painted_w;
    if bar_right < 0.0 || bar.x > timeline_w {
        return None;
    }
    let estimated = title_width.max(24.0);
    let gap = outside_title_right_gap(
        bar.milestone || bar.marker,
        show_link_handles,
        overflow_badge,
    );
    let right_x = bar_right + gap;
    if full_outside_title_fits(right_x, estimated, timeline_w, today_x) {
        return Some((right_x, estimated));
    }
    let today_blocks_right = today_x.is_some_and(|today| {
        right_x < today && right_x + estimated > today - TODAY_LABEL_CLEARANCE
    });
    // Milestones stay on the right of the diamond across Week/Month/Quarter
    // even when the full title clips at the timeline edge. Flip left only
    // when Today blocks the right side, or the title would not even start
    // on-screen.
    let right_starts_on_screen = right_x >= 0.0 && right_x < timeline_w - 4.0;
    if (bar.milestone || bar.marker) && right_starts_on_screen && !today_blocks_right {
        return Some((right_x, estimated));
    }
    let left_x = bar.x - estimated - OUTSIDE_TITLE_LEFT_GAP;
    if full_outside_title_fits(left_x, estimated, timeline_w, today_x) {
        return Some((left_x, estimated));
    }
    // Last-resort right would paint through Today. Prefer a left chip
    // beside the bar when that side is only slightly past x=0. A deeply
    // negative left_x (Persist cart ~144px at bar.x=40) stays on the
    // right of the bar — the Today line draws a gap through the chip.
    if today_blocks_right && !bar.milestone && !bar.marker {
        let clamped = left_x.max(2.0);
        let misses_today = today_x.is_none_or(|today| {
            clamped >= today + TODAY_LABEL_CLEARANCE
                || clamped + estimated <= today - TODAY_LABEL_CLEARANCE
        });
        let misses_bar = clamped + estimated <= bar.x + 0.1;
        let beside = (clamped - left_x).abs() <= MAX_LABEL_BAR_SHIFT;
        if misses_today && misses_bar && beside && clamped + estimated > 8.0 {
            return Some((clamped, estimated));
        }
    }
    Some((right_x, estimated))
}

/// True when an outside chip sits in the natural beside-bar slot, not
/// parked tens of pixels away at the Today line.
pub fn outside_label_is_beside_bar(
    bar: &BarLayout,
    label_x: f32,
    label_w: f32,
    show_link_handles: bool,
    overflow_badge: bool,
) -> bool {
    let painted_w = if bar.milestone || bar.marker {
        MILESTONE_SIZE
    } else {
        bar.width.max(8.0)
    };
    let bar_right = bar.x + painted_w;
    let gap = outside_title_right_gap(
        bar.milestone || bar.marker,
        show_link_handles,
        overflow_badge,
    );
    let right_x = bar_right + gap;
    let left_x = bar.x - label_w.max(24.0) - OUTSIDE_TITLE_LEFT_GAP;
    (label_x - right_x).abs() <= MAX_LABEL_BAR_SHIFT
        || (label_x - left_x).abs() <= MAX_LABEL_BAR_SHIFT
}

/// Body-relative Y ranges where the Today line should break.
///
/// Year/Quarter/Month/Week stay a continuous stroke. Labels paint on top
/// of the line (behind the chips) instead of punching holes through it.
pub fn today_line_chip_skips(
    _bars: &[BarLayout],
    _today_x: f32,
    _timeline_w: f32,
    _scale: GanttScale,
) -> Vec<(f32, f32)> {
    Vec::new()
}

/// Painted Today-line segments `(top, height)` in `y0..y1`, skipping `skips`.
pub fn today_line_segments(y0: f32, y1: f32, skips: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut cursor = y0;
    let mut out = Vec::new();
    for &(skip_top, skip_bottom) in skips {
        let top = skip_top.max(y0);
        let bottom = skip_bottom.min(y1);
        if top >= bottom {
            continue;
        }
        if top > cursor + 1.0 {
            out.push((cursor, top - cursor));
        }
        cursor = cursor.max(bottom);
    }
    if y1 > cursor + 1.0 {
        out.push((cursor, y1 - cursor));
    }
    out
}

/// ‹ / › chip when a dated bar sits fully past the timeline clip.
/// Milestones keep the on-bar ◆ so a Day-view Go-live cue still reads
/// as a diamond, not a plain title.
pub(crate) fn offscreen_edge_chip(
    bar: &BarLayout,
    timeline_w: f32,
) -> Option<(f32, f32, String, &'static str)> {
    let edge = bar_offscreen_edge(bar, timeline_w)?;
    const GLYPH_W: f32 = 16.0;
    const DIAMOND_W: f32 = 16.0;
    let title_w = bar_title_paint_px(&bar.title).min(180.0);
    let diamond_w = if bar.milestone { DIAMOND_W } else { 0.0 };
    let chip_w = GLYPH_W + diamond_w + 8.0 + title_w;
    let label = match (edge, bar.milestone) {
        (OffscreenEdge::Left, true) => format!("‹ ◆ {}", bar.title),
        (OffscreenEdge::Right, true) => format!("◆ {} ›", bar.title),
        (OffscreenEdge::Left, false) => format!("‹ {}", bar.title),
        (OffscreenEdge::Right, false) => format!("{} ›", bar.title),
    };
    match edge {
        OffscreenEdge::Left => Some((2.0, chip_w, label, "left")),
        OffscreenEdge::Right => {
            Some(((timeline_w - chip_w - 2.0).max(2.0), chip_w, label, "right"))
        }
    }
}

/// Which timeline clip a fully off-screen dated bar sits past.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OffscreenEdge {
    Left,
    Right,
}

/// `None` for peeking fragments, gutter markers, and in-view bars.
pub(crate) fn bar_offscreen_edge(bar: &BarLayout, timeline_w: f32) -> Option<OffscreenEdge> {
    if bar.marker || bar.kind == GanttBarKind::Unscheduled || bar.kind == GanttBarKind::NoFields {
        return None;
    }
    let width = if bar.milestone {
        MILESTONE_SIZE
    } else {
        bar.width.max(8.0)
    };
    if bar.x + width <= 0.0 {
        Some(OffscreenEdge::Left)
    } else if bar.x >= timeline_w {
        Some(OffscreenEdge::Right)
    } else {
        None
    }
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
        HANDLE_WIDTH.min(bar.width / 2.0)
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
    // Paint insets the inner label by the end-link sliver (`right(grip +
    // resize_end_link_inset)`). Ignore that and a title that "fits" is
    // clipped by the last 6px.
    let link_inset = bar.resize_end_link_inset();
    let inner = (visible_w - grip * 2.0 - h_pad - cue - link_inset).max(0.0);
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
    let title_w = bar_title_paint_px(&bar.title);
    if bar_title_is_inside(bar) {
        bar.x + painted_w
    } else if bar.milestone || bar.marker {
        bar.x + painted_w + 6.0 + title_w
    } else {
        bar.x + painted_w + HANDLE_WIDTH + LINK_HANDLE_HIT + 16.0 + title_w
    }
}

fn painted_pred_end(pred: &BarLayout) -> f32 {
    if pred.milestone || pred.marker {
        pred.x + MILESTONE_SIZE
    } else {
        pred.x + pred.width
    }
}

/// Source starts at/before the target and its painted end sits past the
/// target start. A left inbound (`succ.x - 12`) is still on/after the
/// source, so a wrap drop at `pred_end + 12` would run through the target
/// (Cart UI body/cue). Any pixel of overlap counts: Year at 1.5 px/day
/// only overlaps Persist cart → Cart UI by ~6px, which used to miss a
/// 12px-past check and let `separate_parallel_verticals` shove the inbound
/// onto the successor body.
fn overlap_enters_from_the_left(pred: &BarLayout, succ: &BarLayout) -> bool {
    let pred_end = painted_pred_end(pred);
    let natural_in = succ.x - DependencyArrow::WRAP_STUB;
    pred_end > succ.x && natural_in + 0.5 >= pred.x
}

/// Keep overlapping finish-to-start inbounds left of the successor after
/// later fans. `separate_parallel_verticals` otherwise pushes a short stub
/// 16px right onto Cart UI (Week drop at x≈884 through the bar).
fn force_overlap_back_jog(
    arrows: &mut [DependencyArrow],
    bars: &[BarLayout],
    include_labels: bool,
    clip_left: f32,
    clip_right: f32,
    today_x: Option<f32>,
) {
    let by_id: HashMap<i64, &BarLayout> = bars.iter().map(|bar| (bar.id, bar)).collect();
    for arrow in arrows.iter_mut() {
        if arrow.inbound_right {
            continue;
        }
        let Some(pred) = by_id.get(&arrow.from_id) else {
            continue;
        };
        let Some(succ) = by_id.get(&arrow.to_id) else {
            continue;
        };
        if !overlap_enters_from_the_left(pred, succ) {
            continue;
        }
        let (left, right) = bar_occupied_x(succ, include_labels, Some(clip_right), today_x);
        let stub = (succ.x - DependencyArrow::BACK_JOG_STUB).max(clip_left);
        let left_of_occ = (left - 8.0).max(clip_left);
        let want = stub.min(left_of_occ);
        if x_hits_span(arrow.x2, left, right) || arrow.x2 + 0.5 >= succ.x || arrow.x2 > want + 0.5 {
            arrow.x2 = want;
        }
        let pred_end = painted_pred_end(pred);
        let tight_exit = (pred_end + DependencyArrow::BACK_JOG_STUB).clamp(clip_left, clip_right);
        if arrow.x1 + 0.5 < pred_end || arrow.x1 > tight_exit + 0.5 {
            arrow.x1 = tight_exit;
        }
    }
}

/// Painted bar body, milestone diamond, or open-ended dashed cue-to-clip.
/// Outside titles are not obstacles — they sit in the label slot.
/// Left pad matches the painted handle so a drop at `bar.x - HANDLE_WIDTH`
/// is still treated as crossing the bar.
fn painted_obstacle_x(bar: &BarLayout, clip_right: f32, timeline_w: f32) -> (f32, f32) {
    // Off-range bars paint a ‹ / › chip at the clip, not at bar.x. Day
    // Persist cart → Go-live at x≈885 ran through that chip (~880–1025).
    if let Some((chip_x, chip_w, _, _)) = offscreen_edge_chip(bar, timeline_w.max(clip_right)) {
        return (chip_x - 2.0, chip_x + chip_w + 2.0);
    }
    if bar.milestone || bar.marker {
        return (bar.x - 2.0, bar.x + MILESTONE_SIZE + 4.0);
    }
    let mut right = bar.x + bar.width.max(8.0);
    if bar.open_ended {
        right = right.max(clip_right);
    }
    (bar.x - HANDLE_WIDTH, right)
}

fn vertical_crosses_painted(y_a: f32, y_b: f32, bar: &BarLayout) -> bool {
    let lo = y_a.min(y_b);
    let hi = y_a.max(y_b);
    if hi - lo <= 4.0 {
        return false;
    }
    let top = bar.y + bar.paint_top();
    let bottom = top + bar.paint_h();
    lo < bottom - 1.0 && hi > top + 1.0
}

fn painted_obstacles_on_span(
    y_a: f32,
    y_b: f32,
    bars: &[BarLayout],
    skip: impl Fn(&BarLayout) -> bool,
    clip_right: f32,
    timeline_w: f32,
) -> Vec<(f32, f32)> {
    bars.iter()
        .filter(|bar| !skip(bar) && vertical_crosses_painted(y_a, y_b, bar))
        .map(|bar| painted_obstacle_x(bar, clip_right, timeline_w))
        .collect()
}

fn x_hits_any_obstacle(x: f32, obstacles: &[(f32, f32)]) -> bool {
    obstacles
        .iter()
        .any(|(left, right)| x_hits_span(x, *left, *right))
}

fn first_clear_vertical_x_preferring_spaced(
    candidates: impl IntoIterator<Item = f32>,
    obstacles_at: impl Fn(f32) -> Vec<(f32, f32)>,
    clip_left: f32,
    clip_right: f32,
    prefer_min_x: f32,
    siblings: &[f32],
) -> Option<f32> {
    let xs: Vec<f32> = candidates.into_iter().collect();
    for sibling_min_dx in [PARALLEL_VERT_MIN, 8.0, 0.0] {
        for require_right_of_source in [true, false] {
            for raw in &xs {
                if require_right_of_source && *raw + 0.5 < prefer_min_x {
                    continue;
                }
                let clipped = raw.clamp(clip_left, clip_right);
                // Prefer the painted x, but do not clamp *onto* an
                // obstacle (Day wrap used to drop through the off-range
                // successor chip). A raw x left of clip_left can still
                // sit in the gutter beside an open-ended cue.
                let obstacles = obstacles_at(clipped);
                let x = if x_hits_any_obstacle(clipped, &obstacles) {
                    let fallback = raw.clamp(4.0, clip_right);
                    if x_hits_any_obstacle(fallback, &obstacles) {
                        continue;
                    }
                    fallback
                } else {
                    clipped
                };
                if x_hits_any_obstacle(x, &obstacles) {
                    continue;
                }
                if sibling_min_dx > 0.0
                    && siblings
                        .iter()
                        .any(|other| (x - *other).abs() < sibling_min_dx)
                {
                    continue;
                }
                return Some(x);
            }
        }
    }
    None
}

/// Pick each polyline vertical so it misses every painted bar/milestone in
/// the rows it spans. Keep a clear x from earlier passes; otherwise try
/// exit-right (`pred_end+12`), back-jog-left of the successor, then left
/// of each obstacle (inter-row gutter at the left edge last). Prefer not
/// to detour left of the source when a clear x still exists on its right.
#[allow(clippy::needless_range_loop)]
fn route_obstacle_aware_verticals(
    arrows: &mut [DependencyArrow],
    bars: &[BarLayout],
    clip_left: f32,
    clip_right: f32,
    timeline_w: f32,
) {
    const CLEAR_GAP: f32 = 8.0;
    let by_id: HashMap<i64, &BarLayout> = bars.iter().map(|bar| (bar.id, bar)).collect();
    let mut routed_x2: Vec<f32> = Vec::new();
    let mut routed_x1: Vec<f32> = Vec::new();
    for i in 0..arrows.len() {
        let from_id = arrows[i].from_id;
        let to_id = arrows[i].to_id;
        let Some(pred) = by_id.get(&from_id).copied() else {
            continue;
        };
        let Some(succ) = by_id.get(&to_id).copied() else {
            continue;
        };
        let pred_end = painted_pred_end(pred);
        let exit_right = pred_end
            + if overlap_enters_from_the_left(pred, succ) {
                DependencyArrow::BACK_JOG_STUB
            } else {
                DependencyArrow::WRAP_STUB
            };
        let back_jog = arrows[i].x_head
            - if overlap_enters_from_the_left(pred, succ) {
                DependencyArrow::BACK_JOG_STUB
            } else {
                DependencyArrow::WRAP_STUB
            };
        let lane_y = arrows[i].lane_y;
        let y2 = arrows[i].y2;
        let y1 = arrows[i].y1;
        let x_head = arrows[i].x_head;
        let x1_now = arrows[i].x1;
        let gutter_y = arrows[i].gutter_y();
        let pred_id = pred.id;
        let succ_id = succ.id;
        let inbound_obs_at = |x: f32| {
            let wrap = x > x_head + DependencyArrow::WRAP_STUB && x1_now + 1.0 >= x_head;
            let end = if wrap { gutter_y } else { y2 };
            let mut obs = painted_obstacles_on_span(
                lane_y,
                end,
                bars,
                |bar| {
                    bar_is_gutter(bar)
                        || bar.id == pred_id
                        || (wrap
                            && bar.id == succ_id
                            && offscreen_edge_chip(bar, timeline_w).is_none())
                },
                clip_right,
                timeline_w,
            );
            // Wrap skips the successor body so the stub can sit at the
            // clip, but the off-range ‹ / › chip still paints there.
            if let Some((chip_x, chip_w, _, _)) = offscreen_edge_chip(succ, timeline_w) {
                obs.push((chip_x - 2.0, chip_x + chip_w + 2.0));
            }
            obs
        };
        let conservative_obs = inbound_obs_at(arrows[i].x2);
        let mut inbound_cands = vec![arrows[i].x2, exit_right, back_jog];
        let mut obstacle_lefts: Vec<f32> = conservative_obs
            .iter()
            .map(|(left, _)| *left - CLEAR_GAP)
            .collect();
        for x in &routed_x2 {
            inbound_cands.push(*x + PARALLEL_VERT_MIN);
            inbound_cands.push(*x - PARALLEL_VERT_MIN);
        }
        obstacle_lefts.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        inbound_cands.extend(obstacle_lefts);
        inbound_cands.push(clip_left + CLEAR_GAP);
        inbound_cands.push(4.0);
        if let Some(x2) = first_clear_vertical_x_preferring_spaced(
            inbound_cands,
            inbound_obs_at,
            clip_left,
            clip_right,
            pred.x,
            &routed_x2,
        ) {
            arrows[i].x2 = x2;
        }
        routed_x2.push(arrows[i].x2);
        let outgoing_obs = painted_obstacles_on_span(
            y1,
            lane_y,
            bars,
            |bar| bar_is_gutter(bar) || bar.id == pred.id,
            clip_right,
            timeline_w,
        );
        let mut outgoing_cands = vec![
            arrows[i].x1,
            exit_right,
            arrows[i].x2,
            clip_left + CLEAR_GAP,
            4.0,
        ];
        for x in &routed_x1 {
            outgoing_cands.push(*x + PARALLEL_VERT_MIN);
            outgoing_cands.push(*x - PARALLEL_VERT_MIN);
        }
        if let Some(x1) = first_clear_vertical_x_preferring_spaced(
            outgoing_cands,
            |_| outgoing_obs.clone(),
            clip_left,
            clip_right,
            pred.x,
            &routed_x1,
        ) {
            arrows[i].x1 = x1;
        }
        routed_x1.push(arrows[i].x1);
    }
}

/// A short inbound stub must not land inside an intervening painted bar
/// (solid body or open-ended dashed tail). Outside titles stay ignored
/// so Year/Quarter chips do not shove the drop tens of days away.
fn stub_recrosses_intervening(stub: f32, intervening: &[&BarLayout], clip_right: f32) -> bool {
    intervening.iter().any(|mid| {
        let (left, right) = bar_occupied_x(mid, false, Some(clip_right), None);
        x_hits_span(stub, left, right)
    })
}

/// Body, and optionally an external title, so a vertical drop misses the row.
/// Open-ended start-only bars paint a dashed cue to the clip, so routing
/// must treat that tail as occupied or the elbow runs through it. Label
/// occupancy uses the painted chip (`bar_outside_label_frame`) so a
/// Today/clip flip to the left is occupied on the letters, not a phantom
/// span to the right of the bar.
fn bar_occupied_x(
    bar: &BarLayout,
    include_label: bool,
    cue_right: Option<f32>,
    today_x: Option<f32>,
) -> (f32, f32) {
    bar_occupied_x_on(bar, include_label, cue_right, today_x, None)
}

fn bar_occupied_x_on(
    bar: &BarLayout,
    include_label: bool,
    cue_right: Option<f32>,
    today_x: Option<f32>,
    label_timeline_w: Option<f32>,
) -> (f32, f32) {
    if bar_is_gutter(bar) {
        return (bar.x - 2.0, bar.x + bar.width + 4.0);
    }
    let timeline_w = label_timeline_w.or(cue_right).unwrap_or(10_000.0).max(1.0);
    // Fully off-range bars paint a ‹ / › pill at the clip, not at bar.x.
    // Day Persist cart → spike at the clip was running through that chip.
    if let Some((chip_x, chip_w, _, _)) = offscreen_edge_chip(bar, timeline_w) {
        return (chip_x - 2.0, chip_x + chip_w + 2.0);
    }
    let mut left = if bar.milestone || bar.marker {
        bar.x - 2.0
    } else {
        bar.x - HANDLE_WIDTH
    };
    let mut right = if bar.milestone || bar.marker {
        bar.x + MILESTONE_SIZE + 4.0
    } else {
        bar.x + bar.width.max(8.0) + 4.0
    };
    if include_label {
        // Label frame uses the painted timeline, not the arrow inset clip
        // (`timeline_width - 10`) passed as `cue_right` after clip.
        if let Some((x0, x1)) = bar_outside_title_span_clipped(bar, timeline_w, today_x) {
            left = left.min(x0);
            right = right.max(x1);
        } else {
            right = right.max(bar_label_right(bar));
        }
    }
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
#[allow(clippy::too_many_arguments)]
fn clear_vertical_x(
    pref: f32,
    intervening: &[&BarLayout],
    succ_edge: f32,
    inbound_left: bool,
    x_head: f32,
    include_labels: bool,
    cue_right: Option<f32>,
    today_x: Option<f32>,
) -> f32 {
    let blocked: Vec<(f32, f32)> = intervening
        .iter()
        .map(|bar| bar_occupied_x(bar, include_labels, cue_right, today_x))
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

/// Re-apply wrap-drop clearance after later fans so a Day Persist cart →
/// spike loop cannot restack through the Go-live outside title.
fn dodge_wraps_off_intervening(
    arrows: &mut [DependencyArrow],
    bars: &[BarLayout],
    include_labels: bool,
    clip_left: f32,
    clip_right: f32,
    today_x: Option<f32>,
) {
    let by_id: HashMap<i64, &BarLayout> = bars.iter().map(|bar| (bar.id, bar)).collect();
    for arrow in arrows.iter_mut() {
        if !arrow.wraps_around_successor() {
            continue;
        }
        let Some(pred) = by_id.get(&arrow.from_id) else {
            continue;
        };
        let Some(succ) = by_id.get(&arrow.to_id) else {
            continue;
        };
        let lo = pred.y.min(succ.y);
        let hi = pred.y.max(succ.y);
        let intervening: Vec<&BarLayout> = bars
            .iter()
            .filter(|mid| {
                mid.id != pred.id && mid.id != succ.id && mid.y > lo + 1.0 && mid.y < hi - 1.0
            })
            .collect();
        if intervening.is_empty() {
            continue;
        }
        let next = clear_wrap_drop_x(
            arrow.x2,
            &intervening,
            include_labels,
            arrow.x_head,
            pred.x + 12.0,
            clip_right,
            today_x,
        );
        arrow.x2 = next.clamp(clip_left, clip_right);
    }
}

/// Slide a long vertical (outgoing or inbound) off an intervening chip so
/// Persist cart → spike does not run through the Go-live pill at Day.
fn dodge_verticals_off_intervening_pills(
    arrows: &mut [DependencyArrow],
    bars: &[BarLayout],
    include_labels: bool,
    clip_left: f32,
    clip_right: f32,
    today_x: Option<f32>,
    timeline_w: f32,
) {
    if !include_labels {
        return;
    }
    let by_id: HashMap<i64, &BarLayout> = bars.iter().map(|bar| (bar.id, bar)).collect();
    for arrow in arrows.iter_mut() {
        if arrow.wraps_around_successor() || arrow.inbound_right {
            continue;
        }
        let Some(pred) = by_id.get(&arrow.from_id) else {
            continue;
        };
        let Some(succ) = by_id.get(&arrow.to_id) else {
            continue;
        };
        let lo = pred.y.min(succ.y);
        let hi = pred.y.max(succ.y);
        let intervening: Vec<&BarLayout> = bars
            .iter()
            .filter(|mid| {
                mid.id != pred.id && mid.id != succ.id && mid.y > lo + 1.0 && mid.y < hi - 1.0
            })
            .collect();
        if intervening.is_empty() {
            continue;
        }
        let blocked: Vec<(f32, f32)> = intervening
            .iter()
            .map(|bar| bar_occupied_x_on(bar, true, Some(clip_right), today_x, Some(timeline_w)))
            .collect();
        let x_head = arrow.x_head;
        let wrap_limit = x_head + DependencyArrow::WRAP_STUB;
        let slide_off = |x: f32| -> f32 {
            let mut x = x.clamp(clip_left, clip_right);
            for _ in 0..=blocked.len() {
                let hit = blocked
                    .iter()
                    .copied()
                    .find(|&(left, right)| x_hits_span(x, left, right));
                let Some((left, right)) = hit else {
                    return x;
                };
                let left_cand = (left - 8.0).max(clip_left);
                if left_cand < x - 0.5 && !x_hits_span(left_cand, left, right) {
                    x = left_cand;
                    continue;
                }
                let right_cand = (right + 8.0).min(clip_right);
                if right_cand > x + 0.5
                    && right_cand < x_head - 2.0
                    && !x_hits_span(right_cand, left, right)
                {
                    x = right_cand;
                    continue;
                }
                return x;
            }
            x
        };
        let next2 = slide_off(arrow.x2);
        if (next2 - arrow.x2).abs() > 0.5 && next2 <= wrap_limit {
            arrow.x2 = next2;
        }
        let out_spans_intervening =
            arrow.y1.min(arrow.lane_y) < hi - 1.0 && arrow.y1.max(arrow.lane_y) > lo + ROW_HEIGHT;
        if out_spans_intervening {
            let next1 = slide_off(arrow.x1);
            if (next1 - arrow.x1).abs() > 0.5 {
                arrow.x1 = next1;
            }
        }
    }
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
    today_x: Option<f32>,
) -> f32 {
    let blocked: Vec<(f32, f32)> = intervening
        .iter()
        .map(|bar| bar_occupied_x(bar, include_labels, Some(clip_right), today_x))
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
    today_x: Option<f32>,
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
        let (left, right) = bar_occupied_x(bar, include_labels, cue_right, today_x);
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

/// Finish-to-start chain whose last Target Date is latest. Only work items
/// with both Start Date and Target Date are vertices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CriticalPath {
    pub ids: HashSet<i64>,
    pub links: HashSet<(i64, i64)>,
}

/// Derived highlight, not a stored field. Walks back from every dated item
/// that shares the latest Target Date, preferring the predecessor that
/// itself finishes latest.
pub fn critical_path(nodes: &[WorkItemNode]) -> CriticalPath {
    let dated: HashMap<i64, Date> = nodes
        .iter()
        .filter_map(|node| {
            parse_iso_date(node.start_date.as_deref())?;
            let target = parse_iso_date(node.target_date.as_deref())?;
            Some((node.id, target))
        })
        .collect();
    if dated.is_empty() {
        return CriticalPath::default();
    }
    let mut pred: HashMap<i64, Vec<i64>> = HashMap::new();
    for node in nodes {
        if !dated.contains_key(&node.id) {
            continue;
        }
        for &from in &node.predecessors {
            if dated.contains_key(&from) {
                pred.entry(node.id).or_default().push(from);
            }
        }
    }
    let latest = dated.values().copied().max().expect("dated is non-empty");
    let ends: Vec<i64> = dated
        .iter()
        .filter(|(_, target)| **target == latest)
        .map(|(id, _)| *id)
        .collect();
    let mut ids = HashSet::new();
    let mut links = HashSet::new();
    for end in ends {
        let mut cur = end;
        let mut seen = HashSet::new();
        ids.insert(cur);
        while seen.insert(cur) {
            let Some(next) = pred.get(&cur).and_then(|preds| {
                preds
                    .iter()
                    .copied()
                    .filter(|id| dated.contains_key(id))
                    .max_by_key(|id| (dated[id], *id))
            }) else {
                break;
            };
            links.insert((next, cur));
            ids.insert(next);
            cur = next;
        }
    }
    CriticalPath { ids, links }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowTone {
    /// The Flavor's `warning` token.
    Clash,
    Critical,
    Emphasis,
    /// Off the Critical path while the highlight is on.
    Dimmed,
    Plain,
}

impl ArrowTone {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clash => "clash",
            Self::Critical => "critical",
            Self::Emphasis => "emphasis",
            Self::Dimmed => "dimmed",
            Self::Plain => "plain",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArrowState {
    pub clash: bool,
    pub on_critical: bool,
    /// Selected or hovered.
    pub focused: bool,
    /// On the hovered bar's Dependency chain.
    pub related: bool,
    pub critical_path_on: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowPaint {
    pub tone: ArrowTone,
    pub stroke: f32,
}

/// A Clash takes the warning colour over every other state; a clashing
/// Critical path arrow keeps the path's width.
pub fn dependency_arrow_paint(state: ArrowState) -> ArrowPaint {
    let tone = if state.clash {
        ArrowTone::Clash
    } else if state.on_critical {
        ArrowTone::Critical
    } else if state.focused || state.related {
        ArrowTone::Emphasis
    } else if state.critical_path_on {
        ArrowTone::Dimmed
    } else {
        ArrowTone::Plain
    };
    let stroke = if state.focused || state.on_critical {
        3.0
    } else {
        2.0
    };
    ArrowPaint { tone, stroke }
}

/// One `Starts Jul 9 · #12 finishes Jul 10` line on a Clash badge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClashLine {
    pub predecessor_id: i64,
    pub starts: Date,
    pub predecessor_finishes: Date,
}

impl ClashLine {
    pub fn text(&self) -> String {
        format!(
            "Starts {} · #{} finishes {}",
            compact_date(self.starts),
            self.predecessor_id,
            compact_date(self.predecessor_finishes)
        )
    }
}

/// The warning `!` at a bar's start edge. One badge per bar: a line per
/// clashing predecessor, then a line per failed Date cascade write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScheduleBadge {
    pub clashes: Vec<ClashLine>,
    /// Reasons a Date cascade write to this bar failed for good.
    pub failures: Vec<String>,
}

impl ScheduleBadge {
    pub fn is_empty(&self) -> bool {
        self.clashes.is_empty() && self.failures.is_empty()
    }

    pub fn lines(&self) -> Vec<String> {
        self.clashes
            .iter()
            .map(ClashLine::text)
            .chain(
                self.failures
                    .iter()
                    .map(|reason| format!("Couldn't move: {reason}")),
            )
            .collect()
    }

    pub fn tooltip(&self) -> String {
        self.lines().join("\n")
    }

    pub fn accessibility_label(&self) -> String {
        format!("Schedule warning: {}", self.lines().join("; "))
    }
}

/// Clashing Dependencies and the badges they put on successor bars.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClashMarks {
    pub links: HashSet<DependencyLink>,
    pub badges: HashMap<i64, ScheduleBadge>,
}

/// Work items connected to `id` by finish-to-start links in either direction
/// (the hovered bar's whole dependency chain, matching monday.com highlight).
pub fn related_dependency_ids(nodes: &[WorkItemNode], id: i64) -> HashSet<i64> {
    let mut succs: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut preds: HashMap<i64, Vec<i64>> = HashMap::new();
    for node in nodes {
        for &from in &node.predecessors {
            succs.entry(from).or_default().push(node.id);
            preds.entry(node.id).or_default().push(from);
        }
    }
    let mut out = HashSet::from([id]);
    let mut stack = vec![id];
    while let Some(cur) = stack.pop() {
        for next in preds
            .get(&cur)
            .into_iter()
            .flatten()
            .chain(succs.get(&cur).into_iter().flatten())
        {
            if out.insert(*next) {
                stack.push(*next);
            }
        }
    }
    out
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
    completion(nodes.iter().filter(|node| node.parent_id == Some(id)))
}

fn child_completion_indexed(
    nodes: &[WorkItemNode],
    index: &HierarchyIndex,
    id: i64,
) -> Option<f32> {
    completion(index.children(nodes, id))
}

fn completion<'a>(children: impl Iterator<Item = &'a WorkItemNode>) -> Option<f32> {
    let (count, total) = children.fold((0usize, 0.0f32), |(count, total), node| {
        (count + 1, total + state_progress(&node.state))
    });
    (count > 0).then(|| total / count as f32)
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

    fn assert_inbound_drops_before_intervening_bar(
        arrow: &DependencyArrow,
        bar: &BarLayout,
        clip_right: f32,
    ) {
        let (left, right) = bar_occupied_x(bar, false, Some(clip_right), None);
        assert!(
            !x_hits_span(arrow.x2, left, right),
            "inbound vertical must miss {}'s painted bar, x2={} occupied={left}..{right} bar.x={}",
            bar.title,
            arrow.x2,
            bar.x
        );
        assert!(
            arrow.x2 < bar.x || arrow.x2 > right,
            "drop before {} starts or after its painted span, x2={} bar.x={}",
            bar.title,
            arrow.x2,
            bar.x
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
            overflow_badge: false,
            floored: false,
        };
        let bars = [bar.clone()];
        assert_eq!(
            GanttView::hit_test_bar(&bars, 102.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::ResizeStart)
        );
        assert_eq!(
            GanttView::hit_test_bar(&bars, 176.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "2–4px inside the painted end must resize"
        );
        assert_eq!(
            GanttView::hit_test_bar(&bars, 174.5, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "5.5px inside the painted end is still the 6px resize halo, not a leftover link sliver"
        );
        assert_eq!(
            GanttView::hit_test_bar(&bars, 168.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "the 14px just left of the link sliver must still resize"
        );
        assert_eq!(
            GanttView::hit_test_bar(&bars, 162.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "12px inside the right edge (past the 6px link sliver) must resize"
        );
        assert_eq!(
            GanttView::hit_test_bar(&bars, 111.0, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::ResizeStart),
            "12px inside the left edge must resize"
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
            "2–4px inside the right end must resize"
        );
        assert_eq!(
            GanttView::hit_test_bar(&bars, 100.0 + 80.0 - 5.5, 4.0).map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "5.5px inside the painted end must resize (halo covers edge−6..+1)"
        );
        let (end_x, end_w) = resize_end_handle(&bar);
        assert!(
            (end_x - (bar.x + bar.width - RESIZE_HIT_WIDTH - LINK_HANDLE_INSET)).abs() < 0.1,
            "end grip must sit just left of the 6px link sliver, got {end_x}"
        );
        assert!(
            (end_w - RESIZE_HIT_WIDTH).abs() < 0.1,
            "end grip must be the inner 14px, got {end_w}"
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
            "a parent with its own dates resizes 2–4px inside the end"
        );
        assert_eq!(
            GanttView::hit_test_bar(std::slice::from_ref(&summary), 174.5, 4.0)
                .map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "a parent with its own dates resizes 5.5px inside the end"
        );
        assert_eq!(
            GanttView::hit_test_bar(std::slice::from_ref(&summary), 168.0, 4.0)
                .map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "a parent with its own dates still resizes just left of the link sliver"
        );
        assert_eq!(
            summary.hit_zone_at_window_x(168.0, summary.x, summary.x + summary.width),
            BarHitZone::ResizeEnd
        );
        assert_eq!(
            summary.hit_zone_at_window_x(177.0, summary.x, summary.x + summary.width),
            BarHitZone::ResizeEnd,
            "3px inside a summary end is the resize halo"
        );
        assert_eq!(
            summary.hit_zone_at_window_x(174.5, summary.x, summary.x + summary.width),
            BarHitZone::ResizeEnd,
            "5.5px inside a summary end is the resize halo, not a leftover link sliver"
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
            overflow_badge: false,
            floored: false,
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
            "the last pixel of a Week-scale bar must resize (halo covers edge−4..+6)"
        );
        assert_eq!(
            GanttView::hit_test_bar(
                std::slice::from_ref(&bar),
                bar.x + bar.width - LINK_HANDLE_INSET - RESIZE_HIT_WIDTH / 2.0,
                4.0
            )
            .map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "the 14px just left of the link sliver must still resize"
        );
        assert_eq!(
            GanttView::hit_test_bar(std::slice::from_ref(&bar), bar.x + bar.width / 2.0, 4.0)
                .map(|(_, kind)| kind),
            Some(GestureKind::Move),
            "the body of a wide bar must still move"
        );
        assert_eq!(
            GanttView::hit_test_bar(std::slice::from_ref(&bar), bar.x + bar.width + 2.0, 4.0)
                .map(|(_, kind)| kind),
            None,
            "the link-handle gap past the bar must not be a resize"
        );
        assert_eq!(
            bar.resize_kind_at(bar.x + bar.width - LINK_HANDLE_INSET - 3.0),
            Some(GestureKind::ResizeEnd),
            "3px inside the shifted end grip must be ResizeEnd"
        );
        assert_eq!(
            GanttView::hit_test_bar(std::slice::from_ref(&bar), bar.x + bar.width - 3.0, 4.0)
                .map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "2–4px inside the painted end must resize"
        );
        assert_eq!(
            GanttView::hit_test_bar(std::slice::from_ref(&bar), bar.x + bar.width - 5.5, 4.0)
                .map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "5.5px inside the painted end must resize (halo covers edge−6..+1)"
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
                bar.hit_zone_at_window_x(
                    painted_right - LINK_HANDLE_INSET - inset,
                    painted_left,
                    painted_right
                ),
                BarHitZone::ResizeEnd,
                "{inset}px inside the shifted end grip must resize, not move"
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
    fn auto_fit_keeps_the_current_scale_and_month_width() {
        let mut item = node(1, None);
        item.start_date = Some("2026-07-06T00:00:00Z".into());
        item.target_date = Some("2026-11-20T00:00:00Z".into());
        let nodes = vec![item];
        let mut gantt = GanttView {
            origin: Date::new(2026, 11, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let before = gantt.period_width(Date::new(2026, 7, 1).unwrap());
        gantt.auto_fit(&nodes, &[], 800.0);
        assert_eq!(
            gantt.scale,
            GanttScale::Month,
            "Auto fit must keep Month so column width does not jump, got {:?}",
            gantt.scale
        );
        assert!(
            (gantt.zoom - 1.0).abs() < f32::EPSILON,
            "Auto fit must not retune zoom, zoom={}",
            gantt.zoom
        );
        let after = gantt.period_width(Date::new(2026, 7, 1).unwrap());
        assert!(
            (after - before).abs() < 0.5,
            "Auto fit must not change month width, {before} vs {after}"
        );
        assert_eq!(gantt.origin, Date::new(2026, 7, 4).unwrap());
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        assert!(!bars.is_empty());
        assert!(
            bars[0].x >= -1.0,
            "fitted bar must start on screen, x={} width={}",
            bars[0].x,
            bars[0].width
        );
    }

    #[test]
    fn auto_fit_month_header_keeps_the_first_full_month_label() {
        let mut item = node(1, None);
        item.start_date = Some("2026-07-01T00:00:00Z".into());
        item.target_date = Some("2026-09-30T00:00:00Z".into());
        let nodes = vec![item];
        let mut gantt = GanttView {
            origin: Date::new(2026, 11, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.auto_fit(&nodes, &[], 800.0);
        assert_eq!(gantt.origin, Date::new(2026, 6, 29).unwrap());
        let labels: Vec<String> = gantt
            .header_tick_labels(800.0)
            .into_iter()
            .map(|(_, label)| label)
            .collect();
        assert_eq!(
            labels.first().map(String::as_str),
            Some("Jul"),
            "a two-day June sliver must not hide the Jul label: {labels:?}"
        );
        assert!(
            !labels.iter().any(|label| label == "Jun"),
            "the June sliver label must give way to Jul: {labels:?}"
        );
        let title = gantt.viewport_range_title();
        assert!(
            title.starts_with("Jul"),
            "toolbar must name the first painted month, got {title:?} for {labels:?}"
        );
    }

    #[test]
    fn leftover_month_label_stays_when_the_next_tick_has_room() {
        let gantt = GanttView {
            origin: Date::new(2026, 6, 20).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let labels: Vec<String> = gantt
            .header_tick_labels(800.0)
            .into_iter()
            .map(|(_, label)| label)
            .collect();
        assert_eq!(
            &labels[..2],
            ["Jun", "Jul"],
            "an 11-day June column has room for its label: {labels:?}"
        );
        assert!(gantt.viewport_range_title().starts_with("Jun"));
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
        assert_eq!(gantt.origin, Date::new(2026, 7, 4).unwrap());
        assert!(
            (gantt.zoom - 0.4).abs() < f32::EPSILON,
            "Auto fit must keep zoom so month columns stay put, zoom={}",
            gantt.zoom
        );
        if today > latest {
            assert_ne!(
                gantt.origin, today,
                "Today outside the item span must not steal the Auto fit origin"
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
    fn both_offscreen_endpoints_skip_the_arrow() {
        let mut pred = node(1, None);
        pred.start_date = Some("2026-01-01T00:00:00Z".into());
        pred.target_date = Some("2026-01-15T00:00:00Z".into());
        let mut succ = node(2, None);
        succ.start_date = Some("2026-01-16T00:00:00Z".into());
        succ.target_date = Some("2026-01-31T00:00:00Z".into());
        succ.predecessors = vec![1];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 400.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&[pred.clone(), succ.clone()]);
        let bars = gantt.layout_bars(&[pred, succ], &rows, &[], 720.0);
        assert!(
            bars.iter()
                .all(|bar| bar_offscreen_edge(bar, gantt.timeline_width).is_some()),
            "precondition: both bars must sit fully left of July"
        );
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert!(
            arrows.is_empty(),
            "a link whose ends are both off-screen must not paint a clip-edge stub, got {arrows:?}"
        );
    }

    #[test]
    fn undated_dependency_does_not_draw_a_connector() {
        let mut pred = node(1, None);
        pred.start_date = None;
        pred.target_date = None;
        let mut succ = node(2, None);
        succ.start_date = None;
        succ.target_date = None;
        succ.predecessors = vec![1];
        let gantt = GanttView {
            origin: Date::new(2026, 11, 2).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&[pred.clone(), succ.clone()]);
        let bars = gantt.layout_bars(&[pred, succ], &rows, &[], 720.0);
        assert!(
            bars.iter().all(bar_is_gutter),
            "precondition: both ends must be gutter pills, got {bars:?}"
        );
        let arrows = gantt.layout_dependency_arrows(&bars);
        assert!(
            arrows.is_empty(),
            "a predecessor with no dates must not paint a connector, got {arrows:?}"
        );
    }

    #[test]
    fn bar_offscreen_edge_flags_fully_outside_bars() {
        let on_screen = BarLayout {
            id: 2,
            kind: GanttBarKind::Scheduled,
            x: 80.0,
            width: 120.0,
            y: 0.0,
            summary: false,
            milestone: false,
            label: String::new(),
            title: "Persist cart".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
            overflow_badge: false,
            floored: false,
        };
        assert_eq!(bar_offscreen_edge(&on_screen, 512.0), None);
        let peeking = BarLayout {
            x: -40.0,
            width: 80.0,
            ..on_screen.clone()
        };
        assert_eq!(
            bar_offscreen_edge(&peeking, 512.0),
            None,
            "a fragment still in view must not get an edge chevron"
        );
        let left = BarLayout {
            x: -200.0,
            width: 80.0,
            ..on_screen.clone()
        };
        assert_eq!(bar_offscreen_edge(&left, 512.0), Some(OffscreenEdge::Left));
        let right = BarLayout {
            x: 600.0,
            width: 80.0,
            ..on_screen
        };
        assert_eq!(
            bar_offscreen_edge(&right, 512.0),
            Some(OffscreenEdge::Right)
        );
        let marker = BarLayout {
            x: -80.0,
            width: MILESTONE_SIZE,
            marker: true,
            kind: GanttBarKind::Unscheduled,
            ..left
        };
        assert_eq!(
            bar_offscreen_edge(&marker, 512.0),
            None,
            "gutter markers must not paint a timeline-edge chevron"
        );
    }

    #[test]
    fn offscreen_edge_chip_includes_the_title() {
        let go_live = BarLayout {
            id: 1013,
            kind: GanttBarKind::Scheduled,
            x: 600.0,
            width: MILESTONE_SIZE,
            y: 160.0,
            summary: false,
            milestone: true,
            label: String::new(),
            title: "Go-live milestone".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
            overflow_badge: false,
            floored: false,
        };
        let (x, w, label, side) =
            offscreen_edge_chip(&go_live, 512.0).expect("Go-live past the clip must show a cue");
        assert_eq!(side, "right");
        assert!(
            label.contains("Go-live milestone") && label.contains('›') && label.contains('◆'),
            "Day off-screen milestone cue must show the diamond and title, not a bare ›, got {label:?}"
        );
        assert!(
            w >= 16.0 + 16.0 + 8.0 + bar_title_paint_px("Go-live milestone").min(180.0) - 0.5,
            "cue must be wide enough for the diamond and title, w={w} x={x}"
        );
        let (occ_left, occ_right) = bar_occupied_x(&go_live, false, Some(512.0), None);
        assert!(
            occ_left < 512.0 && occ_right > x && occ_right <= 512.0 + 4.0,
            "off-range occupancy must be the painted pill, not the off-clip diamond, occ={occ_left}..{occ_right} chip={x}..{}",
            x + w
        );
        assert!(
            (occ_left - (x - 2.0)).abs() < 1.0 && (occ_right - (x + w + 2.0)).abs() < 1.0,
            "occupancy must match the edge chip, occ={occ_left}..{occ_right} chip={x}..{}",
            x + w
        );
        let left = BarLayout {
            x: -200.0,
            ..go_live
        };
        let (_, _, left_label, left_side) =
            offscreen_edge_chip(&left, 512.0).expect("left-edge cue");
        assert_eq!(left_side, "left");
        assert!(
            left_label.contains("Go-live milestone")
                && left_label.contains('‹')
                && left_label.contains('◆'),
            "left-edge milestone cue must include the diamond and title, got {left_label:?}"
        );
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
            bars[0].paint_h() + 0.1 >= YEAR_MIN_BAR_H,
            "Year-view bars must keep a {YEAR_MIN_BAR_H}px body, height={}",
            bars[0].paint_h()
        );
        assert!(
            !bars[0].can_resize(),
            "a {YEAR_MIN_BAR_WIDTH}px Year bar must stay all-Move, not two grips with no body"
        );
        assert!(
            bars[0].link_handle_insets_into_bar(),
            "a dated Year sliver's last pixels must start a link"
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
    fn year_min_zoom_bars_read_as_bars_and_stay_all_move() {
        let nodes = mock_fixture_nodes();
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: GanttView::next_zoom(0.0, -1),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let bar = |id: i64| bars.iter().find(|bar| bar.id == id).unwrap();
        for id in [1002, 1003, 1005, 1012] {
            let bar = bar(id);
            assert!(
                bar.floored,
                "precondition: {} is shorter than the Year floor at min zoom",
                bar.title
            );
            assert!(
                bar.width + 0.1 >= YEAR_MIN_BAR_WIDTH && bar.width > bar.paint_h() * 1.25,
                "Year {} must read as a bar, not a dot, {}x{}",
                bar.title,
                bar.width,
                bar.paint_h()
            );
            assert!(
                !bar.can_resize() && !bar.shows_rollup_edges(),
                "Year {} edges are not its dates; it must stay all-Move",
                bar.title
            );
            let (left, right) = (bar.x, bar.x + bar.width);
            assert_eq!(
                bar.hit_zone_at_window_x(left + 1.0, left, right),
                BarHitZone::Move,
                "{}",
                bar.title
            );
        }
        let epic = bar(1001);
        assert!(
            !epic.floored && epic.width > YEAR_MIN_BAR_WIDTH,
            "a Year bar wider than the floor keeps its date width, {}",
            epic.width
        );
    }

    #[test]
    fn only_slivers_and_floored_start_only_parents_paint_thicker() {
        let parent = |width: f32, floored: bool| BarLayout {
            id: 1,
            kind: GanttBarKind::Scheduled,
            x: 100.0,
            width,
            y: 0.0,
            summary: true,
            milestone: false,
            label: String::new(),
            title: "Cart".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: true,
            overflow_badge: false,
            floored,
        };
        assert_eq!(parent(YEAR_MIN_BAR_WIDTH, true).paint_h(), YEAR_MIN_BAR_H);
        assert_eq!(parent(24.0, false).paint_h(), YEAR_MIN_BAR_H);
        assert_eq!(
            parent(32.0, false).paint_h(),
            SUMMARY_BAR_H,
            "a 32px start-only parent off Year keeps the 14px summary body"
        );
    }

    #[test]
    fn wide_dated_bar_last_pixels_start_a_link() {
        let bar = BarLayout {
            id: 1003,
            kind: GanttBarKind::Scheduled,
            x: 100.0,
            width: 168.0,
            y: 0.0,
            summary: true,
            milestone: false,
            label: "Persist cart".into(),
            title: "Persist cart".into(),
            color: 0x89b4fa,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
            overflow_badge: false,
            floored: false,
        };
        assert!(bar.can_resize(), "Month Persist cart must still resize");
        assert!(
            bar.link_handle_insets_into_bar(),
            "a wide dated bar's last pixels must start a link"
        );
        assert_eq!(
            GanttView::hit_test_bar(std::slice::from_ref(&bar), bar.x + bar.width - 3.0, 4.0)
                .map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "2–4px inside the end edge must resize"
        );
        assert_eq!(
            bar.hit_zone_at_window_x(bar.x + bar.width - 3.0, bar.x, bar.x + bar.width),
            BarHitZone::ResizeEnd,
            "2–4px inside the end edge must be ResizeEnd, not a dead Outside/Link"
        );
        assert_eq!(
            bar.hit_zone_at_window_x(bar.x + bar.width + 0.5, bar.x, bar.x + bar.width),
            BarHitZone::ResizeEnd,
            "1px past the painted end must still resize"
        );
        assert_eq!(
            bar.hit_zone_at_window_x(bar.x + bar.width + 4.0, bar.x, bar.x + bar.width),
            BarHitZone::Outside,
            "the outer link handle, 4px past the end, must not be stolen by resize"
        );
        for inset in [2.0, 4.0, 6.0] {
            assert_eq!(
                bar.hit_zone_at_window_x(bar.x + bar.width - inset, bar.x, bar.x + bar.width),
                BarHitZone::ResizeEnd,
                "{inset}px inside the painted end must be ResizeEnd, not a dead Outside/Link"
            );
        }
        assert_eq!(
            bar.hit_zone_at_window_x(bar.x + bar.width - 5.5, bar.x, bar.x + bar.width),
            BarHitZone::ResizeEnd,
            "5.5px inside the painted end is still the 6px resize halo"
        );
        assert_eq!(
            GanttView::hit_test_bar(
                std::slice::from_ref(&bar),
                bar.x + bar.width - LINK_HANDLE_INSET - 4.0,
                4.0
            )
            .map(|(_, kind)| kind),
            Some(GestureKind::ResizeEnd),
            "the inner 14px left of the sliver must still resize"
        );
    }

    #[test]
    fn short_dated_bar_keeps_link_resize_and_move() {
        let bar = BarLayout {
            id: 1003,
            kind: GanttBarKind::Scheduled,
            x: 545.0,
            width: 64.0,
            y: 0.0,
            summary: true,
            milestone: false,
            label: "Persist cart".into(),
            title: "Persist cart".into(),
            color: 0x89b4fa,
            progress: Some(0.5),
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
            overflow_badge: false,
            floored: false,
        };
        let (start, end, link) = bar.edge_zones();
        assert!(
            start >= 13.5 && end >= 13.5 && link >= 5.5,
            "a 64px bar must keep a 14px resize and ~6px link, got start={start} end={end} link={link}"
        );
        let right = bar.x + bar.width;
        assert_eq!(
            bar.hit_zone_at_window_x(right - 3.0, bar.x, right),
            BarHitZone::ResizeEnd,
            "2–4px inside the end edge must resize"
        );
        assert_eq!(
            bar.hit_zone_at_window_x(right + 0.5, bar.x, right),
            BarHitZone::ResizeEnd,
            "1px past the painted end must still resize"
        );
        assert_eq!(
            bar.hit_zone_at_window_x(right + 4.0, bar.x, right),
            BarHitZone::Outside,
            "the outer link handle, 4px past the end, must not be stolen by resize"
        );
        assert!(
            (bar.outer_link_hit() - SHORT_BAR_OUTER_LINK_HIT).abs() < 0.1,
            "a 64px bar must grow the outbound plate, got {}",
            bar.outer_link_hit()
        );
        let plate_left = bar.end_link_hit_left();
        assert!(
            plate_left >= right + RESIZE_HALO_END_OUTSIDE - 0.1,
            "the enlarged plate must start past the 1px resize halo, plate_left={plate_left} right={right}"
        );
        assert!(
            plate_left + bar.outer_link_hit() >= right + 20.0,
            "the enlarged plate must cover grabs well past the painted end"
        );
        assert_eq!(
            bar.hit_zone_at_window_x(right - 5.5, bar.x, right),
            BarHitZone::ResizeEnd,
            "5.5px inside the painted end must resize (halo covers edge−6..+1)"
        );
        assert_eq!(
            bar.hit_zone_at_window_x(right - link - end / 2.0, bar.x, right),
            BarHitZone::ResizeEnd,
            "the 14px just left of the sliver must resize"
        );
        assert_eq!(
            bar.hit_zone_at_window_x(bar.x + bar.width / 2.0, bar.x, right),
            BarHitZone::Move
        );
        for width in [16.0, 20.0, 24.0] {
            let short = BarLayout {
                width,
                ..bar.clone()
            };
            assert!(
                !short.can_resize(),
                "{width}px must stay all-Move so grips cannot eat the body"
            );
            assert!(
                !short.shows_rollup_edges(),
                "{width}px parent must not paint not-allowed rollup edges"
            );
            let right = short.x + width;
            assert_eq!(
                short.hit_zone_at_window_x(short.x + width / 2.0, short.x, right),
                BarHitZone::Move,
                "{width}px center must move"
            );
            assert_eq!(
                short.hit_zone_at_window_x(short.x + 1.0, short.x, right),
                BarHitZone::Move,
                "{width}px start edge must move, not RollupEdge"
            );
            assert_eq!(
                short.hit_zone_at_window_x(right - 1.0, short.x, right),
                BarHitZone::Move,
                "{width}px end edge must move, not RollupEdge"
            );
        }
        let resizable = BarLayout { width: 32.0, ..bar };
        assert!(resizable.can_resize(), "25px+ dated bars still resize");
        let (start, end, link) = resizable.edge_zones();
        assert!(
            start <= 32.0 / 3.0 + 0.5 && end <= 32.0 / 3.0 + 0.5,
            "a 32px bar must cap each edge at a third, got start={start} end={end} link={link}"
        );
        assert!(
            start + end + 0.5 < resizable.width,
            "short-bar zones must leave a hittable Move body, start={start} end={end} link={link}"
        );
        let right = resizable.x + resizable.width;
        assert_eq!(
            resizable.hit_zone_at_window_x(right - 1.0, resizable.x, right),
            BarHitZone::ResizeEnd,
            "the last pixels of a short dated bar are the resize halo; link from the outer handle"
        );
        assert_eq!(
            resizable.hit_zone_at_window_x(right - end / 2.0, resizable.x, right),
            BarHitZone::ResizeEnd
        );
        assert_eq!(
            resizable.hit_zone_at_window_x(resizable.x + resizable.width / 2.0, resizable.x, right),
            BarHitZone::Move,
            "the middle third of a short bar must move"
        );
    }

    #[test]
    fn short_bar_caps_edge_zones_so_the_middle_moves() {
        let bar = BarLayout {
            id: 1003,
            kind: GanttBarKind::Scheduled,
            x: 629.0,
            width: 33.0,
            y: 0.0,
            summary: true,
            milestone: false,
            label: "Persist cart".into(),
            title: "Persist cart".into(),
            color: 0x89b4fa,
            progress: Some(0.5),
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
            overflow_badge: false,
            floored: false,
        };
        assert!(bar.can_resize(), "a 33px Persist cart still resizes");
        let (start, end, link) = bar.edge_zones();
        assert!(
            start <= 33.0 / 3.0 + 0.5 && end <= 33.0 / 3.0 + 0.5,
            "each edge must be ≤ a third of 33px, start={start} end={end} link={link}"
        );
        let right = bar.x + bar.width;
        assert_eq!(
            bar.hit_zone_at_window_x(bar.x + bar.width / 2.0, bar.x, right),
            BarHitZone::Move,
            "a grab in the middle of a 33px bar must move, not resize"
        );
        assert_eq!(
            bar.hit_zone_at_window_x(right - 1.0, bar.x, right),
            BarHitZone::ResizeEnd,
            "a drag from near the right end (x≈662) must resize, not be a dead Outside/Link"
        );
        assert_eq!(
            bar.hit_zone_at_window_x(bar.x + 1.0, bar.x, right),
            BarHitZone::ResizeStart
        );
    }

    #[test]
    fn painted_end_hit_is_live_from_minus_6_to_plus_4() {
        let bar = BarLayout {
            id: 1002,
            kind: GanttBarKind::Scheduled,
            x: 100.0,
            width: 80.0,
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
            overflow_badge: false,
            floored: false,
        };
        let left = bar.x;
        let right = bar.x + bar.width;
        for offset in -6..=4 {
            let zone = bar.hit_zone_at_window_x(right + offset as f32, left, right);
            if offset <= RESIZE_HALO_END_OUTSIDE as i32 {
                assert_eq!(
                    zone,
                    BarHitZone::ResizeEnd,
                    "painted end {offset:+}px must be ResizeEnd (4px inside is not a dead zone), got {zone:?}"
                );
            } else {
                assert_ne!(
                    zone,
                    BarHitZone::ResizeEnd,
                    "the outer link handle, {offset:+}px past the end, must not be stolen by resize, got {zone:?}"
                );
            }
        }
        for offset in -6..=4 {
            let zone = bar.hit_zone_at_window_x(left + offset as f32, left, right);
            if offset >= -(RESIZE_HALO_OUTSIDE as i32) && offset <= RESIZE_HALO_INSIDE as i32 {
                assert_eq!(
                    zone,
                    BarHitZone::ResizeStart,
                    "painted start {offset:+}px must be ResizeStart, got {zone:?}"
                );
            } else if offset < 0 {
                assert_ne!(
                    zone,
                    BarHitZone::ResizeStart,
                    "start halo must not steal the inbound link handle at {offset:+}px, got {zone:?}"
                );
            }
        }
    }

    #[test]
    fn start_halo_does_not_steal_inbound_link_hits() {
        let bar = BarLayout {
            id: 1002,
            kind: GanttBarKind::Scheduled,
            x: 100.0,
            width: 80.0,
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
            overflow_badge: false,
            floored: false,
        };
        let left = bar.x;
        let right = bar.x + bar.width;
        let connector = left - (LINK_HANDLE_GAP + LINK_HANDLE / 2.0);
        assert_ne!(
            bar.hit_zone_at_window_x(connector, left, right),
            BarHitZone::ResizeStart,
            "the inbound connector at bar.x−10 must not be ResizeStart"
        );
        assert_ne!(
            bar.hit_zone_at_window_x(left - 6.0, left, right),
            BarHitZone::ResizeStart,
            "6px left of the bar is the inbound handle plate, not the start halo"
        );
        assert_eq!(
            bar.hit_zone_at_window_x(left - RESIZE_HALO_OUTSIDE, left, right),
            BarHitZone::ResizeStart,
            "the 1px start halo must still resize"
        );
        assert_eq!(
            bar.hit_zone_at_window_x(left + 4.0, left, right),
            BarHitZone::ResizeStart,
            "4px inside the painted start must still resize"
        );
    }

    #[test]
    fn bar_title_fit_subtracts_the_end_link_inset() {
        let bar = BarLayout {
            id: 1003,
            kind: GanttBarKind::Scheduled,
            x: 100.0,
            width: 100.0,
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
            overflow_badge: false,
            floored: false,
        };
        assert!(bar.can_resize());
        assert!(bar.resize_end_link_inset() > 0.0);
        // 100 - 2*8 grip - 8 pad = 76, which clears BAR_LABEL_INSIDE_MIN.
        // Paint also insets the last 6px link sliver, so the true inner is 70.
        assert!(
            !bar_title_is_inside(&bar),
            "title fit must count the 6px link inset, not claim Cart fits in 70px"
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
            overflow_badge: false,
            floored: false,
        };
        assert!(
            !bar.link_handle_insets_into_bar(),
            "a wide start-only bar must not overlay an inset link handle"
        );
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
            overflow_badge: false,
            floored: false,
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
        assert!(
            (gantt.zoom - 0.4).abs() < f32::EPSILON,
            "Auto fit must not reset zoom when there is nothing to fit, zoom={}",
            gantt.zoom
        );
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
    fn link_gesture_seeds_live_y_on_the_lane_under_the_bar() {
        let nodes = crate::state::fake_hierarchy(3);
        let mut gantt = GanttView::default();
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let bar = bars
            .iter()
            .find(|bar| !bar.milestone && !bar.summary && bar.width > 8.0)
            .expect("dated leaf");
        gantt
            .begin_gesture(bar, GestureKind::Link, bar.x + bar.width, &nodes, &[])
            .expect("link");
        let live_y = gantt.gesture.as_ref().unwrap().live_y;
        let lane = bar.link_preview_lane_y();
        assert!(
            (live_y - lane).abs() < 0.5,
            "link live_y {live_y} must start on the preview lane {lane}, not row center {}",
            bar.y + ROW_HEIGHT / 2.0
        );
        let bar_bottom = bar.y + bar.paint_top() + bar.paint_h();
        assert!(
            live_y >= bar_bottom + 2.0,
            "lane must sit below the painted bar, live_y={live_y} bar_bottom={bar_bottom}"
        );
        assert!(
            (bar.paint_mid_y() - (bar.y + ROW_HEIGHT / 2.0)).abs() < 0.5,
            "paint_mid_y should match the connector"
        );
    }

    #[test]
    fn start_handle_link_marks_the_source_as_successor() {
        let nodes = crate::state::fake_hierarchy(3);
        let mut gantt = GanttView::default();
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let source = bars
            .iter()
            .find(|bar| !bar.milestone && !bar.summary && bar.width > 8.0)
            .expect("dated leaf");
        gantt
            .begin_gesture(
                source,
                GestureKind::Link,
                source.start_link_origin(),
                &nodes,
                &[],
            )
            .expect("start-handle link");
        let gesture = gantt.gesture.as_ref().expect("gesture");
        assert!(
            gesture.link_from_start,
            "a press on the inbound connector must flag link_from_start"
        );
        let target_id = bars
            .iter()
            .map(|bar| bar.id)
            .find(|id| *id != source.id)
            .expect("another row");
        gantt.set_link_target(target_id);
        assert_eq!(
            gantt.take_link_gesture(),
            Some((source.id, Some(target_id), true)),
            "start-handle drop keeps this bar as source so commit can swap FS ends"
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
    fn step_period_is_a_quarter_of_the_visible_span() {
        for scale in GanttScale::ALL {
            let mut gantt = GanttView {
                origin: Date::new(2026, 8, 1).unwrap(),
                scale,
                zoom: 1.0,
                timeline_width: 800.0,
                ..Default::default()
            };
            let viewport_days = 800.0 / scale.pixels_per_day(1.0);
            let expected = (viewport_days * 0.25).round().max(1.0) as i32;
            assert_eq!(
                gantt.pan_step_days(),
                expected,
                "{scale:?} pan must be ¼ of the visible span, viewport_days={viewport_days}"
            );
            let before = gantt.origin;
            gantt.step_period(-1);
            assert_eq!(
                gantt.origin,
                before.saturating_sub(expected.days()),
                "{scale:?} ‹ must pan {expected} days, {} vs {}",
                gantt.origin,
                before
            );
            gantt.step_period(1);
            assert_eq!(
                gantt.origin, before,
                "{scale:?} a matching › must restore the origin"
            );
        }
    }

    #[test]
    fn every_pan_step_moves_the_range_title_with_the_view() {
        for scale in [GanttScale::Month, GanttScale::Quarter, GanttScale::Year] {
            for direction in [1, -1] {
                let mut gantt = GanttView {
                    origin: Date::new(2026, 8, 1).unwrap(),
                    scale,
                    zoom: 1.0,
                    timeline_width: 512.0,
                    ..Default::default()
                };
                let mut title = gantt.viewport_range_title();
                for step in 0..8 {
                    gantt.retarget_year_gutter(|gantt| gantt.step_period(direction));
                    let next = gantt.viewport_range_title();
                    assert_ne!(
                        next, title,
                        "{scale:?} step {step} ({direction:+}) must update the title"
                    );
                    let end = gantt.date_for_x(511.0);
                    assert!(
                        next.ends_with(&format!("{}, {}", compact_date(end), end.year())),
                        "{scale:?} title {next:?} must end on the last visible day {end}"
                    );
                    title = next;
                }
            }
        }
    }

    #[test]
    fn dated_span_names_years_once_unless_crossed() {
        let d = |y, m, day| Date::new(y, m, day).unwrap();
        assert_eq!(
            dated_span(d(2026, 8, 12), d(2026, 9, 24)),
            "Aug 12 – Sep 24, 2026"
        );
        assert_eq!(
            dated_span(d(2026, 8, 3), d(2026, 8, 24)),
            "Aug 3 – 24, 2026"
        );
        assert_eq!(
            dated_span(d(2026, 12, 4), d(2027, 2, 8)),
            "Dec 4, 2026 – Feb 8, 2027"
        );
        assert_eq!(dated_span(d(2026, 8, 3), d(2026, 8, 3)), "Aug 3, 2026");
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
        assert_eq!(gantt.take_link_gesture(), Some((1, None, false)));
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
        let gap = pred.y.min(succ.y) + pred.paint_top() + pred.paint_h();
        assert!(
            arrows[0].lane_y >= gap && arrows[0].lane_y < pred.y.min(succ.y) + ROW_HEIGHT,
            "horizontal run must sit in the row gap below the predecessor bar, got lane {} gap {} row {}",
            arrows[0].lane_y,
            gap,
            pred.y.min(succ.y)
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
        assert!(
            dx >= PARALLEL_VERT_MIN,
            "shared-source Persist cart jogs must sit ≥{PARALLEL_VERT_MIN}px apart, dx={dx} x1={},{} lane={},{}",
            arrows[0].x1,
            arrows[1].x1,
            arrows[0].lane_y,
            arrows[1].lane_y
        );
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let live = arrows.iter().find(|arrow| arrow.to_id == 1013).unwrap();
        if let Some((left, right)) = bar_outside_title_span(pred) {
            for arrow in &arrows {
                if arrow.x_head <= right + SOURCE_LABEL_CLEARANCE {
                    continue;
                }
                assert!(
                    !x_hits_span(arrow.x1, left, right),
                    "outgoing vertical must miss the Persist cart label, x1={} label={left}..{right} to={}",
                    arrow.x1,
                    arrow.to_id
                );
                assert!(
                    arrow.x1 >= right + 6.0,
                    "long Persist cart outgoing must sit ≥6px past the label, x1={} label_right={right} to={}",
                    arrow.x1,
                    arrow.to_id
                );
            }
            assert!(
                live.x1 >= right + 6.0,
                "Persist cart → Go-live must sit ≥6px past the Persist cart label, x1={} label={left}..{right}",
                live.x1
            );
        }
        let go_live = bars.iter().find(|bar| bar.id == 1013).unwrap();
        assert!(
            (live.x_head - go_live.x).abs() < 1.0,
            "Go-live arrowhead must sit on the diamond, x_head={} diamond={}",
            live.x_head,
            go_live.x
        );
        if let Some((left, right)) = bar_outside_title_span(pred) {
            assert!(
                live.x1 >= right + PARALLEL_VERT_MIN - 1.0
                    || live.x1.max(live.x2) < left
                    || live.lane_y
                        >= pred.y
                            + ((ROW_HEIGHT - OUTSIDE_LABEL_H) / 2.0).max(2.0)
                            + OUTSIDE_LABEL_H
                            + PARALLEL_LANE_MIN
                            - 1.0,
                "Persist cart → Go-live must clear the source title, x1={} label={left}..{right} lane={} pred.y={}",
                live.x1,
                live.lane_y,
                pred.y
            );
        }
    }

    #[test]
    fn dependency_hover_tooltip_names_both_ends() {
        assert_eq!(
            dependency_link_tooltip("Persist cart", "Go-live milestone"),
            "Persist cart → Go-live milestone, finish-to-start"
        );
    }

    #[test]
    fn month_shared_source_persist_jogs_stay_sixteen_apart() {
        let mut nodes = mock_fixture_nodes();
        if let Some(spike) = nodes.iter_mut().find(|node| node.id == 1012) {
            spike.predecessors = vec![1003];
        }
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            timeline_width: 512.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let persist_spike = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
            .expect("Persist cart → spike");
        let persist_live = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("Persist cart → Go-live");
        let dx = (persist_spike.x1 - persist_live.x1).abs();
        assert!(
            dx >= PARALLEL_VERT_MIN,
            "Month 512 shared-source jogs must sit ≥{PARALLEL_VERT_MIN}px apart, dx={dx} spike x1={} live x1={}",
            persist_spike.x1,
            persist_live.x1
        );
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        if let Some((left, right)) = bar_outside_title_span(pred) {
            assert!(
                !x_hits_span(persist_live.x1, left, right),
                "Persist cart → Go-live must not cross the Persist cart label, x1={} label={left}..{right}",
                persist_live.x1
            );
            assert!(
                persist_live.x1 >= right + 6.0,
                "Persist cart → Go-live must sit ≥6px past the Persist cart label, x1={} label={left}..{right}",
                persist_live.x1
            );
        }
        let go_live = bars.iter().find(|bar| bar.id == 1013).unwrap();
        assert!(
            (persist_live.x_head - go_live.x).abs() < 1.0,
            "Go-live ▶ must sit on the diamond, x_head={} diamond={}",
            persist_live.x_head,
            go_live.x
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
        for xa in [a.x1, a.x2, a.stub_start()] {
            for xb in [b.x1, b.x2, b.stub_start()] {
                min_dx = min_dx.min((xa - xb).abs());
            }
        }
        min_dx
    }

    #[test]
    fn clipped_edge_verticals_stay_sixteen_apart() {
        let a = DependencyArrow {
            from_id: 1003,
            to_id: 1013,
            x1: 200.0,
            y1: 20.0,
            x2: 600.0,
            y2: 180.0,
            x_head: 603.0,
            lane_y: 39.0,
            inbound_right: false,
        };
        let b = DependencyArrow {
            from_id: 1005,
            to_id: 1012,
            x1: 603.0,
            y1: 60.0,
            x2: 603.0,
            y2: 220.0,
            x_head: 480.0,
            lane_y: 79.0,
            inbound_right: false,
        };
        let mut arrows = vec![a, b];
        separate_parallel_verticals(&mut arrows, 0.0, 603.0);
        let dx = closest_vertical_gap(&arrows[0], &arrows[1]);
        assert!(
            dx >= PARALLEL_VERT_MIN,
            "clip-edge 600/603 stack must separate, dx={dx} persist x2={} spike x2={}",
            arrows[0].x2,
            arrows[1].x2
        );
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

    /// `e2e/fixtures/work-items.json` topology: Persist cart → spike already
    /// exists; Persist cart → Go-live is the runtime handle-drop.
    fn live_fixture_nodes() -> Vec<WorkItemNode> {
        let mut nodes = mock_fixture_nodes();
        if let Some(spike) = nodes.iter_mut().find(|node| node.id == 1012) {
            spike.predecessors = vec![1003];
        }
        if let Some(go_live) = nodes.iter_mut().find(|node| node.id == 1013) {
            go_live.predecessors = vec![1003];
        }
        nodes
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
            dx >= PARALLEL_VERT_MIN,
            "{:?}: Persist cart → Go-live vertical must sit ≥{PARALLEL_VERT_MIN}px from Cart UI → spike, dx={dx} persist x1={} x2={} spike x1={} x2={} lanes={},{}",
            scale,
            persist_live.x1,
            persist_live.x2,
            cart_spike.x1,
            cart_spike.x2,
            persist_live.lane_y,
            cart_spike.lane_y
        );
        let cart = bars.iter().find(|bar| bar.id == 1005).unwrap();
        if persist_live.x_head <= timeline_width {
            assert_inbound_drops_before_intervening_bar(persist_live, cart, timeline_width);
        }
        let cue_bottom = cart.y + cart.paint_top() + cart.paint_h();
        assert!(
            persist_live.lane_y < cart.y || persist_live.lane_y >= cart.y + ROW_HEIGHT,
            "{scale:?}: Persist cart → Go-live must not ride the Cart UI row, lane {} cart {}",
            persist_live.lane_y,
            cart.y
        );
        if let Some(go_live) = bars.iter().find(|bar| bar.id == 1013) {
            assert!(
                persist_live.lane_y < go_live.y || persist_live.lane_y >= go_live.y + ROW_HEIGHT,
                "{scale:?}: Persist cart → Go-live must not run through the Go-live row, lane {} go-live {}",
                persist_live.lane_y,
                go_live.y
            );
        }
        assert!(
            cart_spike.lane_y >= cue_bottom && cart_spike.lane_y <= cart.y + ROW_HEIGHT,
            "{scale:?}: Cart UI → spike must sit in the gap below the Cart UI cue, lane {} cue_bottom {} row {}",
            cart_spike.lane_y,
            cue_bottom,
            cart.y
        );
    }

    #[test]
    fn mock_fixture_parallel_verticals_are_at_least_eight_apart() {
        for width in [512.0, 800.0, 1200.0] {
            assert_mock_fixture_parallel_verticals(GanttScale::Month, width);
            assert_mock_fixture_parallel_verticals(GanttScale::Week, width);
        }
        assert_mock_fixture_parallel_verticals(GanttScale::Day, 2400.0);
    }

    #[test]
    fn live_fixture_shared_source_inbounds_stay_eight_apart() {
        let nodes = live_fixture_nodes();
        for width in [512.0, 800.0, 1200.0] {
            let gantt = GanttView {
                origin: Date::new(2026, 7, 1).unwrap(),
                scale: GanttScale::Month,
                zoom: 1.0,
                timeline_width: width,
                ..Default::default()
            };
            let rows = gantt.flatten(&nodes);
            let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
            let arrows = gantt.layout_dependency_arrows(&bars);
            let persist_spike = arrows
                .iter()
                .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
                .expect("Persist cart → spike");
            let persist_live = arrows
                .iter()
                .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
                .expect("Persist cart → Go-live");
            let dx = closest_vertical_gap(persist_spike, persist_live);
            assert!(
                dx >= PARALLEL_VERT_MIN,
                "Month {width}: shared-source verticals must sit ≥{PARALLEL_VERT_MIN}px apart, dx={dx} spike x1={} x2={} live x1={} x2={}",
                persist_spike.x1,
                persist_spike.x2,
                persist_live.x1,
                persist_live.x2
            );
            let cart = bars.iter().find(|bar| bar.id == 1005).unwrap();
            assert_inbound_drops_before_intervening_bar(persist_live, cart, width);
            assert!(
                persist_live.x2 < cart.x,
                "Month {width}: Persist cart → Go-live must still drop before Cart UI, x2={} cart.x={}",
                persist_live.x2,
                cart.x
            );
        }
    }

    #[test]
    fn runtime_created_shared_source_persist_jogs_stay_sixteen_apart() {
        let mut nodes = mock_fixture_nodes();
        if let Some(spike) = nodes.iter_mut().find(|node| node.id == 1012) {
            spike.predecessors = vec![1003];
        }
        if let Some(go_live) = nodes.iter_mut().find(|node| node.id == 1013) {
            go_live.predecessors.clear();
        }
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 0.7,
            timeline_width: 512.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let before = gantt.layout_dependency_arrows(&bars);
        assert!(
            before
                .iter()
                .any(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012),
            "live fixture: Persist cart → spike already exists"
        );
        assert!(
            before
                .iter()
                .all(|arrow| !(arrow.from_id == 1003 && arrow.to_id == 1013)),
            "Persist cart → Go-live is created next, like a handle drop"
        );
        if let Some(go_live) = nodes.iter_mut().find(|node| node.id == 1013) {
            go_live.predecessors = vec![1003];
        }
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let persist_spike = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
            .expect("Persist cart → spike");
        let persist_live = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("runtime Persist cart → Go-live");
        let dx = (persist_spike.x1 - persist_live.x1).abs();
        assert!(
            dx >= PARALLEL_VERT_MIN,
            "runtime shared-source jogs must sit ≥{PARALLEL_VERT_MIN}px apart, dx={dx} spike x1={} live x1={}",
            persist_spike.x1,
            persist_live.x1
        );
        let inbound_dx = closest_vertical_gap(persist_spike, persist_live);
        assert!(
            inbound_dx >= PARALLEL_VERT_MIN,
            "runtime shared-source inbounds must sit ≥{PARALLEL_VERT_MIN}px apart, dx={inbound_dx} spike x2={} live x2={}",
            persist_spike.x2,
            persist_live.x2
        );
        let cart_ui = bars.iter().find(|bar| bar.id == 1005).unwrap();
        assert_inbound_drops_before_intervening_bar(persist_live, cart_ui, gantt.timeline_width);
        assert_inbound_drops_before_intervening_bar(persist_spike, cart_ui, gantt.timeline_width);
        assert!(
            persist_spike.x2 < cart_ui.x,
            "runtime Persist cart → spike must not jump onto Cart UI's left edge, x2={} cart.x={}",
            persist_spike.x2,
            cart_ui.x
        );
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let (left, right) = bar_outside_title_span(pred)
            .expect("zoomed-out Month Persist cart must paint an outside title");
        for arrow in [persist_spike, persist_live] {
            if arrow.x_head <= pred.x + pred.width.max(8.0) + 12.0 {
                continue;
            }
            assert!(
                !x_hits_span(arrow.x1, left, right),
                "outgoing vertical must miss the Persist cart label, x1={} label={left}..{right} to={}",
                arrow.x1,
                arrow.to_id
            );
            assert!(
                arrow.x1 + 0.5 >= right + 6.0 || arrow.x1 <= left - 6.0 + 0.5,
                "outgoing must sit ≥6px off the Persist cart label (a left-of-bar jog is ok), x1={} label={left}..{right} to={}",
                arrow.x1,
                arrow.to_id
            );
        }
        assert!(
            persist_live.x1 + 0.5 >= right + 6.0 || persist_live.x1 <= left - 6.0 + 0.5,
            "runtime Persist cart → Go-live must sit ≥6px off the Persist cart label, x1={} label={left}..{right}",
            persist_live.x1
        );
    }

    #[test]
    fn auto_fit_month_runtime_persist_go_live_stays_eight_apart() {
        let mut nodes = mock_fixture_nodes();
        if let Some(spike) = nodes.iter_mut().find(|node| node.id == 1012) {
            spike.predecessors = vec![1003];
        }
        if let Some(go_live) = nodes.iter_mut().find(|node| node.id == 1013) {
            go_live.predecessors.clear();
        }
        let mut gantt = GanttView {
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.auto_fit(&nodes, &[], 800.0);
        gantt.timeline_width = 512.0;
        if let Some(go_live) = nodes.iter_mut().find(|node| node.id == 1013) {
            go_live.predecessors = vec![1003];
        }
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let persist_spike = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
            .expect("Persist cart → spike");
        let persist_live = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("runtime Persist cart → Go-live");
        let inbound_dx = (persist_spike.x2 - persist_live.x2).abs();
        assert!(
            inbound_dx >= 8.0,
            "auto-fit Month inbounds must sit ≥8px apart, dx={inbound_dx} spike x2={} live x2={}",
            persist_spike.x2,
            persist_live.x2
        );
        let outgoing_dx = (persist_spike.x1 - persist_live.x1).abs();
        assert!(
            outgoing_dx >= PARALLEL_VERT_MIN,
            "auto-fit Month outgoings must sit ≥{PARALLEL_VERT_MIN}px apart after dodge, dx={outgoing_dx} spike x1={} live x1={}",
            persist_spike.x1,
            persist_live.x1
        );
        let cart = bars.iter().find(|bar| bar.id == 1005).unwrap();
        assert_inbound_drops_before_intervening_bar(persist_live, cart, gantt.timeline_width);
        assert_inbound_drops_before_intervening_bar(persist_spike, cart, gantt.timeline_width);
        assert!(
            persist_spike.x2 < cart.x,
            "auto-fit Month Persist cart → spike must not jump onto Cart UI's left edge, x2={} cart.x={}",
            persist_spike.x2,
            cart.x
        );
        assert!(
            persist_live.x2 < cart.x,
            "auto-fit Month Persist cart → Go-live must drop before Cart UI, x2={} cart.x={}",
            persist_live.x2,
            cart.x
        );
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        if let Some((left, right)) = bar_outside_title_span(pred) {
            assert!(
                persist_live.x1 + 0.5 >= right + 6.0 || persist_live.x1 <= left - 6.0 + 0.5,
                "auto-fit Month first stub must sit ≥6px from the Persist cart label, x1={} label={left}..{right}",
                persist_live.x1
            );
        }
    }

    #[test]
    fn year_toolbar_today_gaps_go_live_spike_and_cart_ui() {
        let nodes = mock_fixture_nodes();
        let mut gantt = GanttView {
            timeline_width: 512.0,
            ..Default::default()
        };
        gantt.auto_fit(&nodes, &[], 800.0);
        gantt.timeline_width = 512.0;
        gantt.retarget_year_gutter(|gantt| {
            gantt.set_scale_from_toolbar(GanttScale::Year, jiff::Zoned::now().date());
        });
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let today = jiff::Zoned::now().date();
        let today_px = gantt.x_for_date(today);
        let Some(today_x) =
            (today_px >= 0.0 && today_px <= gantt.timeline_width).then_some(today_px)
        else {
            return;
        };
        let skips = today_line_chip_skips(&bars, today_x, gantt.timeline_width, GanttScale::Year);
        assert!(
            skips.is_empty(),
            "Year Today stays a continuous stroke behind labels, skips={skips:?}"
        );
    }

    #[test]
    fn year_persist_go_live_outgoing_stays_off_the_source_title() {
        let nodes = mock_fixture_nodes();
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let go_live = bars.iter().find(|bar| bar.id == 1013).unwrap();
        let live = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("Year Persist cart → Go-live");
        let (left, right) =
            bar_outside_title_span(pred).expect("Year Persist cart paints an outside title");
        assert!(
            go_live.x < right + SOURCE_LABEL_CLEARANCE,
            "fixture: Go-live must sit under the Persist cart title so wrap-limit cannot clear it, diamond={} label={left}..{right}",
            go_live.x
        );
        assert!(
            !x_hits_span(live.x1, left, right),
            "Year Persist cart → Go-live must not park its outgoing vertical on the title, x1={} label={left}..{right} x_head={}",
            live.x1,
            live.x_head
        );
        assert!(
            live.x1 <= left - SOURCE_LABEL_CLEARANCE + 0.5,
            "when the successor sits under the title, the stub must sit ≥{SOURCE_LABEL_CLEARANCE}px left of the letters, x1={} label_left={left}",
            live.x1
        );
        assert!(
            (live.x_head - go_live.x).abs() < 1.0,
            "Go-live ▶ must stay on the diamond, x_head={} diamond={}",
            live.x_head,
            go_live.x
        );
    }

    #[test]
    fn year_persist_go_live_lane_stays_in_the_source_row() {
        let nodes = mock_fixture_nodes();
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let cart_ui = bars.iter().find(|bar| bar.id == 1005).unwrap();
        let live = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("Year Persist cart → Go-live");
        assert!(
            (cart_ui.y - (pred.y + ROW_HEIGHT)).abs() < 0.5,
            "fixture: Cart UI is the next row under Persist cart, persist.y={} cart.y={}",
            pred.y,
            cart_ui.y
        );
        assert!(
            live.lane_y >= pred.y && live.lane_y < pred.y + ROW_HEIGHT - 1.0,
            "outside-title Persist cart → Go-live must stay in the Persist cart row, lane={} persist={}..{}",
            live.lane_y,
            pred.y,
            pred.y + ROW_HEIGHT
        );
        assert!(
            live.lane_y < cart_ui.y,
            "must not ride Cart UI, lane={} cart.y={}",
            live.lane_y,
            cart_ui.y
        );
        let (gap_top, gap_bottom) = source_row_gap(pred);
        let min_lane = (gap_top + 6.0).min(gap_bottom - 3.0);
        let max_lane = (gap_bottom - 3.0).max(min_lane);
        let mid = (min_lane + max_lane) / 2.0;
        assert!(
            (live.lane_y - mid).abs() < 1.0,
            "horizontal must sit in the middle of the row gap, lane={} mid={} gap={gap_top}..{gap_bottom}",
            live.lane_y,
            mid
        );
        let today = jiff::Zoned::now().date();
        let today_px = gantt.x_for_date(today);
        let today_x = (today_px >= 0.0 && today_px <= gantt.timeline_width).then_some(today_px);
        let (body_left, body_right) = bar_occupied_x(cart_ui, false, None, None);
        assert!(
            !x_hits_span(live.x2, body_left, body_right),
            "inbound vertical must miss Cart UI's bar, x2={} body={body_left}..{body_right}",
            live.x2
        );
        if let Some((label_left, label_right)) =
            bar_outside_title_span_clipped(cart_ui, gantt.timeline_width, today_x)
        {
            assert!(
                !x_hits_span(live.x2, label_left, label_right),
                "inbound vertical must miss Cart UI's beside-bar label, x2={} label={label_left}..{label_right}",
                live.x2
            );
        }
        let label_top = pred.y + ((ROW_HEIGHT - OUTSIDE_LABEL_H) / 2.0).max(2.0);
        assert!(
            live.lane_y + 1.0 >= label_top + OUTSIDE_LABEL_H + 6.0,
            "horizontal must still sit ≥6px below the Persist cart chip, lane={} chip_bottom={}",
            live.lane_y,
            label_top + OUTSIDE_LABEL_H
        );
    }

    #[test]
    fn year_shared_source_outside_title_lanes_stay_apart() {
        let mut nodes = mock_fixture_nodes();
        if let Some(spike) = nodes.iter_mut().find(|node| node.id == 1012) {
            spike.predecessors = vec![1003];
        }
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        assert!(
            !bar_title_is_inside(pred),
            "precondition: Year Persist cart paints an outside title"
        );
        let spike = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
            .expect("Year Persist cart → spike");
        let live = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("Year Persist cart → Go-live");
        let label_top = pred.y + ((ROW_HEIGHT - OUTSIDE_LABEL_H) / 2.0).max(2.0);
        let min_lane = label_top + OUTSIDE_LABEL_H + 6.0;
        let in_row = pred.y + ROW_HEIGHT - 2.0 - 1.0;
        for arrow in [spike, live] {
            assert!(
                arrow.lane_y + 0.5 >= min_lane && arrow.lane_y <= in_row + 0.5,
                "shared-source lane must sit ≥6px below the chip and in-row, to={} lane={} band={min_lane}..{in_row}",
                arrow.to_id,
                arrow.lane_y
            );
        }
        let dy = (spike.lane_y - live.lane_y).abs();
        assert!(
            dy >= 2.0,
            "clamp must not collapse both Persist cart outgoings onto one stroke, dy={dy} spike={} live={}",
            spike.lane_y,
            live.lane_y
        );
    }

    #[test]
    fn year_shared_source_inbounds_miss_cart_ui_sliver() {
        let nodes = live_fixture_nodes();
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let cart_ui = bars.iter().find(|bar| bar.id == 1005).unwrap();
        let (body_left, body_right) = bar_occupied_x(cart_ui, false, None, None);
        let spike = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
            .expect("Year Persist cart → spike");
        let live = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("Year Persist cart → Go-live");
        assert!(
            !x_hits_span(live.x2, body_left, body_right),
            "Year Persist cart → Go-live must miss Cart UI's sliver after the inbound fan, x2={} body={body_left}..{body_right}",
            live.x2
        );
        let dx = (spike.x2 - live.x2).abs();
        assert!(
            dx >= 8.0,
            "Year shared-source inbounds must stay ≥8px apart after the sliver nudge, dx={dx} spike x2={} live x2={}",
            spike.x2,
            live.x2
        );
    }

    #[test]
    fn year_shared_source_inbounds_do_not_restack_on_past_body() {
        // Live Year Cart UI is a start-only sliver; spike's head sits on it so
        // that inbound cannot use `past_body`. Park both successors well past
        // the sliver (and past each other's occupancy) so extra-nudge restack
        // is the path under test.
        let mut nodes = live_fixture_nodes();
        if let Some(go_live) = nodes.iter_mut().find(|node| node.id == 1013) {
            go_live.start_date = Some("2026-09-01T00:00:00Z".into());
            go_live.target_date = Some("2026-09-14T00:00:00Z".into());
        }
        if let Some(spike) = nodes.iter_mut().find(|node| node.id == 1012) {
            spike.start_date = Some("2026-09-15T00:00:00Z".into());
            spike.target_date = Some("2026-09-28T00:00:00Z".into());
        }
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let cart_ui = bars.iter().find(|bar| bar.id == 1005).unwrap();
        let (body_left, body_right) = bar_occupied_x(cart_ui, false, None, None);
        let live = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("Persist cart → Go-live");
        let spike = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
            .expect("Persist cart → spike");
        assert!(
            !x_hits_span(live.x2, body_left, body_right),
            "Go-live inbound must miss Cart UI's sliver, x2={} body={body_left}..{body_right}",
            live.x2
        );
        assert!(
            !x_hits_span(spike.x2, body_left, body_right),
            "spike inbound must miss Cart UI's sliver after the post-fan nudge, x2={} body={body_left}..{body_right}",
            spike.x2
        );
        let dx = (live.x2 - spike.x2).abs();
        assert!(
            dx >= 8.0,
            "Year inbounds that both clear a sliver must not restack onto one past_body x, dx={dx} live x2={} spike x2={}",
            live.x2,
            spike.x2
        );
    }

    #[test]
    fn occupancy_skips_gutter_pills() {
        let pill = BarLayout {
            id: 1014,
            kind: GanttBarKind::NoFields,
            x: 10.0,
            width: 80.0,
            y: 0.0,
            summary: false,
            milestone: false,
            label: String::new(),
            title: "Backlog grooming".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
            overflow_badge: false,
            floored: false,
        };
        assert!(
            bar_outside_title_span_clipped(&pill, 800.0, None).is_none(),
            "No-dates pills never paint an outside title, so arrows must not dodge a phantom chip"
        );
        assert!(
            bar_outside_label_frame(&pill, 800.0, true, None, false).is_none(),
            "gutter pills must not get an outside overlay frame"
        );
    }

    #[test]
    fn milestone_label_clears_the_handle_dot() {
        let gap = outside_title_right_gap(true, true, false);
        assert!(
            gap >= LINK_HANDLE_GAP + LINK_HANDLE + 2.0,
            "Go-live title must start past the handle dot, gap={gap}"
        );
        assert!(
            (gap - MILESTONE_LABEL_GAP).abs() < 0.1,
            "milestone gap must match MILESTONE_LABEL_GAP, got {gap}"
        );
    }

    #[test]
    fn link_plates_keep_eight_px_from_the_outside_title() {
        let bar = |milestone: bool, x: f32, width: f32| BarLayout {
            id: 1013,
            kind: GanttBarKind::Scheduled,
            x,
            width,
            y: 0.0,
            summary: false,
            milestone,
            label: String::new(),
            title: "Go-live milestone".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
            overflow_badge: false,
            floored: false,
        };
        let go_live = bar(true, 400.0, MILESTONE_SIZE);
        let (label_x, _) =
            bar_outside_label_frame(&go_live, 1200.0, true, None, false).expect("right title");
        let plate_right = go_live.end_link_hit_left() + go_live.outer_link_hit();
        assert!(
            label_x - plate_right >= HANDLE_TITLE_CLEARANCE - 0.1,
            "Go-live outbound plate must sit ≥8px left of its title, plate_right={plate_right} label_x={label_x}"
        );
        // Today just past the diamond flips the title left of the inbound plate.
        let today = go_live.x + MILESTONE_SIZE + 40.0;
        let (label_x, w) = bar_outside_label_frame(&go_live, 1200.0, true, Some(today), false)
            .expect("flipped title");
        let plate_left = go_live.start_link_origin() - LINK_HANDLE_HIT / 2.0;
        assert!(
            label_x + w <= go_live.x,
            "precondition: Today must flip the title left, label={label_x}+{w}"
        );
        assert!(
            plate_left - (label_x + w) >= HANDLE_TITLE_CLEARANCE - 0.1,
            "Go-live inbound plate must sit ≥8px right of a flipped title, plate_left={plate_left} label_right={}",
            label_x + w
        );
        let short = bar(false, 400.0, 30.0);
        let (label_x, w) = bar_outside_label_frame(&short, 1200.0, true, Some(520.0), false)
            .expect("flipped bar title");
        let plate_left = short.start_link_origin() - LINK_HANDLE_HIT / 2.0;
        assert!(
            label_x + w <= short.x && plate_left - (label_x + w) >= HANDLE_TITLE_CLEARANCE - 0.1,
            "a flipped bar title must sit ≥8px left of the inbound plate, plate_left={plate_left} label_right={}",
            label_x + w
        );
    }

    #[test]
    fn occupancy_matches_a_today_flipped_chip() {
        let bar = BarLayout {
            id: 1003,
            kind: GanttBarKind::Scheduled,
            x: 300.0,
            width: 24.0,
            y: 80.0,
            summary: true,
            milestone: false,
            label: String::new(),
            title: "Persist cart".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
            overflow_badge: false,
            floored: false,
        };
        let estimated = bar_title_paint_px(&bar.title);
        let right_x = bar.x + bar.width.max(8.0) + LINK_HANDLE_GAP + LINK_HANDLE_HIT + 20.0;
        let today_x = right_x + 20.0;
        assert!(
            today_x - right_x < estimated,
            "precondition: full title cannot fit before Today"
        );
        let (paint_x, paint_w) = bar_outside_label_frame(&bar, 800.0, true, Some(today_x), false)
            .expect("Today on the right must keep a full Persist cart chip");
        let (occ_left, occ_right) = bar_outside_title_span_clipped(&bar, 800.0, Some(today_x))
            .expect("occupancy must see the painted chip");
        assert!(
            (occ_left - paint_x).abs() < 0.5 && (occ_right - (paint_x + paint_w)).abs() < 0.5,
            "dodge occupancy must match the painted frame, paint={paint_x}..{} occ={occ_left}..{occ_right}",
            paint_x + paint_w
        );
        assert!(
            paint_x + paint_w <= bar.x + 0.1,
            "fixture: Today must flip the chip left of the bar, paint_x={paint_x} bar.x={}",
            bar.x
        );
        let (occ_left, occ_right) = bar_occupied_x(&bar, true, Some(800.0), Some(today_x));
        assert!(
            occ_left <= paint_x + 0.5 && occ_right + 0.5 >= paint_x + paint_w,
            "intervening occupancy must cover the flipped letters, occ={occ_left}..{occ_right} paint={paint_x}..{}",
            paint_x + paint_w
        );
        let phantom_right = bar_label_right(&bar);
        assert!(
            !x_hits_span(phantom_right - 8.0, occ_left, occ_right),
            "occupancy must not keep a phantom right-side chip after a Today flip, phantom={phantom_right} occ={occ_left}..{occ_right}"
        );
        assert!(
            x_hits_span(paint_x + 8.0, occ_left, occ_right),
            "occupancy must hit the left-side letters, paint_x={paint_x} occ={occ_left}..{occ_right}"
        );
    }

    #[test]
    fn occupancy_covers_an_overflow_badge_flip() {
        let bar = BarLayout {
            id: 1003,
            kind: GanttBarKind::Scheduled,
            x: 300.0,
            width: 24.0,
            y: 80.0,
            summary: true,
            milestone: false,
            label: String::new(),
            title: "Persist cart".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
            overflow_badge: true,
            floored: false,
        };
        let estimated = bar_title_paint_px(&bar.title);
        let right_plain = bar.x + bar.width.max(8.0) + LINK_HANDLE_GAP + LINK_HANDLE_HIT + 20.0;
        let right_badge = bar.x + bar.width.max(8.0) + OUTSIDE_LABEL_GAP;
        // Room for the no-badge chip, not the `!`+title chip.
        let today_x = right_plain + estimated + TODAY_LABEL_CLEARANCE + 8.0;
        assert!(
            full_outside_title_fits(right_plain, estimated, 800.0, Some(today_x)),
            "precondition: no-badge Persist cart still fits before Today"
        );
        assert!(
            !full_outside_title_fits(right_badge, estimated, 800.0, Some(today_x)),
            "precondition: overflow-badge Persist cart cannot fit before Today"
        );
        let (paint_x, paint_w) = bar_outside_label_frame(&bar, 800.0, true, Some(today_x), true)
            .expect("overflow-badge Today must keep a full Persist cart chip");
        assert!(
            paint_x + paint_w <= bar.x + 0.1,
            "fixture: overflow badge must flip the chip left, paint_x={paint_x} bar.x={}",
            bar.x
        );
        let (occ_left, occ_right) = bar_occupied_x(&bar, true, Some(800.0), Some(today_x));
        assert!(
            x_hits_span(paint_x + 8.0, occ_left, occ_right),
            "occupancy must cover the overflow-flipped letters, paint={paint_x}..{} occ={occ_left}..{occ_right}",
            paint_x + paint_w
        );
    }

    #[test]
    fn occupancy_uses_the_painted_timeline_not_the_arrow_inset() {
        let bar = BarLayout {
            id: 1003,
            kind: GanttBarKind::Scheduled,
            x: 582.0,
            width: 24.0,
            y: 80.0,
            summary: true,
            milestone: false,
            label: String::new(),
            title: "Persist cart".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
            overflow_badge: false,
            floored: false,
        };
        let estimated = bar_title_paint_px(&bar.title);
        let right_x = bar.x + bar.width.max(8.0) + LINK_HANDLE_GAP + LINK_HANDLE_HIT + 20.0;
        assert!(
            full_outside_title_fits(right_x, estimated, 800.0, None),
            "precondition: full title fits on the real 800px timeline"
        );
        assert!(
            !full_outside_title_fits(right_x, estimated, 790.0, None),
            "precondition: the arrow inset clip (timeline-10) cannot hold the same right chip"
        );
        let (paint_x, paint_w) = bar_outside_label_frame(&bar, 800.0, true, None, false)
            .expect("paint uses the real timeline");
        assert!(
            (paint_x - right_x).abs() < 0.5,
            "fixture: paint keeps the chip on the right, paint_x={paint_x} right_x={right_x}"
        );
        let (occ_left, occ_right) = bar_occupied_x_on(&bar, true, Some(790.0), None, Some(800.0));
        assert!(
            occ_left <= paint_x + 0.5 && occ_right + 0.5 >= paint_x + paint_w,
            "post-clip occupancy must match the painted right chip, not a left flip from the inset, occ={occ_left}..{occ_right} paint={paint_x}..{}",
            paint_x + paint_w
        );
        let inset_only = bar_occupied_x(&bar, true, Some(790.0), None);
        assert!(
            inset_only.0 < paint_x - 1.0 || inset_only.1 < paint_x + paint_w - 1.0,
            "precondition: inset-as-timeline occupancy must disagree with paint so the dodge pass needs the real width"
        );
    }

    #[test]
    fn today_dodge_does_not_cover_the_bar() {
        let bar = BarLayout {
            id: 1002,
            kind: GanttBarKind::Scheduled,
            x: 40.0,
            width: 24.0,
            y: 40.0,
            summary: true,
            milestone: false,
            label: String::new(),
            title: "Cart".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
            overflow_badge: false,
            floored: false,
        };
        let estimated = bar_title_paint_px(&bar.title);
        let right_x = bar.x + bar.width.max(8.0) + LINK_HANDLE_GAP + LINK_HANDLE_HIT + 20.0;
        let today_x = right_x + 20.0;
        assert!(
            today_x - right_x < estimated,
            "precondition: full Cart title cannot fit before Today"
        );
        let left_x = bar.x - estimated - OUTSIDE_TITLE_LEFT_GAP;
        assert!(
            left_x.max(2.0) + estimated > bar.x,
            "precondition: a clamp-to-2 chip would cover the sliver"
        );
        let (x, w) = bar_outside_label_frame(&bar, 800.0, true, Some(today_x), false)
            .expect("Year Cart must keep a full outside title");
        assert!(
            w + 0.1 >= estimated,
            "must keep the full title, not Ca…, max_w={w} estimated={estimated}"
        );
        assert!(
            x + w <= bar.x + 0.1 || x + 0.5 >= bar.x + bar.width.max(8.0),
            "Today dodge must not paint over the bar, label={x}..{} bar.x={}",
            x + w,
            bar.x
        );
        assert!(
            outside_label_is_beside_bar(&bar, x, w, true, false),
            "Today dodge must keep the chip beside the bar, not park at the line, label={x}+{w} today={today_x} bar.x={}",
            bar.x
        );
    }

    #[test]
    fn year_toolbar_after_auto_fit_keeps_full_outside_titles() {
        let nodes = mock_fixture_nodes();
        let mut gantt = GanttView {
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.auto_fit(&nodes, &[], 800.0);
        gantt.retarget_year_gutter(|gantt| {
            gantt.set_scale_from_toolbar(GanttScale::Year, jiff::Zoned::now().date());
        });
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let today = jiff::Zoned::now().date();
        let today_px = gantt.x_for_date(today);
        let today_x = (today_px >= 0.0 && today_px <= gantt.timeline_width).then_some(today_px);
        for id in [1002_i64, 1003, 1005, 1012, 1013] {
            let bar = bars
                .iter()
                .find(|bar| bar.id == id)
                .unwrap_or_else(|| panic!("fixture bar {id}"));
            if bar_offscreen_edge(bar, gantt.timeline_width).is_some() {
                continue;
            }
            assert!(
                !bar_title_is_inside(bar),
                "launch Year {} is a sliver and must not clip inside, width={}",
                bar.title,
                bar.width
            );
            let (x, w) = bar_outside_label_frame(bar, gantt.timeline_width, true, today_x, false)
                .unwrap_or_else(|| {
                    panic!("launch Year {} must have a full outside frame", bar.title)
                });
            let estimated = bar_title_paint_px(&bar.title);
            assert!(
                w + 0.1 >= estimated,
                "launch Year {} must keep the full title, not Pers…/Go-l…, max_w={w} estimated={estimated} x={x}",
                bar.title
            );
            assert!(
                outside_label_is_beside_bar(bar, x, w, true, false),
                "launch Year {} must stay within {MAX_LABEL_BAR_SHIFT}px of its bar, not park at Today, label={x}+{w} bar.x={}",
                bar.title,
                bar.x
            );
        }
    }

    #[test]
    fn quarter_and_year_outside_titles_stay_beside_the_bar() {
        let nodes = mock_fixture_nodes();
        for scale in [GanttScale::Quarter, GanttScale::Year] {
            let mut gantt = GanttView {
                timeline_width: 800.0,
                ..Default::default()
            };
            gantt.auto_fit(&nodes, &[], 800.0);
            gantt.retarget_year_gutter(|gantt| {
                gantt.set_scale_from_toolbar(scale, jiff::Zoned::now().date());
            });
            let rows = gantt.flatten(&nodes);
            let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
            let today = jiff::Zoned::now().date();
            let today_px = gantt.x_for_date(today);
            let today_x = (today_px >= 0.0 && today_px <= gantt.timeline_width).then_some(today_px);
            for id in [1001_i64, 1003, 1005, 1013] {
                let bar = bars
                    .iter()
                    .find(|bar| bar.id == id)
                    .unwrap_or_else(|| panic!("fixture bar {id}"));
                if bar_offscreen_edge(bar, gantt.timeline_width).is_some()
                    || bar_title_is_inside(bar)
                {
                    continue;
                }
                let (x, w) =
                    bar_outside_label_frame(bar, gantt.timeline_width, true, today_x, false)
                        .unwrap_or_else(|| {
                            panic!("{scale:?} {} must have a full outside frame", bar.title)
                        });
                assert!(
                    outside_label_is_beside_bar(bar, x, w, true, false),
                    "{scale:?} {} must stay within {MAX_LABEL_BAR_SHIFT}px of its bar, label={x}+{w} bar.x={} today={today_x:?}",
                    bar.title,
                    bar.x
                );
            }
        }
    }

    #[test]
    fn today_line_gaps_through_a_beside_bar_chip() {
        let bar = BarLayout {
            id: 1003,
            kind: GanttBarKind::Scheduled,
            x: 40.0,
            width: 24.0,
            y: 80.0,
            summary: true,
            milestone: false,
            label: String::new(),
            title: "Persist cart".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
            overflow_badge: false,
            floored: false,
        };
        let right_x = bar.x + bar.width.max(8.0) + LINK_HANDLE_GAP + LINK_HANDLE_HIT + 20.0;
        let estimated = bar_title_paint_px(&bar.title);
        let today_x = right_x + 20.0;
        assert!(
            today_x - right_x < estimated,
            "precondition: the natural right chip crosses Today"
        );
        let (x, w) = bar_outside_label_frame(&bar, 800.0, true, Some(today_x), false)
            .expect("must keep a full Persist cart chip");
        assert!(
            outside_label_is_beside_bar(&bar, x, w, true, false),
            "must stay beside the bar even when that slot crosses Today, label={x}+{w} today={today_x}"
        );
        assert!(
            x < today_x && x + w > today_x,
            "fixture: the beside-bar chip crosses the Today x, label={x}..{} today={today_x}",
            x + w
        );
        let skips =
            today_line_chip_skips(std::slice::from_ref(&bar), today_x, 800.0, GanttScale::Year);
        assert!(
            skips.is_empty(),
            "Year Today stays a continuous stroke behind labels, skips={skips:?}"
        );
        let segments = today_line_segments(0.0, 400.0, &skips);
        assert_eq!(
            segments.len(),
            1,
            "Today must run behind the chip as one stroke, segments={segments:?}"
        );
    }

    #[test]
    fn quarter_today_does_not_false_gap_go_live_or_spike() {
        let nodes = mock_fixture_nodes();
        let mut gantt = GanttView {
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.auto_fit(&nodes, &[], 800.0);
        gantt.retarget_year_gutter(|gantt| {
            gantt.set_scale_from_toolbar(GanttScale::Quarter, jiff::Zoned::now().date());
        });
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let today = jiff::Zoned::now().date();
        let today_px = gantt.x_for_date(today);
        let today_x = (today_px >= 0.0 && today_px <= gantt.timeline_width).then_some(today_px);
        let Some(today_x) = today_x else {
            return;
        };
        let skips =
            today_line_chip_skips(&bars, today_x, gantt.timeline_width, GanttScale::Quarter);
        assert!(
            skips.is_empty(),
            "Quarter Today must stay a continuous line, skips={skips:?} today={today_x}"
        );
    }

    #[test]
    fn year_cart_ui_label_clears_or_gaps_today() {
        let nodes = mock_fixture_nodes();
        for (tag, gantt) in [
            (
                "july1",
                GanttView {
                    origin: Date::new(2026, 7, 1).unwrap(),
                    scale: GanttScale::Year,
                    zoom: 1.0,
                    timeline_width: 800.0,
                    ..Default::default()
                },
            ),
            ("auto_fit", {
                let mut gantt = GanttView {
                    timeline_width: 800.0,
                    ..Default::default()
                };
                gantt.auto_fit(&nodes, &[], 800.0);
                gantt.retarget_year_gutter(|gantt| {
                    gantt.set_scale_from_toolbar(GanttScale::Year, jiff::Zoned::now().date());
                });
                gantt
            }),
        ] {
            let rows = gantt.flatten(&nodes);
            let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
            let cart = bars.iter().find(|bar| bar.id == 1005).unwrap();
            let today = jiff::Zoned::now().date();
            let today_px = gantt.x_for_date(today);
            let Some(today_x) =
                (today_px >= 0.0 && today_px <= gantt.timeline_width).then_some(today_px)
            else {
                continue;
            };
            let Some((x, w)) =
                bar_outside_label_frame(cart, gantt.timeline_width, true, Some(today_x), false)
            else {
                continue;
            };
            let skips =
                today_line_chip_skips(&bars, today_x, gantt.timeline_width, GanttScale::Year);
            assert!(
                skips.is_empty(),
                "{tag}: Year Today stays a continuous stroke behind the Cart UI label {x}..{}, skips={skips:?}",
                x + w
            );
        }
    }

    #[test]
    fn launch_year_persist_go_live_misses_cart_ui_and_source_title() {
        let nodes = mock_fixture_nodes();
        let mut gantt = GanttView {
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.auto_fit(&nodes, &[], 800.0);
        gantt.retarget_year_gutter(|gantt| {
            gantt.set_scale_from_toolbar(GanttScale::Year, jiff::Zoned::now().date());
        });
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let cart_ui = bars.iter().find(|bar| bar.id == 1005).unwrap();
        let live = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("launch Year Persist cart → Go-live");
        let today = jiff::Zoned::now().date();
        let today_px = gantt.x_for_date(today);
        let today_x = (today_px >= 0.0 && today_px <= gantt.timeline_width).then_some(today_px);
        if let Some((left, right)) =
            bar_outside_title_span_clipped(pred, gantt.timeline_width, today_x)
        {
            assert!(
                !x_hits_span(live.x1, left, right),
                "outgoing vertical must miss the Persist cart title, x1={} label={left}..{right}",
                live.x1
            );
            let past_right = live.x1 >= right + SOURCE_LABEL_CLEARANCE - 0.5;
            let past_left = live.x1 <= left - SOURCE_LABEL_CLEARANCE + 0.5;
            assert!(
                past_right || past_left,
                "outgoing stub must sit ≥{SOURCE_LABEL_CLEARANCE}px off the Persist cart letters, x1={} label={left}..{right}",
                live.x1
            );
        }
        let (body_left, body_right) = bar_occupied_x(cart_ui, false, None, None);
        assert!(
            !x_hits_span(live.x2, body_left, body_right),
            "inbound vertical must miss Cart UI's bar, x2={} body={body_left}..{body_right} cart.x={}",
            live.x2,
            cart_ui.x
        );
        if let Some((label_left, label_right)) =
            bar_outside_title_span_clipped(cart_ui, gantt.timeline_width, today_x)
        {
            assert!(
                !x_hits_span(live.x2, label_left, label_right),
                "inbound vertical must miss Cart UI's beside-bar label, x2={} label={label_left}..{label_right}",
                live.x2
            );
        }
    }

    #[test]
    fn milestone_chip_stays_right_of_the_diamond_across_scales() {
        let nodes = mock_fixture_nodes();
        for scale in [GanttScale::Week, GanttScale::Month, GanttScale::Quarter] {
            let mut gantt = GanttView {
                timeline_width: 800.0,
                ..Default::default()
            };
            gantt.auto_fit(&nodes, &[], 800.0);
            gantt.retarget_year_gutter(|gantt| {
                gantt.set_scale_from_toolbar(scale, jiff::Zoned::now().date());
            });
            let rows = gantt.flatten(&nodes);
            let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
            let bar = bars.iter().find(|bar| bar.id == 1013).unwrap();
            if bar_offscreen_edge(bar, gantt.timeline_width).is_some() {
                continue;
            }
            let today = jiff::Zoned::now().date();
            let today_px = gantt.x_for_date(today);
            let today_x = (today_px >= 0.0 && today_px <= gantt.timeline_width).then_some(today_px);
            let (x, w) = bar_outside_label_frame(bar, gantt.timeline_width, true, today_x, false)
                .expect("Go-live must keep an outside chip");
            let diamond_right = bar.x + MILESTONE_SIZE;
            let on_right = x + 1.0 >= diamond_right;
            let on_left = x + w <= bar.x + 0.1;
            assert!(
                on_right || on_left,
                "{scale:?}: Go-live chip must sit beside the diamond, x={x} diamond={diamond_right}"
            );
            if today_x.is_none()
                || today_x
                    .is_some_and(|today| diamond_right + MILESTONE_LABEL_GAP + w <= today - 8.0)
            {
                assert!(
                    on_right,
                    "{scale:?}: Go-live chip must stay on the right of the diamond when there is room, x={x} diamond={diamond_right}"
                );
            }
        }
    }

    #[test]
    fn month_persist_go_live_horizontal_clears_the_source_title() {
        let mut nodes = mock_fixture_nodes();
        if let Some(spike) = nodes.iter_mut().find(|node| node.id == 1012) {
            spike.predecessors = vec![1003];
        }
        let mut gantt = GanttView {
            timeline_width: 800.0,
            zoom: 0.4,
            ..Default::default()
        };
        gantt.auto_fit(&nodes, &[], 800.0);
        if gantt.scale != GanttScale::Month {
            gantt.retarget_year_gutter(|gantt| {
                gantt.set_scale_from_toolbar(GanttScale::Month, jiff::Zoned::now().date());
            });
        }
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let pred = bars
            .iter()
            .find(|bar| bar.id == 1003)
            .expect("Persist cart");
        let live = gantt
            .layout_dependency_arrows(&bars)
            .into_iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("Persist cart → Go-live");
        let (left, right) = bar_outside_title_span(pred)
            .expect("auto-fit Month Persist cart paints an outside title");
        let painted_w = pred.width.max(8.0);
        if live.x_head > pred.x + painted_w + 12.0 {
            assert!(
                live.x1 + 0.5 >= right + 6.0 || live.x1 <= left - 6.0 + 0.5,
                "Month Persist cart → Go-live horizontal must sit ≥6px from the label, x1={} label={left}..{right}",
                live.x1
            );
        }
        let cart_ui = bars.iter().find(|bar| bar.id == 1005).unwrap();
        assert_inbound_drops_before_intervening_bar(&live, cart_ui, gantt.timeline_width);
        assert!(
            !live.wraps_around_successor(),
            "forward Persist cart → Go-live must not wrap along the Go-live row"
        );
        let label_top = pred.y + ((ROW_HEIGHT - OUTSIDE_LABEL_H) / 2.0).max(2.0);
        let overlaps_x = live.x1.min(live.x2) < right && live.x1.max(live.x2) > left;
        if overlaps_x && live.y2 > live.y1 {
            assert!(
                live.lane_y + 1.0 >= label_top + OUTSIDE_LABEL_H + 6.0,
                "Month Persist cart → Go-live must sit ≥6px below the chip, lane={} chip_bottom={}",
                live.lane_y,
                label_top + OUTSIDE_LABEL_H
            );
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
            dx >= PARALLEL_VERT_MIN,
            "a newly created Persist cart → Go-live must sit ≥{PARALLEL_VERT_MIN}px off Cart UI → spike, dx={dx} persist x1={} x2={} spike x1={} x2={}",
            persist_live.x1,
            persist_live.x2,
            cart_spike.x1,
            cart_spike.x2
        );
        if persist_live.x_head <= gantt.timeline_width {
            let cart_ui = bars.iter().find(|bar| bar.id == 1005).unwrap();
            assert_inbound_drops_before_intervening_bar(
                persist_live,
                cart_ui,
                gantt.timeline_width,
            );
        }
    }

    #[test]
    fn runtime_persist_go_live_inbound_drops_before_cart_ui() {
        let mut nodes = mock_fixture_nodes();
        if let Some(go_live) = nodes.iter_mut().find(|node| node.id == 1013) {
            go_live.predecessors.clear();
        }
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 1200.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        assert!(
            gantt
                .layout_dependency_arrows(&bars)
                .iter()
                .all(|arrow| !(arrow.from_id == 1003 && arrow.to_id == 1013)),
            "precondition: Persist cart → Go-live is created at runtime"
        );
        if let Some(go_live) = nodes.iter_mut().find(|node| node.id == 1013) {
            go_live.predecessors = vec![1003];
        }
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let live = gantt
            .layout_dependency_arrows(&bars)
            .into_iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("runtime Persist cart → Go-live");
        let cart_ui = bars.iter().find(|bar| bar.id == 1005).unwrap();
        assert!(
            cart_ui.open_ended,
            "fixture: Cart UI is the intervening start-only bar"
        );
        assert_inbound_drops_before_intervening_bar(&live, cart_ui, gantt.timeline_width);
        assert!(
            live.x2 < cart_ui.x,
            "second vertical must drop before Cart UI starts, x2={} cart.x={}..{}",
            live.x2,
            cart_ui.x,
            cart_ui.x + cart_ui.width
        );
        assert!(
            !live.wraps_around_successor(),
            "forward Persist cart → Go-live must not wrap along the Go-live row"
        );
    }

    #[test]
    fn prefer_a_leftward_jog_over_crossing_an_intervening_bar() {
        // Cart UI is an open-ended bar from its marker to the clip, sitting
        // between Persist cart and the Go-live diamond. A drop at the diamond
        // column (x_head-12) would cut through that painted bar. Prefer a
        // drop left of Cart UI even when that means x1 > x2 — a short
        // leftward jog from the handle.
        let mut nodes = mock_fixture_nodes();
        if let Some(spike) = nodes.iter_mut().find(|node| node.id == 1012) {
            spike.predecessors = vec![1003];
        }
        if let Some(go_live) = nodes.iter_mut().find(|node| node.id == 1013) {
            go_live.predecessors.clear();
        }
        let mut gantt = GanttView {
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.auto_fit(&nodes, &[], 800.0);
        gantt.timeline_width = 512.0;
        if let Some(go_live) = nodes.iter_mut().find(|node| node.id == 1013) {
            go_live.predecessors = vec![1003];
        }
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let live = gantt
            .layout_dependency_arrows(&bars)
            .into_iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("Persist cart → Go-live");
        let cart = bars.iter().find(|bar| bar.id == 1005).unwrap();
        assert!(
            cart.open_ended,
            "fixture: Cart UI paints a dashed tail to the clip"
        );
        assert!(
            live.x_head > cart.x + cart.width.max(8.0) + 8.0,
            "fixture: Go-live sits to the right of Cart UI's marker, x_head={} cart.x={}",
            live.x_head,
            cart.x
        );
        let (left, right) = bar_occupied_x(cart, false, Some(gantt.timeline_width), None);
        assert!(
            live.x2 < cart.x && live.x2 < left,
            "must drop left of Cart UI rather than through it, x2={} cart.x={} occupied={left}..{right}",
            live.x2,
            cart.x
        );
        assert!(
            !x_hits_span(live.x2, left, right),
            "inbound must not cross Cart UI's painted bar, x2={} occupied={left}..{right}",
            live.x2
        );
        assert!(
            live.x_head - live.x2 > MAX_FORWARD_INBOUND,
            "diamond-column stub would cross the bar; a left-of-bar drop (and any leftward jog) is required, x2={} x_head={}",
            live.x2,
            live.x_head
        );
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let pred_end = pred.x + pred.width.max(8.0);
        assert!(
            live.x1 + 0.5 >= pred_end || live.x1 <= live.x2 + 1.0,
            "outgoing may stay at the handle (a jog) or collapse onto the inbound, x1={} x2={} pred_end={pred_end}",
            live.x1,
            live.x2
        );
    }

    #[test]
    fn overlapping_fs_link_back_jogs_into_the_target_from_the_left() {
        // Persist cart resized to Jul 25 overlaps Cart UI (Jul 21, start-only).
        // A wrap drop at persist_end+12 runs through Cart UI's body/cue.
        // Back-jog on the source lane and enter Cart UI from the left.
        let mut persist = node(1003, Some(1002));
        persist.title = "Persist cart".into();
        persist.start_date = Some("2026-07-06T00:00:00Z".into());
        persist.target_date = Some("2026-07-25T00:00:00Z".into());
        let mut cart_ui = node(1005, Some(1003));
        cart_ui.title = "Cart UI".into();
        cart_ui.start_date = Some("2026-07-21T00:00:00Z".into());
        cart_ui.target_date = None;
        cart_ui.predecessors = vec![1003];
        let nodes = vec![node(1001, None), node(1002, Some(1001)), persist, cart_ui];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 1005).unwrap();
        assert!(
            pred.x + pred.width > succ.x + 12.0,
            "fixture: Persist cart end must sit past Cart UI start, pred_end={} succ.x={}",
            pred.x + pred.width,
            succ.x
        );
        let arrow = gantt
            .layout_dependency_arrows(&bars)
            .into_iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1005)
            .expect("Persist cart → Cart UI");
        assert!(
            !arrow.wraps_around_successor(),
            "overlap must not wrap through the target, x1={} x2={} x_head={}",
            arrow.x1,
            arrow.x2,
            arrow.x_head
        );
        assert!(
            (arrow.x_head - succ.x).abs() < 0.5,
            "must still enter Cart UI from the start, x_head={} succ.x={}",
            arrow.x_head,
            succ.x
        );
        assert!(
            arrow.x2 < succ.x,
            "back-jog inbound must sit left of Cart UI, x2={} succ.x={}",
            arrow.x2,
            succ.x
        );
        let today = jiff::Zoned::now().date();
        let today_px = gantt.x_for_date(today);
        let today_x = (today_px >= 0.0 && today_px <= gantt.timeline_width).then_some(today_px);
        let (left, right) = bar_occupied_x(succ, true, Some(gantt.timeline_width), today_x);
        assert!(
            !x_hits_span(arrow.x2, left, right),
            "inbound vertical must not run through Cart UI, x2={} occupied={left}..{right}",
            arrow.x2
        );
        assert!(
            arrow.x1 + 0.5 >= pred.x + pred.width,
            "outgoing must leave the source end, x1={} pred_end={}",
            arrow.x1,
            pred.x + pred.width
        );
        assert!(
            arrow.x1 > arrow.x2,
            "overlap route is a leftward jog, x1={} x2={}",
            arrow.x1,
            arrow.x2
        );
        assert!(
            arrow.lane_y >= pred.y && arrow.lane_y < pred.y + ROW_HEIGHT,
            "back-jog must stay on the source row, not drop through Cart UI, lane={} persist={}..{}",
            arrow.lane_y,
            pred.y,
            pred.y + ROW_HEIGHT
        );
    }

    fn overlap_persist_cart_ui_nodes() -> Vec<WorkItemNode> {
        let mut persist = node(1003, Some(1002));
        persist.title = "Persist cart".into();
        persist.start_date = Some("2026-07-06T00:00:00Z".into());
        persist.target_date = Some("2026-07-25T00:00:00Z".into());
        let mut cart_ui = node(1005, Some(1003));
        cart_ui.title = "Cart UI".into();
        cart_ui.start_date = Some("2026-07-21T00:00:00Z".into());
        cart_ui.target_date = None;
        cart_ui.predecessors = vec![1003];
        vec![node(1001, None), node(1002, Some(1001)), persist, cart_ui]
    }

    fn assert_overlap_back_jog(gantt: &GanttView, nodes: &[WorkItemNode]) {
        let rows = gantt.flatten(nodes);
        let bars = gantt.layout_bars(nodes, &rows, &[], 720.0);
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 1005).unwrap();
        if pred.x + pred.width <= succ.x {
            return;
        }
        let arrow = gantt
            .layout_dependency_arrows(&bars)
            .into_iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1005)
            .expect("Persist cart → Cart UI");
        assert!(
            !arrow.wraps_around_successor(),
            "{:?} overlap must not wrap through the target, x1={} x2={} x_head={}",
            gantt.scale,
            arrow.x1,
            arrow.x2,
            arrow.x_head
        );
        assert!(
            arrow.x2 < succ.x,
            "{:?} back-jog inbound must sit left of Cart UI, x2={} succ.x={}",
            gantt.scale,
            arrow.x2,
            succ.x
        );
        let today = jiff::Zoned::now().date();
        let today_px = gantt.x_for_date(today);
        let today_x = (today_px >= 0.0 && today_px <= gantt.timeline_width).then_some(today_px);
        let (left, right) = bar_occupied_x(succ, true, Some(gantt.timeline_width), today_x);
        assert!(
            !x_hits_span(arrow.x2, left, right),
            "{:?} inbound vertical must not run through Cart UI, x2={} occupied={left}..{right}",
            gantt.scale,
            arrow.x2
        );
        assert!(
            arrow.x1 > arrow.x2,
            "{:?} overlap route is a leftward jog, x1={} x2={}",
            gantt.scale,
            arrow.x1,
            arrow.x2
        );
        let pred_end = pred.x + pred.width;
        assert!(
            arrow.x1 <= pred_end + DependencyArrow::BACK_JOG_STUB + 1.0,
            "{:?} back-jog loop must stay tight, x1={} pred_end={} stub={}",
            gantt.scale,
            arrow.x1,
            pred_end,
            DependencyArrow::BACK_JOG_STUB
        );
    }

    #[test]
    fn overlapping_fs_link_back_jogs_in_every_scale() {
        let nodes = overlap_persist_cart_ui_nodes();
        for (scale, zoom) in [
            (GanttScale::Day, 1.0),
            (GanttScale::Week, 1.0),
            (GanttScale::Month, 1.0),
            (GanttScale::Quarter, 2.0),
            (GanttScale::Year, 1.0),
            (GanttScale::Year, 4.0),
        ] {
            let gantt = GanttView {
                origin: Date::new(2026, 7, 1).unwrap(),
                timeline_width: 1100.0,
                scale,
                zoom,
                ..Default::default()
            };
            assert_overlap_back_jog(&gantt, &nodes);
        }
    }

    #[test]
    fn year_default_zoom_overlap_still_back_jogs() {
        let nodes = overlap_persist_cart_ui_nodes();
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 1100.0,
            scale: GanttScale::Year,
            zoom: 1.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 1005).unwrap();
        let overlap = pred.x + pred.width - succ.x;
        assert!(
            overlap > 0.0 && overlap <= YEAR_MIN_BAR_WIDTH,
            "fixture: Year zoom 1 must overlap by less than the floored bar, overlap={overlap} pred_end={} succ.x={}",
            pred.x + pred.width,
            succ.x
        );
        assert_overlap_back_jog(&gantt, &nodes);
    }

    #[test]
    fn week_overlapping_fs_link_with_sibling_arrows_back_jogs() {
        let mut nodes = overlap_persist_cart_ui_nodes();
        let mut go_live = node(1013, Some(1001));
        go_live.title = "Go-live milestone".into();
        go_live.start_date = None;
        go_live.target_date = Some("2026-08-15T00:00:00Z".into());
        go_live.predecessors = vec![1003];
        let mut spike = node(1012, None);
        spike.title = "Unparented spike".into();
        spike.start_date = Some("2026-07-21T00:00:00Z".into());
        spike.target_date = Some("2026-08-04T00:00:00Z".into());
        spike.predecessors = vec![1005];
        nodes.push(go_live);
        nodes.push(spike);
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 1100.0,
            scale: GanttScale::Week,
            ..Default::default()
        };
        assert_overlap_back_jog(&gantt, &nodes);
    }

    fn persist_dates_to_spike_nodes(start: &str, target: &str) -> Vec<WorkItemNode> {
        let mut nodes = live_fixture_nodes();
        if let Some(persist) = nodes.iter_mut().find(|node| node.id == 1003) {
            persist.start_date = Some(format!("{start}T00:00:00Z"));
            persist.target_date = Some(format!("{target}T00:00:00Z"));
        }
        nodes
    }

    fn moved_persist_to_spike_nodes() -> Vec<WorkItemNode> {
        persist_dates_to_spike_nodes("2026-07-20", "2026-08-03")
    }

    fn assert_verticals_miss_crossed_bars(gantt: &GanttView, nodes: &[WorkItemNode]) {
        let rows = gantt.flatten(nodes);
        let bars = gantt.layout_bars(nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let clip_right = gantt.timeline_width.max(1.0);
        for arrow in &arrows {
            let Some(pred) = bars.iter().find(|bar| bar.id == arrow.from_id) else {
                continue;
            };
            let Some(succ) = bars.iter().find(|bar| bar.id == arrow.to_id) else {
                continue;
            };
            for bar in &bars {
                if bar.id == pred.id || bar.id == succ.id || bar_is_gutter(bar) {
                    continue;
                }
                let (left, right) = painted_obstacle_x(bar, clip_right, gantt.timeline_width);
                let inbound_end = if arrow.wraps_around_successor() {
                    arrow.gutter_y()
                } else {
                    arrow.y2
                };
                if vertical_crosses_painted(arrow.lane_y, inbound_end, bar) {
                    assert!(
                        !x_hits_span(arrow.x2, left, right),
                        "{:?} inbound {}→{} at x2={} must miss {} painted {left}..{right}",
                        gantt.scale,
                        arrow.from_id,
                        arrow.to_id,
                        arrow.x2,
                        bar.title
                    );
                }
                if vertical_crosses_painted(arrow.y1, arrow.lane_y, bar) {
                    assert!(
                        !x_hits_span(arrow.x1, left, right),
                        "{:?} outgoing {}→{} at x1={} must miss {} painted {left}..{right}",
                        gantt.scale,
                        arrow.from_id,
                        arrow.to_id,
                        arrow.x1,
                        bar.title
                    );
                }
            }
        }
    }

    #[test]
    fn month_moved_persist_to_spike_misses_cart_ui() {
        let nodes = moved_persist_to_spike_nodes();
        for (timeline_width, zoom) in [(512.0, 1.0), (800.0, 1.0), (1100.0, 1.0), (800.0, 1.6)] {
            let gantt = GanttView {
                origin: Date::new(2026, 7, 1).unwrap(),
                timeline_width,
                scale: GanttScale::Month,
                zoom,
                ..Default::default()
            };
            let rows = gantt.flatten(&nodes);
            let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
            let persist = bars.iter().find(|bar| bar.id == 1003).unwrap();
            let cart = bars.iter().find(|bar| bar.id == 1005).unwrap();
            let spike = bars.iter().find(|bar| bar.id == 1012).unwrap();
            assert!(
                persist.x + persist.width > cart.x + 12.0,
                "fixture: moved Persist cart must overlap Cart UI, persist_end={} cart.x={} width={timeline_width} zoom={zoom}",
                persist.x + persist.width,
                cart.x
            );
            let arrow = gantt
                .layout_dependency_arrows(&bars)
                .into_iter()
                .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
                .expect("Persist cart → Unparented spike");
            let today = jiff::Zoned::now().date();
            let today_px = gantt.x_for_date(today);
            let today_x = (today_px >= 0.0 && today_px <= gantt.timeline_width).then_some(today_px);
            let (left, right) = bar_occupied_x(cart, true, Some(gantt.timeline_width), today_x);
            assert!(
                !x_hits_span(arrow.x2, left, right),
                "Month Persist cart → spike inbound must not drop through Cart UI, x2={} occupied={left}..{right} persist_end={} cart.x={} spike.x={} width={timeline_width} zoom={zoom}",
                arrow.x2,
                persist.x + persist.width,
                cart.x,
                spike.x
            );
            assert_verticals_miss_crossed_bars(&gantt, &nodes);
        }
    }

    #[test]
    fn moved_persist_to_spike_verticals_miss_crossed_bars_in_every_scale() {
        let nodes = moved_persist_to_spike_nodes();
        for (scale, zoom) in [
            (GanttScale::Day, 1.0),
            (GanttScale::Week, 1.0),
            (GanttScale::Month, 1.0),
            (GanttScale::Quarter, 2.0),
            (GanttScale::Year, 4.0),
        ] {
            let gantt = GanttView {
                origin: Date::new(2026, 7, 1).unwrap(),
                timeline_width: 1100.0,
                scale,
                zoom,
                ..Default::default()
            };
            assert_verticals_miss_crossed_bars(&gantt, &nodes);
        }
    }

    #[test]
    fn overlapping_fs_verticals_miss_painted_bars_across_scales_and_overlaps() {
        let windows = [
            ("2026-07-06", "2026-07-25"),
            ("2026-07-20", "2026-08-03"),
            ("2026-07-22", "2026-08-05"),
            ("2026-07-21", "2026-08-10"),
        ];
        let scales = [
            (GanttScale::Day, 1.0),
            (GanttScale::Week, 1.0),
            (GanttScale::Month, 1.0),
            (GanttScale::Quarter, 2.0),
            (GanttScale::Year, 1.0),
        ];
        for (start, target) in windows {
            let nodes = persist_dates_to_spike_nodes(start, target);
            for (scale, zoom) in scales {
                for timeline_width in [800.0, 1100.0] {
                    let gantt = GanttView {
                        origin: Date::new(2026, 7, 1).unwrap(),
                        timeline_width,
                        scale,
                        zoom,
                        ..Default::default()
                    };
                    assert_verticals_miss_crossed_bars(&gantt, &nodes);
                    let rows = gantt.flatten(&nodes);
                    let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
                    let Some(arrow) = gantt
                        .layout_dependency_arrows(&bars)
                        .into_iter()
                        .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
                    else {
                        continue;
                    };
                    let Some(cart) = bars.iter().find(|bar| bar.id == 1005) else {
                        continue;
                    };
                    let inbound_end = if arrow.wraps_around_successor() {
                        arrow.gutter_y()
                    } else {
                        arrow.y2
                    };
                    if vertical_crosses_painted(arrow.lane_y, inbound_end, cart) {
                        let (left, right) =
                            painted_obstacle_x(cart, timeline_width, timeline_width);
                        assert!(
                            !x_hits_span(arrow.x2, left, right),
                            "{scale:?} zoom={zoom} width={timeline_width} persist {start}→{target} inbound x2={} must miss Cart UI painted {left}..{right}",
                            arrow.x2
                        );
                    }
                    let (mid_x, mid_y) = arrow.midpoint();
                    let min_x = arrow.x1.min(arrow.x2).min(arrow.x_head);
                    let max_x = arrow.x1.max(arrow.x2).max(arrow.x_head);
                    assert!(
                        mid_x >= min_x - 1.0 && mid_x <= max_x + 1.0,
                        "{scale:?} hover chip midpoint x={mid_x} must sit on the polyline {min_x}..{max_x}"
                    );
                    let min_y = arrow.y1.min(arrow.y2).min(arrow.lane_y);
                    let max_y = arrow.y1.max(arrow.y2).max(arrow.lane_y);
                    assert!(
                        mid_y >= min_y - 1.0 && mid_y <= max_y + ROW_HEIGHT,
                        "{scale:?} hover chip midpoint y={mid_y} must sit on the polyline {min_y}..{max_y}"
                    );
                }
            }
        }
    }

    #[test]
    fn dependency_loop_message_cuts_to_five_ids() {
        assert_eq!(
            dependency_loop_message(&[12, 34, 56, 12]),
            "Would loop: #12 → #34 → #56 → #12"
        );
        assert_eq!(
            dependency_loop_message(&[1, 2, 3, 4, 5]),
            "Would loop: #1 → #2 → #3 → #4 → #5"
        );
        assert_eq!(
            dependency_loop_message(&[1, 2, 3, 4, 5, 6, 1]),
            "Would loop: #1 → #2 → #3 → #4 → #5 → …"
        );
    }

    #[test]
    fn critical_path_is_the_dated_fs_chain_that_finishes_latest() {
        let mut a = node(1, None);
        a.start_date = Some("2026-07-06T00:00:00Z".into());
        a.target_date = Some("2026-07-10T00:00:00Z".into());
        let mut b = node(2, None);
        b.start_date = Some("2026-07-11T00:00:00Z".into());
        b.target_date = Some("2026-07-20T00:00:00Z".into());
        b.predecessors = vec![1];
        let mut c = node(3, None);
        c.start_date = Some("2026-07-11T00:00:00Z".into());
        c.target_date = Some("2026-07-25T00:00:00Z".into());
        c.predecessors = vec![1];
        let mut undated = node(4, None);
        undated.start_date = Some("2026-07-21T00:00:00Z".into());
        undated.target_date = None;
        undated.predecessors = vec![3];
        let path = critical_path(&[a, b, c, undated]);
        assert_eq!(
            path.ids,
            HashSet::from([1, 3]),
            "the chain ending on the latest Target Date is A→C, not A→B, got {:?}",
            path.ids
        );
        assert_eq!(path.links, HashSet::from([(1, 3)]));
        assert!(
            !path.ids.contains(&4),
            "start-only items are not critical-path vertices"
        );
        assert!(
            !path.ids.contains(&2),
            "the earlier-finishing branch is not on the critical path"
        );
    }

    #[test]
    fn related_dependency_ids_walk_the_whole_fs_chain() {
        let mut a = node(1, None);
        a.start_date = Some("2026-07-06T00:00:00Z".into());
        a.target_date = Some("2026-07-10T00:00:00Z".into());
        let mut b = node(2, None);
        b.start_date = Some("2026-07-11T00:00:00Z".into());
        b.target_date = Some("2026-07-20T00:00:00Z".into());
        b.predecessors = vec![1];
        let mut c = node(3, None);
        c.start_date = Some("2026-07-21T00:00:00Z".into());
        c.target_date = Some("2026-07-25T00:00:00Z".into());
        c.predecessors = vec![2];
        let d = node(4, None);
        let related = related_dependency_ids(&[a, b, c, d], 2);
        assert_eq!(
            related,
            HashSet::from([1, 2, 3]),
            "hovering the middle of A→B→C must highlight the whole chain, got {related:?}"
        );
    }

    #[test]
    fn critical_path_walks_back_through_the_latest_predecessor() {
        let mut a = node(1, None);
        a.target_date = Some("2026-07-10T00:00:00Z".into());
        let mut b = node(2, None);
        b.start_date = Some("2026-07-11T00:00:00Z".into());
        b.target_date = Some("2026-07-18T00:00:00Z".into());
        b.predecessors = vec![1];
        let mut c = node(3, None);
        c.start_date = Some("2026-07-11T00:00:00Z".into());
        c.target_date = Some("2026-07-22T00:00:00Z".into());
        c.predecessors = vec![1];
        let mut d = node(4, None);
        d.start_date = Some("2026-07-23T00:00:00Z".into());
        d.target_date = Some("2026-07-30T00:00:00Z".into());
        d.predecessors = vec![2, 3];
        let path = critical_path(&[a, b, c, d]);
        assert_eq!(path.ids, HashSet::from([1, 3, 4]));
        assert_eq!(path.links, HashSet::from([(1, 3), (3, 4)]));
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
            overflow_badge: false,
            floored: false,
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
        let pred_end = live_pred.x + live_pred.width;
        let stub = live_arrows[0].x1 - pred_end;
        assert!(
            (DependencyArrow::BACK_JOG_STUB - 1.0..=DependencyArrow::WRAP_STUB + 1.0)
                .contains(&stub),
            "outgoing stub must follow the live end edge, got {} vs pred_end={pred_end}",
            live_arrows[0].x1
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
                "{} → {} · 15d",
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
    fn day_header_keeps_the_month_and_today_number() {
        let today = Date::new(2026, 11, 2).unwrap();
        let gantt = GanttView {
            origin: today.saturating_sub(5.days()),
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
                .all(|(x, _)| (x - today_x).abs() < 2.0),
            "the 2 must stay on its column under the Today pill: {labels:?} today_x={today_x}"
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
    fn week_tick_does_not_sit_on_the_today_line() {
        let monday = Date::new(2026, 11, 2).unwrap();
        let mut gantt = GanttView {
            scale: GanttScale::Week,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.go_today(monday);
        let today_x = gantt.today_x(monday);
        let labels = gantt.header_tick_labels_except(800.0, Some(today_x));
        for (x, label) in &labels {
            let right = *x + header_label_px(label);
            assert!(
                right < today_x - 5.0 || *x > today_x + 5.0,
                "Week tick {label:?} at {x}..{right} must miss Today at {today_x}, labels={labels:?}"
            );
        }
        let tuesday = Date::new(2026, 11, 3).unwrap();
        gantt.go_today(tuesday);
        let today_x = gantt.today_x(tuesday);
        let labels = gantt.header_tick_labels_except(800.0, Some(today_x));
        assert!(
            labels.iter().all(|(_, label)| label != "Nov 2"),
            "Monday 'Nov 2' must not collide with Tuesday's Today line, got {labels:?}"
        );
        for (x, label) in &labels {
            let right = *x + header_label_px(label);
            assert!(
                right < today_x - 5.0 || *x > today_x + 5.0,
                "Week tick {label:?} at {x}..{right} must miss Today at {today_x}, labels={labels:?}"
            );
        }
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
        let ticks = gantt.header_tick_labels(800.0);
        let first = ticks.first().and_then(|(x, label)| {
            gantt
                .timeline_ticks(800.0)
                .into_iter()
                .filter(|tick| tick.label == *label)
                .min_by(|a, b| {
                    (a.x.max(0.0) - *x)
                        .abs()
                        .partial_cmp(&(b.x.max(0.0) - *x).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|tick| tick.date)
        });
        let last = ticks.last().and_then(|(x, label)| {
            gantt
                .timeline_ticks(800.0)
                .into_iter()
                .filter(|tick| tick.label == *label)
                .min_by(|a, b| {
                    (a.x.max(0.0) - *x)
                        .abs()
                        .partial_cmp(&(b.x.max(0.0) - *x).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|tick| tick.date)
        });
        let first = first.expect("painted Day start");
        let last = last.expect("painted Day end");
        assert!(
            title.starts_with(&compact_date(first))
                && title.contains('–')
                && title.contains(&compact_date(last)),
            "Day toolbar must match painted ticks {first}–{last}, got {title:?}"
        );
        let pixel_end = gantt.date_for_x(799.0);
        if pixel_end != last {
            assert!(
                !title.contains(&compact_date(pixel_end)),
                "Day toolbar must not name the last pixel ({pixel_end}) when the grid ends on {last}, got {title:?}"
            );
        }
        assert!(
            !title.eq(&compact_date(first)),
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
        // Under half a day into Oct 23's leftover column: the sticky "23"
        // still fits while date_for_x(0) rounds to Oct 24.
        gantt.zoom = 2.0;
        gantt.scroll_x = -40.0;
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
        let first = gantt.header_tick_labels(800.0);
        assert_eq!(
            first.first().map(|(_, label)| label.as_str()),
            Some("23"),
            "the leftover the toolbar names must be painted"
        );
    }

    #[test]
    fn year_axis_start_follows_the_painted_leftover_after_thinning() {
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        // Aug 1 lands at x = 30, too close for "Jul", but Year thinning drops
        // Aug so the leftover Jul keeps its slot.
        gantt.scroll_x = 16.5;
        let labels: Vec<String> = gantt
            .header_tick_labels(800.0)
            .into_iter()
            .take(2)
            .map(|(_, label)| label)
            .collect();
        assert_eq!(labels, ["Jul", "Sep"], "precondition: thinned Year axis");
        assert_eq!(
            gantt.viewport_axis_start_date(),
            Date::new(2026, 7, 1).unwrap(),
            "axis start must name the painted leftover Jul, not the thinned Aug"
        );
    }

    #[test]
    fn day_leftover_too_narrow_for_its_label_gives_way_to_the_next_day() {
        let mut gantt = GanttView {
            origin: Date::new(2026, 10, 24).unwrap(),
            scale: GanttScale::Day,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.scroll_x = -20.0;
        let labels: Vec<String> = gantt
            .header_tick_labels(800.0)
            .into_iter()
            .take(3)
            .map(|(_, label)| label)
            .collect();
        assert_eq!(
            labels,
            ["24", "25", "26"],
            "a 20px leftover must not hide the next day"
        );
        assert!(
            gantt.viewport_range_title().starts_with("Oct 24"),
            "toolbar must name the first painted day, got {:?}",
            gantt.viewport_range_title()
        );
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
        let (child_left, child_right) =
            bar_occupied_x(child, false, Some(gantt.timeline_width), None);
        let (live_left, live_right) = bar_occupied_x(go_live, false, None, None);
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
        let (left, right) = bar_occupied_x(mid, false, None, None);
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
            overflow_badge: false,
            floored: false,
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
            overflow_badge: false,
            floored: false,
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
            overflow_badge: false,
            floored: false,
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
            overflow_badge: false,
            floored: false,
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
            overflow_badge: false,
            floored: false,
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
        let (left1, right1) = bar_occupied_x(&mid1, false, None, None);
        let (left2, right2) = bar_occupied_x(&mid2, false, None, None);
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
            overflow_badge: false,
            floored: false,
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
            overflow_badge: false,
            floored: false,
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
            overflow_badge: false,
            floored: false,
        };
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let arrows = gantt.layout_dependency_arrows(&[pred.clone(), mid.clone(), succ]);
        assert_eq!(arrows.len(), 1);
        let stub = pred.x + pred.width + 12.0;
        let (cue_left, cue_right) = bar_occupied_x(&mid, false, Some(gantt.timeline_width), None);
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
            overflow_badge: false,
            floored: false,
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
            overflow_badge: false,
            floored: false,
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
            overflow_badge: false,
            floored: false,
        };
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let arrows = gantt.layout_dependency_arrows(&[pred, pill.clone(), succ.clone()]);
        assert_eq!(arrows.len(), 1);
        let (left, right) = bar_occupied_x(&pill, false, None, None);
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
        // column at succ.x-12 sits left of the source. Keep the wrap at
        // the source stub.
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
        let (cue_left, cue_right) = bar_occupied_x(child, false, Some(gantt.timeline_width), None);
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
        assert_inbound_drops_before_intervening_bar(arrow, cart, gantt.timeline_width);
        assert!(
            !arrow.wraps_around_successor(),
            "forward Persist cart → Go-live must not wrap along the Go-live row"
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
        assert_inbound_drops_before_intervening_bar(arrow, cart, gantt.timeline_width);
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 1013).unwrap();
        assert_exits_right_without_left_detour(arrow, pred, succ, gantt.timeline_width);
        assert!(
            !arrow.wraps_around_successor(),
            "Day Persist cart → Go-live must stay a forward elbow, not a wrap on the Go-live row, x1={} x2={} x_head={}",
            arrow.x1,
            arrow.x2,
            arrow.x_head
        );
        assert!(
            arrow.lane_y < succ.y || arrow.lane_y >= succ.y + ROW_HEIGHT,
            "Day Persist cart → Go-live must not run through the Go-live row, lane={} go-live={}..{}",
            arrow.lane_y,
            succ.y,
            succ.y + ROW_HEIGHT
        );
        assert!(
            arrow.lane_y >= pred.y && arrow.lane_y < pred.y + ROW_HEIGHT,
            "Day Persist cart → Go-live must stay in the Persist cart row gap, lane={} persist={}..{}",
            arrow.lane_y,
            pred.y,
            pred.y + ROW_HEIGHT
        );
    }

    #[test]
    fn day_view_mock_fixture_link_skips_the_go_live_row() {
        let nodes = mock_fixture_nodes();
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
        let pred = bars.iter().find(|bar| bar.id == 1003).unwrap();
        let succ = bars.iter().find(|bar| bar.id == 1013).unwrap();
        assert!(
            bar_title_is_inside(pred),
            "precondition: Day Persist cart paints an inside title, width={}",
            pred.width
        );
        assert!(
            !arrow.wraps_around_successor(),
            "forward Day Persist cart → Go-live must not wrap onto the Go-live row, x1={} x2={} x_head={} lane={}",
            arrow.x1,
            arrow.x2,
            arrow.x_head,
            arrow.lane_y
        );
        assert!(
            arrow.lane_y < succ.y || arrow.lane_y >= succ.y + ROW_HEIGHT,
            "Day Persist cart → Go-live must not run through the Go-live row, lane={} go-live={}..{}",
            arrow.lane_y,
            succ.y,
            succ.y + ROW_HEIGHT
        );
        assert!(
            arrow.lane_y >= pred.y && arrow.lane_y < pred.y + ROW_HEIGHT,
            "Day Persist cart → Go-live must stay in the Persist cart row, lane={} persist={}..{}",
            arrow.lane_y,
            pred.y,
            pred.y + ROW_HEIGHT
        );
        let box_left = succ.x;
        let box_right = succ.x + MILESTONE_SIZE;
        let box_top = succ.y + succ.paint_top();
        let box_bottom = box_top + succ.paint_h();
        let out_crosses = arrow.y1.min(arrow.lane_y) < box_bottom - 1.0
            && arrow.y1.max(arrow.lane_y) > box_top + 1.0;
        let in_crosses = arrow.lane_y.min(arrow.y2) < box_bottom - 1.0
            && arrow.lane_y.max(arrow.y2) > box_top + 1.0;
        if out_crosses {
            assert!(
                arrow.x1 < box_left - 1.0 || arrow.x1 > box_right + 1.0,
                "Day Persist cart → Go-live outgoing vertical must miss the Go-live box, x1={} box={box_left}..{box_right}",
                arrow.x1
            );
        }
        if in_crosses {
            assert!(
                arrow.x2 < box_left - 1.0 || arrow.x2 > box_right + 1.0,
                "Day Persist cart → Go-live inbound vertical must miss the Go-live box, x2={} box={box_left}..{box_right}",
                arrow.x2
            );
        }
    }

    fn assert_arrow_misses_go_live_pill(
        arrow: &DependencyArrow,
        go_live: &BarLayout,
        timeline_w: f32,
        today_x: Option<f32>,
        clip_right: f32,
        label: &str,
    ) {
        let (left, right) =
            bar_occupied_x_on(go_live, true, Some(clip_right), today_x, Some(timeline_w));
        let pill_top = go_live.y;
        let pill_bottom = go_live.y + ROW_HEIGHT;
        let out_spans = arrow.y1.min(arrow.lane_y) < pill_bottom - 1.0
            && arrow.y1.max(arrow.lane_y) > pill_top + 1.0;
        let inv_spans = arrow.lane_y.min(arrow.y2) < pill_bottom - 1.0
            && arrow.lane_y.max(arrow.y2) > pill_top + 1.0;
        if out_spans {
            assert!(
                !x_hits_span(arrow.x1, left, right),
                "{label}: outgoing vertical must miss the Go-live pill, x1={} occupied={left}..{right} go-live y={}..{}",
                arrow.x1,
                pill_top,
                pill_bottom
            );
        }
        if inv_spans || arrow.wraps_around_successor() {
            assert!(
                !x_hits_span(arrow.x2, left, right),
                "{label}: inbound/wrap vertical must miss the Go-live pill, x2={} occupied={left}..{right} wrap={} go-live y={}..{}",
                arrow.x2,
                arrow.wraps_around_successor(),
                pill_top,
                pill_bottom
            );
        }
        if arrow.lane_y > pill_top + 1.0 && arrow.lane_y < pill_bottom - 1.0 {
            let span = arrow_horizontal_span(arrow);
            assert!(
                span.1 < left - 4.0 || span.0 > right + 4.0,
                "{label}: horizontal on the Go-live row must route around the pill, span={:?} occupied={left}..{right} lane={}",
                span,
                arrow.lane_y
            );
        }
    }

    #[test]
    fn day_persist_to_spike_misses_the_go_live_pill() {
        let nodes = live_fixture_nodes();
        for (label, width, auto_fit) in [
            ("day-2400", 2400.0, false),
            ("day-auto-800", 800.0, true),
            ("day-auto-512", 512.0, true),
            // Default Day clip: Go-live is off-right and paints the wide
            // outlined pill at the edge (reviewer ~880–1025).
            ("day-512-off-range", 512.0, false),
        ] {
            let mut gantt = GanttView {
                origin: Date::new(2026, 7, 1).unwrap(),
                scale: GanttScale::Day,
                timeline_width: width,
                ..Default::default()
            };
            if auto_fit {
                gantt.auto_fit(&nodes, &[], width);
                gantt.timeline_width = width;
            }
            let rows = gantt.flatten(&nodes);
            let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
            let arrows = gantt.layout_dependency_arrows(&bars);
            let arrow = arrows
                .iter()
                .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
                .expect("Persist cart → spike");
            let go_live = bars.iter().find(|bar| bar.id == 1013).unwrap();
            let today = jiff::Zoned::now().date();
            let today_px = gantt.x_for_date(today);
            let today_x = (today_px >= 0.0 && today_px <= gantt.timeline_width).then_some(today_px);
            assert_arrow_misses_go_live_pill(
                arrow,
                go_live,
                gantt.timeline_width,
                today_x,
                (gantt.timeline_width - 10.0).max(24.0),
                label,
            );
        }
    }

    #[test]
    fn day_off_range_persist_to_go_live_misses_the_edge_chip() {
        let mut nodes = live_fixture_nodes();
        if let Some(persist) = nodes.iter_mut().find(|node| node.id == 1003) {
            persist.start_date = Some("2026-07-22T00:00:00Z".into());
            persist.target_date = Some("2026-08-05T00:00:00Z".into());
        }
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Day,
            timeline_width: 1024.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let arrows = gantt.layout_dependency_arrows(&bars);
        let arrow = arrows
            .iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("Persist cart → Go-live");
        let go_live = bars.iter().find(|bar| bar.id == 1013).unwrap();
        assert!(
            bar_offscreen_edge(go_live, gantt.timeline_width).is_some(),
            "precondition: Day Go-live must be off-range, x={}",
            go_live.x
        );
        let today = jiff::Zoned::now().date();
        let today_px = gantt.x_for_date(today);
        let today_x = (today_px >= 0.0 && today_px <= gantt.timeline_width).then_some(today_px);
        assert_arrow_misses_go_live_pill(
            arrow,
            go_live,
            gantt.timeline_width,
            today_x,
            (gantt.timeline_width - 10.0).max(24.0),
            "day-1024-persist-jul22-aug5",
        );
    }

    #[test]
    fn month_persist_to_spike_still_misses_the_go_live_pill() {
        let nodes = live_fixture_nodes();
        for width in [512.0, 800.0] {
            let gantt = GanttView {
                origin: Date::new(2026, 7, 1).unwrap(),
                scale: GanttScale::Month,
                timeline_width: width,
                ..Default::default()
            };
            let rows = gantt.flatten(&nodes);
            let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
            let arrows = gantt.layout_dependency_arrows(&bars);
            let arrow = arrows
                .iter()
                .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1012)
                .expect("Persist cart → spike");
            let go_live = bars.iter().find(|bar| bar.id == 1013).unwrap();
            let today = jiff::Zoned::now().date();
            let today_px = gantt.x_for_date(today);
            let today_x = (today_px >= 0.0 && today_px <= gantt.timeline_width).then_some(today_px);
            assert_arrow_misses_go_live_pill(
                arrow,
                go_live,
                gantt.timeline_width,
                today_x,
                (gantt.timeline_width - 10.0).max(24.0),
                &format!("month-{width}"),
            );
        }
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
            "same-start overlap must keep the short backward loop, x1={} x2={} x_head={}",
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
        assert_arrow_stays_on_screen(arrow, gantt.timeline_width);
        let pred_end = bar_end(pred);
        assert!(
            arrow.x1 + 0.5 >= pred_end,
            "must still leave the source to the right, x1={} pred_end={pred_end}",
            arrow.x1
        );
        assert!(
            (arrow.x_head - succ.x).abs() < 3.0,
            "finish-to-start must enter the target start, x_head={} succ.x={}",
            arrow.x_head,
            succ.x
        );
        let child = bars.iter().find(|bar| bar.id == 1005).unwrap();
        let go_live = bars.iter().find(|bar| bar.id == 1013).unwrap();
        let (child_left, child_right) = bar_occupied_x(child, false, None, None);
        let (live_left, live_right) = bar_occupied_x(go_live, false, None, None);
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
            overflow_badge: false,
            floored: false,
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
            overflow_badge: false,
            floored: false,
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
            overflow_badge: false,
            floored: false,
        };
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            timeline_width: 800.0,
            ..Default::default()
        };
        let arrows = gantt.layout_dependency_arrows(&[pred, mid.clone(), succ.clone()]);
        assert_eq!(arrows.len(), 1);
        let (left, right) = bar_occupied_x(&mid, true, None, None);
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
        // Same-date bars: the drop sits one stub past the floored predecessor.
        assert!(
            head_span < YEAR_MIN_BAR_WIDTH + DependencyArrow::WRAP_STUB + 4.0,
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
    fn milestone_hover_tooltip_lists_title_and_date() {
        let mut item = node(1013, None);
        item.title = "Go-live milestone".into();
        item.start_date = None;
        item.target_date = Some("2026-08-15T00:00:00Z".into());
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(std::slice::from_ref(&item));
        let bars = gantt.layout_bars(std::slice::from_ref(&item), &rows, &[], 720.0);
        assert!(bars[0].milestone, "Go-live must layout as a diamond");
        let tip = bar_hover_tooltip(&bars[0], std::slice::from_ref(&item));
        assert!(
            tip.contains("Go-live milestone") && (tip.contains("Aug") || tip.contains("15")),
            "milestone hover tooltip must show the title and date, got {tip:?}"
        );
    }

    #[test]
    fn clear_hover_drops_a_pinned_bar_tooltip() {
        let mut gantt = GanttView {
            pin_bar_tooltip: Some(2),
            hovered_id: Some(2),
            bar_hovered_id: Some(2),
            ..Default::default()
        };
        gantt.clear_hover();
        assert_eq!(gantt.pin_bar_tooltip, None);
        assert_eq!(gantt.hovered_id, None);
        assert_eq!(gantt.bar_hovered_id, None);
    }

    #[test]
    fn clear_row_hover_keeps_a_pinned_bar_tooltip() {
        let mut gantt = GanttView {
            pin_bar_tooltip: Some(2),
            hovered_id: Some(2),
            bar_hovered_id: Some(2),
            ..Default::default()
        };
        gantt.clear_row_hover();
        assert_eq!(gantt.pin_bar_tooltip, Some(2));
        assert_eq!(gantt.hovered_id, None);
        assert_eq!(gantt.bar_hovered_id, None);
    }

    #[test]
    fn foreign_hit_target_clears_a_pinned_bar_tooltip() {
        let mut gantt = GanttView {
            pin_bar_tooltip: Some(1002),
            hovered_id: Some(1002),
            bar_hovered_id: Some(1002),
            ..Default::default()
        };
        assert!(
            !gantt.note_hit_target(1002),
            "the pinned bar's own badge or handle must keep the dates chip"
        );
        assert_eq!(gantt.pin_bar_tooltip, Some(1002));
        assert_eq!(gantt.bar_hovered_id, Some(1002));
        assert!(
            gantt.note_hit_target(1003),
            "Persist cart's ! must unpin Cart's dates chip"
        );
        assert_eq!(gantt.pin_bar_tooltip, None);
        assert_eq!(
            gantt.bar_hovered_id, None,
            "Cart resize/link handles must clear when the pin moves to another row"
        );
        assert_eq!(gantt.hovered_id, None);
        gantt.pin_bar_tooltip = Some(1002);
        gantt.bar_hovered_id = Some(1002);
        assert!(
            gantt.note_hit_target(0),
            "a link or arrowhead is not the pinned bar"
        );
        assert_eq!(gantt.pin_bar_tooltip, None);
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
    fn scheduled_span_ignores_unscheduled_items() {
        let mut item = node(1, None);
        item.start_date = None;
        item.target_date = None;
        assert!(GanttView::scheduled_span(std::slice::from_ref(&item), &[]).is_none());
        let dated = node(2, None);
        assert!(GanttView::scheduled_span(std::slice::from_ref(&dated), &[]).is_some());
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
        let year_majors = year.header_major_labels(800.0);
        let first = year_start.year().to_string();
        let last = year_majors
            .last()
            .map(|(_, label)| label.as_str())
            .unwrap_or(first.as_str());
        assert!(
            year_title.contains(&first),
            "Year title {year_title:?} must name the start year {first}, majors={year_majors:?}"
        );
        assert!(
            year_title.contains(last),
            "Year title {year_title:?} must cover the last painted year {last}, majors={year_majors:?}"
        );
        if first != last {
            assert!(
                year_title.contains('–'),
                "Year spanning {first}–{last} must use an en dash, got {year_title:?}"
            );
        }
    }

    #[test]
    fn year_header_and_title_stop_at_the_clip() {
        // Zoomed-out Year: Jan 2029 sits just past the clip. Its banner and
        // tick must not slide onto the edge, and the title ends on the last
        // visible day of 2028.
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 0.6,
            timeline_width: 800.0,
            ..Default::default()
        };
        let jan_2029 = gantt.x_for_date(Date::new(2029, 1, 1).unwrap());
        let pixel_end = gantt.date_for_x(799.0);
        assert!(
            jan_2029 > 800.0 && jan_2029 < 800.0 + MAJOR_LABEL_ESTIMATE,
            "precondition: Jan 2029 just past the clip, got x={jan_2029}"
        );
        let majors = gantt.header_major_labels(800.0);
        let ticks = gantt.header_tick_labels(800.0);
        assert!(
            !majors.iter().any(|(_, label)| label == "2029"),
            "an off-screen 2029 must not paint, majors={majors:?}"
        );
        assert_ne!(
            ticks.last().map(|(_, label)| label.as_str()),
            Some("Jan"),
            "an off-screen Jan 2029 tick must not paint, ticks={ticks:?}"
        );
        let title = gantt.viewport_range_title();
        assert!(
            title.ends_with(&format!(
                "{}, {}",
                compact_date(pixel_end),
                pixel_end.year()
            )),
            "Year title {title:?} must end on the last visible day {pixel_end}"
        );
        assert!(
            !title.contains("2029"),
            "title {title:?} names off-screen 2029"
        );
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
        let q3_x = gantt.x_for_date(Date::new(2026, 7, 1).unwrap());
        assert!(
            q3_x > 0.0 && q3_x < 48.0,
            "precondition: Q3 cramped against the left edge, got x={q3_x}"
        );
        assert_eq!(
            labels.first().map(|(_, label)| label.as_str()),
            Some("Q3"),
            "Quarter axis must drop the cramped leftover Q2, got {labels:?}"
        );
        assert!(
            title.starts_with("Jul 1, 2026 – "),
            "cramped leftover Q2 must not title the range, got {title:?} labels={labels:?}"
        );
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
    fn month_today_label_does_not_crowd_nov() {
        let today = Date::new(2026, 11, 2).unwrap();
        let gantt = GanttView {
            origin: today.saturating_sub(40.days()),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let today_x = gantt.today_x(today);
        let ticks = gantt.header_tick_labels_except(800.0, Some(today_x));
        let Some(nov) = ticks.iter().find(|(_, label)| label == "Nov") else {
            panic!("Month ticks must keep Nov, got {ticks:?}");
        };
        let nov_x = gantt.x_for_date(Date::new(2026, 11, 1).unwrap());
        assert!(
            (nov.0 - nov_x).abs() < 2.0,
            "Month Nov must stay on its column, not dodge Today, nov={} column={nov_x} today={today_x} ticks={ticks:?}",
            nov.0
        );
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
    fn year_go_today_in_november_keeps_january() {
        let today = Date::new(2026, 11, 2).unwrap();
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Year,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        gantt.go_today(today);
        let ticks = gantt.header_tick_labels(800.0);
        let names: Vec<&str> = ticks.iter().map(|(_, label)| label.as_str()).collect();
        assert!(
            names.contains(&"Jan"),
            "Year axis centered on Nov must still paint January, got {names:?}"
        );
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
    fn bar_hover_tooltip_explains_the_overflow_bang() {
        let mut parent = node(1, None);
        parent.title = "Cart".into();
        parent.start_date = Some("2026-07-01T00:00:00Z".into());
        parent.target_date = Some("2026-07-10T00:00:00Z".into());
        let mut child = node(2, Some(1));
        child.title = "Persist cart".into();
        child.start_date = Some("2026-07-06T00:00:00Z".into());
        child.target_date = Some("2026-07-20T00:00:00Z".into());
        let nodes = vec![parent, child];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let persist = bars.iter().find(|bar| bar.id == 2).expect("Persist cart");
        let tip = bar_hover_tooltip(persist, &nodes);
        assert!(
            tip.contains('!')
                && tip.contains("Outside parent's dates")
                && tip.contains("Cart")
                && tip.contains("extends outside"),
            "Persist cart tooltip must explain the ! after Cart moved, got {tip:?}"
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
    fn toolbar_day_week_month_keeps_the_centre_date() {
        let today = Date::new(2026, 11, 3).unwrap();
        let mut gantt = GanttView {
            origin: Date::new(2026, 9, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let centre = gantt.viewport_center_date();
        gantt.apply_toolbar_scale(GanttScale::Week, today, None);
        assert_eq!(
            gantt.viewport_center_date(),
            centre,
            "Month→Week must keep the centred date, not jump the visible range"
        );
        gantt.apply_toolbar_scale(GanttScale::Day, today, None);
        assert_eq!(
            gantt.viewport_center_date(),
            centre,
            "Week→Day must keep the centred date"
        );
        gantt.apply_toolbar_scale(GanttScale::Month, today, None);
        assert_eq!(
            gantt.viewport_center_date(),
            centre,
            "Day→Month must restore the centred date"
        );
    }

    #[test]
    fn toolbar_day_week_keeps_the_selected_date() {
        let today = Date::new(2026, 11, 3).unwrap();
        let keep = Date::new(2026, 10, 12).unwrap();
        let mut gantt = GanttView {
            origin: Date::new(2026, 9, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let before = gantt.x_for_date(keep);
        assert!(
            (0.0..=gantt.timeline_width).contains(&before),
            "precondition: selection must start on-screen, x={before}"
        );
        gantt.apply_toolbar_scale(GanttScale::Week, today, Some(keep));
        let after = gantt.x_for_date(keep);
        assert!(
            (after - before).abs() < 2.0,
            "Month→Week must keep the selection at its screen x, not centre it, x={after} was {before}"
        );
        gantt.apply_toolbar_scale(GanttScale::Day, today, Some(keep));
        let day_x = gantt.x_for_date(keep);
        assert!(
            (day_x - before).abs() < 2.0,
            "Week→Day must keep the selection at its screen x, x={day_x} was {before}"
        );
    }

    #[test]
    fn toolbar_week_day_ignores_an_off_screen_selection() {
        let today = Date::new(2026, 11, 3).unwrap();
        let keep = Date::new(2026, 7, 6).unwrap();
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 20).unwrap(),
            scale: GanttScale::Week,
            zoom: 1.0,
            timeline_width: 840.0,
            ..Default::default()
        };
        assert!(
            gantt.x_for_date(keep) < 0.0,
            "precondition: the selection starts before the visible range"
        );
        let centre = gantt.viewport_center_date();
        gantt.apply_toolbar_scale(GanttScale::Day, today, Some(keep));
        assert_eq!(
            gantt.viewport_center_date(),
            centre,
            "Week→Day must keep the visible range, not jump to an off-screen selection, range={}",
            gantt.viewport_range_title()
        );
        gantt.apply_toolbar_scale(GanttScale::Week, today, Some(keep));
        assert_eq!(
            gantt.viewport_center_date(),
            centre,
            "Day→Week must keep the visible range too, range={}",
            gantt.viewport_range_title()
        );
    }

    #[test]
    fn toolbar_scale_round_trip_restores_the_identical_range() {
        let today = Date::new(2026, 11, 3).unwrap();
        let cart = Date::new(2026, 7, 6).unwrap();
        let mut gantt = GanttView {
            origin: Date::new(2026, 6, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 0.5,
            timeline_width: 800.0,
            ..Default::default()
        };
        let range = gantt.viewport_range_title();
        let cart_x = gantt.x_for_date(cart);
        assert!(
            (0.0..=gantt.timeline_width).contains(&cart_x),
            "precondition: Cart start must be on-screen, x={cart_x} range={range}"
        );
        for _ in 0..2 {
            gantt.apply_toolbar_scale(GanttScale::Week, today, Some(cart));
            gantt.apply_toolbar_scale(GanttScale::Day, today, Some(cart));
            gantt.apply_toolbar_scale(GanttScale::Month, today, Some(cart));
            assert_eq!(
                gantt.viewport_range_title(),
                range,
                "Month→Week→Day→Month must restore {range}, got {}",
                gantt.viewport_range_title()
            );
        }
        assert_eq!(gantt.scale, GanttScale::Month);
        let after = gantt.x_for_date(cart);
        assert!(
            (after - cart_x).abs() < 2.0,
            "the selection must stay at its screen x, not drift, x={after} was {cart_x}"
        );
        gantt.apply_toolbar_scale(GanttScale::Day, today, None);
        gantt.apply_toolbar_scale(GanttScale::Month, today, None);
        assert_eq!(
            gantt.viewport_range_title(),
            range,
            "a plain Day→Month must restore {range} (not shift a month), got {}",
            gantt.viewport_range_title()
        );
    }

    #[test]
    fn toolbar_selection_round_trip_stays_on_the_selected_date() {
        let today = Date::new(2026, 11, 3).unwrap();
        let keep = Date::new(2026, 10, 12).unwrap();
        let mut gantt = GanttView {
            origin: Date::new(2026, 9, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let range = gantt.viewport_range_title();
        let keep_x = gantt.x_for_date(keep);
        for _ in 0..2 {
            gantt.apply_toolbar_scale(GanttScale::Week, today, Some(keep));
            gantt.apply_toolbar_scale(GanttScale::Day, today, Some(keep));
            gantt.apply_toolbar_scale(GanttScale::Month, today, Some(keep));
        }
        assert_eq!(
            gantt.viewport_range_title(),
            range,
            "a second Day/Week/Month round trip must restore {range}, got {}",
            gantt.viewport_range_title()
        );
        let x = gantt.x_for_date(keep);
        assert!(
            (x - keep_x).abs() < 2.0,
            "a second round trip must keep the selection at its screen x, not centre it, x={x} was {keep_x}"
        );
        assert_eq!(gantt.scale, GanttScale::Month);
    }

    #[test]
    fn toolbar_reclick_of_current_scale_does_not_pan() {
        let today = Date::new(2026, 11, 3).unwrap();
        let keep = Date::new(2026, 7, 6).unwrap();
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            scroll_x: 40.0,
            ..Default::default()
        };
        let scroll = gantt.scroll_x;
        let keep_x = gantt.x_for_date(keep);
        let range = gantt.viewport_range_title();
        let centre_x = gantt.timeline_width * 0.5;
        assert!(
            (0.0..=gantt.timeline_width).contains(&keep_x),
            "precondition: selection must be on-screen, x={keep_x}"
        );
        assert!(
            (keep_x - centre_x).abs() > 8.0,
            "precondition: selection is not already centred, keep_x={keep_x} centre={centre_x}"
        );
        gantt.apply_toolbar_scale(GanttScale::Month, today, Some(keep));
        assert_eq!(gantt.scale, GanttScale::Month);
        assert!(
            (gantt.scroll_x - scroll).abs() < 0.5,
            "re-clicking Month must not pan, scroll {} vs {scroll}",
            gantt.scroll_x
        );
        let after = gantt.x_for_date(keep);
        assert!(
            (after - keep_x).abs() < 0.5,
            "re-clicking Month must not move the selection, x={after} was {keep_x}"
        );
        assert_eq!(gantt.viewport_range_title(), range);
    }

    #[test]
    fn toolbar_today_then_scale_with_selection_stays_on_today() {
        let today = Date::new(2026, 11, 3).unwrap();
        let keep = Date::new(2026, 7, 6).unwrap();
        for scale in [GanttScale::Week, GanttScale::Day, GanttScale::Month] {
            let mut gantt = GanttView {
                origin: Date::new(2026, 7, 1).unwrap(),
                scale: GanttScale::Month,
                zoom: 1.0,
                timeline_width: 800.0,
                ..Default::default()
            };
            gantt.go_today(today);
            let aligned = gantt.timeline_width.max(1.0) * TODAY_ALIGN;
            assert!(
                (gantt.x_for_date(today) - aligned).abs() < 2.0,
                "precondition: Today must be centred"
            );
            gantt.apply_toolbar_scale(scale, today, Some(keep));
            assert_eq!(gantt.scale, scale);
            assert!(
                gantt.keep_today,
                "{scale:?} after Today must keep Today-follow"
            );
            let after = gantt.x_for_date(today);
            assert!(
                (after - aligned).abs() < 2.0,
                "Today then {scale:?} with a July selection must stay on Today, after={after} aligned={aligned}"
            );
            let keep_x = gantt.x_for_date(keep);
            assert!(
                (keep_x - aligned).abs() > 50.0,
                "must not jump onto the selected start, keep_x={keep_x} aligned={aligned}"
            );
        }
    }

    #[test]
    fn toolbar_month_year_month_with_selection_round_trips() {
        let today = Date::new(2026, 11, 3).unwrap();
        let keep = Date::new(2026, 7, 6).unwrap();
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 0.6,
            timeline_width: 800.0,
            ..Default::default()
        };
        let range = gantt.viewport_range_title();
        let left = gantt.date_for_x(0.0);
        let jul = Date::new(2026, 7, 1).unwrap();
        let jul_x = gantt.x_for_date(jul);
        let keep_x = gantt.x_for_date(keep);
        assert!(
            (0.0..=gantt.timeline_width).contains(&keep_x),
            "precondition: selection must be on-screen, x={keep_x}"
        );
        gantt.apply_toolbar_scale(GanttScale::Year, today, Some(keep));
        gantt.apply_toolbar_scale(GanttScale::Month, today, Some(keep));
        assert_eq!(gantt.scale, GanttScale::Month);
        assert_eq!(
            gantt.viewport_range_title(),
            range,
            "Month→Year→Month with a selection must restore {range}, got {}",
            gantt.viewport_range_title()
        );
        assert_eq!(
            gantt.date_for_x(0.0),
            left,
            "left-edge date must round-trip, got {}",
            gantt.date_for_x(0.0)
        );
        assert!(
            (gantt.x_for_date(jul) - jul_x).abs() < 1.0,
            "Jul must stay at the same screen x, {} vs {jul_x}",
            gantt.x_for_date(jul)
        );
        let after_keep = gantt.x_for_date(keep);
        assert!(
            (after_keep - keep_x).abs() < 2.0,
            "selection must not be recentred, x={after_keep} was {keep_x}"
        );
    }

    #[test]
    fn persist_cart_label_moves_with_the_bar_after_jul_22() {
        let mut nodes = live_fixture_nodes();
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            zoom: 1.0,
            timeline_width: 800.0,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let before_bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let before = before_bars
            .iter()
            .find(|bar| bar.id == 1003)
            .expect("Persist cart");
        let before_x = before.x;
        let before_label =
            bar_outside_label_frame(before, 800.0, true, None, before.overflow_badge)
                .map(|(x, _)| x)
                .unwrap_or(before.x);
        if let Some(node) = nodes.iter_mut().find(|node| node.id == 1003) {
            node.start_date = Some("2026-07-22T00:00:00Z".into());
            node.target_date = Some("2026-08-05T00:00:00Z".into());
        }
        let after_bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let after = after_bars
            .iter()
            .find(|bar| bar.id == 1003)
            .expect("moved Persist cart");
        let after_label = bar_outside_label_frame(after, 800.0, true, None, after.overflow_badge)
            .map(|(x, w)| {
                assert!(
                    outside_label_is_beside_bar(after, x, w, true, after.overflow_badge),
                    "Jul 22→Aug 5 Persist cart title must stay beside the bar, label={x}+{w} bar.x={} width={}",
                    after.x,
                    after.width
                );
                x
            })
            .unwrap_or(after.x);
        let bar_dx = after.x - before_x;
        let label_dx = after_label - before_label;
        assert!(
            bar_dx.abs() > 8.0,
            "precondition: Jul 6→Jul 22 must move the bar, dx={bar_dx}"
        );
        assert!(
            (label_dx - bar_dx).abs() < 8.0,
            "the title must travel with the bar, not sit ~68px behind, bar_dx={bar_dx} label_dx={label_dx}"
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

    fn dated(id: i64, start: &str, target: &str, preds: &[i64]) -> WorkItemNode {
        WorkItemNode {
            start_date: Some(format!("{start}T00:00:00Z")),
            target_date: Some(format!("{target}T00:00:00Z")),
            predecessors: preds.to_vec(),
            ..node(id, None)
        }
    }

    fn marks_for(gantt: &GanttView, nodes: &[WorkItemNode]) -> ClashMarks {
        let rows = gantt.flatten(nodes);
        let bars = gantt.layout_bars(nodes, &rows, &[], 720.0);
        let loaded = ado_core::clashes(nodes);
        gantt.clash_marks(&loaded, nodes, &HierarchyIndex::new(nodes), &bars)
    }

    #[test]
    fn clash_takes_the_warning_tone_and_keeps_the_critical_width() {
        let plain = dependency_arrow_paint(ArrowState::default());
        assert_eq!(plain.tone, ArrowTone::Plain);
        assert_eq!(plain.stroke, 2.0);
        let clash = dependency_arrow_paint(ArrowState {
            clash: true,
            critical_path_on: true,
            ..ArrowState::default()
        });
        assert_eq!(clash.tone, ArrowTone::Clash);
        assert_eq!(clash.stroke, 2.0);
        let critical = dependency_arrow_paint(ArrowState {
            on_critical: true,
            critical_path_on: true,
            ..ArrowState::default()
        });
        let critical_clash = dependency_arrow_paint(ArrowState {
            clash: true,
            on_critical: true,
            critical_path_on: true,
            ..ArrowState::default()
        });
        assert_eq!(critical.tone, ArrowTone::Critical);
        assert_eq!(critical_clash.tone, ArrowTone::Clash);
        assert_eq!(critical_clash.stroke, critical.stroke);
        let hovered_clash = dependency_arrow_paint(ArrowState {
            clash: true,
            focused: true,
            ..ArrowState::default()
        });
        assert_eq!(hovered_clash.tone, ArrowTone::Clash);
    }

    #[test]
    fn schedule_badge_lists_clashes_then_move_failures() {
        let badge = ScheduleBadge {
            clashes: vec![
                ClashLine {
                    predecessor_id: 12,
                    starts: Date::new(2026, 7, 9).unwrap(),
                    predecessor_finishes: Date::new(2026, 7, 10).unwrap(),
                },
                ClashLine {
                    predecessor_id: 14,
                    starts: Date::new(2026, 7, 9).unwrap(),
                    predecessor_finishes: Date::new(2026, 7, 15).unwrap(),
                },
            ],
            failures: vec!["The Work Item was changed by someone else.".into()],
        };
        assert_eq!(
            badge.tooltip(),
            "Starts Jul 9 · #12 finishes Jul 10\n\
             Starts Jul 9 · #14 finishes Jul 15\n\
             Couldn't move: The Work Item was changed by someone else."
        );
        assert!(
            badge
                .accessibility_label()
                .starts_with("Schedule warning: Starts Jul 9 · #12 finishes Jul 10; ")
        );
        assert!(ScheduleBadge::default().is_empty());
    }

    #[test]
    fn clash_marks_badge_the_successor_with_one_line_per_predecessor() {
        let nodes = vec![
            dated(12, "2026-07-01", "2026-07-10", &[]),
            dated(14, "2026-07-01", "2026-07-15", &[]),
            dated(16, "2026-07-01", "2026-07-05", &[]),
            dated(34, "2026-07-09", "2026-07-20", &[12, 14, 16]),
        ];
        let gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            ..Default::default()
        };
        let marks = marks_for(&gantt, &nodes);
        assert_eq!(marks.links, HashSet::from([(12, 34), (14, 34)]));
        assert_eq!(
            marks.badges[&34].lines(),
            vec![
                "Starts Jul 9 · #12 finishes Jul 10".to_string(),
                "Starts Jul 9 · #14 finishes Jul 15".to_string(),
            ]
        );
        assert_eq!(marks.badges.len(), 1);
    }

    #[test]
    fn live_drag_rejudges_its_links_and_cancel_restores_them() {
        let nodes = vec![
            dated(1, "2026-07-01", "2026-07-10", &[]),
            dated(2, "2026-07-11", "2026-07-20", &[1]),
            dated(3, "2026-07-21", "2026-07-30", &[2]),
        ];
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            ..Default::default()
        };
        assert!(marks_for(&gantt, &nodes).links.is_empty());
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let bar = bars.iter().find(|bar| bar.id == 2).unwrap().clone();
        let origin = bar.x + bar.width / 2.0;
        gantt
            .begin_gesture(&bar, GestureKind::Move, origin, &nodes, &[])
            .expect("move");
        assert_eq!(
            gantt.gesture.as_ref().unwrap().links.len(),
            2,
            "the press finds the bar's in and out links once"
        );
        let ppd = gantt.scale.pixels_per_day(gantt.zoom);
        gantt.update_gesture(origin - ppd * 3.0);
        let marks = marks_for(&gantt, &nodes);
        assert_eq!(marks.links, HashSet::from([(1, 2)]));
        assert_eq!(
            marks.badges[&2].lines(),
            vec!["Starts Jul 8 · #1 finishes Jul 10".to_string()]
        );
        gantt.update_gesture(origin + ppd * 3.0);
        let marks = marks_for(&gantt, &nodes);
        assert_eq!(marks.links, HashSet::from([(2, 3)]));
        assert!(marks.badges.contains_key(&3));
        gantt.cancel_gesture();
        assert_eq!(marks_for(&gantt, &nodes), ClashMarks::default());
    }

    #[test]
    fn resize_end_clashes_only_the_outgoing_link() {
        let nodes = vec![
            dated(1, "2026-07-01", "2026-07-10", &[]),
            dated(2, "2026-07-11", "2026-07-20", &[1]),
            dated(3, "2026-07-21", "2026-07-30", &[2]),
        ];
        let mut gantt = GanttView {
            origin: Date::new(2026, 7, 1).unwrap(),
            scale: GanttScale::Month,
            ..Default::default()
        };
        let rows = gantt.flatten(&nodes);
        let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
        let bar = bars.iter().find(|bar| bar.id == 2).unwrap().clone();
        let origin = bar.x + bar.width;
        gantt
            .begin_gesture(&bar, GestureKind::ResizeEnd, origin, &nodes, &[])
            .expect("resize");
        let ppd = gantt.scale.pixels_per_day(gantt.zoom);
        gantt.update_gesture(origin + ppd * 2.0);
        assert_eq!(
            marks_for(&gantt, &nodes).links,
            HashSet::from([(2, 3)]),
            "Target Jul 22 runs past #3's Jul 21 start"
        );
    }
}
