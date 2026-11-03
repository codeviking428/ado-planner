use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use ado_core::{
    AuthMode, FLAVORS, FormControlKind, color_for_type, compact_date, dates_column_text,
    format_date_range, identity_from_field, is_unscheduled, iso_date_only, parse_iso_date,
    parse_user_date,
};
use ado_session::PlannerSession;
use chrono::Datelike;
use gpui_kit::assets::IconName;
use gpui_kit::base::{ElementExt as _, Popup, Selectable};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable;
use gpui_kit::component::Sizable;
use gpui_kit::component::Size;
use gpui_kit::component::avatar::Avatar;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::calendar::{CalendarEvent, CalendarState, Date as PickerDate};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::text::TextView;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::component::{FocusableExt as _, Placement};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, CursorStyle, Entity, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyDownEvent, KeystrokeEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Render, Role, ScrollEvent, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, actions, div, px, rgb,
};
use jiff::ToSpan;

use crate::error::AppError;
use crate::flavor_theme::apply_flavor;
use crate::form::ado_edit_url_on;
use crate::gantt::{
    ASSIGNEE_COL_WIDTH, BarHitZone, CAMERA_EASE_MS, DATES_COL_WIDTH, DETAILS_PILL_WIDTH,
    DependencyArrow, EXPANDER_SLOT, GanttScale, GanttView, GestureKind, HANDLE_WIDTH, ID_COL_WIDTH,
    LEAF_BAR_H, LINK_HANDLE, LINK_HANDLE_HIT, MILESTONE_SIZE, MIN_TIMELINE_WIDTH, NO_FIELDS_HINT_W,
    NUDGE_IDLE_MS, NudgeAxis, OVERFLOW_BADGE, OVERFLOW_BADGE_GAP, PLACEHOLDER_BAR_H, ROW_HEIGHT,
    SCALE_BUTTON_WIDTH, SELECTED_ACCENT_W, SELECTED_ROW_TINT, SPLITTER_WIDTH, SUMMARY_BAR_H,
    SavedGanttView, TIMELINE_HEADER_H, TREE_INDENT, TYPE_COL_WIDTH, UNSCHEDULED_GUTTER_RIGHT,
    bar_hover_tooltip, bar_title_is_inside, header_label_px, nudge_today_off_ticks,
    parent_date_overflow, parent_overflow_tip, timeline_is_empty_of_scheduled, today_pill_left,
};
use crate::paths::AppPaths;
use crate::state::{
    DateSaveRole, PendingDateSave, PlannerState, coalesce_superseded_date_saves,
    undone_message as undone_toast_message,
};
use crate::toasts::{HistoryEntry, ToastExtra, ToastKind, UndoAction};

actions!(
    ado_planner,
    [Quit, NudgeLeft, NudgeRight, EscapeCancel, OpenSelected]
);

const DATES_RANGE_HINT: &str = "Start → Target, or start only";
const DATE_HINT: &str = "Date";
const DATES_ERROR_SLOT_H: f32 = 48.0;
const SAVING_HOLD_MS: u64 = 400;
const PROGRESS_ROLLUP_TIP: &str =
    "Progress is rolled up from child work-item states. Active counts as half done.";
const DATES_ROLLUP_TIP: &str = "Dates roll up from children";

fn debug_hit_enabled() -> bool {
    std::env::var("ADO_PLANNER_DEBUG_HIT")
        .map(|value| {
            value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InlineEdit {
    Title(i64),
    Start(i64),
    Target(i64),
    Dates(i64),
    Assignee(i64),
}

impl InlineEdit {
    fn item_id(self) -> i64 {
        match self {
            Self::Title(id)
            | Self::Start(id)
            | Self::Target(id)
            | Self::Dates(id)
            | Self::Assignee(id) => id,
        }
    }
}

pub struct PlannerView {
    pub state: PlannerState,
    org_input: Entity<InputState>,
    pat_input: Entity<InputState>,
    filter_query: Entity<InputState>,
    scope_query: Entity<InputState>,
    inline_input: Entity<InputState>,
    form_inputs: HashMap<String, Entity<InputState>>,
    form_dates: HashMap<String, Entity<CalendarState>>,
    form_dates_preview: Option<String>,
    form_inputs_for: Option<i64>,
    form_title_focused_for: Option<i64>,
    form_calendar_open: bool,
    form_dates_typed_seed: String,
    /// Calendar picks write here, then Render copies into the Dates input and seed.
    pending_form_dates_text: Option<String>,
    pending_form_picker: Option<PickerDate>,
    /// After Clear dates I/O, rewrite the dialog from the adopted form.
    sync_form_dates_after_io: bool,
    form_focus: FocusHandle,
    open_filter: Option<&'static str>,
    open_scope: Option<&'static str>,
    open_flavor: bool,
    inline_edit: Option<InlineEdit>,
    inline_dates: Option<Entity<CalendarState>>,
    /// Typed ISO text at picker open; used so a later calendar pick is not
    /// overwritten unless the user actually edited the field.
    inline_dates_typed_seed: String,
    inline_dates_error: Option<String>,
    pending_typed_preview: Option<String>,
    pending_delete: Option<(i64, String)>,
    /// True after Enter commits an inline editor so the same key does not
    /// also open the details dialog.
    inline_enter_guard: bool,
    dates_anchor: Option<(f32, f32)>,
    banner_dismissed: bool,
    form_loading: Option<(i64, String)>,
    io_busy: bool,
    saving_shown_at: Option<Instant>,
    saving_hold_gen: u64,
    /// Undo/Redo should not leave the 400ms Saving… hold in the header.
    skip_saving_hold: bool,
    date_save_busy: HashSet<i64>,
    date_save_queue: VecDeque<PendingDateSave>,
    pending_team: Option<String>,
    update_checked: bool,
    toast_expiry_armed: bool,
    toast_expiry_gen: u64,
    loading_shown_at: Option<std::time::Instant>,
    loading_hold_gen: u64,
    last_undo_at: Option<Instant>,
    last_redo_at: Option<Instant>,
    camera_anim: Option<CameraAnim>,
    camera_anim_gen: u64,
    pending_camera_from: Option<SavedGanttView>,
    skip_camera_anim: bool,
    /// True after a drag move with the left button down. A later move with
    /// the button up then commits, covering mouse-up outside the window.
    saw_pressed_drag: bool,
    /// Window-x where an unscheduled row press began; a click without travel
    /// must not auto-schedule.
    schedule_origin_x: Option<f32>,
    /// Window-x where an unscheduled gutter-pill press began. A release with
    /// no movement, or still in the gutter, must not PATCH dates.
    pill_press_window_x: Option<f32>,
    /// Window-x where a dated bar press began. A flick (mouse-up with no
    /// mouse-move) applies this delta so the release still commits.
    bar_press_window_x: Option<f32>,
    /// Last window-x while the bar-drag button was down. A missed mouse-up
    /// commits here, not at a later hover x (Jul 28 shrink).
    bar_last_window_x: Option<f32>,
    /// Shift was down when the bar press began. A parent Move then shifts
    /// descendants with it instead of offering "Move children too".
    bar_press_shift: bool,
    /// Live window-x while dragging to schedule an unscheduled row.
    schedule_live_x: Option<f32>,
    /// Nested hover count so moving onto Undo does not unpause expiry.
    toast_hover_depth: u32,
    /// Pointer is down on a toast; skip Gantt mouse-up commit for this click.
    toast_pointer: bool,
    /// Toast Undo clicked while spawn_state is busy; replay when I/O frees.
    pending_toast_undo: Option<(u64, UndoAction)>,
    /// True while a non-date toast Undo spawn is in flight, so a second
    /// click (mouse-down + click) cannot dismiss the toast as a no-op.
    toast_undo_inflight: bool,
    /// After Add item I/O, Render opens inline rename on this id.
    pending_inline_title: Option<i64>,
    /// After Add/Undo I/O, Render blurs leftover title-button focus ticks.
    pending_focus_planner: bool,
    /// Painted top of `gantt-timeline-body`, used so link-drop hit-testing
    /// matches the row bands instead of the toolbar-sized origin estimate.
    timeline_body_origin_y: Rc<Cell<f32>>,
    /// Painted left of `gantt-timeline-body`. Tree flex-shrink makes
    /// `tree_width + splitter` a miss, so bar mouse-down uses this.
    timeline_body_origin_x: Rc<Cell<Option<f32>>>,
    /// Painted bar left/right in window coordinates, filled from on_prepaint.
    painted_bar_bounds: Rc<RefCell<HashMap<i64, PaintedBar>>>,
    /// Last bar mouse-down classification (window x vs painted bounds).
    last_bar_hit: Option<BarHitDebug>,
    /// Start date of the last committed date gesture. Persist without a
    /// mock may revert the node; tests still see that the flick committed.
    last_committed_start_for_test: Option<jiff::civil::Date>,
    last_committed_target_for_test: Option<jiff::civil::Date>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Copy, Debug)]
struct PaintedBar {
    left: f32,
    right: f32,
    layout_x: f32,
    /// Prepaint content origin. Independent of the 1px/2px selection border
    /// so selecting a bar does not shift the live gesture mapping.
    content_left: f32,
}

/// Window-space hit recorded on every bar mouse-down for tests and DEBUG_HIT.
#[derive(Clone, Copy, Debug)]
pub struct BarHitDebug {
    pub event_x: f32,
    pub painted_left: f32,
    pub painted_right: f32,
    pub zone: BarHitZone,
    pub kind: GestureKind,
}

struct CameraAnim {
    from: SavedGanttView,
    to: SavedGanttView,
    start: Instant,
}

impl PlannerView {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        session: PlannerSession,
        paths: AppPaths,
        ado_base: Option<String>,
        vssps_base: Option<String>,
    ) -> Self {
        let state = PlannerState::new(paths, session, ado_base, vssps_base);
        apply_flavor(cx, state.flavor());
        let org_input = cx.new(|cx| InputState::new(window, cx));
        let pat_input = cx.new(|cx| InputState::new(window, cx).masked(true));
        let filter_query = cx.new(|cx| InputState::new(window, cx));
        let scope_query = cx.new(|cx| InputState::new(window, cx));
        let inline_input = cx.new(|cx| InputState::new(window, cx));
        let mut view = Self::from_session(
            window,
            cx,
            state,
            org_input,
            pat_input,
            filter_query,
            scope_query,
            inline_input,
        );
        view.spawn_state(cx, |mut state| async move {
            let result = state.bootstrap().await;
            (state, result)
        });
        view
    }

    #[allow(clippy::too_many_arguments)]
    fn from_session(
        window: &mut Window,
        cx: &mut Context<Self>,
        state: PlannerState,
        org_input: Entity<InputState>,
        pat_input: Entity<InputState>,
        filter_query: Entity<InputState>,
        scope_query: Entity<InputState>,
        inline_input: Entity<InputState>,
    ) -> Self {
        let view = cx.weak_entity();
        let keystrokes = cx.intercept_keystrokes(move |event: &KeystrokeEvent, window, cx| {
            let key = event.keystroke.key.as_str();
            let Some(view) = view.upgrade() else {
                return;
            };
            view.update(cx, |this, cx| {
                let overlay = this.details_overlay()
                    || this.state.gantt.context_menu.is_some()
                    || this.popup_open()
                    || this.inline_edit.is_some()
                    || this.pending_delete.is_some();
                if key == "f2"
                    && !this.details_overlay()
                    && this.inline_edit.is_none()
                    && let Some(id) = this.state.gantt.selected_id
                {
                    this.open_inline_title(id, window, cx);
                    cx.stop_propagation();
                    return;
                }
                if this.form_loading.is_some() && key != "escape" {
                    // Swallow keys while the modal is opening so they cannot
                    // land as a stray character in the title field.
                    cx.stop_propagation();
                    return;
                }
                if key == "escape" && overlay {
                    if this.details_overlay() && this.form_date_picker_focused(window, cx) {
                        return;
                    }
                    this.on_key(
                        &KeyDownEvent {
                            keystroke: event.keystroke.clone(),
                            is_held: false,
                            prefer_character_input: false,
                        },
                        cx,
                    );
                    cx.stop_propagation();
                    return;
                }
                if key == "enter" && this.inline_edit.is_some() {
                    this.on_key(
                        &KeyDownEvent {
                            keystroke: event.keystroke.clone(),
                            is_held: false,
                            prefer_character_input: false,
                        },
                        cx,
                    );
                    cx.stop_propagation();
                    return;
                }
                let mods = event.keystroke.modifiers;
                let undo = (mods.control || mods.platform) && key == "z";
                let redo =
                    (mods.control || mods.platform) && (key == "y" || (key == "z" && mods.shift));
                let delete_link = matches!(key, "delete" | "backspace")
                    && this.state.gantt.selected_link.is_some();
                let delete_item = matches!(key, "delete" | "backspace")
                    && this.pending_delete.is_none()
                    && this.state.gantt.selected_link.is_none()
                    && this.state.gantt.selected_id.is_some()
                    && !this.details_overlay()
                    && this.inline_edit.is_none()
                    && !this.input_focused(window, cx);
                // Enter confirms, Escape cancels. Delete/Backspace only open the
                // dialog — key-repeat has no is_held on intercept, so treating
                // them as confirm would delete on the first hold.
                let confirm_delete = this.pending_delete.is_some()
                    && matches!(key, "enter" | "escape" | "delete" | "backspace");
                let open_details = key == "enter"
                    && this.pending_delete.is_none()
                    && !this.io_busy
                    && !this.details_overlay()
                    && this.inline_edit.is_none()
                    && this.state.gantt.selected_id.is_some()
                    && !this.input_focused(window, cx);
                let nav = matches!(key, "up" | "down" | "pageup" | "pagedown" | "home" | "end");
                if undo || redo || delete_link || delete_item || confirm_delete || open_details {
                    this.on_key(
                        &KeyDownEvent {
                            keystroke: event.keystroke.clone(),
                            is_held: false,
                            prefer_character_input: false,
                        },
                        cx,
                    );
                    cx.stop_propagation();
                    return;
                }
                if !nav || overlay || this.input_focused(window, cx) {
                    return;
                }
                if this.form_focus.is_focused(window) {
                    // capture_key_down on the focused planner already handles
                    // Down/Up. Handling it here too skipped a row after a bar
                    // click focused the planner.
                    return;
                }
                this.on_key(
                    &KeyDownEvent {
                        keystroke: event.keystroke.clone(),
                        is_held: false,
                        prefer_character_input: false,
                    },
                    cx,
                );
                cx.stop_propagation();
            });
        });
        let mut this = Self {
            state,
            org_input,
            pat_input,
            filter_query,
            scope_query,
            inline_input,
            form_inputs: HashMap::new(),
            form_dates: HashMap::new(),
            form_dates_preview: None,
            form_inputs_for: None,
            form_title_focused_for: None,
            form_calendar_open: false,
            form_dates_typed_seed: String::new(),
            pending_form_dates_text: None,
            pending_form_picker: None,
            sync_form_dates_after_io: false,
            form_focus: cx.focus_handle(),
            open_filter: None,
            open_scope: None,
            open_flavor: false,
            inline_edit: None,
            inline_dates: None,
            inline_dates_typed_seed: String::new(),
            inline_dates_error: None,
            pending_typed_preview: None,
            pending_delete: None,
            inline_enter_guard: false,
            dates_anchor: None,
            banner_dismissed: false,
            form_loading: None,
            io_busy: false,
            saving_shown_at: None,
            saving_hold_gen: 0,
            skip_saving_hold: false,
            date_save_busy: HashSet::new(),
            date_save_queue: VecDeque::new(),
            pending_team: None,
            update_checked: false,
            toast_expiry_armed: false,
            toast_expiry_gen: 0,
            loading_shown_at: None,
            loading_hold_gen: 0,
            last_undo_at: None,
            last_redo_at: None,
            camera_anim: None,
            camera_anim_gen: 0,
            pending_camera_from: None,
            skip_camera_anim: false,
            saw_pressed_drag: false,
            schedule_origin_x: None,
            pill_press_window_x: None,
            bar_press_window_x: None,
            bar_last_window_x: None,
            bar_press_shift: false,
            schedule_live_x: None,
            toast_hover_depth: 0,
            toast_pointer: false,
            pending_toast_undo: None,
            toast_undo_inflight: false,
            pending_inline_title: None,
            pending_focus_planner: false,
            timeline_body_origin_y: Rc::new(Cell::new(0.0)),
            timeline_body_origin_x: Rc::new(Cell::new(None)),
            painted_bar_bounds: Rc::new(RefCell::new(HashMap::new())),
            last_bar_hit: None,
            last_committed_start_for_test: None,
            last_committed_target_for_test: None,
            _subscriptions: vec![keystrokes],
        };
        this.scope_query.update(cx, |input, cx| {
            input.set_placeholder("Search", window, cx);
        });
        this.filter_query.update(cx, |input, cx| {
            input.set_placeholder("Search", window, cx);
        });
        this.bind_sign_in_events(window, cx);
        this.bind_inline_input(cx);
        this
    }

    /// Build a view that does not fetch a session. Tests use this to render a
    /// prepared `PlannerState` without racing bootstrap.
    pub fn for_state(window: &mut Window, cx: &mut Context<Self>, state: PlannerState) -> Self {
        apply_flavor(cx, state.flavor());
        let org_input = cx.new(|cx| InputState::new(window, cx));
        let pat_input = cx.new(|cx| InputState::new(window, cx).masked(true));
        let filter_query = cx.new(|cx| InputState::new(window, cx));
        let scope_query = cx.new(|cx| InputState::new(window, cx));
        let inline_input = cx.new(|cx| InputState::new(window, cx));
        let mut view = Self::from_session(
            window,
            cx,
            state,
            org_input,
            pat_input,
            filter_query,
            scope_query,
            inline_input,
        );
        view.update_checked = true;
        view.skip_camera_anim = true;
        view
    }

    /// Tests turn easing back on (`for_state` skips it so snapshots stay stable).
    pub fn enable_camera_anim(&mut self) {
        self.skip_camera_anim = false;
    }

    /// Seed an in-flight camera blend so tests can click again mid-ease.
    pub fn seed_camera_anim(&mut self, from: SavedGanttView, elapsed_ms: u64) {
        let to = self.state.gantt.snapshot();
        self.camera_anim = Some(CameraAnim {
            from,
            to,
            start: Instant::now() - Duration::from_millis(elapsed_ms),
        });
    }

    /// Same path as the +/− buttons, for tests that must zoom without
    /// letting the 150 ms ease race the click.
    pub fn zoom_at_center_animated(&mut self, factor: f32, cx: &mut Context<Self>) {
        self.animate_gantt(cx, |gantt| {
            gantt.retarget_year_gutter(|gantt| {
                gantt.zoom_at_center(factor);
            });
        });
        cx.notify();
    }

    pub fn camera_anim_endpoints(&self) -> Option<(SavedGanttView, SavedGanttView)> {
        self.camera_anim.as_ref().map(|anim| (anim.from, anim.to))
    }

    pub fn window_title(&self) -> String {
        if !self.state.session_info.signed_in {
            return "ADO Planner".into();
        }
        match self.state.scope() {
            Some(scope) => format!(
                "ADO Planner — {} / {} / {}",
                scope.org, scope.project, scope.team
            ),
            None => "ADO Planner".into(),
        }
    }

    pub fn inline_placeholder(&self, cx: &App) -> String {
        self.inline_input
            .read(cx)
            .presentation()
            .placeholder()
            .to_string()
    }

    pub fn scope_placeholder(&self, cx: &App) -> String {
        self.scope_query
            .read(cx)
            .presentation()
            .placeholder()
            .to_string()
    }

    /// Tests type into the org/team/assignee Search box.
    pub fn type_scope_search(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.scope_query.update(cx, |input, cx| {
            input.set_value(value, window, cx);
        });
        cx.notify();
    }

    pub fn open_scope_menu(&mut self, id: &'static str, cx: &mut Context<Self>) {
        self.open_filter = None;
        self.open_flavor = false;
        self.state.gantt.context_menu = None;
        self.open_scope = Some(id);
        cx.notify();
    }

    pub fn set_open_flavor(&mut self, open: bool) {
        self.open_filter = None;
        self.open_scope = None;
        self.state.gantt.context_menu = None;
        self.open_flavor = open;
    }

    /// Open the inline title editor. Tests use this to assert painted width.
    pub fn open_inline_title(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let title = self
            .state
            .nodes
            .iter()
            .find(|node| node.id == id)
            .map(|node| node.title.clone())
            .unwrap_or_default();
        self.begin_inline(InlineEdit::Title(id), title, window, cx);
    }

    /// Title click: single-click selects, double-click opens details.
    /// Rename with F2 or the context-menu action.
    pub fn activate_title(
        &mut self,
        id: i64,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.gantt.select_row(id);
        if click_count >= 2 {
            self.open_form(id, cx);
            return;
        }
        if self.inline_edit.is_none() {
            window.focus(&self.form_focus, cx);
        }
    }

    fn focus_planner(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.inline_edit.is_none() {
            window.focus(&self.form_focus, cx);
        }
    }

    pub fn planner_keys_are_focused(&self, window: &Window) -> bool {
        self.form_focus.is_focused(window)
    }

    pub fn inline_picker_date(&self, cx: &App) -> Option<PickerDate> {
        self.inline_dates
            .as_ref()
            .map(|picker| picker.read(cx).date())
    }

    pub fn inline_input_value(&self, cx: &App) -> String {
        self.inline_input.read(cx).value().to_string()
    }

    pub fn form_picker_date(&self, cx: &App) -> Option<PickerDate> {
        self.form_dates
            .get(ado_core::START_DATE_FIELD)
            .or_else(|| self.form_dates.get(ado_core::TARGET_DATE_FIELD))
            .map(|picker| picker.read(cx).date())
    }

    pub fn details_icon_hint() -> &'static str {
        "Open details (Enter)"
    }

    pub fn open_inline_assignee(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let assigned = self
            .state
            .nodes
            .iter()
            .find(|node| node.id == id)
            .and_then(|node| node.assigned_to.as_ref().map(|a| a.display_name.clone()))
            .unwrap_or_else(|| "Unassigned".into());
        self.begin_inline(InlineEdit::Assignee(id), assigned, window, cx);
    }

    pub fn open_inline_dates(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.dates_anchor = Some((self.state.gantt.tree_width.max(240.0), 160.0));
        self.begin_inline(InlineEdit::Dates(id), String::new(), window, cx);
    }

    /// Tests type a complete range into the inline dates field.
    pub fn type_inline_dates(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.inline_input.update(cx, |input, cx| {
            input.replace_all(text, window, cx);
        });
        self.inline_dates_error = self.typed_dates_error_for_current(text);
        cx.notify();
    }

    pub fn inline_dates_error_text(&self) -> Option<String> {
        self.inline_dates_error.clone()
    }

    fn dates_example_for(&self, id: i64) -> (Option<jiff::civil::Date>, Option<jiff::civil::Date>) {
        let Some(node) = self.state.nodes.iter().find(|node| node.id == id) else {
            return (None, None);
        };
        (
            parse_iso_date(node.start_date.as_deref()),
            parse_iso_date(node.target_date.as_deref()),
        )
    }

    fn typed_dates_error_for_item(&self, typed: &str, id: i64) -> Option<String> {
        let (start, target) = self.dates_example_for(id);
        typed_dates_error_in(typed, start, target)
    }

    fn typed_dates_error_for_current(&self, typed: &str) -> Option<String> {
        match self.inline_edit {
            Some(edit) => self.typed_dates_error_for_item(typed, edit.item_id()),
            None => typed_dates_error(typed),
        }
    }

    pub fn form_input_value(&self, field: &str, cx: &App) -> Option<String> {
        self.form_inputs
            .get(field)
            .map(|input| input.read(cx).value().to_string())
    }

    /// Tests type into a named form field and sync the draft.
    pub fn type_form_field(
        &mut self,
        field: &str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.form_inputs.get(field).cloned() else {
            return;
        };
        input.update(cx, |input, cx| {
            input.replace_all(text, window, cx);
        });
        self.sync_form_inputs(cx);
        cx.notify();
    }

    /// Tests type a range into the details Dates field and sync the draft.
    pub fn type_form_dates(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self
            .form_inputs
            .get(ado_core::START_DATE_FIELD)
            .cloned()
            .or_else(|| self.form_inputs.get(ado_core::TARGET_DATE_FIELD).cloned())
        else {
            return;
        };
        input.update(cx, |input, cx| {
            input.replace_all(text, window, cx);
        });
        self.sync_form_inputs(cx);
        cx.notify();
    }

    pub fn form_dates_error_text(&self, cx: &App) -> Option<String> {
        self.form_dates_error(cx)
    }

    pub fn form_save_is_enabled(&self, cx: &App) -> bool {
        self.form_has_unsaved_edits(cx)
            && self.form_dates_error(cx).is_none()
            && self.state.form.as_ref().is_none_or(|form| !form.saving)
    }

    pub fn sync_form_inputs_for_test(&mut self, cx: &mut Context<Self>) {
        self.sync_form_inputs(cx);
    }

    fn flush_pending_form_dates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(date) = self.pending_form_picker.take()
            && let Some(picker) = self
                .form_dates
                .get(ado_core::START_DATE_FIELD)
                .cloned()
                .or_else(|| self.form_dates.get(ado_core::TARGET_DATE_FIELD).cloned())
        {
            picker.update(cx, |picker, cx| {
                picker.set_date(date, window, cx);
            });
        }
        let Some(text) = self.pending_form_dates_text.take() else {
            return;
        };
        if let Some(input) = self
            .form_inputs
            .get(ado_core::START_DATE_FIELD)
            .cloned()
            .or_else(|| self.form_inputs.get(ado_core::TARGET_DATE_FIELD).cloned())
        {
            input.update(cx, |input, cx| {
                input.set_value(&text, window, cx);
            });
        }
        self.form_dates_typed_seed = text;
    }

    fn queue_form_dates_from_draft(&mut self) {
        let Some(form) = self.state.form.as_ref() else {
            return;
        };
        let start = form
            .values
            .get(ado_core::START_DATE_FIELD)
            .and_then(serde_json::Value::as_str)
            .and_then(|text| parse_iso_date(Some(text)));
        let target = form
            .values
            .get(ado_core::TARGET_DATE_FIELD)
            .and_then(serde_json::Value::as_str)
            .and_then(|text| parse_iso_date(Some(text)));
        let text = format_date_range(start, target);
        self.pending_form_dates_text = Some(text.clone());
        self.form_dates_typed_seed = text;
        self.form_dates_preview = None;
        let range = self.form_dates.contains_key(ado_core::START_DATE_FIELD);
        self.pending_form_picker = Some(if range {
            PickerDate::Range(
                start.and_then(jiff_to_naive),
                target.and_then(jiff_to_naive),
            )
        } else {
            PickerDate::Single(target.or(start).and_then(jiff_to_naive))
        });
    }

    fn form_date_picker_focused(&self, window: &Window, cx: &App) -> bool {
        self.form_dates
            .values()
            .any(|picker| picker.read(cx).focus_handle.contains_focused(window, cx))
    }

    fn form_field_focused(&self, window: &Window, cx: &App) -> bool {
        self.form_inputs
            .values()
            .any(|input| input.read(cx).focus_handle(cx).is_focused(window))
            || self
                .form_dates
                .values()
                .any(|picker| picker.read(cx).focus_handle.is_focused(window))
            || (self.inline_edit.is_some()
                && self
                    .inline_input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window))
    }

    fn input_focused(&self, window: &Window, cx: &App) -> bool {
        self.form_field_focused(window, cx)
            || self.org_input.read(cx).focus_handle(cx).is_focused(window)
            || self.pat_input.read(cx).focus_handle(cx).is_focused(window)
            || self
                .filter_query
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
            || self
                .scope_query
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
    }

    fn bind_sign_in_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pat = cx.subscribe(&self.pat_input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit_pat(cx);
            }
        });
        let org = cx.subscribe_in(
            &self.org_input,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit_pat(cx),
                InputEvent::Change => {
                    let value = input.read(cx).value().to_string();
                    input.update(cx, |state, cx| {
                        state.set_masked(looks_like_pat(&value), window, cx);
                    });
                }
                _ => {}
            },
        );
        self._subscriptions.push(pat);
        self._subscriptions.push(org);
        let filter = cx.subscribe(&self.filter_query, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        let scope = cx.subscribe(&self.scope_query, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        self._subscriptions.push(filter);
        self._subscriptions.push(scope);
    }

    fn bind_inline_input(&mut self, cx: &mut Context<Self>) {
        let events = cx.subscribe(&self.inline_input, |this, input, event: &InputEvent, cx| {
            if matches!(this.inline_edit, Some(InlineEdit::Dates(_)))
                && matches!(event, InputEvent::Change)
            {
                let value = input.read(cx).value().to_string();
                this.inline_dates_error = this.typed_dates_error_for_current(&value);
                cx.notify();
            }
            if matches!(event, InputEvent::PressEnter { .. }) && this.inline_edit.is_some() {
                this.inline_enter_guard = true;
                this.commit_inline(cx);
            }
        });
        self._subscriptions.push(events);
    }

    fn spawn_state<F, Fut>(&mut self, cx: &mut Context<Self>, work: F) -> bool
    where
        F: FnOnce(PlannerState) -> Fut + Send + 'static,
        Fut: Future<Output = (PlannerState, Result<(), AppError>)> + Send + 'static,
    {
        self.spawn_io(cx, true, work)
    }

    /// Fetch-only I/O (open details). Must not flip the pill to Saving….
    fn spawn_load<F, Fut>(&mut self, cx: &mut Context<Self>, work: F) -> bool
    where
        F: FnOnce(PlannerState) -> Fut + Send + 'static,
        Fut: Future<Output = (PlannerState, Result<(), AppError>)> + Send + 'static,
    {
        self.spawn_io(cx, false, work)
    }

    fn spawn_io<F, Fut>(&mut self, cx: &mut Context<Self>, saving: bool, work: F) -> bool
    where
        F: FnOnce(PlannerState) -> Fut + Send + 'static,
        Fut: Future<Output = (PlannerState, Result<(), AppError>)> + Send + 'static,
    {
        if self.io_busy {
            return false;
        }
        self.io_busy = true;
        self.state.gantt.context_menu = None;
        if saving {
            self.arm_saving_status(cx);
        }
        let runtime = self.state.runtime.clone();
        let mut working = self.state.clone();
        working.gantt.context_menu = None;
        let rx = runtime.spawn_result(async move { work(working).await });
        cx.spawn(async move |this, cx| {
            let outcome = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.io_busy = false;
                if saving {
                    this.finish_or_hold_saving_status(cx);
                }
                match outcome {
                    Ok((working, result)) => {
                        let shown_at = this.loading_shown_at;
                        let discard_form = this.form_loading.is_none() && this.state.form.is_none();
                        let prev_selected = this.state.gantt.selected_id;
                        this.state.adopt_spawned(working);
                        this.state.gantt.context_menu = None;
                        if discard_form {
                            this.state.form = None;
                        }
                        if this.sync_form_dates_after_io {
                            this.sync_form_dates_after_io = false;
                            this.queue_form_dates_from_draft();
                        }
                        if let Some(at) = shown_at
                            && !this.state.hierarchy_loading
                        {
                            let remain = Duration::from_millis(280).saturating_sub(at.elapsed());
                            if remain > Duration::ZERO {
                                this.state.hierarchy_loading = true;
                                this.loading_hold_gen += 1;
                                let hold_gen = this.loading_hold_gen;
                                cx.spawn(async move |this, cx| {
                                    cx.background_executor().timer(remain).await;
                                    if let Some(this) = this.upgrade() {
                                        this.update(cx, |this, cx| {
                                            if this.loading_hold_gen != hold_gen {
                                                return;
                                            }
                                            this.state.hierarchy_loading = false;
                                            this.loading_shown_at = None;
                                            if let Some(from) = this.pending_camera_from.take() {
                                                this.start_camera_anim(from, cx);
                                            }
                                            cx.notify();
                                        });
                                    }
                                })
                                .detach();
                            } else {
                                this.loading_shown_at = None;
                                if let Some(from) = this.pending_camera_from.take() {
                                    this.start_camera_anim(from, cx);
                                }
                            }
                        } else if let Some(from) = this.pending_camera_from.take() {
                            this.start_camera_anim(from, cx);
                        }
                        if let Err(err) = result {
                            tracing::warn!("planner action failed: {err}");
                        }
                        this.after_io_selection_change(prev_selected);
                    }
                    Err(_) => tracing::warn!("planner action cancelled"),
                }
                this.form_loading = None;
                this.replay_pending_toast_undo(cx);
                if let Some(team) = this.pending_team.take() {
                    this.queue_team_switch(team, cx);
                }
                if !this.update_checked && this.state.session_info.signed_in {
                    this.update_checked = true;
                    this.spawn_update_check(cx);
                }
                this.note_toasts_changed(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
        true
    }

    fn after_io_selection_change(&mut self, prev_selected: Option<i64>) {
        self.drop_stale_inline_edit();
        if self.state.gantt.selected_id == prev_selected {
            return;
        }
        self.state.gantt.clear_hover();
        self.state.gantt.hover_locked = true;
        if self.state.gantt.selected_id.is_some() {
            let rows = self.state.gantt.flatten(&self.state.nodes);
            self.state.gantt.select_relative(&rows, 0);
        }
        if let Some(id) = self.state.gantt.selected_id
            && matches!(
                self.state.history.undo.last(),
                Some(HistoryEntry::Created { id: created, .. }) if *created == id
            )
        {
            self.pending_inline_title = Some(id);
            self.pending_focus_planner = false;
        } else {
            self.pending_inline_title = None;
            self.pending_focus_planner = true;
        }
    }

    fn drop_stale_inline_edit(&mut self) {
        let Some(edit) = self.inline_edit else {
            return;
        };
        let id = edit.item_id();
        if self.state.nodes.iter().any(|node| node.id == id) {
            return;
        }
        self.inline_edit = None;
        self.inline_dates = None;
        self.inline_dates_typed_seed.clear();
        self.inline_dates_error = None;
        self.pending_typed_preview = None;
        self.pending_inline_title = None;
        self.pending_focus_planner = true;
        self.state.gantt.clear_hover();
        self.state.gantt.hover_locked = true;
    }

    pub fn after_io_selection_change_for_test(&mut self, prev_selected: Option<i64>) {
        self.after_io_selection_change(prev_selected);
    }

    fn spawn_unbounded<F, Fut>(&mut self, cx: &mut Context<Self>, work: F)
    where
        F: FnOnce(PlannerState) -> Fut + Send + 'static,
        Fut: Future<Output = (PlannerState, Result<(), AppError>)> + Send + 'static,
    {
        let runtime = self.state.runtime.clone();
        let working = self.state.clone();
        let rx = runtime.spawn_result(async move { work(working).await });
        cx.spawn(async move |this, cx| {
            let outcome = rx.await;
            let _ = this.update(cx, |this, cx| {
                match outcome {
                    Ok((working, result)) => {
                        this.state.adopt_spawned(working);
                        if let Err(err) = result {
                            tracing::warn!("planner action failed: {err}");
                        }
                    }
                    Err(_) => tracing::warn!("planner action cancelled"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn visible_camera(&mut self) -> SavedGanttView {
        self.animated_camera()
            .unwrap_or_else(|| self.state.gantt.snapshot())
    }

    /// Commit the painted pose and drop any in-flight ease so a later mutation
    /// cannot snap from a stale destination.
    fn cancel_camera_anim(&mut self) {
        if let Some(pose) = self.animated_camera() {
            self.state.gantt.restore(pose);
        }
        self.camera_anim = None;
        self.camera_anim_gen += 1;
    }

    fn animate_gantt(&mut self, cx: &mut Context<Self>, apply: impl FnOnce(&mut GanttView)) {
        self.state.gantt.clear_hover();
        let from = self.visible_camera();
        self.state.gantt.restore(from);
        apply(&mut self.state.gantt);
        self.start_camera_anim(from, cx);
    }

    fn start_camera_anim(&mut self, from: SavedGanttView, cx: &mut Context<Self>) {
        let to = self.state.gantt.snapshot();
        self.camera_anim_gen += 1;
        if from == to || self.skip_camera_anim {
            self.camera_anim = None;
            return;
        }
        let anim_gen = self.camera_anim_gen;
        self.camera_anim = Some(CameraAnim {
            from,
            to,
            start: Instant::now(),
        });
        cx.spawn(async move |this, cx| {
            let frame = Duration::from_millis(16);
            let limit = Duration::from_millis(CAMERA_EASE_MS);
            loop {
                cx.background_executor().timer(frame).await;
                let cont = this.update(cx, |this, cx| {
                    if this.camera_anim_gen != anim_gen {
                        return false;
                    }
                    let done = this
                        .camera_anim
                        .as_ref()
                        .is_none_or(|anim| anim.start.elapsed() >= limit);
                    if done {
                        this.camera_anim = None;
                    }
                    cx.notify();
                    !done
                });
                cx.refresh();
                match cont {
                    Ok(true) => {}
                    _ => break,
                }
            }
        })
        .detach();
    }

    fn animated_camera(&mut self) -> Option<SavedGanttView> {
        let anim = self.camera_anim.as_ref()?;
        let t = anim.start.elapsed().as_secs_f32() / (CAMERA_EASE_MS as f32 / 1000.0);
        if t >= 1.0 {
            self.camera_anim = None;
            return None;
        }
        Some(anim.from.lerp(anim.to, t))
    }

    fn spawn_update_check(&mut self, cx: &mut Context<Self>) {
        if self.state.updater.prompt.is_some() || !self.state.session_info.signed_in {
            return;
        }
        let runtime = self.state.runtime.clone();
        let mut updater = self.state.updater.clone();
        let version = env!("CARGO_PKG_VERSION").to_string();
        let rx = runtime.spawn_result(async move {
            tokio::task::spawn_blocking(move || updater.check_blocking(&version))
                .await
                .unwrap_or_else(|err| Err(err.to_string()))
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Some(prompt)) => this.state.updater.inject(prompt),
                    Ok(None) => {}
                    Err(err) => tracing::debug!("updater check skipped: {err}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn write_clipboard(&self, cx: &mut Context<Self>, text: String) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    pub fn dismiss_overlays(&mut self) {
        self.open_filter = None;
        self.open_scope = None;
        self.open_flavor = false;
        self.inline_edit = None;
        self.inline_dates = None;
        self.inline_dates_typed_seed.clear();
        self.inline_dates_error = None;
        self.pending_typed_preview = None;
        self.dates_anchor = None;
        self.pending_delete = None;
        self.state.gantt.context_menu = None;
        self.state.gantt.selected_link = None;
    }

    fn dismiss_all_overlays(&mut self) {
        self.dismiss_overlays();
        self.form_loading = None;
        self.state.close_form();
        self.state.cancel_gesture();
        self.state.gantt.schedule_drag = None;
        self.schedule_origin_x = None;
        self.pill_press_window_x = None;
        self.bar_press_window_x = None;
        self.bar_last_window_x = None;
        self.bar_press_shift = false;
        self.schedule_live_x = None;
        self.state.gantt.clear_hover();
        self.dates_anchor = None;
    }

    fn details_overlay(&self) -> bool {
        self.state.form.is_some() || self.form_loading.is_some()
    }

    fn set_bar_hovered(&mut self, id: Option<i64>, cx: &mut Context<Self>) {
        if id.is_none() {
            self.state.gantt.pin_bar_tooltip = None;
            self.state.gantt.pin_ignore_leave = false;
            if self.state.gantt.hover_locked {
                self.state.gantt.hover_locked = false;
                cx.notify();
                return;
            }
        }
        if self.state.gantt.bar_hovered_id != id {
            self.state.gantt.bar_hovered_id = id;
            cx.notify();
        }
        if id.is_some() {
            self.set_hovered(id, cx);
        }
    }

    fn set_hovered(&mut self, id: Option<i64>, cx: &mut Context<Self>) {
        let linking = self
            .state
            .gantt
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.kind == GestureKind::Link);
        if linking {
            if let Some(id) = id {
                self.state.gantt.set_link_target(id);
                cx.notify();
            }
            return;
        }
        if id.is_none() {
            if self.state.gantt.hover_locked {
                return;
            }
            if self.state.gantt.hovered_id.is_some()
                || self.state.gantt.bar_hovered_id.is_some()
                || self.state.gantt.hovered_link.is_some()
            {
                self.state.gantt.clear_hover();
                cx.notify();
            }
            return;
        }
        if self.state.gantt.hover_locked {
            self.state.gantt.hover_locked = false;
        }
        if self.state.gantt.hovered_id != id {
            self.state.gantt.hovered_id = id;
            cx.notify();
        }
    }

    fn history_hotkey_is_duplicate(&mut self, undo: bool) -> bool {
        let now = std::time::Instant::now();
        let slot = if undo {
            &mut self.last_undo_at
        } else {
            &mut self.last_redo_at
        };
        if let Some(previous) = *slot
            && now.saturating_duration_since(previous) < Duration::from_millis(16)
        {
            return true;
        }
        *slot = Some(now);
        false
    }

    fn begin_inline(
        &mut self,
        edit: InlineEdit,
        initial: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut typed = initial;
        self.inline_dates_error = None;
        if let InlineEdit::Dates(id) = edit {
            let node = self.state.nodes.iter().find(|node| node.id == id);
            let start = node.and_then(|node| {
                parse_iso_date(node.start_date.as_deref()).and_then(jiff_to_naive)
            });
            let target = node.and_then(|node| {
                parse_iso_date(node.target_date.as_deref()).and_then(jiff_to_naive)
            });
            typed = format_date_range(
                start.and_then(naive_to_jiff),
                target.and_then(naive_to_jiff),
            );
            self.pending_typed_preview = None;
            self.inline_dates_typed_seed = typed.clone();
            let picker = cx.new(|cx| {
                let mut state = CalendarState::new(window, cx);
                match (start, target) {
                    (Some(start), Some(target)) => {
                        state.set_date(PickerDate::Range(Some(start), Some(target)), window, cx);
                    }
                    (Some(start), None) => {
                        state.set_date(PickerDate::Range(Some(start), None), window, cx);
                    }
                    (None, Some(target)) => {
                        state.set_date(PickerDate::Range(None, Some(target)), window, cx);
                    }
                    _ => {
                        state.set_date(PickerDate::Range(None, None), window, cx);
                    }
                }
                state
            });
            let item_id = id;
            self._subscriptions.push(cx.subscribe(
                &picker,
                move |this, _, event: &CalendarEvent, cx| {
                    if this.inline_edit != Some(InlineEdit::Dates(item_id)) {
                        return;
                    }
                    let CalendarEvent::Selected(date) = event;
                    if range_picker_is_complete(*date) {
                        this.commit_picker_dates(item_id, *date, true, true, cx);
                    }
                },
            ));
            self._subscriptions
                .push(cx.observe(&picker, move |this, picker, cx| {
                    if this.inline_edit != Some(InlineEdit::Dates(item_id)) {
                        return;
                    }
                    let date = picker.read(cx).date();
                    if range_picker_is_complete(date) {
                        return;
                    }
                    if let Some(preview) = pending_range_typed(date) {
                        this.pending_typed_preview = Some(preview);
                        cx.notify();
                    }
                }));
            self.inline_dates = Some(picker);
        } else {
            self.inline_dates = None;
            self.inline_dates_typed_seed.clear();
        }
        self.inline_edit = Some(edit);
        let placeholder = match edit {
            InlineEdit::Dates(_) => DATES_RANGE_HINT,
            InlineEdit::Start(_) | InlineEdit::Target(_) => DATE_HINT,
            InlineEdit::Title(_) | InlineEdit::Assignee(_) => "",
        };
        self.inline_input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx);
            input.set_value(&typed, window, cx);
            input.select_all(window, cx);
        });
        Self::focus_input(&self.inline_input, window, cx);
        cx.notify();
    }

    fn commit_picker_dates(
        &mut self,
        id: i64,
        date: PickerDate,
        close: bool,
        from_calendar: bool,
        cx: &mut Context<Self>,
    ) {
        // Calendar auto-commit must ignore live typed text. Otherwise typing a
        // complete range into an already-complete picker saves before Enter.
        let typed = if from_calendar {
            self.inline_dates_typed_seed.clone()
        } else {
            self.inline_input.read(cx).value().to_string()
        };
        let Some(node) = self.state.nodes.iter().find(|node| node.id == id) else {
            return;
        };
        let start_year = parse_iso_date(node.start_date.as_deref()).map(jiff::civil::Date::year);
        let target_year = parse_iso_date(node.target_date.as_deref()).map(jiff::civil::Date::year);
        let Some((start, target)) = dates_from_range_picker_in_years(
            date,
            &typed,
            &self.inline_dates_typed_seed,
            start_year,
            target_year,
        ) else {
            return;
        };
        let next_start = start.map(iso_date_only);
        let next_target = target.map(iso_date_only);
        let unchanged = next_start == node.start_date && next_target == node.target_date;
        if close {
            self.inline_edit = None;
            self.inline_dates = None;
            self.inline_dates_typed_seed.clear();
        }
        if unchanged {
            cx.notify();
            return;
        }
        self.spawn_range_dates(id, start, target, cx);
        cx.notify();
    }

    fn spawn_range_dates(
        &mut self,
        id: i64,
        start: Option<jiff::civil::Date>,
        target: Option<jiff::civil::Date>,
        cx: &mut Context<Self>,
    ) {
        if let Some(pending) = self.state.commit_range_dates_now(id, start, target) {
            self.enqueue_date_save(cx, pending);
        }
    }

    fn commit_inline(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.inline_edit else {
            return;
        };
        self.inline_enter_guard = true;
        let value = self.inline_input.read(cx).value().to_string();
        if let InlineEdit::Dates(id) = edit {
            if let Some(err) = self.typed_dates_error_for_item(&value, id) {
                self.inline_dates_error = Some(err);
                cx.notify();
                return;
            }
            self.inline_dates_error = None;
            self.inline_edit = None;
            let date = self
                .inline_dates
                .as_ref()
                .map(|picker| picker.read(cx).date());
            self.inline_dates = None;
            if let Some(date) = date {
                self.commit_picker_dates(id, date, true, false, cx);
            } else {
                let node = self.state.nodes.iter().find(|node| node.id == id);
                let start_year = node
                    .and_then(|node| parse_iso_date(node.start_date.as_deref()))
                    .map(jiff::civil::Date::year);
                let target_year = node
                    .and_then(|node| parse_iso_date(node.target_date.as_deref()))
                    .map(jiff::civil::Date::year);
                let picker = node
                    .map(picker_date_from_node)
                    .unwrap_or(PickerDate::Range(None, None));
                if let Some((start, target)) = dates_from_range_picker_in_years(
                    picker,
                    &value,
                    &self.inline_dates_typed_seed,
                    start_year,
                    target_year,
                ) {
                    self.spawn_range_dates(id, start, target, cx);
                }
            }
            self.inline_dates_typed_seed.clear();
            return;
        }
        self.inline_edit = None;
        match edit {
            InlineEdit::Title(id) => {
                self.spawn_state(cx, move |mut state| async move {
                    let result = state.save_title(id, value).await;
                    (state, result)
                });
            }
            InlineEdit::Dates(_) => {}
            InlineEdit::Start(id) | InlineEdit::Target(id) => {
                let parsed = ado_core::parse_iso_date(Some(value.trim()));
                let node = self.state.nodes.iter().find(|n| n.id == id).cloned();
                if let (Some(date), Some(node)) = (parsed, node) {
                    let start = if matches!(edit, InlineEdit::Start(_)) {
                        date
                    } else {
                        ado_core::parse_iso_date(node.start_date.as_deref()).unwrap_or(date)
                    };
                    let target = if matches!(edit, InlineEdit::Target(_)) {
                        date
                    } else {
                        ado_core::parse_iso_date(node.target_date.as_deref()).unwrap_or(date)
                    };
                    self.spawn_state(cx, move |mut state| async move {
                        let result = state.save_dates(id, start, target).await;
                        (state, result)
                    });
                }
            }
            InlineEdit::Assignee(id) => {
                let needle = value.trim().to_lowercase();
                let identity = self
                    .state
                    .members
                    .iter()
                    .find(|member| {
                        member.display_name.to_lowercase().contains(&needle)
                            || member.unique_name.to_lowercase().contains(&needle)
                    })
                    .cloned();
                self.spawn_state(cx, move |mut state| async move {
                    let result = state.save_assignee(id, identity).await;
                    (state, result)
                });
            }
        }
        cx.notify();
    }

    pub fn set_open_filter(&mut self, id: Option<&'static str>) {
        self.open_scope = None;
        self.open_flavor = false;
        self.state.gantt.context_menu = None;
        self.open_filter = id;
    }

    pub fn popup_is_open(&self) -> bool {
        self.popup_open()
    }

    fn popup_open(&self) -> bool {
        self.open_filter.is_some() || self.open_scope.is_some() || self.open_flavor
    }

    fn popup_panel(
        &self,
        id: impl Into<gpui_kit::ElementId>,
        cx: &Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        v_flex()
            .id(id)
            .debug_selector(|| "overlay-popup-panel".into())
            .w(px(280.))
            .min_w(px(240.))
            .min_h(px(120.))
            .max_h(px(360.))
            .overflow_scroll()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .rounded_md()
            .p_1()
            .shadow_md()
            .occlude()
    }

    fn focus_input(input: &Entity<InputState>, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&input.read(cx).focus_handle(cx), cx);
    }

    fn submit_pat(&mut self, cx: &mut Context<Self>) {
        self.state.sign_in_org = self.org_input.read(cx).value().to_string();
        self.state.sign_in_pat = self.pat_input.read(cx).value().to_string();
        self.spawn_state(cx, |mut state| async move {
            let result = state.login_pat().await;
            (state, result)
        });
    }

    fn render_error_banner(&self, cx: &Context<Self>) -> impl IntoElement {
        if self.banner_dismissed || self.details_overlay() {
            return div().into_any_element();
        }
        let Some(message) = self.state.team_settings_error.as_ref() else {
            return div().into_any_element();
        };
        h_flex()
            .id("team-settings-error")
            .debug_selector(|| "team-settings-error".into())
            .absolute()
            .top(px(8.))
            .left(px(12.))
            .max_w(px(560.))
            .items_start()
            .px_3()
            .py_1()
            .gap_2()
            .rounded_md()
            .shadow_md()
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .border_1()
            .border_color(cx.theme().border)
            .occlude()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_sm()
                    .child(message.clone()),
            )
            .child(
                Button::new("dismiss-team-banner")
                    .ghost()
                    .compact()
                    .label("Dismiss")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.banner_dismissed = true;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn render_toasts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("toasts")
            .absolute()
            .bottom_6()
            .left_0()
            .right_0()
            .items_center()
            .gap_2()
            .children(self.state.toasts.items.iter().map(|toast| {
                let id = toast.id;
                let copyable = toast.copyable;
                let undo = toast.undo.clone();
                let redo = toast.redo.clone();
                let message = toast.message.clone();
                let kind = toast.kind;
                h_flex()
                    .id(("toast", id))
                    .debug_selector(|| "planner-toast".into())
                    .occlude()
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.set_toast_hover(*hovered, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.arm_toast_pointer();
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .w(px(520.))
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(match kind {
                        ToastKind::Error => cx.theme().danger.opacity(0.35),
                        ToastKind::Warning => cx.theme().warning.opacity(0.35),
                        ToastKind::Success => cx.theme().success.opacity(0.35),
                    })
                    .bg(match kind {
                        ToastKind::Error => cx.theme().danger.opacity(0.14),
                        ToastKind::Warning => cx.theme().warning.opacity(0.14),
                        ToastKind::Success => cx.theme().success.opacity(0.14),
                    })
                    .text_color(match kind {
                        ToastKind::Error => cx.theme().danger,
                        ToastKind::Warning => cx.theme().warning,
                        ToastKind::Success => cx.theme().success,
                    })
                    .child(
                        div()
                            .id(("toast-title", id))
                            .debug_selector(|| "toast-title".into())
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .text_sm()
                            .child(message.clone()),
                    )
                    .when(copyable, |el| {
                        el.child(
                            div()
                                .id(("toast-copy-wrap", id))
                                .debug_selector(|| "toast-copy".into())
                                .flex_shrink_0()
                                .child(Button::new(("copy-toast", id)).label("Copy").on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.state.gantt.clear_hover();
                                        this.state.copy_toast(id);
                                        if let Some(text) = this.state.copied.clone() {
                                            this.write_clipboard(cx, text);
                                        }
                                        cx.notify();
                                    }),
                                )),
                        )
                    })
                    .when_some(undo.clone(), |el, action| {
                        let action_down = action.clone();
                        el.child(
                            div()
                                .id(("toast-undo-wrap", id))
                                .debug_selector(|| "toast-undo".into())
                                .occlude()
                                .flex_shrink_0()
                                .min_w(px(52.))
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.arm_toast_pointer();
                                        cx.stop_propagation();
                                        this.apply_toast_undo(id, action_down.clone(), cx);
                                    }),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.apply_toast_undo(id, action.clone(), cx);
                                }))
                                .child(Button::new(("undo-toast", id)).label("Undo")),
                        )
                    })
                    .when_some(redo.clone(), |el, action| {
                        let action_down = action.clone();
                        el.child(
                            div()
                                .id(("toast-redo-wrap", id))
                                .debug_selector(|| "toast-redo".into())
                                .occlude()
                                .flex_shrink_0()
                                .min_w(px(52.))
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.arm_toast_pointer();
                                        cx.stop_propagation();
                                        this.apply_toast_undo(id, action_down.clone(), cx);
                                    }),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.apply_toast_undo(id, action.clone(), cx);
                                }))
                                .child(Button::new(("redo-toast", id)).label("Redo")),
                        )
                    })
                    .when_some(toast.extra.clone(), |el, extra| {
                        let extra_down = extra.clone();
                        el.child(
                            div()
                                .id(("toast-extra-wrap", id))
                                .debug_selector(|| "toast-move-children".into())
                                .occlude()
                                .flex_shrink_0()
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.arm_toast_pointer();
                                        cx.stop_propagation();
                                        this.apply_toast_extra(id, extra_down.clone(), cx);
                                    }),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.apply_toast_extra(id, extra.clone(), cx);
                                }))
                                .child(Button::new(("extra-toast", id)).label("Move children too")),
                        )
                    })
                    .child(
                        div()
                            .id(("toast-dismiss-wrap", id))
                            .debug_selector(|| "toast-dismiss".into())
                            .flex_shrink_0()
                            .child(Button::new(("dismiss-toast", id)).label("×").on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.state.gantt.clear_hover();
                                    this.state.toasts.dismiss(id);
                                    this.clamp_toast_hover();
                                    cx.notify();
                                }),
                            )),
                    )
            }))
    }

    fn set_toast_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self.toast_hover_depth = self.toast_hover_depth.saturating_add(1).min(8);
        } else {
            self.toast_hover_depth = self.toast_hover_depth.saturating_sub(1);
        }
        if self.state.toasts.items.is_empty() {
            self.toast_hover_depth = 0;
        }
        self.state.toasts.paused = self.toast_hover_depth > 0;
        cx.notify();
    }

    fn clamp_toast_hover(&mut self) {
        if self.state.toasts.items.is_empty() {
            self.toast_hover_depth = 0;
        } else {
            self.toast_hover_depth = self.toast_hover_depth.min(1);
        }
        self.state.toasts.paused = self.toast_hover_depth > 0;
    }

    fn arm_toast_pointer(&mut self) {
        self.toast_pointer = true;
        self.state.cancel_gesture();
        self.state.gantt.schedule_drag = None;
        self.state.gantt.clear_hover();
        self.saw_pressed_drag = false;
        self.schedule_origin_x = None;
        self.pill_press_window_x = None;
        self.bar_press_window_x = None;
        self.bar_last_window_x = None;
        self.bar_press_shift = false;
        self.schedule_live_x = None;
    }

    /// Undo unmounts the toast before mouse-up. A leftover `toast_pointer`
    /// would cancel the next bar press on mouse-up; clear it on a new press.
    fn clear_toast_pointer_for_new_press(&mut self) {
        self.toast_pointer = false;
        self.state.gantt.hover_locked = false;
    }

    pub fn apply_toast_undo(&mut self, toast_id: u64, action: UndoAction, cx: &mut Context<Self>) {
        self.apply_toast_undo_with(toast_id, action, true, cx);
    }

    fn replay_pending_toast_undo(&mut self, cx: &mut Context<Self>) {
        if let Some((toast_id, action)) = self.pending_toast_undo.take() {
            self.apply_toast_undo_with(toast_id, action, false, cx);
        }
    }

    fn apply_toast_undo_with(
        &mut self,
        toast_id: u64,
        action: UndoAction,
        require_toast: bool,
        cx: &mut Context<Self>,
    ) {
        if self.toast_undo_inflight {
            return;
        }
        if require_toast
            && !self
                .state
                .toasts
                .items
                .iter()
                .any(|toast| toast.id == toast_id)
        {
            return;
        }
        self.state.gantt.context_menu = None;
        self.state.gantt.clear_hover();
        self.state.gantt.hover_locked = false;
        let original = self.state.history_action_message_for_undo(&action);
        let undid = undone_toast_message(&original);
        if matches!(
            action,
            UndoAction::RestoreDates { .. } | UndoAction::RestoreDatesBatch { .. }
        ) {
            let pending = self.state.toast_restore_or_undo_dates(action);
            if !pending.is_empty() {
                self.clamp_toast_hover();
                let already_toasted = pending.iter().any(|save| save.role == DateSaveRole::Undo);
                self.queue_form_dates_from_draft();
                for save in pending {
                    self.enqueue_date_save(cx, save);
                }
                if !already_toasted {
                    // History was not the matching stack entry; don't offer
                    // StackUndo/Redo against leftover dates.
                    self.state.toasts.dismiss_undo();
                    self.state.toasts.dismiss(toast_id);
                    self.state.toasts.push(ToastKind::Success, undid);
                }
                self.note_toasts_changed(cx);
            }
            self.request_repaint(None, cx);
            return;
        }
        // Queue while spawn_state holds io_busy so Undo is not a silent dismiss.
        // Replay even if the toast later expires or is dismissed. StackUndo/
        // StackRedo also use spawn_state for non-date history, so they must
        // wait here instead of calling undo_last/redo_last while busy.
        if self.io_busy {
            self.pending_toast_undo = Some((toast_id, action));
            cx.notify();
            return;
        }
        if matches!(action, UndoAction::StackUndo) {
            self.undo_last(cx);
            return;
        }
        if matches!(action, UndoAction::StackRedo) {
            self.redo_last(cx);
            return;
        }
        self.toast_undo_inflight = true;
        self.spawn_toast_undo(cx, action, undid);
        self.clamp_toast_hover();
        cx.notify();
    }

    fn apply_toast_extra(&mut self, toast_id: u64, extra: ToastExtra, cx: &mut Context<Self>) {
        let Some(toast) = self
            .state
            .toasts
            .items
            .iter_mut()
            .find(|toast| toast.id == toast_id)
        else {
            return;
        };
        if toast.extra.take().is_none() {
            return;
        }
        let ToastExtra::MoveChildren {
            parent_id,
            delta_days,
        } = extra;
        let pending = self.state.shift_descendants_now(parent_id, delta_days);
        for save in pending {
            self.enqueue_date_save(cx, save);
        }
        self.queue_form_dates_from_draft();
        self.note_toasts_changed(cx);
        self.request_repaint(None, cx);
    }

    fn spawn_toast_undo(&mut self, cx: &mut Context<Self>, action: UndoAction, undid: String) {
        self.io_busy = true;
        self.arm_saving_status(cx);
        let runtime = self.state.runtime.clone();
        let mut working = self.state.clone();
        let merge_action = action.clone();
        let rx = runtime.spawn_result(async move {
            let result = working.apply_undo(action).await;
            (working, result)
        });
        cx.spawn(async move |this, cx| {
            let outcome = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.io_busy = false;
                this.skip_saving_hold = true;
                this.finish_or_hold_saving_status(cx);
                this.toast_undo_inflight = false;
                match outcome {
                    Ok((working, Ok(()))) => {
                        let prev_selected = this.state.gantt.selected_id;
                        this.state.merge_undo_action(&working, &merge_action);
                        this.state.push_stack_toast(undid, true);
                        this.after_io_selection_change(prev_selected);
                    }
                    Ok((working, Err(err))) => {
                        tracing::warn!("toast undo failed: {err}");
                        if let Some(msg) = working.toasts.last_error_message() {
                            this.state.toasts.push(ToastKind::Error, msg.to_string());
                        }
                    }
                    Err(_) => tracing::warn!("toast undo cancelled"),
                }
                this.replay_pending_toast_undo(cx);
                if let Some(team) = this.pending_team.take() {
                    this.queue_team_switch(team, cx);
                }
                this.note_toasts_changed(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn undo_last(&mut self, cx: &mut Context<Self>) {
        self.state.gantt.clear_hover();
        self.state.gantt.context_menu = None;
        if self.history_hotkey_is_duplicate(true) || self.state.history.undo.is_empty() {
            cx.notify();
            return;
        }
        let pending = self.state.undo_date_stack_now();
        if !pending.is_empty() {
            self.queue_form_dates_from_draft();
            for save in pending {
                self.enqueue_date_save(cx, save);
            }
            self.note_toasts_changed(cx);
            self.request_repaint(None, cx);
            return;
        }
        self.skip_saving_hold = true;
        self.spawn_state(cx, |mut state| async move {
            let result = state.undo().await;
            (state, result)
        });
    }

    fn redo_last(&mut self, cx: &mut Context<Self>) {
        self.state.gantt.clear_hover();
        if self.history_hotkey_is_duplicate(false) || self.state.history.redo.is_empty() {
            cx.notify();
            return;
        }
        let pending = self.state.redo_date_stack_now();
        if !pending.is_empty() {
            self.queue_form_dates_from_draft();
            for save in pending {
                self.enqueue_date_save(cx, save);
            }
            self.note_toasts_changed(cx);
            self.request_repaint(None, cx);
            return;
        }
        self.skip_saving_hold = true;
        self.spawn_state(cx, |mut state| async move {
            let result = state.redo().await;
            (state, result)
        });
    }

    fn apply_scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let mods = event.modifiers;
        let delta = event.delta.pixel_delta(px(16.));
        // GPUI reports wheel-down / scroll-left as negative on this backend;
        // invert so the content moves the same way as the wheel.
        let dy = -f32::from(delta.y);
        let dx = -f32::from(delta.x);
        let rows = self.state.visible_rows().len();
        let viewport_h = self.state.gantt.viewport_h;
        if mods.control || mods.platform {
            // After invert, wheel-up (negative dy) zooms in, matching the + button.
            let direction = if dy < 0.0 { 1 } else { -1 };
            let pointer = self.timeline_local_x(f32::from(event.position.x));
            let timeline_width = self.state.gantt.timeline_width;
            self.animate_gantt(cx, |gantt| {
                gantt.retarget_year_gutter(|gantt| {
                    let next = GanttView::next_zoom(gantt.zoom, direction);
                    let factor = if gantt.zoom > 0.0 {
                        next / gantt.zoom
                    } else {
                        1.0
                    };
                    if pointer >= 0.0 && pointer <= timeline_width {
                        gantt.zoom_at(factor, pointer, 0.0);
                    } else {
                        gantt.zoom_at_center(factor);
                    }
                    gantt.clamp_scroll(rows, viewport_h);
                });
            });
        } else {
            self.cancel_camera_anim();
            self.state.gantt.apply_wheel(dx, dy, mods.shift);
            self.state.gantt.clamp_scroll(rows, viewport_h);
        }
        cx.notify();
    }

    fn clear_canvas_hover(&mut self, cx: &mut Context<Self>) {
        if self
            .state
            .gantt
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.kind == GestureKind::Link)
        {
            self.state.gantt.clear_link_target();
            cx.notify();
            return;
        }
        if self.state.gantt.hovered_id.is_some()
            || self.state.gantt.bar_hovered_id.is_some()
            || self.state.gantt.hovered_link.is_some()
            || self.state.gantt.pin_bar_tooltip.is_some()
        {
            self.state.gantt.clear_hover();
            cx.notify();
        }
    }

    fn render_sign_in(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let pat_mode = self.state.session_info.auth_mode == AuthMode::Pat;
        v_flex()
            .id("sign-in-shell")
            .size_full()
            .items_center()
            .justify_center()
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .w(px(420.))
                    .gap_4()
                    .p_8()
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .child(
                        Label::new("Azure DevOps")
                            .text_xs()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_xl()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("ADO Planner"),
                    )
                    .when(pat_mode, |el| {
                        el.child(
                            v_flex()
                                .gap_3()
                                .child(Label::new("Organization"))
                                .child(Input::new(&self.org_input).id("organization-input"))
                                .child(Label::new("Personal access token"))
                                .child(Input::new(&self.pat_input).id("pat-input"))
                                .child(
                                    Button::new("sign-in").primary().label("Continue").on_click(
                                        cx.listener(|this, _, _, cx| this.submit_pat(cx)),
                                    ),
                                ),
                        )
                    })
                    .when(!pat_mode, |el| {
                        el.child(Button::new("sign-in").primary().label("Sign in").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.spawn_state(cx, |mut state| async move {
                                    let result = state.login_entra().await;
                                    (state, result)
                                });
                            }),
                        ))
                    }),
            )
    }

    fn render_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self
            .state
            .session_info
            .display_name
            .clone()
            .unwrap_or_else(|| "Signed in".into());
        let initials = self.state.initials();
        let locked = self.state.scope_locked
            || self.state.projects_loading
            || self.state.teams_loading
            || self.state.iterations_loading;
        let org = self.state.prefs.planner.org.clone();
        let project = self.state.prefs.planner.project.clone();
        let team = self.state.prefs.planner.team.clone();
        let iteration = if self.state.prefs.planner.iteration_path.is_empty() {
            "All".to_string()
        } else {
            self.state.prefs.planner.iteration_path.clone()
        };
        let assignee = self.assignee_label();
        let org_empty = org.is_empty();
        let project_empty = project.is_empty();
        let team_empty = team.is_empty();
        h_flex()
            .id("signed-in-chrome")
            .debug_selector(|| "signed-in-chrome".into())
            .w_full()
            .min_h(px(64.))
            .flex_shrink_0()
            .flex_wrap()
            .items_center()
            .px_5()
            .py_2()
            .gap_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        div()
                            .size(px(36.))
                            .rounded_full()
                            .bg(cx.theme().primary)
                            .text_color(cx.theme().primary_foreground)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .child(initials),
                    )
                    .child(
                        v_flex()
                            .child(
                                Label::new("ADO Planner")
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground),
                            )
                            .child(div().id("session-name").child(name)),
                    ),
            )
            .child(self.render_scope("org", "Org", org, locked, cx))
            .child(self.render_scope("project", "Project", project, locked || org_empty, cx))
            .when(self.state.projects_loading, |el| {
                el.child(div().id("project-loading").child("…"))
            })
            .child(self.render_scope("team", "Team", team, locked || project_empty, cx))
            .child(self.render_scope(
                "iteration",
                "Iteration",
                iteration,
                locked || team_empty,
                cx,
            ))
            .child(self.render_scope("assignee", "Assignee", assignee, team_empty, cx))
            .child(div().flex_1().min_w(px(8.)))
            .child(
                div()
                    .id("header-filters")
                    .debug_selector(|| "header-filters".into())
                    .flex_shrink_0()
                    .child(self.render_filters(cx)),
            )
            .child(
                h_flex()
                    .id("header-account-actions")
                    .debug_selector(|| "header-account-actions".into())
                    .flex_shrink_0()
                    .gap_1()
                    .items_center()
                    .child(self.render_flavor(cx))
                    .child(
                        div()
                            .id("header-log-out")
                            .debug_selector(|| "header-log-out".into())
                            .child(
                                Button::new("log-out")
                                    .ghost()
                                    .compact()
                                    .icon(IconName::LogOut)
                                    .tooltip("Log out")
                                    .tooltip_placement(Placement::Bottom)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.spawn_state(cx, |mut state| async move {
                                            let result = state.logout().await;
                                            (state, result)
                                        });
                                    })),
                            ),
                    ),
            )
    }

    fn assignee_label(&self) -> String {
        let value = &self.state.prefs.planner.assignee;
        self.state
            .assignee_options()
            .into_iter()
            .find(|(v, _)| v == value)
            .map(|(_, l)| l)
            .unwrap_or_else(|| "Anyone".into())
    }

    fn render_scope(
        &mut self,
        id: &'static str,
        label: &'static str,
        value: String,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let open = self.open_scope == Some(id);
        let options = self.scope_options(id);
        let search = self.scope_query.read(cx).value().to_string();
        let muted = cx.theme().muted_foreground;
        let current = value.clone();
        let trigger = div()
            .id(SharedString::from(format!("{id}-trigger")))
            .debug_selector(move || format!("{id}-trigger"))
            .w_full()
            .overflow_hidden()
            .child(
                Button::new(id)
                    .label(if current.is_empty() {
                        format!("Select {label}")
                    } else {
                        current.clone()
                    })
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, event, window, cx| {
                        this.open_filter = None;
                        this.open_flavor = false;
                        this.state.gantt.context_menu = None;
                        this.open_scope = if this.open_scope == Some(id) {
                            None
                        } else {
                            Some(id)
                        };
                        this.scope_query.update(cx, |input, cx| {
                            input.set_value("", window, cx);
                        });
                        if this.open_scope == Some(id) {
                            Self::focus_input(&this.scope_query, window, cx);
                        }
                        let _ = event;
                        cx.notify();
                    })),
            );
        let option_buttons: Vec<AnyElement> = if open {
            crate::state::PlannerState::filter_scope_options(&options, &search)
                .into_iter()
                .map(|(val, lab)| {
                    let val_click = val.clone();
                    let selected = val == current;
                    let caption = if selected {
                        format!("✓ {lab}")
                    } else {
                        lab.clone()
                    };
                    div()
                        .id(SharedString::from(format!("{id}-option-{val}")))
                        .when(selected, |el| el.debug_selector(|| "scope-current".into()))
                        .when(!selected, |el| {
                            let key = format!("{id}-option-{val}");
                            el.debug_selector(move || key.clone())
                        })
                        .child(
                            Button::new(SharedString::from(format!("{id}-{val}")))
                                .label(caption)
                                .selected(selected)
                                .when(selected, |btn| btn.primary())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.choose_scope(id, val_click.clone(), cx);
                                })),
                        )
                        .into_any_element()
                })
                .collect()
        } else {
            Vec::new()
        };
        let empty_label = match id {
            "assignee" => "No team members loaded",
            _ => "No options",
        };
        let mut popup = Popup::new(format!("{id}-popup"), trigger).offset(px(12.));
        if open {
            popup = popup.content(
                self.popup_panel(format!("{id}-panel"), cx)
                    .child(
                        Input::new(&self.scope_query)
                            .id(format!("{id}-search"))
                            .w_full(),
                    )
                    .when(option_buttons.is_empty(), |el| {
                        el.child(Label::new(empty_label).text_xs().text_color(muted))
                    })
                    .children(option_buttons),
            );
        }
        v_flex()
            .gap_1()
            .w(px(148.))
            .min_w(px(132.))
            .max_w(px(180.))
            .overflow_hidden()
            .child(Label::new(label).text_xs().text_color(muted))
            .child(popup)
    }

    fn scope_options(&self, id: &str) -> Vec<(String, String)> {
        match id {
            "org" => self
                .state
                .orgs
                .iter()
                .map(|o| (o.account_name.clone(), o.account_name.clone()))
                .collect(),
            "project" => self
                .state
                .projects
                .iter()
                .map(|p| (p.name.clone(), p.name.clone()))
                .collect(),
            "team" => self
                .state
                .teams
                .iter()
                .map(|t| (t.name.clone(), t.name.clone()))
                .collect(),
            "iteration" => {
                let mut rows = vec![("".into(), "All".into())];
                rows.extend(self.state.iteration_options());
                rows
            }
            "assignee" => self.state.assignee_options(),
            _ => Vec::new(),
        }
    }

    fn queue_team_switch(&mut self, team: String, cx: &mut Context<Self>) {
        if self.io_busy {
            self.pending_team = Some(team);
            return;
        }
        self.start_team_switch(team, cx);
    }

    fn start_team_switch(&mut self, team: String, cx: &mut Context<Self>) {
        self.cancel_camera_anim();
        self.pending_camera_from = Some(self.state.gantt.snapshot());
        self.loading_hold_gen += 1;
        self.state.begin_team_switch(team.clone());
        self.loading_shown_at = Some(std::time::Instant::now());
        self.banner_dismissed = false;
        self.spawn_state(cx, move |mut state| async move {
            let result = state.set_team(team).await;
            (state, result)
        });
    }

    fn choose_scope(&mut self, id: &str, value: String, cx: &mut Context<Self>) {
        self.open_scope = None;
        match id {
            "org" => {
                self.spawn_state(cx, move |mut state| async move {
                    let result = state.set_org(value).await;
                    (state, result)
                });
            }
            "project" => {
                self.spawn_state(cx, move |mut state| async move {
                    let result = state.set_project(value).await;
                    (state, result)
                });
            }
            "team" => self.queue_team_switch(value, cx),
            "iteration" => self.state.set_iteration(value),
            "assignee" => self.state.set_assignee(value),
            _ => {}
        }
        cx.notify();
    }

    fn render_filters(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_1()
            .child(
                div()
                    .id("header-filters-label")
                    .debug_selector(|| "header-filters-label".into())
                    .child(Label::new("Filters").text_xs().text_color(muted)),
            )
            .child(
                h_flex()
                    .id("filters")
                    .gap_2()
                    .child(self.filter_button(
                        "root-filter",
                        "Roots",
                        self.state.hidden_root_count(),
                        "Top-level types shown on the Gantt. Defaults to every loaded type.",
                        cx,
                    ))
                    .child(self.filter_button(
                        "type-filter",
                        "Types",
                        self.state.prefs.planner.hidden_types.len(),
                        "Hide work item types from the tree and timeline.",
                        cx,
                    ))
                    .child(self.filter_button(
                        "state-filter",
                        "States",
                        self.state.prefs.planner.hidden_states.len(),
                        "Hide work item states from the tree and timeline.",
                        cx,
                    )),
            )
    }

    fn filter_button(
        &mut self,
        id: &'static str,
        label: &'static str,
        hidden: usize,
        tooltip: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let open = self.open_filter == Some(id);
        let items = match id {
            "root-filter" | "type-filter" => self.state.types(),
            _ => self.state.states(),
        };
        let caption = if hidden == 0 {
            format!("{label}: All")
        } else if id == "root-filter" {
            let selected = items.len().saturating_sub(hidden);
            format!("{label}: {selected}")
        } else {
            format!("{label}: {hidden} hidden")
        };
        let hidden_set = match id {
            "type-filter" => self.state.prefs.planner.hidden_types.clone(),
            "state-filter" => self.state.prefs.planner.hidden_states.clone(),
            _ => self.state.hidden_roots(),
        };
        let trigger = div().w(px(120.)).flex_shrink_0().overflow_hidden().child(
            Button::new(id)
                .label(caption)
                .tooltip(tooltip)
                .selected(open || hidden > 0)
                .when(open || hidden > 0, |btn| btn.primary())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_scope = None;
                    this.open_flavor = false;
                    this.state.gantt.context_menu = None;
                    this.open_filter = if this.open_filter == Some(id) {
                        None
                    } else {
                        Some(id)
                    };
                    this.filter_query.update(cx, |input, cx| {
                        input.set_value("", window, cx);
                    });
                    if this.open_filter == Some(id) {
                        Self::focus_input(&this.filter_query, window, cx);
                    }
                    cx.notify();
                })),
        );
        let mut popup = Popup::new(format!("{id}-popup"), trigger).offset(px(12.));
        if open {
            popup = popup.content(self.filter_menu(id, items, hidden_set, cx));
        }
        popup
    }

    fn filter_menu(
        &mut self,
        id: &'static str,
        items: Vec<String>,
        hidden: Vec<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let search = self.filter_query.read(cx).value().to_string();
        let filtered = self.state.filter_options(&items, &search);
        let muted = cx.theme().muted_foreground;
        self.popup_panel(format!("{id}-panel"), cx)
            .child(
                Input::new(&self.filter_query)
                    .id(format!("{id}-search"))
                    .w_full(),
            )
            .child(
                Button::new(SharedString::from(format!("show-all-{id}")))
                    .label("Show all")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        match id {
                            "type-filter" => this.state.show_all_types(),
                            "state-filter" => this.state.show_all_states(),
                            _ => this.state.show_all_roots(),
                        }
                        cx.notify();
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("hide-all-{id}")))
                    .label("Hide all")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        match id {
                            "type-filter" => this.state.hide_all_types(),
                            "state-filter" => this.state.hide_all_states(),
                            _ => this.state.hide_all_roots(),
                        }
                        cx.notify();
                    })),
            )
            .when(filtered.is_empty(), |el| {
                el.child(
                    Label::new("No matching options")
                        .text_xs()
                        .text_color(muted),
                )
            })
            .children(filtered.into_iter().map(|item| {
                let checked = !hidden.contains(&item);
                let item_click = item.clone();
                h_flex()
                    .id(SharedString::from(format!("filter-item-{item}")))
                    .w_full()
                    .min_w(px(220.))
                    .gap_2()
                    .px_1()
                    .py_1()
                    .rounded_md()
                    .hover(|el| el.bg(cx.theme().list_hover))
                    .child(
                        Checkbox::new(SharedString::from(format!("cb-{item}")))
                            .checked(checked)
                            .label(item.clone()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        match id {
                            "type-filter" => this.state.toggle_hidden_type(&item_click),
                            "state-filter" => this.state.toggle_hidden_state(&item_click),
                            _ => this.state.toggle_hidden_root(&item_click),
                        }
                        cx.notify();
                    }))
            }))
    }

    fn render_flavor(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.open_flavor;
        let current = self.state.flavor();
        let trigger = div()
            .id("header-theme-trigger")
            .debug_selector(|| "header-theme-trigger".into())
            .child(
                Button::new("theme")
                    .ghost()
                    .compact()
                    .icon(IconName::Palette)
                    .tooltip("Theme")
                    .tooltip_placement(Placement::Bottom)
                    .selected(open)
                    .when(open, |btn| btn.primary())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_filter = None;
                        this.open_scope = None;
                        this.state.gantt.context_menu = None;
                        this.open_flavor = !this.open_flavor;
                        cx.notify();
                    })),
            );
        let mut popup = Popup::new("flavor-popup", trigger).offset(px(12.));
        if open {
            popup = popup.content(
                self.popup_panel("flavor-panel", cx)
                    .w(px(180.))
                    .min_w(px(160.))
                    .min_h(px(0.))
                    .children(FLAVORS.iter().map(|flavor| {
                        let selected = current == *flavor;
                        let flavor = *flavor;
                        let caption = if selected {
                            format!("✓ {}", flavor.label())
                        } else {
                            flavor.label().to_string()
                        };
                        div()
                            .id(SharedString::from(format!(
                                "flavor-option-{}",
                                flavor.as_str()
                            )))
                            .debug_selector(move || format!("flavor-option-{}", flavor.as_str()))
                            .child(
                                Button::new(flavor.as_str())
                                    .label(caption)
                                    .selected(selected)
                                    .when(selected, |btn| btn.primary())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.open_flavor = false;
                                        this.state.set_flavor(flavor);
                                        apply_flavor(cx, flavor);
                                        cx.notify();
                                    })),
                            )
                    })),
            );
        }
        div()
            .id("flavor")
            .debug_selector(|| "flavor".into())
            .child(popup)
    }

    fn render_gantt(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.state.scope().is_none() {
            return v_flex()
                .id("choose-team")
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child("Choose a Team"),
                )
                .child(Label::new(
                    "Pick an org, project, and Team to load the Hierarchy.",
                ))
                .into_any_element();
        }
        let visible = self.state.visible_nodes();
        let empty_canvas = visible.is_empty();
        if empty_canvas
            && !self.state.hierarchy_loading
            && !self.state.has_saved_gantt_view()
            && self.camera_anim.is_none()
        {
            let today = jiff::Zoned::now().date();
            let today_x = self.state.gantt.today_x(today);
            let width = self.state.gantt.timeline_width.max(MIN_TIMELINE_WIDTH);
            if today_x < 0.0 || today_x > width {
                self.state.gantt.go_today(today);
            }
        }
        let committed = self.state.gantt.snapshot();
        if let Some(pose) = self.animated_camera() {
            self.state.gantt.restore(pose);
        }
        let rows = self.state.gantt.flatten(&visible);
        let viewport_h = self.state.gantt.viewport_h.max(120.0);
        let content_h = (viewport_h - TIMELINE_HEADER_H).max(80.0);
        let window = self.state.gantt.visible_window(&rows, content_h).to_vec();
        let bars = self
            .state
            .gantt
            .layout_bars(&visible, &rows, &self.state.iterations, content_h);
        let gantt = v_flex()
            .id("gantt")
            .size_full()
            .relative()
            .child(self.render_gantt_nav(cx))
            .child(
                h_flex()
                    .id("gantt-body")
                    .flex_1()
                    .min_w(px(0.))
                    .relative()
                    .overflow_hidden()
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if !*hovered {
                            this.clear_canvas_hover(cx);
                        }
                    }))
                    .child(self.render_tree(&visible, &window, content_h, cx))
                    .child(
                        h_flex()
                            .id("gantt-splitter")
                            .debug_selector(|| "gantt-splitter".into())
                            .relative()
                            .w(px(SPLITTER_WIDTH))
                            .flex_shrink_0()
                            .h_full()
                            .items_center()
                            .justify_center()
                            .cursor_col_resize()
                            .hover(|el| el.bg(cx.theme().muted.opacity(0.45)))
                            .child(
                                div()
                                    .id("gantt-splitter-line")
                                    .w(px(2.))
                                    .h_full()
                                    .bg(cx.theme().border),
                            )
                            .child(
                                div()
                                    .id("gantt-splitter-grip")
                                    .debug_selector(|| "gantt-splitter-grip".into())
                                    .absolute()
                                    .w(px(4.))
                                    .h(px(36.))
                                    .rounded_full()
                                    .bg(cx.theme().muted_foreground.opacity(0.55)),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                    if event.click_count >= 2 {
                                        this.state.gantt.reset_tree_width();
                                        this.state.gantt.splitter_drag = None;
                                        this.state.persist_tree_width();
                                    } else {
                                        let x: f32 = event.position.x.into();
                                        this.state.gantt.viewport_w =
                                            f32::from(window.viewport_size().width);
                                        this.state.gantt.splitter_drag =
                                            Some((x, this.state.gantt.tree_width));
                                    }
                                    cx.notify();
                                }),
                            ),
                    )
                    .child(self.render_timeline(&window, &bars, content_h, cx))
                    .when(empty_canvas && !self.state.hierarchy_loading, |el| {
                        el.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .occlude()
                                .child(
                                    v_flex()
                                        .id("empty-gantt-card")
                                        .debug_selector(|| "empty-gantt-card".into())
                                        .bg(cx.theme().background)
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .rounded_md()
                                        .p_4()
                                        .shadow_md()
                                        .occlude()
                                        .child(self.render_empty_gantt(cx)),
                                ),
                        )
                    }),
            )
            .when(self.state.hierarchy_loading, |el| {
                el.child(self.render_gantt_loading(cx))
            })
            .into_any_element();
        self.state.gantt.restore(committed);
        gantt
    }

    fn render_gantt_loading(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .id("gantt-loading")
            .debug_selector(|| "gantt-loading".into())
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .bg(cx.theme().overlay)
            .child(Label::new("Loading Hierarchy…"))
            .child(
                v_flex()
                    .id("gantt-loading-skeleton")
                    .debug_selector(|| "gantt-loading-skeleton".into())
                    .w(px(320.))
                    .gap_2()
                    .children(
                        (0..4).map(|_| div().h(px(12.)).w_full().rounded_sm().bg(cx.theme().muted)),
                    ),
            )
    }

    fn render_empty_gantt(&self, cx: &Context<Self>) -> impl IntoElement {
        let team = self.state.prefs.planner.team.clone();
        let hidden = self.state.hidden_by_filters_count();
        let filtered = hidden > 0 || self.state.filters_hide_work_items();
        let message = if let Some(error) = self.state.team_settings_error.as_ref() {
            error.clone()
        } else if filtered {
            let noun = if hidden == 1 { "item" } else { "items" };
            format!("{hidden} {noun} hidden by filters")
        } else if team.is_empty() {
            "Choose a team to load work items.".into()
        } else {
            format!(
                "Team \"{team}\" has no work items. Pick another team, or create work in Azure DevOps."
            )
        };
        v_flex()
            .id("empty-gantt")
            .debug_selector(|| "empty-gantt".into())
            .items_center()
            .justify_center()
            .px_8()
            .gap_2()
            .child(Label::new(message))
            .when(filtered, |el| {
                el.child(
                    div()
                        .id("empty-show-all")
                        .debug_selector(|| "empty-show-all".into())
                        .child(
                            Button::new("empty-show-all-btn")
                                .primary()
                                .label("Show all")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.state.show_all_types();
                                    this.state.show_all_states();
                                    this.state.show_all_roots();
                                    this.state.set_assignee("anyone".into());
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when(!filtered, |el| {
                el.child(
                    Label::new("Use Org / Project / Team above to switch scope.")
                        .text_sm()
                        .text_color(cx.theme().muted_foreground),
                )
            })
    }

    fn render_gantt_nav(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let sync_label = self.sync_status_text();
        let sync_color = if sync_label == "Error" {
            cx.theme().danger
        } else if sync_label == "Saving…" || sync_label == "Loading…" {
            cx.theme().warning
        } else {
            cx.theme().success
        };
        h_flex()
            .id("gantt-nav")
            .debug_selector(|| "gantt-nav".into())
            .w_full()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if *hovered {
                    this.clear_canvas_hover(cx);
                }
            }))
            .child(
                div()
                    .id("gantt-today-btn")
                    .debug_selector(|| "gantt-today-btn".into())
                    .child(
                        Button::new("today")
                            .primary()
                            .label("Today")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.animate_gantt(cx, |gantt| {
                                    gantt.retarget_year_gutter(|gantt| {
                                        gantt.go_today(jiff::Zoned::now().date());
                                    });
                                });
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("gantt-add-item")
                    .debug_selector(|| "gantt-add-item".into())
                    .child(
                        Button::new("add-item")
                            .primary()
                            .label("+ Add item")
                            .tooltip("Add a child work item under the selected row.")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.state.gantt.clear_hover();
                                this.spawn_state(cx, |mut state| async move {
                                    let result = state.add_item().await;
                                    (state, result)
                                });
                            })),
                    ),
            )
            .child(
                Button::new("auto-fit")
                    .primary()
                    .label("Auto Fit")
                    .tooltip("Fit scheduled items into the visible timeline.")
                    .tooltip_placement(Placement::Bottom)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let nodes = this.state.visible_nodes();
                        let iterations = this.state.iterations.clone();
                        let width = this.state.gantt.timeline_width.max(MIN_TIMELINE_WIDTH);
                        this.animate_gantt(cx, |gantt| {
                            gantt.retarget_year_gutter(|gantt| {
                                gantt.auto_fit(&nodes, &iterations, width);
                            });
                        });
                        cx.notify();
                    })),
            )
            .child(
                Button::new("prev-period")
                    .ghost()
                    .compact()
                    .label("‹")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.animate_gantt(cx, |gantt| {
                            gantt.retarget_year_gutter(|gantt| {
                                gantt.step_period(-1);
                            });
                        });
                        cx.notify();
                    })),
            )
            .child(
                Button::new("next-period")
                    .ghost()
                    .compact()
                    .label("›")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.animate_gantt(cx, |gantt| {
                            gantt.retarget_year_gutter(|gantt| {
                                gantt.step_period(1);
                            });
                        });
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("gantt-range-title")
                    .debug_selector(|| "gantt-range-title".into())
                    .w(px(240.))
                    .flex_shrink_0()
                    .truncate()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(self.state.gantt.viewport_range_title()),
            )
            .child(
                h_flex()
                    .id("gantt-scale-group")
                    .debug_selector(|| "gantt-scale-group".into())
                    .flex_shrink_0()
                    .gap_1()
                    .children(GanttScale::ALL.iter().map(|scale| {
                        let scale = *scale;
                        let selected = self.state.gantt.scale == scale;
                        let label = scale.toolbar_label(self.state.gantt.zoom, selected);
                        let selector = format!("gantt-scale-{}", scale.label());
                        div()
                            .id(SharedString::from(selector.clone()))
                            .debug_selector(move || selector)
                            .w(px(SCALE_BUTTON_WIDTH))
                            .flex_shrink_0()
                            .child(
                                Button::new(scale.label())
                                    .label(label)
                                    .compact()
                                    .w(px(SCALE_BUTTON_WIDTH))
                                    .selected(selected)
                                    .when(selected, |btn| btn.primary())
                                    .when(!selected, |btn| btn.ghost())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.dismiss_overlays();
                                        this.animate_gantt(cx, |gantt| {
                                            gantt.retarget_year_gutter(|gantt| {
                                                let today = jiff::Zoned::now().date();
                                                gantt.set_scale_from_toolbar(scale, today);
                                            });
                                        });
                                        cx.notify();
                                    })),
                            )
                    })),
            )
            .child(
                div()
                    .id("gantt-zoom-out")
                    .debug_selector(|| "gantt-zoom-out".into())
                    .flex_shrink_0()
                    .child(
                        Button::new("zoom-out")
                            .ghost()
                            .compact()
                            .label("−")
                            .tooltip("Zoom out")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.animate_gantt(cx, |gantt| {
                                    gantt.retarget_year_gutter(|gantt| {
                                        let today = jiff::Zoned::now().date();
                                        let keep_x = gantt.toolbar_scale_keep_x(today);
                                        gantt.zoom_step_keeping(-1, keep_x);
                                    });
                                });
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("gantt-zoom-level")
                    .debug_selector(|| "gantt-zoom-level".into())
                    .flex_shrink_0()
                    .px_1()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(self.state.gantt.scale.label()),
            )
            .child(
                div()
                    .id("gantt-zoom-in")
                    .debug_selector(|| "gantt-zoom-in".into())
                    .flex_shrink_0()
                    .child(
                        Button::new("zoom-in")
                            .ghost()
                            .compact()
                            .label("+")
                            .tooltip("Zoom in")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.animate_gantt(cx, |gantt| {
                                    gantt.retarget_year_gutter(|gantt| {
                                        let today = jiff::Zoned::now().date();
                                        let keep_x = gantt.toolbar_scale_keep_x(today);
                                        gantt.zoom_step_keeping(1, keep_x);
                                    });
                                });
                                cx.notify();
                            })),
                    ),
            )
            .child(
                Button::new("scroll-up")
                    .ghost()
                    .compact()
                    .label("▲")
                    .tooltip("Scroll up")
                    .tooltip_placement(Placement::Bottom)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.scroll_or_select(-3, cx);
                    })),
            )
            .child(
                Button::new("scroll-down")
                    .ghost()
                    .compact()
                    .label("▼")
                    .tooltip("Scroll down")
                    .tooltip_placement(Placement::Bottom)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.scroll_or_select(3, cx);
                    })),
            )
            .child(
                div()
                    .id("gantt-sync-status")
                    .debug_selector(|| "gantt-sync-status".into())
                    .ml_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .text_color(sync_color)
                    .bg(sync_color.opacity(0.12))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .id("gantt-sync-status-icon")
                                    .debug_selector(|| "gantt-sync-status-icon".into())
                                    .w(px(14.))
                                    .h(px(14.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_sm()
                                    .line_height(gpui_kit::relative(1.))
                                    .text_color(sync_color)
                                    .child(self.sync_status_glyph()),
                            )
                            .child(sync_label),
                    ),
            )
    }

    fn io_saving(&self) -> bool {
        self.io_busy
            || !self.date_save_busy.is_empty()
            || self.state.form.as_ref().is_some_and(|form| form.saving)
    }

    fn arm_saving_status(&mut self, cx: &mut Context<Self>) {
        if self.io_saving() {
            if self.saving_shown_at.is_none() {
                self.saving_shown_at = Some(Instant::now());
                cx.notify();
            }
            self.saving_hold_gen += 1;
            return;
        }
        let Some(at) = self.saving_shown_at else {
            return;
        };
        let remain = Duration::from_millis(SAVING_HOLD_MS).saturating_sub(at.elapsed());
        if remain == Duration::ZERO {
            self.saving_shown_at = None;
            return;
        }
        self.saving_hold_gen += 1;
        let hold_gen = self.saving_hold_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(remain).await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    if this.saving_hold_gen != hold_gen {
                        return;
                    }
                    if !this.io_saving() {
                        this.saving_shown_at = None;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    fn finish_or_hold_saving_status(&mut self, cx: &mut Context<Self>) {
        if self.skip_saving_hold {
            self.skip_saving_hold = false;
            self.saving_hold_gen += 1;
            if !self.io_saving() {
                self.saving_shown_at = None;
                cx.notify();
            }
            return;
        }
        self.arm_saving_status(cx);
    }

    fn sync_status_text(&self) -> String {
        if self.state.hierarchy_loading || self.form_loading.is_some() {
            "Loading…".into()
        } else if self.io_saving() || self.saving_shown_at.is_some() {
            "Saving…".into()
        } else if self
            .state
            .toasts
            .items
            .last()
            .is_some_and(|toast| toast.kind == ToastKind::Error)
        {
            "Error".into()
        } else {
            "Synced".into()
        }
    }

    fn sync_status_glyph(&self) -> &'static str {
        match self.sync_status_text().as_str() {
            "Error" => "⚠",
            "Saving…" | "Loading…" => "↻",
            _ => "☁",
        }
    }

    fn render_tree(
        &mut self,
        nodes: &[ado_core::WorkItemNode],
        window: &[crate::gantt::VisibleRow],
        content_h: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let by_id: std::collections::HashMap<_, _> =
            nodes.iter().map(|n| (n.id, n.clone())).collect();
        let header_fg = cx.theme().muted_foreground;
        let hover_bg = cx.theme().list_hover;
        let selected_bg = cx.theme().list_active;
        let stripe = cx.theme().muted;
        let row_bg = cx.theme().background;
        let hovered_id = self.state.gantt.hovered_id;
        let first_row = self.state.gantt.first_visible_row_index();
        let remainder = self.state.gantt.row_scroll_remainder();
        let show_type = self.state.gantt.show_type_column();
        let show_assignee = self.state.gantt.show_assignee_column();
        let show_dates = self.state.gantt.show_dates_column();
        v_flex()
            .id("gantt-tree")
            .debug_selector(|| "gantt-tree".into())
            .w(px(self.state.gantt.tree_width))
            .min_w(px(0.))
            .max_w(px(self.state.gantt.max_tree_width()))
            .flex_shrink_1()
            .overflow_hidden()
            .h_full()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if *hovered {
                    this.clear_canvas_hover(cx);
                }
            }))
            .child(
                h_flex()
                    .id("gantt-tree-header")
                    .debug_selector(|| "gantt-tree-header".into())
                    .flex_shrink_0()
                    .h(px(TIMELINE_HEADER_H))
                    .px_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(header_fg)
                            .child("Work item"),
                    )
                    .when(show_dates, |el| {
                        el.child(
                            div()
                                .w(px(DATES_COL_WIDTH))
                                .flex_shrink_0()
                                .pl_1()
                                .pr(px(8.))
                                .text_xs()
                                .text_color(header_fg)
                                .child("Dates"),
                        )
                    }),
            )
            .child(
                v_flex()
                    .id("gantt-tree-body")
                    .debug_selector(|| "gantt-tree-body".into())
                    .flex_1()
                    .overflow_hidden()
                    .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                        this.apply_scroll(event, cx);
                    }))
                    .child(
                        v_flex()
                            .mt(px(-remainder))
                            .children(window.iter().enumerate().map(|(offset, row)| {
                                let node = by_id.get(&row.id).cloned();
                                let Some(node) = node else {
                                    return div().into_any_element();
                                };
                                let id = node.id;
                                let selected = self.state.gantt.selected_id == Some(id);
                                let striped = (first_row + offset) % 2 == 1;
                                let hint =
                                    crate::gantt::iteration_for(&node, &self.state.iterations);
                                let dates = dates_column_text(&node, nodes, hint.as_ref());
                                let depth = row.depth;
                                let expanded = row.expanded;
                                let has_children = row.has_children;
                                let unscheduled = is_unscheduled(
                                    node.start_date.as_deref(),
                                    node.target_date.as_deref(),
                                );
                                let can_schedule = unscheduled && node.has_date_fields;
                                let title = node.title.clone();
                                let assigned = node
                                    .assigned_to
                                    .as_ref()
                                    .map(|a| a.display_name.clone())
                                    .unwrap_or_else(|| "Unassigned".into());
                                let editing_title = self.inline_edit == Some(InlineEdit::Title(id));
                                let hovered = hovered_id == Some(id);
                                let dim_selection = self.details_overlay()
                                    || self.pending_delete.is_some();
                                let row_selector = format!("gantt-tree-row-{id}");
                                div()
                                    .id(("work-item", id as u64))
                                    .debug_selector(move || row_selector.clone())
                                    .role(Role::Row)
                                    .aria_label(format!("#{id} {}", node.title))
                                    .relative()
                                    .w_full()
                                    .h(px(ROW_HEIGHT))
                                    .min_h(px(ROW_HEIGHT))
                                    .max_h(px(ROW_HEIGHT))
                                    .flex_basis(px(ROW_HEIGHT))
                                    .flex_shrink_0()
                                    .overflow_hidden()
                                    .bg(if hovered {
                                        hover_bg
                                    } else if selected && !dim_selection {
                                        selected_bg.opacity(SELECTED_ROW_TINT)
                                    } else if striped {
                                        stripe
                                    } else {
                                        row_bg
                                    })
                                    .when(!selected && !hovered, |el| el.hover(|el| el.bg(hover_bg)))
                                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                        this.set_hovered(
                                            if *hovered { Some(id) } else { None },
                                            cx,
                                        );
                                    }))
                                    .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                                        this.set_hovered(Some(id), cx);
                                    }))
                                    .on_click(cx.listener(
                                        move |this, event: &gpui_kit::ClickEvent, window, cx| {
                                            this.open_filter = None;
                                            this.open_scope = None;
                                            this.open_flavor = false;
                                            this.state.gantt.context_menu = None;
                                            if this
                                                .state
                                                .gantt
                                                .pending_nudge
                                                .as_ref()
                                                .is_some_and(|pending| pending.id != id)
                                            {
                                                this.state.discard_nudge();
                                            }
                                            this.state.gantt.select_row(id);
                                            if this.inline_edit.is_none() {
                                                window.focus(&this.form_focus, cx);
                                            }
                                            if event.click_count() >= 2 {
                                                this.open_form(id, cx);
                                            }
                                            cx.notify();
                                        },
                                    ))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            if this.inline_edit.is_some() {
                                                return;
                                            }
                                            if this.state.gantt.context_menu.is_some() {
                                                return;
                                            }
                                            if can_schedule {
                                                this.state.gantt.schedule_drag = Some(id);
                                                this.state.gantt.select_row(id);
                                                this.saw_pressed_drag = false;
                                                this.schedule_origin_x = None;
                                                this.pill_press_window_x = None;
                                                this.schedule_live_x = None;
                                                cx.notify();
                                            }
                                        }),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                            let x: f32 = event.position.x.into();
                                            let y: f32 = event.position.y.into();
                                            this.state.gantt.select_row(id);
                                            this.state.gantt.context_menu =
                                                Some(crate::gantt::ContextTarget {
                                                    id,
                                                    from_bar: false,
                                                    x,
                                                    y,
                                                    link: None,
                                                });
                                            cx.notify();
                                        }),
                                    )
                                    .when(selected && !dim_selection, |el| {
                                        el.child(Self::selected_row_accent(id, cx))
                                    })
                                    .child(
                                        h_flex()
                                            .h_full()
                                            .w_full()
                                            .items_center()
                                            .overflow_hidden()
                                            .px_2()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .id(("tree-indent", id as u64))
                                                    .debug_selector(move || {
                                                        format!("gantt-tree-indent-{id}")
                                                    })
                                                    .w(px(depth as f32 * TREE_INDENT))
                                                    .min_w(px(depth as f32 * TREE_INDENT))
                                                    .flex_shrink_0(),
                                            )
                                            .child(
                                                div()
                                                    .id(("expand-slot", id as u64))
                                                    .debug_selector(move || {
                                                        format!("gantt-expand-slot-{id}")
                                                    })
                                                    .w(px(EXPANDER_SLOT))
                                                    .min_w(px(EXPANDER_SLOT))
                                                    .flex_shrink_0()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(
                                                        Button::new(("expand", id as u64))
                                                            .ghost()
                                                            .compact()
                                                            .tab_stop(false)
                                                            .focus_ring(false)
                                                            .with_size(Size::XSmall)
                                                            .label(if !has_children {
                                                                " "
                                                            } else if expanded {
                                                                "▾"
                                                            } else {
                                                                "▸"
                                                            })
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    this.state.gantt.toggle_expand(id);
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .id(("id-cell", id as u64))
                                                    .w(px(ID_COL_WIDTH))
                                                    .flex_shrink_0()
                                                    .truncate()
                                                    .child(format!("#{id}"))
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(
                                                            move |this, event: &MouseDownEvent, _, cx| {
                                                                if event.click_count >= 2 {
                                                                    this.open_form(id, cx);
                                                                    cx.stop_propagation();
                                                                }
                                                            },
                                                        ),
                                                    ),
                                            )
                                            .child(self.title_cell(id, &title, selected, cx))
                                            .child(
                                                div()
                                                    .id(("gantt-details-slot", id as u64))
                                                    .debug_selector(|| "gantt-details-slot".into())
                                                    .w(px(DETAILS_PILL_WIDTH))
                                                    .flex_shrink_0()
                                                    .h_full()
                                                    .flex()
                                                    .items_center()
                                                    .when(!editing_title, |el| {
                                                        el.child(self.open_details_icon(id, cx))
                                                    }),
                                            )
                                            .when(show_type, |el| {
                                                el.child(self.type_badge(&node.work_item_type, cx))
                                            })
                                            .when(show_assignee, |el| {
                                                el.child(self.assignee_cell(
                                                    id, &assigned, selected, cx,
                                                ))
                                            })
                                            .when(show_dates, |el| {
                                                el.child(self.dates_cell(id, dates, cx))
                                            }),
                                    )
                                    .into_any_element()
                            }))
                            .children({
                                let fill = crate::gantt::GanttView::filler_row_count(
                                    window.len(),
                                    content_h,
                                );
                                (0..fill).map(move |index| {
                                    div()
                                        .id(("gantt-tree-fill", index as u64))
                                        .debug_selector(|| "gantt-grid-fill".into())
                                        .w_full()
                                        .h(px(ROW_HEIGHT))
                                        .min_h(px(ROW_HEIGHT))
                                        .flex_shrink_0()
                                        .bg(row_bg)
                                })
                            }),
                    ),
            )
    }

    fn open_details_icon(&self, id: i64, cx: &Context<Self>) -> impl IntoElement {
        div()
            .id(("gantt-open-details", id as u64))
            .debug_selector(|| "gantt-open-details".into())
            .flex_shrink_0()
            .child(
                Button::new(("open-details", id as u64))
                    .ghost()
                    .compact()
                    .tab_stop(false)
                    .focus_ring(false)
                    .with_size(Size::Small)
                    .label("ⓘ")
                    .when(!self.details_overlay(), |btn| {
                        btn.tooltip(Self::details_icon_hint())
                            .tooltip_placement(Placement::Bottom)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.open_form(id, cx);
                    })),
            )
    }

    fn type_badge(&self, ty: &str, cx: &Context<Self>) -> impl IntoElement {
        let color = color_for_type(ty);
        let parsed = u32::from_str_radix(color.trim_start_matches('#'), 16).unwrap_or(0x89b4fa);
        let abbrev = type_abbrev(ty);
        let full = ty.to_string();
        let show_tip = abbrev != full && !self.details_overlay();
        h_flex()
            .id(SharedString::from(format!("gantt-type-{ty}")))
            .debug_selector(|| "gantt-type-badge".into())
            .gap_1()
            .px_1()
            .max_w(px(TYPE_COL_WIDTH))
            .flex_shrink_0()
            .overflow_hidden()
            .rounded_md()
            .bg(cx.theme().muted)
            .when(show_tip, |el| {
                let tip = full.clone();
                el.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            })
            .child(
                div()
                    .size(px(8.))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(rgb(parsed)),
            )
            .child(
                div()
                    .text_xs()
                    .truncate()
                    .text_color(rgb(parsed))
                    .child(abbrev),
            )
    }

    fn title_cell(
        &self,
        id: i64,
        title: &str,
        _selected: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        if self.inline_edit == Some(InlineEdit::Title(id)) {
            return div()
                .id(("inline-title-wrap", id as u64))
                .debug_selector(|| "gantt-inline-title".into())
                .flex_1()
                .min_w(px(0.))
                .max_w_full()
                .overflow_hidden()
                .child(
                    Input::new(&self.inline_input)
                        .id(("inline-title", id as u64))
                        .w_full(),
                )
                .into_any_element();
        }
        let title_owned = title.to_string();
        let tip = format!("{title_owned} · Double-click to open · F2 or Edit title to edit");
        let form_open = self.details_overlay();
        div()
            .id(("title", id as u64))
            .debug_selector(move || format!("gantt-title-{id}"))
            .flex_1()
            .min_w(px(180.))
            .overflow_hidden()
            .child(
                Button::new(("title-tip", id as u64))
                    .ghost()
                    .compact()
                    .tab_stop(false)
                    .focus_ring(false)
                    .w_full()
                    .px_0()
                    .label(title_owned)
                    .when(!form_open, |btn| {
                        btn.tooltip(tip).tooltip_placement(Placement::Top)
                    }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.activate_title(id, event.click_count, window, cx);
                }),
            )
            .on_click(
                cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                    cx.stop_propagation();
                    this.activate_title(id, event.click_count(), window, cx);
                }),
            )
            .into_any_element()
    }

    fn assignee_cell(
        &self,
        id: i64,
        assigned: &str,
        _selected: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        if self.inline_edit == Some(InlineEdit::Assignee(id)) {
            return div()
                .id(("inline-assignee-wrap", id as u64))
                .debug_selector(|| "gantt-inline-assignee".into())
                .w(px(120.))
                .flex_shrink_0()
                .child(Input::new(&self.inline_input).id(("inline-assignee", id as u64)))
                .into_any_element();
        }
        let assigned_owned = assigned.to_string();
        let tip = assigned_owned.clone();
        div()
            .id(("assignee", id as u64))
            .w(px(ASSIGNEE_COL_WIDTH))
            .flex_shrink_0()
            .truncate()
            .text_xs()
            .child(assigned_owned.clone())
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.state.gantt.select_row(id);
                this.begin_inline(InlineEdit::Assignee(id), assigned_owned.clone(), window, cx);
                cx.stop_propagation();
            }))
            .into_any_element()
    }

    fn dates_cell(&self, id: i64, text: String, cx: &Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let editing = self.inline_edit == Some(InlineEdit::Dates(id))
            || self.inline_edit == Some(InlineEdit::Start(id))
            || self.inline_edit == Some(InlineEdit::Target(id));
        let unscheduled = text.starts_with("Unscheduled");
        let full = if let Some(rest) = text.strip_prefix("Unscheduled") {
            let hint = rest.trim_start_matches([' ', '·']).trim().to_string();
            if hint.is_empty() {
                "Unscheduled · drop on timeline".to_string()
            } else {
                format!("Unscheduled · {hint}")
            }
        } else {
            text.clone()
        };
        let display = if unscheduled && full.len() > 18 {
            "Unscheduled".to_string()
        } else {
            full.clone()
        };
        let overflow = parent_date_overflow(&self.state.nodes, id);
        let overflow_tip = parent_overflow_tip(&self.state.nodes, id);
        let tip = overflow_tip.clone().unwrap_or(full);
        h_flex()
            .id(("dates", id as u64))
            .debug_selector(move || {
                if unscheduled {
                    "unscheduled-chip".into()
                } else {
                    "gantt-dates-cell".into()
                }
            })
            .flex_shrink_0()
            .w(px(DATES_COL_WIDTH))
            .h(px(ROW_HEIGHT))
            .items_center()
            .pl_1()
            .pr(px(8.))
            .gap_1()
            .text_xs()
            .when(unscheduled, |el| el.text_color(muted))
            .when(overflow, |el| el.text_color(cx.theme().warning))
            .when(editing, |el| el.bg(cx.theme().accent.opacity(0.35)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .truncate()
                    .child(display.clone()),
            )
            .when(overflow, |el| {
                let overflow_tip = overflow_tip
                    .clone()
                    .unwrap_or_else(|| "Outside parent's dates".to_string());
                el.child(
                    div()
                        .id(("dates-overflow", id as u64))
                        .debug_selector(|| "gantt-dates-overflow".into())
                        .flex_shrink_0()
                        .w(px(16.))
                        .h(px(16.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(cx.theme().warning)
                        .text_color(cx.theme().background)
                        .text_size(px(14.))
                        .line_height(gpui_kit::relative(1.))
                        .font_weight(gpui_kit::FontWeight::BOLD)
                        .occlude()
                        .tooltip(move |window, cx| {
                            Tooltip::new(overflow_tip.clone()).build(window, cx)
                        })
                        .child("!"),
                )
            })
            .when(!editing, |el| {
                el.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.state.gantt.select_row(id);
                    let x: f32 = event.position.x.into();
                    let y: f32 = event.position.y.into();
                    this.dates_anchor = Some((x, y + 18.0));
                    this.begin_inline(InlineEdit::Dates(id), display.clone(), window, cx);
                }),
            )
            .into_any_element()
    }

    fn render_dates_popover(&self, cx: &Context<Self>) -> impl IntoElement {
        let editing_dates = matches!(
            self.inline_edit,
            Some(InlineEdit::Dates(_) | InlineEdit::Start(_) | InlineEdit::Target(_))
        );
        if !editing_dates {
            return div().into_any_element();
        }
        let (x, mut y) = self.dates_anchor.unwrap_or((320.0, 160.0));
        let window_h = self.state.gantt.viewport_h + 200.0;
        let window_w = self.state.gantt.viewport_w.max(320.0);
        let popover_h = 380.0;
        let popover_w = 280.0;
        if y + popover_h > window_h {
            y = (y - popover_h).max(8.0);
        }
        let x = x.clamp(8.0, (window_w - popover_w).max(8.0));
        div()
            .id("gantt-dates-popover")
            .absolute()
            .left(px(x))
            .top(px(y))
            .when_some(self.inline_dates.clone(), |el, state| {
                el.child(
                    v_flex()
                        .id("gantt-inline-dates")
                        .debug_selector(|| "gantt-inline-dates".into())
                        .w(px(280.))
                        .max_w(px(280.))
                        .p_2()
                        .gap_2()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().popover)
                        .shadow_md()
                        .occlude()
                        .child(
                            div()
                                .id("gantt-dates-typed")
                                .debug_selector(|| "gantt-dates-typed".into())
                                .w_full()
                                .child(Input::new(&self.inline_input).id("inline-date").w_full()),
                        )
                        .child(
                            div()
                                .id("gantt-dates-error")
                                .debug_selector(|| "gantt-dates-error".into())
                                .w_full()
                                .h(px(DATES_ERROR_SLOT_H))
                                .max_h(px(DATES_ERROR_SLOT_H))
                                .overflow_hidden()
                                .text_xs()
                                .text_color(cx.theme().danger)
                                .whitespace_normal()
                                .child(self.inline_dates_error.clone().unwrap_or_default()),
                        )
                        .child(
                            div()
                                .id("gantt-dates-cal-gap")
                                .w_full()
                                .h(px(12.))
                                .flex_shrink_0(),
                        )
                        .child(self.render_calendar_chrome(&state, cx)),
                )
            })
            .when(self.inline_dates.is_none(), |el| {
                el.child(
                    div()
                        .id("inline-date-fallback")
                        .debug_selector(|| "gantt-inline-dates".into())
                        .min_w(px(220.))
                        .child(Input::new(&self.inline_input).id("inline-date")),
                )
            })
            .into_any_element()
    }

    fn render_calendar_chrome(
        &self,
        picker: &Entity<CalendarState>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let month_title = {
            let state = picker.read(cx);
            format!(
                "{} {}",
                month_name(state.current_month()),
                state.current_year()
            )
        };
        let prev = picker.clone();
        let next = picker.clone();
        v_flex()
            .w_full()
            .gap_2()
            .child(
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .id("gantt-dates-prev-month")
                            .debug_selector(|| "gantt-dates-prev-month".into())
                            .min_w(px(32.))
                            .min_h(px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.stop_propagation();
                                prev.update(cx, |state, cx| {
                                    state.prev_month();
                                    cx.notify();
                                });
                            }))
                            .child(
                                Button::new("dates-prev-month")
                                    .ghost()
                                    .label("‹")
                                    .tooltip("Previous month"),
                            ),
                    )
                    .child(
                        div()
                            .id("gantt-dates-month-label")
                            .debug_selector(|| "gantt-dates-month-label".into())
                            .flex_1()
                            .flex()
                            .justify_center()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(month_title),
                    )
                    .child(
                        div()
                            .id("gantt-dates-next-month")
                            .debug_selector(|| "gantt-dates-next-month".into())
                            .min_w(px(32.))
                            .min_h(px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.stop_propagation();
                                next.update(cx, |state, cx| {
                                    state.next_month();
                                    cx.notify();
                                });
                            }))
                            .child(
                                Button::new("dates-next-month")
                                    .ghost()
                                    .label("›")
                                    .tooltip("Next month"),
                            ),
                    ),
            )
            .child(self.render_month_grid(picker, cx))
    }

    fn render_month_grid(
        &self,
        picker: &Entity<CalendarState>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let state = picker.read(cx);
        let weeks = state.month_days().into_iter().next().unwrap_or_default();
        let selected = state.date();
        let current_month = u32::from(state.current_month());
        let muted = cx.theme().muted_foreground;
        let weekdays = ["S", "M", "T", "W", "T", "F", "S"];
        v_flex()
            .id("gantt-dates-grid")
            .debug_selector(|| "gantt-dates-grid".into())
            .w_full()
            .gap_1()
            .child(
                h_flex()
                    .w_full()
                    .children(weekdays.iter().copied().enumerate().map(|(index, label)| {
                        div()
                            .id(("gantt-dates-weekday", index))
                            .flex_1()
                            .h(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .text_color(muted)
                            .child(label)
                    })),
            )
            .children(weeks.into_iter().enumerate().map(|(week_index, week)| {
                let picker = picker.clone();
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .children(week.into_iter().map(move |day| {
                        let (is_start, is_end, in_range) = range_day_flags(selected, day);
                        let other_month = day.month() != current_month;
                        let day_key = day.to_string();
                        let selector = if is_start {
                            "gantt-dates-range-start".to_string()
                        } else if is_end {
                            "gantt-dates-range-end".to_string()
                        } else if in_range {
                            "gantt-dates-in-range".to_string()
                        } else {
                            format!("gantt-dates-day-{day_key}")
                        };
                        let picker = picker.clone();
                        div()
                            .id(SharedString::from(format!(
                                "gantt-dates-day-{day_key}-{week_index}"
                            )))
                            .debug_selector(move || selector.clone())
                            .flex_1()
                            .h(px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .text_xs()
                            .when(other_month, |el| el.text_color(muted))
                            .when(in_range, |el| {
                                el.bg(cx.theme().primary.opacity(0.28))
                                    .text_color(cx.theme().foreground)
                            })
                            .when(is_start || is_end, |el| {
                                el.bg(cx.theme().primary)
                                    .text_color(cx.theme().primary_foreground)
                            })
                            .on_click(cx.listener(move |_, _, _, cx| {
                                picker.update(cx, |state, cx| {
                                    state.activate_date(day, cx);
                                });
                                cx.notify();
                            }))
                            .child(day.day().to_string())
                    }))
            }))
    }

    fn render_timeline(
        &mut self,
        window: &[crate::gantt::VisibleRow],
        bars: &[crate::gantt::BarLayout],
        content_h: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let width = self.state.gantt.timeline_width.max(MIN_TIMELINE_WIDTH);
        let ticks = self.state.gantt.timeline_ticks(width);
        let today = jiff::Zoned::now().date();
        let today_x = self.state.gantt.today_x(today);
        let header_fg = cx.theme().muted_foreground;
        let muted_fg = cx.theme().foreground;
        let bar_label_fg = gpui_kit::hsla(240. / 360., 0.21, 0.12, 1.);
        let grid = cx.theme().border;
        let weekend = cx.theme().muted.opacity(0.35);
        let hover_bg = cx.theme().list_hover;
        let selected_bg = cx.theme().list_active;
        let stripe = cx.theme().muted;
        let first_row = self.state.gantt.first_visible_row_index();
        let today_color = cx.theme().primary;
        let selected_id = self.state.gantt.selected_id;
        let hovered_id = self.state.gantt.hovered_id;
        let dragging = self.state.gantt.gesture.is_some();
        let show_weekends = self.state.gantt.scale.shows_weekend_bands();
        let weekend_w = self.state.gantt.scale.pixels_per_day(self.state.gantt.zoom);
        let hide_today = matches!(
            self.state.gantt.scale,
            GanttScale::Day
                | GanttScale::Week
                | GanttScale::Month
                | GanttScale::Quarter
                | GanttScale::Year
        )
        .then_some(today_x);
        let major_labels = self
            .state
            .gantt
            .header_major_labels_except(width, hide_today);
        let tick_labels = self
            .state
            .gantt
            .header_tick_labels_except(width, hide_today);
        let today_on_screen = today_x >= 0.0 && today_x <= width;
        // Sit on the major row with year/month banners. Year ticks are months
        // at y=22; putting Today there overlaps "Nov".
        let today_label_top = 2.0;
        let today_label_x = if today_on_screen {
            let pill = today_pill_left(today_x, width);
            if matches!(self.state.gantt.scale, GanttScale::Year) {
                nudge_today_off_ticks(pill, &tick_labels, width)
            } else {
                pill
            }
        } else {
            f32::MAX
        };
        let major_painted: Vec<(f32, f32, String, usize)> = {
            let filtered: Vec<(f32, String)> = major_labels
                .into_iter()
                .filter(|(x, _)| *x < width)
                .collect();
            filtered
                .iter()
                .enumerate()
                .filter_map(|(index, (x, label))| {
                    let next = filtered.get(index + 1).map(|(nx, _)| *nx).unwrap_or(width);
                    let natural = header_label_px(label);
                    let cap = if *x < today_label_x && today_label_x < next {
                        (today_label_x - 8.0).max(*x)
                    } else {
                        next
                    };
                    // Do not force the full glyph width over the Today pill.
                    let w = if *x < today_label_x && cap <= today_label_x {
                        cap - *x
                    } else {
                        (cap - *x).max(natural)
                    };
                    (w > 4.0).then_some((*x, w, label.clone(), index))
                })
                .collect()
        };
        div()
            .id("gantt-timeline")
            .debug_selector(|| "gantt-timeline".into())
            .w(px(width))
            .flex_1()
            .flex_shrink_0()
            .min_w(px(MIN_TIMELINE_WIDTH))
            .h_full()
            .relative()
            .overflow_hidden()
            .bg(cx.theme().background)
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                this.apply_scroll(event, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.state.gantt.context_menu.is_some() {
                        this.state.gantt.context_menu = None;
                    }
                    this.state.gantt.selected_link = None;
                    cx.notify();
                }),
            )
            .child({
                let body_origin_y = self.timeline_body_origin_y.clone();
                let body_origin_x = self.timeline_body_origin_x.clone();
                div()
                    .id("gantt-timeline-body")
                    .debug_selector(|| "gantt-timeline-body".into())
                    .absolute()
                    .top(px(TIMELINE_HEADER_H))
                    .left_0()
                    .right_0()
                    .h(px(content_h))
                    .overflow_hidden()
                    .on_prepaint(move |bounds, _, _| {
                        body_origin_y.set(f32::from(bounds.origin.y));
                        body_origin_x.set(Some(f32::from(bounds.origin.x)));
                    })
                    .children(
                        ticks
                            .iter()
                            .filter(|tick| show_weekends && tick.weekend)
                            .map(|tick| {
                                div()
                                    .absolute()
                                    .left(px(tick.x))
                                    .top_0()
                                    .bottom_0()
                                    .w(px(weekend_w))
                                    .bg(weekend)
                            }),
                    )
                    .children({
                        self.state
                            .gantt
                            .gridline_xs(width)
                            .into_iter()
                            .map(move |x| {
                            div()
                                .absolute()
                                .left(px(x))
                                .top_0()
                                .bottom_0()
                                .w(px(1.))
                                .bg(grid)
                        })
                    })
                    .child(
                        v_flex()
                            .mt(px(-self.state.gantt.row_scroll_remainder()))
                            .children(window.iter().enumerate().map(|(offset, row)| {
                                let id = row.id;
                                let selected = selected_id == Some(id);
                                let hovered = hovered_id == Some(id);
                                let dim_selection = self.details_overlay()
                                    || self.pending_delete.is_some();
                                let striped = (first_row + offset) % 2 == 1;
                                let bar = bars.iter().find(|bar| bar.id == id).cloned();
                                let hit_bar = bar.clone();
                                let band_selector = format!("gantt-row-band-{id}");
                                div()
                                    .id(("gantt-row-band", id as u64))
                                    .debug_selector(move || band_selector.clone())
                                    .relative()
                                    .w_full()
                                    .h(px(ROW_HEIGHT))
                                    .min_h(px(ROW_HEIGHT))
                                    .max_h(px(ROW_HEIGHT))
                                    .flex_basis(px(ROW_HEIGHT))
                                    .flex_shrink_0()
                                    .bg(if hovered {
                                        hover_bg
                                    } else if selected && !dim_selection {
                                        selected_bg.opacity(SELECTED_ROW_TINT)
                                    } else if striped {
                                        stripe
                                    } else {
                                        gpui_kit::hsla(0., 0., 0., 0.)
                                    })
                                    .when(selected && !dim_selection, |el| {
                                        el.child(Self::selected_row_accent(id, cx))
                                    })
                                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                        this.set_hovered(
                                            if *hovered { Some(id) } else { None },
                                            cx,
                                        );
                                    }))
                                    .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                                        this.set_hovered(Some(id), cx);
                                        if this
                                            .state
                                            .gantt
                                            .gesture
                                            .as_ref()
                                            .is_some_and(|g| g.kind == GestureKind::Link)
                                        {
                                            this.state.gantt.set_link_target(id);
                                            cx.notify();
                                        }
                                    }))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                            if this.state.gantt.gesture.is_some()
                                                || this.state.gantt.context_menu.is_some()
                                            {
                                                return;
                                            }
                                            let node = this
                                                .state
                                                .nodes
                                                .iter()
                                                .find(|node| node.id == id);
                                            let can_schedule = node.is_some_and(|node| {
                                                node.has_date_fields
                                                    && is_unscheduled(
                                                        node.start_date.as_deref(),
                                                        node.target_date.as_deref(),
                                                    )
                                            });
                                            if can_schedule {
                                                let x: f32 = event.position.x.into();
                                                this.state.gantt.schedule_drag = Some(id);
                                                this.state.gantt.select_row(id);
                                                this.state.gantt.hovered_id = Some(id);
                                                this.saw_pressed_drag = false;
                                                this.schedule_origin_x = Some(x);
                                                this.schedule_live_x = Some(x);
                                                this.focus_planner(window, cx);
                                                cx.notify();
                                                return;
                                            }
                                            let Some(bar) = hit_bar.as_ref() else {
                                                return;
                                            };
                                            let x: f32 = event.position.x.into();
                                            let painted = this.painted_bar_for(id, bar);
                                            if x < painted.left || x > painted.right {
                                                return;
                                            }
                                            this.state.gantt.select_row(id);
                                            this.focus_planner(window, cx);
                                            this.dismiss_overlays();
                                            this.bar_press_shift = event.modifiers.shift;
                                            this.begin_classified_bar_gesture(
                                                id, bar, x, window, cx,
                                            );
                                        }),
                                    )
                                    .children(bar.map(|bar| {
                                        self.render_gantt_bar(
                                            &bar,
                                            dragging,
                                            bar_label_fg,
                                            muted_fg,
                                            cx,
                                        )
                                    }))
                            }))
                            .children({
                                let fill = crate::gantt::GanttView::filler_row_count(
                                    window.len(),
                                    content_h,
                                );
                                (0..fill).map(move |index| {
                                    div()
                                        .id(("gantt-row-fill", index as u64))
                                        .debug_selector(|| "gantt-grid-fill".into())
                                        .w_full()
                                        .h(px(ROW_HEIGHT))
                                        .min_h(px(ROW_HEIGHT))
                                        .flex_shrink_0()
                                        .bg(gpui_kit::hsla(0., 0., 0., 0.))
                                })
                            }),
                    )
                    .child(
                        div()
                            .id("gantt-today")
                            .debug_selector(|| "gantt-today".into())
                            .absolute()
                            .left(px(today_x))
                            .top_0()
                            .bottom_0()
                            .w(px(3.))
                            .bg(today_color),
                    )
                    .children({
                        let arrows = self.state.gantt.layout_dependency_arrows(bars);
                        arrows
                            .into_iter()
                            .map(|arrow| self.render_dependency_arrow(&arrow, cx))
                    })
                    .children(bars.iter().filter_map(|bar| {
                        self.render_gantt_bar_label(bar, dragging, muted_fg, cx)
                    }))
                    .children(bars.iter().filter_map(|bar| {
                        self.render_gantt_link_handles(bar, dragging, cx)
                    }))
                    .children(bars.iter().filter_map(|bar| {
                        self.render_open_ended_cue(bar, cx)
                    }))
                    .children(bars.iter().filter_map(|bar| {
                        self.render_parent_overflow_badge(bar, cx)
                    }))
                    .children({
                        let rubber = self.state.gantt.gesture.as_ref().and_then(|gesture| {
                            if gesture.kind != GestureKind::Link {
                                return None;
                            }
                            let bar = bars.iter().find(|bar| bar.id == gesture.id)?;
                            Some((bar.x + bar.width + 10.0, bar.y + ROW_HEIGHT - 7.0, gesture.live_x))
                        });
                        rubber.map(|(x1, y, x2)| {
                            let min_x = x1.min(x2);
                            div()
                                .debug_selector(|| "gantt-link-preview".into())
                                .absolute()
                                .left(px(min_x))
                                .top(px(y - 1.0))
                                .w(px((x2 - x1).abs().max(2.0)))
                                .h(px(2.))
                                .bg(cx.theme().primary)
                        })
                    })
                    .children({
                        let ghost = match (
                            self.state.gantt.schedule_drag,
                            self.schedule_origin_x,
                            self.schedule_live_x.or(self.schedule_origin_x),
                        ) {
                            (Some(id), Some(origin), Some(live))
                                if (live - origin).abs() >= 6.0 =>
                            {
                                let (start, target) =
                                    self.state.gantt.schedule_span_from_window_xs(origin, live);
                                let x = self.state.gantt.x_for_date(start);
                                let end = self.state.gantt.x_for_date(target.saturating_add(1.days()));
                                let w = (end - x).abs().max(8.0);
                                let y = window
                                    .iter()
                                    .position(|row| row.id == id)
                                    .map(|offset| {
                                        offset as f32 * ROW_HEIGHT
                                            - self.state.gantt.row_scroll_remainder()
                                    })
                                    .unwrap_or(0.0);
                                Some((x, w, y))
                            }
                            _ => None,
                        };
                        ghost.map(|(x, w, y)| {
                            div()
                                .id("gantt-schedule-ghost")
                                .debug_selector(|| "gantt-schedule-ghost".into())
                                .absolute()
                                .left(px(x))
                                .top(px(y + ((ROW_HEIGHT - PLACEHOLDER_BAR_H) / 2.0).max(4.0)))
                                .h(px(PLACEHOLDER_BAR_H))
                                .w(px(w))
                                .rounded_sm()
                                .bg(cx.theme().primary.opacity(0.28))
                                .border_1()
                                .border_dashed()
                                .border_color(cx.theme().primary)
                        })
                    })
                    .when(
                        timeline_is_empty_of_scheduled(bars)
                            && !self.state.nodes.is_empty()
                            && !self.state.filters_hide_work_items(),
                        |el| {
                        el.child(
                            div()
                                .absolute()
                                .left(px(NO_FIELDS_HINT_W + 36.0))
                                .right_0()
                                .top(px(TIMELINE_HEADER_H))
                                .bottom_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .px_6()
                                .child(
                                    div()
                                        .id("unscheduled-empty-hint")
                                        .debug_selector(|| "unscheduled-empty-hint".into())
                                        .max_w(px(280.))
                                        .px_3()
                                        .py_2()
                                        .rounded_md()
                                        .bg(cx.theme().background)
                                        .occlude()
                                        .text_sm()
                                        .text_color(header_fg)
                                        .child(
                                            "No scheduled dates. Drag a work item onto the timeline to set Start and Target.",
                                        ),
                                ),
                        )
                    })
            })
            .child(
                div()
                    .id("gantt-scale-header")
                    .debug_selector(|| "gantt-scale-header".into())
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(TIMELINE_HEADER_H))
                    .overflow_hidden()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .children(major_painted.into_iter().map(|(x, w, label, index)| {
                        let selector = format!("gantt-major-{index}");
                        div()
                            .id(("gantt-major", index as u64))
                            .debug_selector(move || selector)
                            .absolute()
                            .left(px(x))
                            .top(px(2.))
                            .w(px(w))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_xs()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(muted_fg)
                            .child(label)
                    }))
                    .children({
                        let ticks: Vec<(f32, String)> = tick_labels
                            .into_iter()
                            .filter(|(x, _)| *x > -40.0 && *x < width)
                            .collect();
                        ticks
                            .iter()
                            .enumerate()
                            .map(|(index, (x, label))| {
                                let selector = format!("gantt-tick-{index}");
                                let next = ticks.get(index + 1).map(|(nx, _)| *nx).unwrap_or(width);
                                // Year/Quarter ticks sit on the row below Today.
                                // Capping their width to the pill hid "Nov".
                                let cap = if matches!(
                                    self.state.gantt.scale,
                                    GanttScale::Year | GanttScale::Quarter
                                ) {
                                    next
                                } else if *x < today_label_x && today_label_x < next {
                                    (today_label_x - 4.0).max(*x)
                                } else {
                                    next
                                };
                                let max_w = (cap - *x).max(4.0);
                                div()
                                    .id(("gantt-tick", index as u64))
                                    .debug_selector(move || selector)
                                    .absolute()
                                    .left(px(*x))
                                    .top(px(22.))
                                    .max_w(px(max_w))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_xs()
                                    .text_color(header_fg)
                                    .child(label.clone())
                            })
                            .collect::<Vec<_>>()
                    }),
            )
            .when(today_on_screen, |el| {
                el.child(
                    div()
                        .id("gantt-today-label")
                        .debug_selector(|| "gantt-today-label".into())
                        .absolute()
                        .left(px(today_label_x))
                        .top(px(today_label_top))
                        .whitespace_nowrap()
                        .text_xs()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(today_color)
                        .child("Today"),
                )
            })
            .children({
                self.state.gantt.gesture.as_ref().and_then(|gesture| {
                    if gesture.kind == GestureKind::Link {
                        return None;
                    }
                    let text = crate::gantt::GanttView::drag_preview_text(gesture);
                    let bar = bars.iter().find(|bar| bar.id == gesture.id);
                    let (x, y) = match bar {
                        Some(bar) => {
                            let x = match gesture.kind {
                                GestureKind::ResizeEnd => bar.x + bar.width + 6.0,
                                // Sit over the bar body, not the start grip, so
                                // mouse-up on a left-end resize is not eaten.
                                GestureKind::ResizeStart => (bar.x + 12.0).max(4.0),
                                _ => bar.x.max(4.0),
                            };
                            let y = (TIMELINE_HEADER_H + bar.y - 24.0).max(2.0);
                            (x, y)
                        }
                        None => (gesture.live_x.max(4.0), 4.0),
                    };
                    Some(
                        div()
                            .id("gantt-drag-preview")
                            .debug_selector(|| "gantt-drag-preview".into())
                            .absolute()
                            .left(px(x))
                            .top(px(y))
                            .px_2()
                            .py(px(3.))
                            .rounded_sm()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().popover)
                            .text_color(cx.theme().popover_foreground)
                            .text_xs()
                            .whitespace_nowrap()
                            .shadow_md()
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                                    let x: f32 = event.position.x.into();
                                    let y: f32 = event.position.y.into();
                                    this.release_bar_gesture(x, y, window, cx);
                                }),
                            )
                            .child(text),
                    )
                })
            })
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                this.on_bar_drag_move(event, window, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    let x: f32 = event.position.x.into();
                    let y: f32 = event.position.y.into();
                    this.release_bar_gesture(x, y, window, cx);
                }),
            )
    }

    fn render_gantt_bar(
        &self,
        bar: &crate::gantt::BarLayout,
        dragging: bool,
        bar_label_fg: gpui_kit::Hsla,
        muted_fg: gpui_kit::Hsla,
        cx: &Context<Self>,
    ) -> AnyElement {
        let layout = bar.clone();
        let id = bar.id;
        let summary = bar.summary;
        let unscheduled = bar.kind == ado_core::GanttBarKind::Unscheduled;
        let no_fields = bar.kind == ado_core::GanttBarKind::NoFields;
        let milestone = bar.milestone;
        let marker = bar.marker;
        let open_ended = bar.open_ended;
        let clip_pad = (-bar.x).max(0.0);
        let inside = Self::bar_title_fits_inside(bar);
        let selected = self.state.gantt.selected_id == Some(id);
        let hovered = self.state.gantt.hovered_id == Some(id);
        let can_resize = bar.can_resize();
        let bar_h = if milestone || marker {
            MILESTONE_SIZE
        } else if summary {
            SUMMARY_BAR_H
        } else if unscheduled || no_fields {
            PLACEHOLDER_BAR_H
        } else {
            LEAF_BAR_H
        };
        let bar_top = ((ROW_HEIGHT - bar_h) / 2.0).max(2.0);
        let label_pad = if can_resize { HANDLE_WIDTH + 4.0 } else { 6.0 };
        let open_ended_link_origin = bar.x + bar.width;
        let progress_value = bar.progress.unwrap_or(0.0);
        let show_progress = !open_ended && (summary || progress_value > 0.0);
        let progress_w =
            (bar.width.max(8.0) * progress_value).max(if progress_value > 0.0 { 4.0 } else { 0.0 });
        let visible_progress_w = (progress_w - clip_pad).max(0.0);
        let inner_label_fg = painted_bar_title_color(bar, cx.theme().background);
        let bar_selector = format!("gantt-bar-{id}");
        let progress_selector = format!("gantt-bar-progress-{id}");
        let painted_bars = self.painted_bar_bounds.clone();
        let layout_x = bar.x;
        let grip_w = bar.inner_grip_width();
        // on_prepaint reports the content box; hits use the visible border box.
        let paint_border = if milestone || marker || unscheduled || no_fields || open_ended {
            0.0
        } else if selected {
            2.0
        } else {
            1.0
        };
        let mut items = vec![
            div()
                .id(("gantt-bar", id as u64))
                .debug_selector(move || bar_selector.clone())
                .role(Role::Button)
                .aria_label(format!("Gantt bar #{id} {}", bar.title))
                .absolute()
                .on_prepaint(move |bounds, _, _| {
                    let content_left = f32::from(bounds.origin.x);
                    let left = content_left - paint_border;
                    let right = content_left + f32::from(bounds.size.width) + paint_border;
                    painted_bars.borrow_mut().insert(
                        id,
                        PaintedBar {
                            left,
                            right,
                            layout_x,
                            content_left,
                        },
                    );
                })
                .left(px(bar.x))
                .top(px(bar_top))
                .w(px(if milestone || marker {
                    MILESTONE_SIZE
                } else {
                    bar.width.max(8.0)
                }))
                .h(px(bar_h))
                .rounded_md()
                .relative()
                .when(milestone, |el| {
                    el.bg(gpui_kit::transparent_white())
                        .border_0()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .text_lg()
                                .line_height(gpui_kit::relative(1.))
                                .text_color(rgb(bar.color))
                                .child("◆"),
                        )
                })
                .when(marker, |el| {
                    el.bg(gpui_kit::transparent_white())
                        .border_2()
                        .border_color(muted_fg)
                        .rounded_md()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .text_lg()
                                .line_height(gpui_kit::relative(1.))
                                .text_color(muted_fg)
                                .child("◇"),
                        )
                })
                .when(summary && !milestone && !marker, |el| {
                    el.bg(rgb(bar.color).opacity(0.55)).rounded_sm()
                })
                .when(
                    !summary && !milestone && !marker && !unscheduled && !no_fields && !open_ended,
                    |el| {
                        el.bg(rgb(bar.color))
                            .when(hovered && !dragging, |el| el.opacity(0.92))
                    },
                )
                .when(open_ended && !summary && !marker, |el| {
                    el.bg(rgb(bar.color).opacity(0.22))
                        .border_1()
                        .border_dashed()
                        .border_color(rgb(bar.color))
                        .child(
                            div()
                                .absolute()
                                .right(px(1.))
                                .top_0()
                                .bottom_0()
                                .w(px(crate::gantt::OPEN_ENDED_LINK_HIT))
                                .flex()
                                .items_center()
                                .justify_end()
                                .text_color(rgb(bar.color))
                                .occlude()
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.continue_or_begin_bar_gesture(
                                            id,
                                            GestureKind::Link,
                                            open_ended_link_origin,
                                            open_ended_link_origin,
                                            window,
                                            cx,
                                        );
                                    }),
                                )
                                .child("▸"),
                        )
                })
                .when(unscheduled && !summary && !marker, |el| {
                    el.bg(cx.theme().muted.opacity(0.28))
                        .border_2()
                        .border_color(cx.theme().muted_foreground)
                })
                .when(no_fields, |el| {
                    el.bg(cx.theme().muted.opacity(0.16))
                        .border_1()
                        .border_dashed()
                        .border_color(muted_fg.opacity(0.55))
                        .items_center()
                        .px_1()
                        .child(
                            div()
                                .text_xs()
                                .truncate()
                                .text_color(muted_fg)
                                .child("No dates"),
                        )
                })
                .text_color(if inside {
                    inner_label_fg
                } else if summary {
                    muted_fg
                } else {
                    bar_label_fg
                })
                .when(!milestone && !marker && !unscheduled && !open_ended, |el| {
                    el.border_color(if selected {
                        cx.theme().primary.opacity(0.8)
                    } else if summary {
                        cx.theme().border.opacity(0.6)
                    } else if hovered && !dragging {
                        cx.theme().primary.opacity(0.55)
                    } else {
                        cx.theme().border
                    })
                    .when(selected, |el| el.border_2())
                    .when(!selected, |el| el.border_1())
                })
                .overflow_hidden()
                .when(show_progress, |el| {
                    el.child(
                        div()
                            .id(("gantt-bar-progress", id as u64))
                            .debug_selector(move || progress_selector.clone())
                            .absolute()
                            .left(px(clip_pad))
                            .top_0()
                            .bottom_0()
                            .w(px(visible_progress_w))
                            .when(summary, |el| {
                                el.bg(rgb(bar.color)).child(
                                    Button::new(("progress-tip", id as u64))
                                        .ghost()
                                        .compact()
                                        .tab_stop(false)
                                        .absolute()
                                        .inset_0()
                                        .w_full()
                                        .h_full()
                                        .tooltip(PROGRESS_ROLLUP_TIP)
                                        .tooltip_placement(Placement::Top)
                                        .tooltip_show_delay(Duration::from_millis(450)),
                                )
                            })
                            .when(!summary, |el| el.bg(gpui_kit::hsla(0., 0., 0., 0.22))),
                    )
                })
                .when(inside && !milestone && !marker, |el| {
                    el.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(px(if can_resize || summary { grip_w } else { 0.0 }))
                            .right(px(if can_resize || summary { grip_w } else { 0.0 }))
                            .pl(px(if can_resize || summary {
                                4.0 + clip_pad
                            } else {
                                label_pad + clip_pad
                            }))
                            .pr(px(if can_resize || summary {
                                4.0
                            } else {
                                label_pad
                            }))
                            .flex()
                            .items_center()
                            .overflow_hidden()
                            .truncate()
                            .text_xs()
                            .text_color(inner_label_fg)
                            .child(bar.title.clone()),
                    )
                })
                .children(self.bar_inner_resize_grips(bar, dragging, cx))
                .children(self.bar_summary_edge_affordances(bar, dragging, cx))
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    this.set_bar_hovered(if *hovered { Some(id) } else { None }, cx);
                }))
                .when(
                    !dragging
                        && !self.details_overlay()
                        && self.state.gantt.pin_bar_tooltip != Some(id),
                    |el| {
                        let tip = bar_hover_tooltip(bar, &self.state.nodes);
                        el.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                            .tooltip_show_delay(Duration::from_millis(150))
                    },
                )
                .on_mouse_move(
                    cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                        this.set_bar_hovered(Some(id), cx);
                        // The bar captures the pointer on mouse-down. External titles
                        // occlude the timeline, so the captured bar must keep the drag.
                        this.on_bar_drag_move(event, window, cx);
                    }),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        let x: f32 = event.position.x.into();
                        if this
                            .state
                            .gantt
                            .gesture
                            .as_ref()
                            .is_some_and(|gesture| gesture.kind == GestureKind::Link)
                        {
                            let _ = this.classify_bar_hit(id, &layout, x);
                            this.state.gantt.set_link_target(id);
                            cx.notify();
                            return;
                        }
                        this.state.gantt.select_row(id);
                        this.focus_planner(window, cx);
                        if event.click_count >= 2 {
                            let _ = this.classify_bar_hit(id, &layout, x);
                            this.open_form(id, cx);
                            return;
                        }
                        if no_fields {
                            cx.notify();
                            return;
                        }
                        if unscheduled {
                            this.clear_toast_pointer_for_new_press();
                            this.dismiss_overlays();
                            let _ = this.classify_bar_hit(id, &layout, x);
                            // Live dates start at Today. Origin is Today on the
                            // date axis so a drag follows the cursor x. A
                            // no-move or still-in-gutter release is cancelled
                            // instead of PATCHing the gutter as a date.
                            this.pill_press_window_x = Some(x);
                            this.bar_press_window_x = Some(x);
                            this.bar_last_window_x = Some(x);
                            let today_x = this.state.gantt.x_for_date(jiff::Zoned::now().date());
                            let _ = this.state.begin_bar_gesture(id, GestureKind::Move, today_x);
                            cx.notify();
                            return;
                        }
                        this.dismiss_overlays();
                        this.bar_press_shift = event.modifiers.shift;
                        this.begin_classified_bar_gesture(id, &layout, x, window, cx);
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseUpEvent, window, cx| {
                        let x: f32 = event.position.x.into();
                        let y: f32 = event.position.y.into();
                        this.release_bar_gesture(x, y, window, cx);
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        let x: f32 = event.position.x.into();
                        let y: f32 = event.position.y.into();
                        this.state.gantt.select_row(id);
                        this.state.gantt.context_menu = Some(crate::gantt::ContextTarget {
                            id,
                            from_bar: true,
                            x,
                            y,
                            link: None,
                        });
                        cx.notify();
                    }),
                )
                .into_any_element(),
        ];
        if self.state.gantt.pin_bar_tooltip == Some(id) && !dragging && !self.details_overlay() {
            let tip = bar_hover_tooltip(bar, &self.state.nodes);
            items.push(
                div()
                    .id(("gantt-bar-tooltip", id as u64))
                    .debug_selector(|| "gantt-bar-tooltip".into())
                    .absolute()
                    .left(px(bar.x))
                    .top(px(bar_top - 36.0))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(cx.theme().popover)
                    .text_color(cx.theme().popover_foreground)
                    .text_xs()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(tip)
                    .into_any_element(),
            );
        }
        div()
            .relative()
            .size_full()
            .children(items)
            .into_any_element()
    }

    fn selected_row_accent(id: i64, cx: &Context<Self>) -> AnyElement {
        div()
            .id(("row-accent", id as u64))
            .debug_selector(move || format!("gantt-row-accent-{id}"))
            .absolute()
            .left_0()
            .top_0()
            .bottom_0()
            .w(px(SELECTED_ACCENT_W))
            .bg(cx.theme().primary)
            .into_any_element()
    }

    /// Inner 8px of each painted end. Last children of the bar so they win
    /// mouse-down over the title, progress fill, and bar body.
    fn bar_inner_resize_grips(
        &self,
        bar: &crate::gantt::BarLayout,
        dragging: bool,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        // Keep grips mounted during a date drag so a shrink to 16px cannot
        // unmount the capturing handle before mouse-up. Open-ended bars
        // keep the start grip so the left dot still resizes Start.
        if !dragging && !bar.can_resize() && !bar.open_ended {
            return Vec::new();
        }
        let id = bar.id;
        let hovered = self.state.gantt.bar_hovered_id == Some(id);
        let show_handles = hovered && !dragging;
        let linking = self
            .state
            .gantt
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.kind == GestureKind::Link);
        let grip_w = bar.inner_grip_width();
        let layout = bar.clone();
        let grips = if bar.open_ended && !bar.can_resize() {
            vec![("start", "resize-start", "resize-grip-start", true)]
        } else {
            vec![
                ("start", "resize-start", "resize-grip-start", true),
                ("end", "resize-end", "resize-grip-end", false),
            ]
        };
        grips
            .into_iter()
            .map(|(name, handle_id, grip_id, at_start)| {
                let selector = format!("gantt-resize-handle-{id}-{name}");
                let layout = layout.clone();
                div()
                    .id((handle_id, id as u64))
                    .debug_selector(move || selector.clone())
                    .absolute()
                    .when(at_start, |el| el.left_0())
                    .when(!at_start, |el| el.right_0())
                    .top_0()
                    .bottom_0()
                    .w(px(grip_w))
                    .when(!linking, |el| el.occlude())
                    .bg(gpui_kit::hsla(0., 0., 0., 0.01))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .when(show_handles, |el| {
                        el.child(
                            div()
                                .id((grip_id, id as u64))
                                .debug_selector(move || format!("gantt-resize-grip-{id}-{name}"))
                                .absolute()
                                .when(at_start, |el| el.left(px(2.)))
                                .when(!at_start, |el| el.right(px(2.)))
                                .top(px(3.))
                                .bottom(px(3.))
                                .w(px(3.))
                                .rounded_full()
                                .bg(cx.theme().background)
                                .border_1()
                                .border_color(cx.theme().primary),
                        )
                    })
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.set_bar_hovered(Some(id), cx);
                        }
                    }))
                    .on_mouse_move(
                        cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                            if this.state.gantt.gesture.as_ref().is_some_and(|gesture| {
                                gesture.kind == GestureKind::Link && gesture.id != id
                            }) {
                                this.state.gantt.set_link_target(id);
                            }
                            this.on_bar_drag_move(event, window, cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            let x: f32 = event.position.x.into();
                            let _ = this.classify_bar_hit(id, &layout, x);
                            let kind = if at_start {
                                GestureKind::ResizeStart
                            } else {
                                GestureKind::ResizeEnd
                            };
                            let origin = this.pointer_layout_x(x, Some(id));
                            this.bar_press_shift = event.modifiers.shift;
                            this.continue_or_begin_bar_gesture(id, kind, origin, x, window, cx);
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseUpEvent, window, cx| {
                            let x: f32 = event.position.x.into();
                            let y: f32 = event.position.y.into();
                            this.release_bar_gesture(x, y, window, cx);
                        }),
                    )
                    .into_any_element()
            })
            .collect()
    }

    /// Inner 8px of a summary bar. Not-allowed cursor + rollup tooltip; no move.
    fn bar_summary_edge_affordances(
        &self,
        bar: &crate::gantt::BarLayout,
        dragging: bool,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        if !bar.shows_rollup_edges() || dragging {
            return Vec::new();
        }
        let id = bar.id;
        let linking = self
            .state
            .gantt
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.kind == GestureKind::Link);
        let grip_w = bar.inner_grip_width();
        let layout = bar.clone();
        [
            ("start", "summary-edge-start", true),
            ("end", "summary-edge-end", false),
        ]
        .into_iter()
        .map(|(name, handle_id, at_start)| {
            let selector = format!("gantt-summary-edge-{id}-{name}");
            let layout = layout.clone();
            div()
                .id((handle_id, id as u64))
                .debug_selector(move || selector.clone())
                .absolute()
                .when(at_start, |el| el.left_0())
                .when(!at_start, |el| el.right_0())
                .top_0()
                .bottom_0()
                .w(px(grip_w))
                .when(!linking, |el| el.occlude())
                .bg(gpui_kit::hsla(0., 0., 0., 0.01))
                .cursor(CursorStyle::OperationNotAllowed)
                .tooltip(move |window, cx| Tooltip::new(DATES_ROLLUP_TIP).build(window, cx))
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    if *hovered {
                        this.set_bar_hovered(Some(id), cx);
                    }
                }))
                .on_mouse_move(
                    cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                        if this.state.gantt.gesture.as_ref().is_some_and(|gesture| {
                            gesture.kind == GestureKind::Link && gesture.id != id
                        }) {
                            this.state.gantt.set_link_target(id);
                        }
                        this.on_bar_drag_move(event, window, cx);
                    }),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        let x: f32 = event.position.x.into();
                        // Force RollupEdge: do not reclassify against the
                        // 2px selection border, which would turn the inner
                        // overlay pixels into Move (leaf grips force Resize*).
                        this.begin_rollup_edge_press(id, &layout, x, window, cx);
                    }),
                )
                .into_any_element()
        })
        .collect()
    }

    /// Paint after arrows so a selected link's × / stub cannot steal the handle.
    fn render_gantt_link_handles(
        &self,
        bar: &crate::gantt::BarLayout,
        dragging: bool,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let id = bar.id;
        let unscheduled = bar.kind == ado_core::GanttBarKind::Unscheduled;
        let linking = self
            .state
            .gantt
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.kind == GestureKind::Link);
        // Keep the handle mounted during a date drag so mouse-up is not lost
        // when the pink overlay would otherwise unmount mid-gesture.
        if unscheduled || bar.kind == ado_core::GanttBarKind::NoFields || bar.marker || linking {
            return None;
        }
        let hovered = self.state.gantt.bar_hovered_id == Some(id);
        let selected = self.state.gantt.selected_id == Some(id);
        let show_dot = hovered && !dragging;
        let (local_left, local_right) = self.painted_timeline_edges(bar);
        // Inner 8px grips resize both ends. These circles sit fully outside
        // the bar (monday.com connectors) and always start a link. Skip past
        // the overflow ! so the badge wins hits and the end grip stays free.
        let start_x = local_left - LINK_HANDLE_HIT - 2.0;
        let overflow_pad = if parent_date_overflow(&self.state.nodes, id) {
            OVERFLOW_BADGE + OVERFLOW_BADGE_GAP + 6.0
        } else {
            6.0
        };
        let end_x = local_right + overflow_pad;
        let handle_top = bar.y + ((ROW_HEIGHT - LINK_HANDLE_HIT) / 2.0).max(0.0);
        let handles =
            [("link-start", start_x), ("link-end", end_x)].map(|(handle_id, handle_x)| {
                let selector = format!("gantt-link-handle-{id}-{handle_id}");
                let origin = handle_x + LINK_HANDLE_HIT / 2.0;
                div()
                    .id((handle_id, id as u64))
                    .debug_selector(move || selector.clone())
                    .absolute()
                    .left(px(handle_x))
                    .top(px(handle_top))
                    .size(px(LINK_HANDLE_HIT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(show_dot || selected || dragging, |el| el.occlude())
                    .bg(gpui_kit::hsla(0., 0., 0., 0.01))
                    .cursor(CursorStyle::DragLink)
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.set_bar_hovered(Some(id), cx);
                        }
                    }))
                    .on_mouse_move(
                        cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                            if this.state.gantt.gesture.as_ref().is_some_and(|gesture| {
                                gesture.kind == GestureKind::Link && gesture.id != id
                            }) {
                                this.state.gantt.set_link_target(id);
                            }
                            this.on_bar_drag_move(event, window, cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            let x: f32 = event.position.x.into();
                            this.bar_press_shift = event.modifiers.shift;
                            this.continue_or_begin_bar_gesture(
                                id,
                                GestureKind::Link,
                                origin,
                                x,
                                window,
                                cx,
                            );
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseUpEvent, window, cx| {
                            let x: f32 = event.position.x.into();
                            let y: f32 = event.position.y.into();
                            this.release_bar_gesture(x, y, window, cx);
                        }),
                    )
                    .child(
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(show_dot, |el| {
                                el.child(
                                    div()
                                        .debug_selector(move || {
                                            format!("gantt-link-dot-{id}-{handle_id}")
                                        })
                                        .size(px(LINK_HANDLE))
                                        .rounded_full()
                                        .bg(cx.theme().background)
                                        .border_2()
                                        .border_color(cx.theme().primary),
                                )
                            }),
                    )
            });
        Some(
            div()
                .absolute()
                .left_0()
                .top_0()
                .w(px(0.))
                .h(px(0.))
                .children(handles)
                .into_any_element(),
        )
    }

    /// Faded tail + ▸ at the clip when a start-only bar continues past the
    /// viewport. The core bar stays default-length so arrows occupy a finite
    /// body instead of the whole remaining timeline.
    fn render_open_ended_cue(
        &self,
        bar: &crate::gantt::BarLayout,
        _cx: &Context<Self>,
    ) -> Option<AnyElement> {
        if !bar.open_ended || bar.summary || bar.marker || bar.milestone {
            return None;
        }
        let clip_right = self.state.gantt.timeline_width.max(MIN_TIMELINE_WIDTH);
        if bar.x >= clip_right {
            return None;
        }
        let core_right = bar.x + bar.width.max(8.0);
        let tail_left = core_right.max(0.0).min(clip_right);
        let tail_w = (clip_right - tail_left).max(0.0);
        if tail_w < 4.0 && core_right <= clip_right {
            return None;
        }
        let bar_h = LEAF_BAR_H;
        let bar_top = ((ROW_HEIGHT - bar_h) / 2.0).max(2.0);
        let id = bar.id;
        let cue_w = tail_w.max(10.0);
        let selector = format!("gantt-open-ended-cue-{id}");
        Some(
            div()
                .id(("open-ended-cue", id as u64))
                .debug_selector(move || selector)
                .absolute()
                .left(px(tail_left))
                .top(px(bar.y + bar_top))
                .w(px(cue_w))
                .h(px(bar_h))
                .bg(rgb(bar.color).opacity(0.12))
                .border_1()
                .border_dashed()
                .border_color(rgb(bar.color).opacity(0.55))
                .rounded_md()
                .overflow_hidden()
                .child(
                    div()
                        .absolute()
                        .right(px(2.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .justify_end()
                        .text_color(rgb(bar.color))
                        .child("▸"),
                )
                .into_any_element(),
        )
    }

    fn bar_title_fits_inside(bar: &crate::gantt::BarLayout) -> bool {
        bar_title_is_inside(bar)
    }

    pub(crate) fn bar_external_label_frame(
        bar: &crate::gantt::BarLayout,
        timeline_w: f32,
        show_link_handles: bool,
        today_x: Option<f32>,
    ) -> Option<(f32, f32)> {
        if Self::bar_title_fits_inside(bar) {
            return None;
        }
        let title_width = bar.title.chars().count() as f32 * 8.0 + 16.0;
        let label_gap = if show_link_handles {
            HANDLE_WIDTH + LINK_HANDLE_HIT + 16.0
        } else {
            10.0
        };
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
        let right_x = bar_right + label_gap;
        let left_x = bar.x - estimated - 6.0;
        // Keep outside titles on the right of the bar. Flipping to the left
        // after a right-edge resize (#1014) is the jump the review caught;
        // hide a fragment at the clip instead of clipping to a letter.
        const MIN_OUTSIDE: f32 = 40.0;
        let mut label_x = right_x;
        if label_x >= timeline_w - 8.0 {
            label_x = (bar_right + 4.0).min(timeline_w - 24.0);
            if label_x < bar_right {
                label_x = bar_right.min(timeline_w - 8.0);
            }
        }
        if label_x >= timeline_w || label_x + 8.0 < 0.0 {
            return None;
        }
        let mut max_w = (timeline_w - label_x - 4.0).max(0.0);
        if max_w < MIN_OUTSIDE {
            return None;
        }
        if let Some(today) = today_x {
            let label_right = label_x + estimated.min(max_w);
            if label_x < today && label_right > today - 4.0 {
                let before = (today - 8.0 - label_x).max(0.0);
                let after = today + 8.0;
                if before >= estimated {
                    max_w = before;
                } else if after + 24.0 < timeline_w {
                    // Don't clip to a letter: jump past Today and keep the title.
                    label_x = after;
                    max_w = (timeline_w - label_x - 4.0).max(24.0);
                } else if left_x >= 0.0 && left_x + estimated <= today - 4.0 {
                    label_x = left_x;
                    max_w = (today - 8.0 - label_x).max(24.0);
                } else {
                    max_w = before.max(estimated);
                }
            }
        }
        Some((label_x, max_w))
    }

    fn render_parent_overflow_badge(
        &self,
        bar: &crate::gantt::BarLayout,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let id = bar.id;
        if !parent_date_overflow(&self.state.nodes, id) {
            return None;
        }
        let overflow_tip = parent_overflow_tip(&self.state.nodes, id)
            .unwrap_or_else(|| "Outside parent's dates".to_string());
        let painted_w = if bar.milestone || bar.marker {
            MILESTONE_SIZE
        } else {
            bar.width.max(8.0)
        };
        let warn = cx.theme().warning;
        Some(
            div()
                .id(("gantt-overflow", id as u64))
                .debug_selector(|| "gantt-parent-overflow".into())
                .absolute()
                .left(px(bar.x + painted_w + OVERFLOW_BADGE_GAP))
                .top(px(bar.y + ((ROW_HEIGHT - OVERFLOW_BADGE) / 2.0).max(0.0)))
                .w(px(OVERFLOW_BADGE))
                .h(px(OVERFLOW_BADGE))
                .min_w(px(OVERFLOW_BADGE))
                .min_h(px(OVERFLOW_BADGE))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(warn)
                .text_color(cx.theme().background)
                .text_size(px(14.))
                .line_height(gpui_kit::relative(1.))
                .font_weight(gpui_kit::FontWeight::BOLD)
                .occlude()
                .tooltip(move |window, cx| Tooltip::new(overflow_tip.clone()).build(window, cx))
                .child("!")
                .into_any_element(),
        )
    }

    fn render_gantt_bar_label(
        &self,
        bar: &crate::gantt::BarLayout,
        dragging: bool,
        muted_fg: gpui_kit::Hsla,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let id = bar.id;
        if bar.kind == ado_core::GanttBarKind::Unscheduled
            || bar.kind == ado_core::GanttBarKind::NoFields
            || bar.marker
        {
            return None;
        }
        let timeline_w = self.state.gantt.timeline_width.max(1.0);
        let today_x = self.state.gantt.today_x(jiff::Zoned::now().date());
        let cramped = matches!(
            self.state.gantt.scale,
            GanttScale::Quarter | GanttScale::Year
        );
        let (label_x, max_w) =
            Self::bar_external_label_frame(bar, timeline_w, true, Some(today_x))?;
        if cramped && max_w < 56.0 {
            return None;
        }
        let label = bar.title.clone();
        Some(
            div()
                .id(("gantt-bar-label", id as u64))
                .debug_selector(move || format!("gantt-bar-label-{id}"))
                .absolute()
                .left(px(label_x))
                .top(px(bar.y + ((ROW_HEIGHT - 16.0) / 2.0).max(2.0)))
                .h(px(16.))
                .max_w(px(max_w))
                .flex()
                .items_center()
                .overflow_hidden()
                .truncate()
                .text_xs()
                .text_color(muted_fg)
                .px_1()
                .bg(cx.theme().background)
                .when(!dragging, |el| el.occlude())
                .child(label)
                .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                    this.apply_scroll(event, cx);
                }))
                .on_click(
                    cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                        this.state.gantt.select_row(id);
                        if this.inline_edit.is_none() {
                            window.focus(&this.form_focus, cx);
                        }
                        if event.click_count() >= 2 {
                            this.open_form(id, cx);
                        }
                        cx.notify();
                    }),
                )
                .into_any_element(),
        )
    }

    fn render_dependency_arrow(&self, arrow: &DependencyArrow, cx: &Context<Self>) -> AnyElement {
        let selected = self.state.gantt.selected_link == Some((arrow.from_id, arrow.to_id));
        let hovered = self.state.gantt.hovered_link == Some((arrow.from_id, arrow.to_id));
        let color = if selected || hovered {
            cx.theme().primary
        } else {
            cx.theme().primary.opacity(0.7)
        };
        let stroke = if selected || hovered { 3.0 } else { 2.0 };
        let stub_start = arrow.stub_start();
        let gutter_y = arrow.gutter_y();
        let wrap = arrow.wraps_around_successor();
        let min_x = arrow.x1.min(arrow.x2).min(arrow.x_head).min(stub_start) - 16.0;
        let min_y = arrow.y1.min(arrow.y2).min(arrow.lane_y).min(gutter_y) - 18.0;
        let from_id = arrow.from_id;
        let to_id = arrow.to_id;
        let v1 = (arrow.lane_y - arrow.y1).abs().max(2.0);
        let h_len = (arrow.x2 - arrow.x1).abs().max(2.0);
        let v2_end = if wrap { gutter_y } else { arrow.y2 };
        let v2 = (v2_end - arrow.lane_y).abs().max(2.0);
        let stub_h = (arrow.x_head - stub_start).abs().max(2.0);
        let gutter_h = (stub_start - arrow.x2).abs().max(2.0);
        let drop = (arrow.y2 - gutter_y).abs().max(2.0);
        div()
            .absolute()
            .left(px(min_x))
            .top(px(min_y))
            .w(px(0.))
            .h(px(0.))
            .child(self.arrow_segment(
                arrow.x1 - min_x,
                arrow.y1.min(arrow.lane_y) - min_y,
                stroke,
                v1,
                color,
                from_id,
                to_id,
                "out",
                cx,
            ))
            .child(self.arrow_segment(
                arrow.x1.min(arrow.x2) - min_x,
                arrow.lane_y - min_y,
                h_len,
                stroke,
                color,
                from_id,
                to_id,
                "h",
                cx,
            ))
            .child(self.arrow_segment(
                arrow.x2 - min_x,
                arrow.lane_y.min(v2_end) - min_y,
                stroke,
                v2,
                color,
                from_id,
                to_id,
                "inv",
                cx,
            ))
            .when(wrap, |el| {
                el.child(self.arrow_segment(
                    stub_start.min(arrow.x2) - min_x,
                    gutter_y - min_y,
                    gutter_h,
                    stroke,
                    color,
                    from_id,
                    to_id,
                    "gutter",
                    cx,
                ))
                .child(self.arrow_segment(
                    stub_start - min_x,
                    gutter_y.min(arrow.y2) - min_y,
                    stroke,
                    drop,
                    color,
                    from_id,
                    to_id,
                    "drop",
                    cx,
                ))
            })
            .child(self.arrow_segment(
                stub_start.min(arrow.x_head) - min_x,
                arrow.y2 - min_y,
                stub_h,
                stroke,
                color,
                from_id,
                to_id,
                "stub",
                cx,
            ))
            .child(self.arrow_head_hit(arrow, min_x, min_y, color, cx))
            .when(selected, |el| {
                el.child(self.link_remove_button(arrow, min_x, min_y, cx))
            })
            .into_any_element()
    }

    fn link_remove_button(
        &self,
        arrow: &DependencyArrow,
        min_x: f32,
        min_y: f32,
        cx: &Context<Self>,
    ) -> AnyElement {
        let from_id = arrow.from_id;
        let to_id = arrow.to_id;
        div()
            .id(SharedString::from(format!(
                "gantt-link-remove-{from_id}-{to_id}"
            )))
            .debug_selector(|| "gantt-link-remove".into())
            .absolute()
            .left(px({
                let (x, _) = arrow.remove_control_pos();
                x - min_x - 10.0
            }))
            .top(px({
                let (_, y) = arrow.remove_control_pos();
                y - min_y - 12.0
            }))
            .occlude()
            .child(
                Button::new(SharedString::from(format!("link-remove-{from_id}-{to_id}")))
                    .primary()
                    .compact()
                    .with_size(Size::XSmall)
                    .label("×")
                    .tooltip("Remove link (Delete)")
                    .tooltip_placement(Placement::Bottom)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.remove_link(from_id, to_id, cx);
                    })),
            )
            .into_any_element()
    }

    fn request_delete(&mut self, id: i64, cx: &mut Context<Self>) {
        let title = self
            .state
            .nodes
            .iter()
            .find(|node| node.id == id)
            .map(|node| node.title.clone())
            .unwrap_or_default();
        self.state.gantt.context_menu = None;
        self.pending_delete = Some((id, title));
        cx.notify();
    }

    fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some((id, _)) = self.pending_delete.clone() else {
            return;
        };
        if !self.spawn_state(cx, move |mut state| async move {
            let result = state.delete_item(id).await;
            (state, result)
        }) {
            return;
        }
        self.pending_delete = None;
        cx.notify();
    }

    fn render_delete_confirm(&self, cx: &Context<Self>) -> impl IntoElement {
        let Some((id, title)) = self.pending_delete.as_ref() else {
            return div().into_any_element();
        };
        let id = *id;
        let title = title.clone();
        v_flex()
            .id("gantt-delete-confirm")
            .debug_selector(|| "gantt-delete-confirm".into())
            .absolute()
            .inset_0()
            .occlude()
            .bg(cx.theme().overlay)
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.pending_delete = None;
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .id("gantt-delete-dialog")
                    .w(px(420.))
                    .gap_3()
                    .p_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        Label::new(format!("Delete #{id} {title}?"))
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD),
                    )
                    .child(
                        div()
                            .id("gantt-delete-warning")
                            .debug_selector(|| "gantt-delete-warning".into())
                            .child(
                                Label::new(self.pending_delete_warning().unwrap_or_else(|| {
                                    "This removes the work item from Azure DevOps.".into()
                                }))
                                .text_xs()
                                .text_color(cx.theme().muted_foreground),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .child(Button::new("cancel-delete").label("Cancel").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.pending_delete = None;
                                    cx.notify();
                                }),
                            ))
                            .child(
                                Button::new("confirm-delete")
                                    .primary()
                                    .label("Delete")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_delete(cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn remove_link(&mut self, from_id: i64, to_id: i64, cx: &mut Context<Self>) {
        self.state.gantt.context_menu = None;
        self.state.gantt.selected_link = None;
        self.state.gantt.clear_hover();
        self.state.gantt.hover_locked = true;
        self.spawn_state(cx, move |mut state| async move {
            let result = state.remove_dependency(to_id, from_id).await;
            (state, result)
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn arrow_segment(
        &self,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        color: gpui_kit::Hsla,
        from_id: i64,
        to_id: i64,
        role: &'static str,
        cx: &Context<Self>,
    ) -> AnyElement {
        const HIT: f32 = 14.0;
        let hit_w = width.max(HIT);
        let hit_h = height.max(HIT);
        let segment_id = if role == "h" {
            format!("gantt-dep-arrow-{from_id}-{to_id}")
        } else {
            format!("gantt-dep-{role}-{from_id}-{to_id}")
        };
        let el = div()
            .id(SharedString::from(segment_id.clone()))
            .debug_selector(move || segment_id.clone())
            .absolute()
            .left(px(left - (hit_w - width) / 2.0))
            .top(px(top - (hit_h - height) / 2.0))
            .w(px(hit_w))
            .h(px(hit_h))
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .cursor_pointer()
            .when(
                self.state.form.is_none()
                    && self.state.gantt.selected_link != Some((from_id, to_id)),
                |el| {
                    el.tooltip(move |window, cx| {
                        Tooltip::new("Click to select · × or Delete to remove").build(window, cx)
                    })
                },
            )
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.state.gantt.hovered_link = if *hovered {
                    Some((from_id, to_id))
                } else if this.state.gantt.hovered_link == Some((from_id, to_id)) {
                    None
                } else {
                    this.state.gantt.hovered_link
                };
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if this.state.gantt.gesture.is_some() {
                        return;
                    }
                    this.state.gantt.selected_link = Some((from_id, to_id));
                    cx.notify();
                }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                if this.state.gantt.gesture.is_some() {
                    return;
                }
                this.state.gantt.selected_link = Some((from_id, to_id));
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.state.gantt.selected_link = Some((from_id, to_id));
                    let x: f32 = event.position.x.into();
                    let y: f32 = event.position.y.into();
                    this.state.gantt.context_menu = Some(crate::gantt::ContextTarget {
                        id: to_id,
                        from_bar: false,
                        x,
                        y,
                        link: Some((from_id, to_id)),
                    });
                    cx.notify();
                }),
            )
            .child(div().w(px(width)).h(px(height)).bg(color));
        el.into_any_element()
    }

    fn arrow_head_hit(
        &self,
        arrow: &DependencyArrow,
        min_x: f32,
        min_y: f32,
        color: gpui_kit::Hsla,
        cx: &Context<Self>,
    ) -> AnyElement {
        let from_id = arrow.from_id;
        let to_id = arrow.to_id;
        div()
            .id(SharedString::from(format!(
                "gantt-dep-arrowhead-{}-{}",
                arrow.from_id, arrow.to_id
            )))
            .debug_selector(|| "gantt-dep-arrowhead".into())
            .absolute()
            .left(px(arrow.arrowhead_left() - min_x))
            .top(px(arrow.y2 - min_y - 10.0))
            .w(px(18.))
            .h(px(20.))
            .occlude()
            .cursor_pointer()
            .text_xl()
            .line_height(gpui_kit::relative(1.))
            .text_color(color)
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.state.gantt.hovered_link = if *hovered {
                    Some((from_id, to_id))
                } else if this.state.gantt.hovered_link == Some((from_id, to_id)) {
                    None
                } else {
                    this.state.gantt.hovered_link
                };
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.state.gantt.selected_link = Some((from_id, to_id));
                    cx.notify();
                }),
            )
            .child(if arrow.inbound_right { "◀" } else { "▶" })
            .into_any_element()
    }

    fn render_context_menu(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(target) = self.state.gantt.context_menu else {
            return div().into_any_element();
        };
        let id = target.id;
        let from_row = !target.from_bar;
        let link = target.link;
        let menu_w = 240.0;
        let x = target
            .x
            .clamp(8.0, (self.state.gantt.viewport_w - menu_w).max(8.0));
        let y = target
            .y
            .clamp(72.0, (self.state.gantt.viewport_h + 64.0).max(72.0));
        v_flex()
            .id("gantt-context-menu")
            .debug_selector(|| "gantt-context-menu".into())
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key.as_str() == "escape" {
                    this.state.gantt.context_menu = None;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(menu_w))
            .p_1()
            .gap_1()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .rounded_md()
            .shadow_md()
            .when_some(link, |el, (from_id, to_id)| {
                el.child(self.menu_item("Remove link", cx, move |this, cx| {
                    this.remove_link(from_id, to_id, cx);
                }))
            })
            .child(self.menu_item("Open", cx, move |this, cx| this.open_form(id, cx)))
            .child(self.menu_item("Edit title", cx, move |this, _cx| {
                this.state.gantt.select_row(id);
                this.pending_inline_title = Some(id);
            }))
            .child(self.menu_item("Add child", cx, move |this, cx| {
                this.state.gantt.select_row(id);
                this.spawn_state(cx, move |mut state| async move {
                    let result = state.add_item_under(Some(id)).await;
                    (state, result)
                });
            }))
            .child(self.menu_item("Delete", cx, move |this, cx| {
                this.request_delete(id, cx);
            }))
            .child(
                self.menu_item("Open in Azure DevOps", cx, move |this, _cx| {
                    this.state.open_in_ado(id);
                }),
            )
            .child(self.menu_item("Copy ID", cx, move |this, cx| {
                let text = this.state.copy_id(id);
                this.write_clipboard(cx, text);
            }))
            .child(self.menu_item("Copy link", cx, move |this, cx| {
                if let Some(url) = this.state.copy_link(id) {
                    this.write_clipboard(cx, url);
                }
            }))
            .child(self.menu_item("Clear dates", cx, move |this, cx| {
                this.spawn_state(cx, move |mut state| async move {
                    let result = state.clear_dates(id).await;
                    (state, result)
                });
            }))
            .child(
                self.menu_item("Schedule to iteration", cx, move |this, cx| {
                    this.spawn_state(cx, move |mut state| async move {
                        let result = state.schedule_to_iteration(id).await;
                        (state, result)
                    });
                }),
            )
            .child(self.menu_item("Expand all", cx, move |this, cx| {
                let nodes = this.state.visible_nodes();
                this.state.gantt.expand_all_below(&nodes, id);
                cx.notify();
            }))
            .child(self.menu_item("Collapse all", cx, move |this, cx| {
                let nodes = this.state.visible_nodes();
                this.state.gantt.collapse_all_below(&nodes, id);
                cx.notify();
            }))
            .when(from_row, |el| {
                let ty = this_type(&self.state, id);
                let state = this_state(&self.state, id);
                el.child(self.menu_item("Hide this type", cx, move |this, cx| {
                    if let Some(ty) = &ty {
                        this.state.toggle_hidden_type(ty);
                    }
                    this.state.gantt.context_menu = None;
                    cx.notify();
                }))
                .child(self.menu_item("Hide this state", cx, move |this, cx| {
                    if let Some(state) = &state {
                        this.state.toggle_hidden_state(state);
                    }
                    this.state.gantt.context_menu = None;
                    cx.notify();
                }))
            })
            .into_any_element()
    }

    fn menu_item<F>(
        &self,
        label: &'static str,
        cx: &mut Context<Self>,
        on_click: F,
    ) -> impl IntoElement
    where
        F: Fn(&mut Self, &mut Context<Self>) + 'static,
    {
        let selector = format!("gantt-menu-{label}");
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.state.gantt.schedule_drag = None;
                    on_click(this, cx);
                    this.state.gantt.context_menu = None;
                    cx.notify();
                }),
            )
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Button::new(label)
                    .tab_stop(false)
                    .focus_ring(false)
                    .label(label),
            )
    }

    fn render_form_skeleton(&self, id: i64, title: String, cx: &Context<Self>) -> AnyElement {
        let dialog_h = (self.state.gantt.viewport_h + 80.0).clamp(360.0, 900.0);
        v_flex()
            .id("work-item-form")
            .debug_selector(|| "work-item-form".into())
            .absolute()
            .inset_0()
            .occlude()
            .bg(cx.theme().overlay)
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.form_loading = None;
                    this.state.close_form();
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .id("work-item-form-loading")
                    .debug_selector(|| "work-item-form-loading".into())
                    .w(px(720.))
                    .h(px(dialog_h))
                    .max_h(px(dialog_h))
                    .bg(cx.theme().popover)
                    .text_color(cx.theme().popover_foreground)
                    .rounded_xl()
                    .shadow_md()
                    .gap_3()
                    .p_6()
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("#{id}")),
                    )
                    .child(div().text_lg().child(if title.is_empty() {
                        "Opening details…".to_string()
                    } else {
                        title
                    }))
                    .child(
                        div()
                            .id("work-item-form-spinner")
                            .debug_selector(|| "work-item-form-spinner".into())
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("Loading fields…"),
                    ),
            )
            .into_any_element()
    }

    fn render_form(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.state.form.is_none() {
            if let Some((id, title)) = self.form_loading.clone() {
                return self.render_form_skeleton(id, title, cx);
            }
            return div().into_any_element();
        }
        let form = self.state.form.as_ref().expect("form");
        let title = form.title();
        let heading = form.heading_title();
        let work_item_type = form.model.work_item_type.clone();
        let pages = form.model.pages.clone();
        let active = form.active_page;
        let layout = crate::form::form_dialog_layout(form);
        let dates = layout.dates;
        let primary = layout.primary;
        let system_rest = layout.system_rest;
        let groups = layout.groups;
        let extra = layout.extra;
        let extra_count = extra.len();
        let show_empty = form.show_empty;
        let dirty = self.form_has_unsaved_edits(cx);
        let dates_error = self.form_dates_error(cx);
        let saving = form.saving;
        let dialog_h = (self.state.gantt.viewport_h + 80.0).clamp(360.0, 900.0);
        v_flex()
            .id("work-item-form")
            .debug_selector(|| "work-item-form".into())
            .absolute()
            .inset_0()
            .occlude()
            .track_focus(&self.form_focus)
            .bg(cx.theme().overlay)
            .items_center()
            .justify_center()
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.as_str() == "escape" {
                    if this.form_date_picker_focused(window, cx) {
                        return;
                    }
                    this.dismiss_all_overlays();
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.form_loading = None;
                    this.state.close_form();
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .id("work-item-form-dialog")
                    .debug_selector(|| "work-item-form-dialog".into())
                    .w(px(720.))
                    .h(px(dialog_h))
                    .max_h(px(dialog_h))
                    .bg(cx.theme().popover)
                    .text_color(cx.theme().popover_foreground)
                    .rounded_xl()
                    .shadow_md()
                    .gap_0()
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        h_flex()
                            .id("work-item-form-header")
                            .debug_selector(|| "work-item-form-header".into())
                            .w_full()
                            .items_start()
                            .px_6()
                            .py_4()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .gap_1()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .overflow_hidden()
                                            .child(self.type_badge(&work_item_type, cx))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .truncate()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(title),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("work-item-form-heading")
                                            .debug_selector(|| "work-item-form-heading".into())
                                            .text_lg()
                                            .truncate()
                                            .child(if heading.is_empty() {
                                                "Untitled".to_string()
                                            } else {
                                                heading
                                            }),
                                    ),
                            )
                            .child(
                                Button::new("close-form")
                                    .label("Close")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.form_loading = None;
                                        this.state.close_form();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .px_6()
                            .pt_3()
                            .gap_2()
                            .children(pages.iter().enumerate().map(|(i, page)| {
                                Button::new(("page", i as u32))
                                    .label(page.label.clone())
                                    .selected(i == active)
                                    .when(i == active, |btn| btn.primary())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(form) = this.state.form.as_mut() {
                                            form.active_page = i;
                                        }
                                        cx.notify();
                                    }))
                            })),
                    )
                    .child(
                        v_flex()
                            .id("work-item-form-body")
                            .flex_1()
                            .min_h(px(0.))
                            .px_6()
                            .py_3()
                            .overflow_y_scrollbar()
                            .gap_3()
                            .children(dates.iter().map(|control| self.render_field(control, cx)))
                            .children(primary.iter().map(|control| self.render_field(control, cx)))
                            .children(
                                system_rest
                                    .iter()
                                    .map(|control| self.render_field(control, cx)),
                            )
                            .children(groups.iter().map(|(label, controls)| {
                                v_flex()
                                    .gap_2()
                                    .p_3()
                                    .rounded_md()
                                    .bg(cx.theme().muted)
                                    .child(
                                        Label::new(label.clone())
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground),
                                    )
                                    .children(
                                        controls
                                            .iter()
                                            .map(|control| self.render_field(control, cx)),
                                    )
                            }))
                            .when(extra_count > 0, |el| {
                                el.child(
                                    div()
                                        .id("form-more-fields")
                                        .debug_selector(|| "form-more-fields".into())
                                        .child(
                                            Button::new("form-more-fields-btn")
                                                .ghost()
                                                .label(if show_empty {
                                                    "Hide empty fields".into()
                                                } else {
                                                    format!("More ({extra_count})")
                                                })
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(form) = this.state.form.as_mut() {
                                                        form.show_empty = !form.show_empty;
                                                    }
                                                    cx.notify();
                                                })),
                                        ),
                                )
                                .when(show_empty, |el| {
                                    el.children(
                                        extra.iter().map(|control| self.render_field(control, cx)),
                                    )
                                })
                            }),
                    )
                    .child(
                        h_flex()
                            .id("work-item-form-footer")
                            .flex_shrink_0()
                            .px_6()
                            .py_3()
                            .gap_2()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(
                                Button::new("save-work-item")
                                    .primary()
                                    .label("Save")
                                    .disabled(!dirty || saving || dates_error.is_some())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_form(window, cx)
                                    })),
                            )
                            .child(
                                div()
                                    .id("cancel-form")
                                    .debug_selector(|| "cancel-form".into())
                                    .child(Button::new("cancel-form").label("Cancel").on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.form_loading = None;
                                            this.state.close_form();
                                            cx.notify();
                                        }),
                                    )),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_field(&self, control: &ado_core::FormControl, cx: &Context<Self>) -> AnyElement {
        let Some(form) = self.state.form.as_ref() else {
            return div().into_any_element();
        };
        let value = form
            .values
            .get(&control.reference_name)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let label = if control.kind == FormControlKind::DateTime
            && (control.reference_name == ado_core::START_DATE_FIELD
                || control.reference_name == ado_core::TARGET_DATE_FIELD)
        {
            "Dates".to_string()
        } else {
            control.label.clone()
        };
        let id = control.reference_name.clone();
        v_flex()
            .gap_1()
            .child(
                div()
                    .id(SharedString::from(if label == "Dates" {
                        "form-dates-label".to_string()
                    } else {
                        format!("form-label-{id}")
                    }))
                    .when(label == "Dates", |el| {
                        el.debug_selector(|| "form-dates-label".into())
                    })
                    .child(
                        Label::new(label)
                            .text_xs()
                            .text_color(cx.theme().muted_foreground),
                    ),
            )
            .child(match control.kind {
                FormControlKind::Html if !form.is_plain_html(&id) => {
                    let url = self
                        .state
                        .scope()
                        .map(|scope| {
                            ado_edit_url_on(
                                self.state.client.ado_base(),
                                &scope.org,
                                &scope.project,
                                form.model.id,
                            )
                        })
                        .unwrap_or_default();
                    v_flex()
                        .gap_1()
                        .child(TextView::html(
                            SharedString::from(format!("html-{id}")),
                            value.as_str().unwrap_or(""),
                        ))
                        .child(
                            Button::new(SharedString::from(format!("edit-ado-{id}")))
                                .label("Edit in Azure DevOps")
                                .on_click(move |_, _, _| {
                                    let _ = open::that(&url);
                                }),
                        )
                        .into_any_element()
                }
                FormControlKind::Html => {
                    let markdown = form.html_markdown.get(&id).cloned().unwrap_or_default();
                    let editor = self.form_inputs.get(&id).cloned();
                    v_flex()
                        .gap_1()
                        .when_some(editor, |el, input| {
                            el.child(Input::new(&input).id(SharedString::from(format!("md-{id}"))))
                        })
                        .child(TextView::markdown(
                            SharedString::from(format!("md-preview-{id}")),
                            markdown,
                        ))
                        .into_any_element()
                }
                FormControlKind::Boolean => {
                    let checked = value.as_bool().unwrap_or(false);
                    let field = id.clone();
                    let entity = cx.entity();
                    h_flex()
                        .id(SharedString::from(format!("form-boolean-{id}")))
                        .debug_selector(|| "form-boolean".into())
                        .gap_2()
                        .items_center()
                        .child(
                            Checkbox::new(SharedString::from(format!("form-bool-{id}")))
                                .checked(checked)
                                .label(if checked { "Yes" } else { "No" })
                                .tab_stop(false)
                                .focus_ring(false)
                                .on_click(move |next, _, app| {
                                    let next = *next;
                                    entity.update(app, |this, cx| {
                                        if let Some(form) = this.state.form.as_mut() {
                                            form.values.insert(
                                                field.clone(),
                                                serde_json::Value::Bool(next),
                                            );
                                        }
                                        cx.notify();
                                    });
                                }),
                        )
                        .into_any_element()
                }
                FormControlKind::DateTime => {
                    let has_start_picker = self.form_dates.contains_key(ado_core::START_DATE_FIELD);
                    let has_target_picker =
                        self.form_dates.contains_key(ado_core::TARGET_DATE_FIELD);
                    if id == ado_core::TARGET_DATE_FIELD && has_start_picker && !has_target_picker {
                        return div().into_any_element();
                    }
                    let range_field =
                        id == ado_core::START_DATE_FIELD || id == ado_core::TARGET_DATE_FIELD;
                    let picker = self.form_dates.get(&id).cloned().or_else(|| {
                        if range_field {
                            self.form_dates
                                .get(ado_core::START_DATE_FIELD)
                                .cloned()
                                .or_else(|| {
                                    self.form_dates.get(ado_core::TARGET_DATE_FIELD).cloned()
                                })
                        } else {
                            None
                        }
                    });
                    let has_picker = picker.is_some();
                    let fallback = date_field_text(&id, &value, &form.values);
                    let editor = self
                        .form_inputs
                        .get(&id)
                        .cloned()
                        .or_else(|| self.form_inputs.get(ado_core::START_DATE_FIELD).cloned())
                        .or_else(|| self.form_inputs.get(ado_core::TARGET_DATE_FIELD).cloned());
                    let calendar_open = self.form_calendar_open;
                    let dates_error = self.form_dates_error(cx);
                    let form_id = form.model.id;
                    v_flex()
                        .id(SharedString::from(format!("form-date-{id}")))
                        .debug_selector(|| "form-date-picker".into())
                        .w_full()
                        .max_w_full()
                        .gap_2()
                        .when_some(picker, |el, state| {
                            el.child(
                                h_flex()
                                    .id("form-dates-value")
                                    .debug_selector(|| "form-dates-value".into())
                                    .w_full()
                                    .gap_2()
                                    .items_center()
                                    .when_some(editor.clone(), |row, input| {
                                        row.child(
                                            div()
                                                .id("form-dates-typed")
                                                .debug_selector(|| "form-dates-typed".into())
                                                .flex_1()
                                                .min_w(px(0.))
                                                .child(
                                                    Input::new(&input)
                                                        .id("form-dates-input")
                                                        .w_full(),
                                                ),
                                        )
                                    })
                                    .child(
                                        Button::new("form-dates-clear")
                                            .ghost()
                                            .compact()
                                            .label("Clear dates")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.sync_form_dates_after_io = true;
                                                if !this.spawn_state(
                                                    cx,
                                                    move |mut state| async move {
                                                        let result =
                                                            state.clear_dates(form_id).await;
                                                        (state, result)
                                                    },
                                                ) {
                                                    this.sync_form_dates_after_io = false;
                                                }
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("form-dates-calendar-toggle")
                                            .ghost()
                                            .compact()
                                            .label(if calendar_open {
                                                "Hide calendar"
                                            } else {
                                                "Calendar"
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.form_calendar_open = !this.form_calendar_open;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .when(calendar_open, |el| {
                                el.child(self.render_calendar_chrome(&state, cx))
                            })
                            .child(
                                div()
                                    .id("form-dates-error")
                                    .debug_selector(|| "form-dates-error".into())
                                    .w_full()
                                    .h(px(16.))
                                    .text_xs()
                                    .text_color(cx.theme().danger)
                                    .child(dates_error.clone().unwrap_or_default()),
                            )
                        })
                        .when(!has_picker, |el| {
                            let empty = fallback.is_empty();
                            el.child(
                                h_flex()
                                    .id("form-dates-value")
                                    .debug_selector(|| "form-dates-value".into())
                                    .w_full()
                                    .min_h(px(36.))
                                    .px_3()
                                    .items_center()
                                    .border_1()
                                    .border_color(cx.theme().input)
                                    .rounded(cx.theme().radius)
                                    .bg(cx.theme().background)
                                    .text_sm()
                                    .when(empty, |row| row.text_color(cx.theme().muted_foreground))
                                    .child(if empty { "—".to_string() } else { fallback }),
                            )
                        })
                        .into_any_element()
                }
                FormControlKind::Identity => {
                    let display = identity_from_field(&value)
                        .map(|identity| identity.display_name)
                        .unwrap_or_default();
                    let results = form.identity_results.clone();
                    let editor = self.form_inputs.get(&id).cloned();
                    let field = id.clone();
                    let chip_name = if display.is_empty() {
                        "Unassigned".to_string()
                    } else {
                        display.clone()
                    };
                    v_flex()
                        .gap_2()
                        .child(
                            h_flex()
                                .id(SharedString::from(format!("form-ident-chip-{id}")))
                                .debug_selector(|| "form-assignee-chip".into())
                                .gap_2()
                                .items_center()
                                .px_2()
                                .py_1()
                                .rounded_full()
                                .bg(cx.theme().muted)
                                .child(
                                    Avatar::new()
                                        .name(chip_name.clone())
                                        .with_size(Size::XSmall),
                                )
                                .child(div().text_sm().child(chip_name)),
                        )
                        .child(h_flex().gap_2().when_some(editor, |el, input| {
                            el.child(
                                Input::new(&input)
                                    .id(SharedString::from(format!("ident-q-{field}")))
                                    .flex_1(),
                            )
                            .child(
                                Button::new(SharedString::from(format!("search-{field}")))
                                    .ghost()
                                    .compact()
                                    .label("Search")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let query = this
                                            .form_inputs
                                            .get(&field)
                                            .map(|input| input.read(cx).value().to_string())
                                            .unwrap_or_default();
                                        this.spawn_unbounded(cx, move |mut state| async move {
                                            let result = state.search_identities(query).await;
                                            (state, result)
                                        });
                                    })),
                            )
                        }))
                        .children(results.into_iter().map(|identity| {
                            let label = identity.display_name.clone();
                            let unique = identity.unique_name.clone();
                            let field = id.clone();
                            let chip = label.clone();
                            h_flex()
                                .id(SharedString::from(format!("ident-{field}-{unique}")))
                                .debug_selector(|| "form-identity-result".into())
                                .gap_2()
                                .items_center()
                                .w_full()
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .hover(|el| el.bg(cx.theme().muted))
                                .cursor_pointer()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(form) = this.state.form.as_mut() {
                                        form.values.insert(
                                            field.clone(),
                                            crate::form::identity_json(&identity),
                                        );
                                    }
                                    cx.notify();
                                }))
                                .child(Avatar::new().name(chip.clone()).with_size(Size::XSmall))
                                .child(div().flex_1().child(label))
                        }))
                        .into_any_element()
                }
                FormControlKind::Picklist => {
                    let options = control.options.clone().unwrap_or_default();
                    let selected = value.as_str().unwrap_or("").to_string();
                    h_flex()
                        .gap_1()
                        .flex_wrap()
                        .children(options.into_iter().map(|opt| {
                            let field = id.clone();
                            let value = opt.clone();
                            Button::new(SharedString::from(format!("opt-{field}-{opt}")))
                                .label(opt)
                                .selected(value == selected)
                                .when(value == selected, |btn| btn.primary())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(form) = this.state.form.as_mut() {
                                        form.set_string(&field, value.clone());
                                    }
                                    cx.notify();
                                }))
                        }))
                        .into_any_element()
                }
                _ => {
                    if let Some(input) = self.form_inputs.get(&id) {
                        Input::new(input)
                            .id(SharedString::from(id))
                            .into_any_element()
                    } else {
                        div()
                            .id(SharedString::from(id))
                            .border_1()
                            .border_color(cx.theme().border)
                            .px_2()
                            .py_1()
                            .child(crate::form::format_form_value(control.kind, &value))
                            .into_any_element()
                    }
                }
            })
            .into_any_element()
    }

    pub fn open_form(&mut self, id: i64, cx: &mut Context<Self>) {
        if self
            .form_loading
            .as_ref()
            .is_some_and(|(open, _)| *open == id)
        {
            return;
        }
        if self
            .state
            .form
            .as_ref()
            .is_some_and(|form| form.model.id == id)
            && self.form_loading.is_none()
        {
            return;
        }
        self.dismiss_overlays();
        self.state.close_form();
        self.form_inputs.clear();
        self.form_dates.clear();
        self.form_dates_preview = None;
        self.form_inputs_for = None;
        self.form_calendar_open = false;
        self.form_dates_typed_seed.clear();
        self.pending_form_dates_text = None;
        let title = self
            .state
            .nodes
            .iter()
            .find(|node| node.id == id)
            .map(|node| node.title.clone())
            .unwrap_or_default();
        self.form_loading = Some((id, title));
        if !self.spawn_load(cx, move |mut state| async move {
            let result = state.open_form(id).await;
            (state, result)
        }) {
            self.form_loading = None;
        }
    }

    pub fn form_is_loading(&self) -> bool {
        self.form_loading.is_some()
    }

    pub fn toggle_form_calendar(&mut self) {
        self.form_calendar_open = !self.form_calendar_open;
    }

    fn form_dates_error(&self, cx: &App) -> Option<String> {
        let form_id = self.state.form.as_ref().map(|form| form.model.id)?;
        if self.form_inputs_for != Some(form_id) {
            return None;
        }
        let typed = self
            .form_inputs
            .get(ado_core::START_DATE_FIELD)
            .or_else(|| self.form_inputs.get(ado_core::TARGET_DATE_FIELD))
            .map(|input| input.read(cx).value().to_string())?;
        self.typed_dates_error_for_item(&typed, form_id)
    }

    fn form_has_unsaved_edits(&self, cx: &App) -> bool {
        let Some(form) = self.state.form.as_ref() else {
            return false;
        };
        if form.is_dirty() {
            return true;
        }
        for control in crate::form::all_controls(&form.model) {
            if matches!(
                control.kind,
                FormControlKind::Identity | FormControlKind::Picklist | FormControlKind::Boolean
            ) {
                continue;
            }
            let Some(input) = self.form_inputs.get(&control.reference_name) else {
                continue;
            };
            let live = input.read(cx).value().to_string();
            let stored = if control.kind == FormControlKind::Html {
                form.html_markdown
                    .get(&control.reference_name)
                    .cloned()
                    .unwrap_or_default()
            } else if control.kind == FormControlKind::DateTime {
                // Ctrl+A on the combined Dates field fires an empty Change
                // first; that must not enable Save. Other DateTime fields
                // treat a blank as a real clear.
                if live.trim().is_empty()
                    && (control.reference_name == ado_core::START_DATE_FIELD
                        || control.reference_name == ado_core::TARGET_DATE_FIELD)
                {
                    continue;
                }
                date_field_text(
                    &control.reference_name,
                    form.values
                        .get(&control.reference_name)
                        .unwrap_or(&serde_json::Value::Null),
                    &form.values,
                )
            } else {
                form.values
                    .get(&control.reference_name)
                    .map(|value| crate::form::format_form_value(control.kind, value))
                    .unwrap_or_default()
            };
            if live.trim() != stored.trim() {
                return true;
            }
        }
        false
    }

    fn sync_form_inputs(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.state.form.as_mut() else {
            return;
        };
        let controls = crate::form::all_controls(&form.model);
        for control in controls {
            let Some(input) = self.form_inputs.get(&control.reference_name) else {
                continue;
            };
            let text = input.read(cx).value().to_string();
            match control.kind {
                FormControlKind::Integer => {
                    if text.trim().is_empty() {
                        form.values
                            .insert(control.reference_name.clone(), serde_json::Value::Null);
                    } else if let Ok(n) = text.trim().parse::<i64>() {
                        form.values
                            .insert(control.reference_name.clone(), serde_json::json!(n));
                    } else {
                        form.set_string(&control.reference_name, text);
                    }
                }
                FormControlKind::Double => {
                    if text.trim().is_empty() {
                        form.values
                            .insert(control.reference_name.clone(), serde_json::Value::Null);
                    } else if let Ok(n) = text.trim().parse::<f64>() {
                        form.values
                            .insert(control.reference_name.clone(), serde_json::json!(n));
                    } else {
                        form.set_string(&control.reference_name, text);
                    }
                }
                FormControlKind::Html if form.is_plain_html(&control.reference_name) => {
                    form.set_markdown(&control.reference_name, text);
                }
                FormControlKind::Identity => {
                    form.identity_query = text;
                }
                FormControlKind::DateTime => {
                    if text.trim().is_empty()
                        && control.reference_name != ado_core::START_DATE_FIELD
                        && control.reference_name != ado_core::TARGET_DATE_FIELD
                    {
                        form.values
                            .insert(control.reference_name.clone(), serde_json::Value::Null);
                    }
                }
                _ => form.set_string(&control.reference_name, text),
            }
        }
        for (reference, picker) in &self.form_dates {
            let typed = self
                .form_inputs
                .get(reference)
                .map(|input| input.read(cx).value().to_string())
                .unwrap_or_default();
            let date = picker.read(cx).date();
            let Some((start, target)) =
                dates_from_range_picker(date, &typed, &self.form_dates_typed_seed)
            else {
                continue;
            };
            // Ctrl+A then typing fires an empty Change first on the combined
            // Dates field. Do not wipe that draft (and flip the layout). Other
            // DateTime pickers write null so an emptied field actually clears.
            let gantt_range = reference == ado_core::START_DATE_FIELD
                || reference == ado_core::TARGET_DATE_FIELD
                || matches!(date, PickerDate::Range(_, _));
            if start.is_none() && target.is_none() {
                if gantt_range {
                    continue;
                }
                form.values
                    .insert(reference.clone(), serde_json::Value::Null);
                continue;
            }
            if !gantt_range && typed.trim().is_empty() {
                form.values
                    .insert(reference.clone(), serde_json::Value::Null);
                continue;
            }
            if matches!(date, PickerDate::Range(_, _))
                || reference == ado_core::START_DATE_FIELD
                || reference == ado_core::TARGET_DATE_FIELD
            {
                form.set_exact_range_dates(start.map(iso_date_only), target.map(iso_date_only));
            } else if let Some(day) = start.or(target) {
                form.values
                    .insert(reference.clone(), serde_json::json!(iso_date_only(day)));
            } else {
                form.values
                    .insert(reference.clone(), serde_json::Value::Null);
            }
        }
    }

    fn save_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_pending_form_dates(window, cx);
        if self.form_dates_error(cx).is_some() {
            return;
        }
        self.sync_form_inputs(cx);
        if self.state.form.as_ref().is_none_or(|form| !form.is_dirty()) {
            return;
        }
        self.spawn_state(cx, |mut state| async move {
            let result = state.save_form().await;
            (state, result)
        });
    }

    fn on_bar_drag_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.gantt.gesture.is_none() {
            return;
        }
        let x: f32 = event.position.x.into();
        let y: f32 = event.position.y.into();
        if event.dragging() || event.pressed_button.is_some() {
            self.bar_last_window_x = Some(x);
            if event.modifiers.shift {
                self.bar_press_shift = true;
            }
            let id = self.state.gantt.gesture.as_ref().map(|gesture| gesture.id);
            let local_x = self.pointer_layout_x(x, id);
            self.state.update_bar_gesture(local_x);
            self.arm_link_target_from_pointer(x, y);
            self.saw_pressed_drag = true;
            cx.notify();
            return;
        }
        // Mouse-up was missed (handle unmounted mid-drag, or a tooltip ate
        // the up). Commit at the last pressed x, not this later hover
        // (Jul 28 shrink). Share the release path so a gutter micro-drag
        // still cancels instead of PATCHing Today.
        let x = self.bar_last_window_x.unwrap_or(x);
        self.release_bar_gesture(x, y, window, cx);
    }

    /// Update the live gesture from the release pointer, then commit.
    fn release_bar_gesture(
        &mut self,
        window_x: f32,
        window_y: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.gantt.gesture.is_none() {
            return;
        }
        let unscheduled = self.state.gantt.gesture.as_ref().is_some_and(|gesture| {
            gesture.kind == GestureKind::Move && !gesture.had_start && !gesture.had_target
        });
        let unscheduled_no_move = unscheduled
            && self
                .pill_press_window_x
                .is_some_and(|origin| (window_x - origin).abs() < 6.0);
        let still_in_gutter =
            unscheduled && self.timeline_local_x(window_x) < UNSCHEDULED_GUTTER_RIGHT;
        self.pill_press_window_x = None;
        if unscheduled_no_move || still_in_gutter {
            self.bar_press_window_x = None;
            self.bar_last_window_x = None;
            self.bar_press_shift = false;
            self.state.cancel_gesture();
            self.saw_pressed_drag = false;
            self.request_repaint(Some(window), cx);
            return;
        }
        let linking = self
            .state
            .gantt
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.kind == GestureKind::Link);
        if linking {
            self.arm_link_target_from_pointer(window_x, window_y);
        }
        // A fast flick has no mouse-move (or only a same-x press move), so
        // live dates still match the origin. Apply the release x. After a
        // real drag, keep the last live preview — remapping mouse-up through
        // a grown bar can snap back to origin (Aug 8 must not become Sep 1).
        self.apply_flick_release_x(window_x);
        self.bar_press_window_x = None;
        self.bar_last_window_x = None;
        self.saw_pressed_drag = true;
        self.commit_gesture(window, cx);
    }

    fn unmoved_date_gesture(&self) -> bool {
        self.state.gantt.gesture.as_ref().is_some_and(|gesture| {
            gesture.kind != GestureKind::Link
                && gesture.live_start == gesture.start
                && gesture.live_target == gesture.target
        })
    }

    /// Map a flick's release window-x onto the live dates. Prefer the
    /// press→release window delta so we do not remap through a painted bar.
    fn apply_flick_release_x(&mut self, window_x: f32) {
        if !self.unmoved_date_gesture() {
            return;
        }
        let unscheduled = self.state.gantt.gesture.as_ref().is_some_and(|gesture| {
            gesture.kind == GestureKind::Move && !gesture.had_start && !gesture.had_target
        });
        let id = self.state.gantt.gesture.as_ref().map(|gesture| gesture.id);
        let origin_x = self
            .state
            .gantt
            .gesture
            .as_ref()
            .map(|gesture| gesture.origin_x);
        let press_x = self.bar_press_window_x.or(self.pill_press_window_x);
        // Unscheduled origin_x is Today, not the gutter press. Map the
        // release to the date under the cursor so a flick is not Today+Δ.
        let local_x = if unscheduled {
            self.pointer_layout_x(window_x, id)
        } else {
            match (origin_x, press_x) {
                (Some(origin_x), Some(press_x)) => origin_x + (window_x - press_x),
                _ => self.pointer_layout_x(window_x, id),
            }
        };
        self.state.update_bar_gesture(local_x);
    }

    fn arm_link_target_from_pointer(&mut self, window_x: f32, window_y: f32) {
        if !self
            .state
            .gantt
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.kind == GestureKind::Link)
        {
            return;
        }
        let source = self.state.gantt.gesture.as_ref().map(|gesture| gesture.id);
        let local_x = self.timeline_local_x(window_x);
        let local_y = self.link_target_local_y(window_y);
        let visible = self.state.visible_nodes();
        let rows = self.state.gantt.flatten(&visible);
        let content_h = (self.state.gantt.viewport_h - TIMELINE_HEADER_H).max(80.0);
        let bars = self
            .state
            .gantt
            .layout_bars(&visible, &rows, &self.state.iterations, content_h);
        match GanttView::hit_test_link_target(&bars, local_x, local_y) {
            Some(id) if source != Some(id) => self.state.gantt.set_link_target(id),
            _ => self.state.gantt.clear_link_target(),
        }
    }

    fn link_target_local_y(&self, window_y: f32) -> f32 {
        let body_y = self.timeline_body_origin_y.get();
        if body_y > 0.0 {
            window_y - body_y
        } else {
            window_y - self.state.gantt.timeline_origin_y - TIMELINE_HEADER_H
        }
    }

    fn timeline_local_x(&mut self, window_x: f32) -> f32 {
        if let Some(origin) = self.timeline_body_origin_x.get() {
            self.state.gantt.timeline_origin_x = origin;
        }
        window_x - self.state.gantt.timeline_origin_x
    }

    fn painted_bar_for(&self, id: i64, layout: &crate::gantt::BarLayout) -> PaintedBar {
        if let Some(painted) = self.painted_bar_bounds.borrow().get(&id).copied() {
            return painted;
        }
        let origin = self
            .timeline_body_origin_x
            .get()
            .unwrap_or(self.state.gantt.timeline_origin_x);
        let width = if layout.milestone || layout.marker {
            MILESTONE_SIZE
        } else {
            layout.width.max(8.0)
        };
        PaintedBar {
            left: origin + layout.x,
            right: origin + layout.x + width,
            layout_x: layout.x,
            content_left: origin + layout.x,
        }
    }

    fn painted_timeline_edges(&self, bar: &crate::gantt::BarLayout) -> (f32, f32) {
        let painted = self.painted_bar_for(bar.id, bar);
        let origin = self
            .timeline_body_origin_x
            .get()
            .unwrap_or(self.state.gantt.timeline_origin_x);
        (painted.left - origin, painted.right - origin)
    }

    fn pointer_layout_x(&mut self, window_x: f32, id: Option<i64>) -> f32 {
        if let Some(id) = id
            && let Some(painted) = self.painted_bar_bounds.borrow().get(&id).copied()
        {
            return painted.layout_x + (window_x - painted.content_left);
        }
        self.timeline_local_x(window_x)
    }

    fn clear_pinned_tooltip_if_pointer_left(
        &mut self,
        window_x: f32,
        window_y: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.state.gantt.pin_bar_tooltip else {
            return;
        };
        let Some(painted) = self.painted_bar_bounds.borrow().get(&id).copied() else {
            self.state.gantt.pin_bar_tooltip = None;
            self.state.gantt.pin_ignore_leave = false;
            cx.notify();
            return;
        };
        let on_bar = window_x >= painted.left
            && window_x <= painted.right
            && self.state.gantt.pointer_in_rows(window_x, window_y);
        if on_bar {
            return;
        }
        self.state.gantt.pin_bar_tooltip = None;
        self.state.gantt.pin_ignore_leave = false;
        cx.notify();
    }

    fn classify_bar_hit(
        &mut self,
        id: i64,
        layout: &crate::gantt::BarLayout,
        window_x: f32,
    ) -> (BarHitZone, GestureKind) {
        let painted = self.painted_bar_for(id, layout);
        let zone = layout.hit_zone_at_window_x(window_x, painted.left, painted.right);
        let kind = zone.gesture_kind().unwrap_or(GestureKind::Move);
        self.last_bar_hit = Some(BarHitDebug {
            event_x: window_x,
            painted_left: painted.left,
            painted_right: painted.right,
            zone,
            kind,
        });
        if debug_hit_enabled() {
            eprintln!(
                "ADO_PLANNER_DEBUG_HIT id={id} event_x={window_x:.2} painted_left={:.2} painted_right={:.2} zone={} kind={kind:?}",
                painted.left,
                painted.right,
                zone.as_str()
            );
        }
        (zone, kind)
    }

    fn begin_classified_bar_gesture(
        &mut self,
        id: i64,
        layout: &crate::gantt::BarLayout,
        window_x: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_toast_pointer_for_new_press();
        if self
            .state
            .gantt
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.kind == GestureKind::Link)
        {
            self.state.gantt.set_link_target(id);
            cx.notify();
            return;
        }
        let (zone, kind) = self.classify_bar_hit(id, layout, window_x);
        self.state.gantt.select_row(id);
        self.focus_planner(window, cx);
        self.dismiss_overlays();
        if matches!(zone, BarHitZone::RollupEdge | BarHitZone::Outside) {
            cx.notify();
            return;
        }
        let origin = self.pointer_layout_x(window_x, Some(id));
        self.bar_press_window_x = Some(window_x);
        self.bar_last_window_x = Some(window_x);
        let _ = self.state.begin_bar_gesture(id, kind, origin);
        cx.notify();
    }

    /// Overlay mouse-down on a summary edge. Always RollupEdge; never a drag.
    fn begin_rollup_edge_press(
        &mut self,
        id: i64,
        layout: &crate::gantt::BarLayout,
        window_x: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_toast_pointer_for_new_press();
        if self
            .state
            .gantt
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.kind == GestureKind::Link)
        {
            self.state.gantt.set_link_target(id);
            cx.notify();
            return;
        }
        let painted = self.painted_bar_for(id, layout);
        self.last_bar_hit = Some(BarHitDebug {
            event_x: window_x,
            painted_left: painted.left,
            painted_right: painted.right,
            zone: BarHitZone::RollupEdge,
            kind: GestureKind::Move,
        });
        if debug_hit_enabled() {
            eprintln!(
                "ADO_PLANNER_DEBUG_HIT id={id} event_x={window_x:.2} painted_left={:.2} painted_right={:.2} zone=rollup-edge kind=forced",
                painted.left, painted.right
            );
        }
        self.state.gantt.select_row(id);
        self.focus_planner(window, cx);
        self.dismiss_overlays();
        cx.notify();
    }

    fn continue_or_begin_bar_gesture(
        &mut self,
        id: i64,
        kind: GestureKind,
        origin: f32,
        window_x: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_toast_pointer_for_new_press();
        if self
            .state
            .gantt
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.kind == GestureKind::Link)
        {
            self.state.gantt.set_link_target(id);
            cx.notify();
            return;
        }
        self.state.gantt.select_row(id);
        self.focus_planner(window, cx);
        self.dismiss_overlays();
        if kind != GestureKind::Link {
            self.bar_press_window_x = Some(window_x);
            self.bar_last_window_x = Some(window_x);
        }
        let _ = self.state.begin_bar_gesture(id, kind, origin);
        cx.notify();
    }

    fn commit_gesture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.saw_pressed_drag = false;
        self.bar_press_window_x = None;
        self.bar_last_window_x = None;
        let shift_children = self.bar_press_shift;
        self.bar_press_shift = false;
        if self.state.gantt.gesture.is_none() {
            return;
        }
        let linking = self
            .state
            .gantt
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.kind == GestureKind::Link);
        if linking {
            let pair = self.state.gantt.take_link_gesture();
            self.state.gantt.selected_link = None;
            self.state.gantt.clear_hover();
            cx.notify();
            window.refresh();
            cx.refresh_windows();
            if let Some((from_id, Some(to_id))) = pair
                && from_id != to_id
            {
                self.spawn_state(cx, move |mut state| async move {
                    let result = state.add_dependency(to_id, from_id).await;
                    (state, result)
                });
            }
            return;
        }
        self.state.gantt.clear_hover();
        if let Some(mut pending) = self.state.commit_date_gesture_now() {
            self.last_committed_start_for_test = pending.start;
            self.last_committed_target_for_test = pending.target;
            if shift_children {
                pending.cascade = self.state.shift_children_with_committed_move(&pending);
            }
            self.enqueue_date_save(cx, pending);
        }
        self.request_repaint(Some(window), cx);
    }

    /// `Window.refresh` is a no-op while GPUI is drawing, so mouse-up during
    /// a frame can leave the preview chip and toast stale until the pointer
    /// moves. Notify now, and again after a frame so a still mouse (left-end
    /// resize under the start grip) still gets a paint.
    fn request_repaint(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        cx.notify();
        if let Some(window) = window {
            window.refresh();
        }
        cx.refresh_windows();
        // Mouse-up can land during a frame so the first refresh is a no-op;
        // tick twice so the committed bar and toast paint without waiting
        // for the next mouse-move.
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(0))
                .await;
            let _ = this.update(cx, |_, cx| {
                cx.notify();
                cx.refresh_windows();
            });
            cx.refresh();
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let _ = this.update(cx, |_, cx| {
                cx.notify();
                cx.refresh_windows();
            });
            cx.refresh();
        })
        .detach();
    }

    fn enqueue_date_save(&mut self, cx: &mut Context<Self>, pending: PendingDateSave) {
        self.date_save_queue.push_back(pending);
        self.drain_date_saves(cx);
    }

    fn drain_date_saves(&mut self, cx: &mut Context<Self>) {
        coalesce_superseded_date_saves(&mut self.date_save_queue);
        let mut index = 0;
        while index < self.date_save_queue.len() {
            let id = self.date_save_queue[index].id;
            if self.date_save_busy.contains(&id) {
                index += 1;
                continue;
            }
            let mut pending = self.date_save_queue.remove(index).expect("queued save");
            let Some(live) = self.state.nodes.iter().find(|node| node.id == pending.id) else {
                self.state.discard_stale_date_save(&pending);
                continue;
            };
            if live.start_date != pending.after_start || live.target_date != pending.after_target {
                self.state.discard_stale_date_save(&pending);
                continue;
            }
            pending.rev = live.rev;
            self.spawn_one_date_save(cx, pending);
        }
    }

    fn spawn_one_date_save(&mut self, cx: &mut Context<Self>, pending: PendingDateSave) {
        let runtime = self.state.runtime.clone();
        let mut working = self.state.clone();
        let id = pending.id;
        let kids = pending.cascade.clone();
        if pending.role == DateSaveRole::Commit {
            // Clone already has the Shift-drag child restores for the toast.
            // Drop them on live so a later non-Shift move cannot reuse them.
            let _ = self.state.take_shifted_children(id);
        }
        self.date_save_busy.insert(id);
        self.arm_saving_status(cx);
        let after_start = pending.after_start.clone();
        let after_target = pending.after_target.clone();
        let previous = pending.previous.clone();
        let role = pending.role;
        let revert = pending.clone();
        let rx = runtime.spawn_result(async move {
            let result = working.persist_pending_dates(pending).await;
            (working, result)
        });
        cx.spawn(async move |this, cx| {
            let outcome = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.date_save_busy.remove(&id);
                if matches!(
                    role,
                    DateSaveRole::Undo | DateSaveRole::Restore | DateSaveRole::Redo
                ) {
                    this.skip_saving_hold = true;
                }
                this.finish_or_hold_saving_status(cx);
                if let Ok((working, result)) = outcome {
                    let still_after = this.state.nodes.iter().any(|node| {
                        node.id == id
                            && node.start_date == after_start
                            && node.target_date == after_target
                    });
                    match result {
                        Ok(()) => {
                            if still_after {
                                if let Some(rev) = working
                                    .nodes
                                    .iter()
                                    .find(|node| node.id == id)
                                    .map(|n| n.rev)
                                    && let Some(live) =
                                        this.state.nodes.iter_mut().find(|node| node.id == id)
                                {
                                    live.rev = rev;
                                }
                                this.state.sync_open_form_from_node(id);
                                this.queue_form_dates_from_draft();
                                if role == DateSaveRole::Commit
                                    && let Some(toast) = working.toasts.items.last()
                                {
                                    if let Some(undo) = toast.undo.clone() {
                                        this.state.toasts.push_undo_extra(
                                            toast.message.clone(),
                                            undo,
                                            toast.extra.clone(),
                                        );
                                    } else {
                                        this.state
                                            .toasts
                                            .push(ToastKind::Success, toast.message.clone());
                                    }
                                    this.note_toasts_changed(cx);
                                }
                                if role == DateSaveRole::Restore {
                                    this.state.complete_restore_date_save(&revert);
                                }
                            } else if let Some(live) =
                                this.state.nodes.iter().find(|node| node.id == id).cloned()
                                && let Some(persisted) =
                                    working.nodes.iter().find(|node| node.id == id).cloned()
                            {
                                this.enqueue_date_save(
                                    cx,
                                    PlannerState::follow_up_date_save(&live, &persisted),
                                );
                            }
                            if role == DateSaveRole::Commit {
                                this.enqueue_matching_cascade(cx, kids);
                            }
                        }
                        Err(err) => {
                            if still_after {
                                this.state.restore_node(previous);
                                this.state.revert_date_save_history(&revert);
                                this.state.restore_date_save_failed(&revert);
                                if role == DateSaveRole::Commit {
                                    this.state.toasts.dismiss_undo();
                                }
                            }
                            if role == DateSaveRole::Commit {
                                this.restore_matching_cascade(&kids);
                            }
                            if let Some(msg) = working.toasts.last_error_message() {
                                this.state.toasts.push(ToastKind::Error, msg.to_string());
                            }
                            tracing::warn!("planner action failed: {err}");
                        }
                    }
                }
                this.drain_date_saves(cx);
                this.request_repaint(None, cx);
            });
        })
        .detach();
    }

    fn enqueue_matching_cascade(&mut self, cx: &mut Context<Self>, kids: Vec<PendingDateSave>) {
        for save in kids {
            if save.live_dates_match(&self.state.nodes) {
                self.enqueue_date_save(cx, save);
            }
        }
    }

    fn restore_matching_cascade(&mut self, kids: &[PendingDateSave]) {
        let matching: Vec<_> = kids
            .iter()
            .filter(|save| save.live_dates_match(&self.state.nodes))
            .cloned()
            .collect();
        self.state.restore_cascade_saves(&matching);
    }

    fn ensure_form_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.state.form.as_ref() else {
            self.form_inputs.clear();
            self.form_dates.clear();
            self.form_dates_preview = None;
            self.pending_form_dates_text = None;
            self.form_inputs_for = None;
            return;
        };
        if self.form_inputs_for != Some(form.model.id) {
            self.form_inputs.clear();
            self.form_dates.clear();
            self.form_dates_preview = None;
            self.pending_form_dates_text = None;
            self.form_dates_typed_seed.clear();
            self.form_calendar_open = false;
            self.form_inputs_for = Some(form.model.id);
        }
        let node_dates = self
            .state
            .nodes
            .iter()
            .find(|node| node.id == form.model.id)
            .map(|node| {
                (
                    parse_iso_date(node.start_date.as_deref()).and_then(jiff_to_naive),
                    parse_iso_date(node.target_date.as_deref()).and_then(jiff_to_naive),
                )
            });
        for control in crate::form::all_controls(&form.model) {
            if control.read_only
                || matches!(
                    control.kind,
                    FormControlKind::Picklist | FormControlKind::Boolean
                )
                || (control.kind == FormControlKind::Html
                    && !form.is_plain_html(&control.reference_name))
            {
                continue;
            }
            if control.kind == FormControlKind::DateTime {
                let controls = crate::form::all_controls(&form.model);
                let has_start = controls
                    .iter()
                    .any(|item| item.reference_name == ado_core::START_DATE_FIELD);
                let has_target = controls
                    .iter()
                    .any(|item| item.reference_name == ado_core::TARGET_DATE_FIELD);
                let (start, target) = node_dates.unwrap_or_else(|| {
                    (
                        form.values
                            .get(ado_core::START_DATE_FIELD)
                            .and_then(serde_json::Value::as_str)
                            .and_then(|text| parse_iso_date(Some(text)))
                            .and_then(jiff_to_naive),
                        form.values
                            .get(ado_core::TARGET_DATE_FIELD)
                            .and_then(serde_json::Value::as_str)
                            .and_then(|text| parse_iso_date(Some(text)))
                            .and_then(jiff_to_naive),
                    )
                });
                let target_only = has_start && has_target && start.is_none() && target.is_some();
                if control.reference_name == ado_core::TARGET_DATE_FIELD
                    && has_start
                    && !target_only
                {
                    continue;
                }
                if control.reference_name == ado_core::START_DATE_FIELD && target_only {
                    continue;
                }
                let store_key = if target_only {
                    ado_core::TARGET_DATE_FIELD.to_string()
                } else {
                    control.reference_name.clone()
                };
                if self.form_dates.contains_key(&store_key) {
                    continue;
                }
                let range = store_key == ado_core::START_DATE_FIELD && has_target;
                let initial = form
                    .values
                    .get(&store_key)
                    .and_then(serde_json::Value::as_str)
                    .and_then(|text| parse_iso_date(Some(text)))
                    .and_then(jiff_to_naive);
                let picker = cx.new(|cx| {
                    let mut state = CalendarState::new(window, cx);
                    match (range, start, target) {
                        (true, Some(start), Some(target)) => {
                            state.set_date(
                                PickerDate::Range(Some(start), Some(target)),
                                window,
                                cx,
                            );
                        }
                        (true, Some(start), None) => {
                            state.set_date(PickerDate::Range(Some(start), None), window, cx);
                        }
                        (true, None, Some(target)) => {
                            state.set_date(PickerDate::Range(None, Some(target)), window, cx);
                        }
                        (true, None, None) => {
                            state.set_date(PickerDate::Range(None, None), window, cx);
                        }
                        (_, Some(start), None) => {
                            state.set_date(PickerDate::Single(Some(start)), window, cx);
                        }
                        (_, None, Some(target)) => {
                            state.set_date(PickerDate::Single(Some(target)), window, cx);
                        }
                        _ => {
                            if let Some(naive) = initial {
                                state.set_date(PickerDate::Single(Some(naive)), window, cx);
                            }
                        }
                    }
                    state
                });
                let field = store_key.clone();
                self._subscriptions.push(cx.subscribe(
                    &picker,
                    move |this, _, event: &CalendarEvent, cx| {
                        let CalendarEvent::Selected(date) = event;
                        this.form_dates_preview = pending_range_typed(*date);
                        if !range_picker_is_complete(*date) {
                            this.pending_form_dates_text = pending_range_typed(*date);
                            cx.notify();
                            return;
                        }
                        let Some(form) = this.state.form.as_mut() else {
                            return;
                        };
                        match *date {
                            PickerDate::Range(start, end) => {
                                form.set_range_dates(
                                    start.and_then(naive_to_jiff).map(iso_date_only),
                                    end.and_then(naive_to_jiff).map(iso_date_only),
                                );
                            }
                            PickerDate::Single(Some(day)) => {
                                if let Some(parsed) = naive_to_jiff(day) {
                                    form.values.insert(
                                        field.clone(),
                                        serde_json::json!(iso_date_only(parsed)),
                                    );
                                }
                            }
                            PickerDate::Single(None) => {
                                form.values.insert(field.clone(), serde_json::Value::Null);
                            }
                        }
                        this.form_dates_preview = pending_range_typed(*date);
                        this.pending_form_dates_text = Some(format_picker_dates(*date));
                        cx.notify();
                    },
                ));
                self._subscriptions
                    .push(cx.observe(&picker, move |this, picker, cx| {
                        if this.state.form.is_none() {
                            return;
                        }
                        let date = picker.read(cx).date();
                        this.form_dates_preview = pending_range_typed(date);
                        if !range_picker_is_complete(date) {
                            this.pending_form_dates_text = pending_range_typed(date);
                        }
                        cx.notify();
                    }));
                let initial_dates = format_date_range(
                    start.and_then(naive_to_jiff),
                    target.and_then(naive_to_jiff),
                );
                if self.form_dates_typed_seed.is_empty() {
                    self.form_dates_typed_seed = initial_dates.clone();
                }
                if !self.form_inputs.contains_key(&store_key) {
                    let seed = initial_dates.clone();
                    let input = cx.new(|cx| {
                        let mut state = InputState::new(window, cx);
                        state.set_placeholder(DATES_RANGE_HINT, window, cx);
                        state.set_value(&seed, window, cx);
                        state
                    });
                    self._subscriptions.push(cx.subscribe(
                        &input,
                        |this, _, event: &InputEvent, cx| {
                            let form_id = this.state.form.as_ref().map(|form| form.model.id);
                            if this.form_inputs_for != form_id {
                                return;
                            }
                            if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. }) {
                                this.sync_form_inputs(cx);
                                cx.notify();
                            }
                        },
                    ));
                    self.form_inputs.insert(store_key.clone(), input);
                }
                self.form_dates.insert(store_key, picker);
                continue;
            }
            if self.form_inputs.contains_key(&control.reference_name) {
                continue;
            }
            let initial = if control.kind == FormControlKind::Html {
                form.html_markdown
                    .get(&control.reference_name)
                    .cloned()
                    .unwrap_or_default()
            } else if control.kind == FormControlKind::Identity {
                form.identity_query.clone()
            } else {
                form.values
                    .get(&control.reference_name)
                    .map(|value| crate::form::format_form_value(control.kind, value))
                    .unwrap_or_default()
            };
            let input = cx.new(|cx| {
                let mut state = InputState::new(window, cx);
                state.set_value(initial, window, cx);
                state
            });
            self._subscriptions
                .push(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. }) {
                        this.sync_form_inputs(cx);
                        cx.notify();
                    }
                }));
            self.form_inputs
                .insert(control.reference_name.clone(), input);
        }
    }

    pub fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        self.on_key(event, cx);
    }

    pub fn is_busy(&self) -> bool {
        self.io_busy
    }

    pub fn set_busy_for_test(&mut self, busy: bool) {
        self.io_busy = busy;
        if busy && self.saving_shown_at.is_none() {
            self.saving_shown_at = Some(Instant::now());
        }
    }

    pub fn complete_undo_io_for_test(&mut self, cx: &mut Context<Self>) {
        self.io_busy = false;
        self.date_save_busy.clear();
        self.skip_saving_hold = true;
        self.finish_or_hold_saving_status(cx);
    }

    pub fn sync_status_for_test(&self) -> String {
        self.sync_status_text()
    }

    pub fn form_heading_for_test(&self) -> Option<String> {
        self.state.form.as_ref().map(|form| form.heading_title())
    }

    pub fn simulate_toast_hover_for_test(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.set_toast_hover(hovered, cx);
    }

    pub fn toast_pause_for_test(&self) -> bool {
        self.state.toasts.paused
    }

    pub fn replay_pending_toast_undo_for_test(&mut self, cx: &mut Context<Self>) {
        self.replay_pending_toast_undo(cx);
    }

    pub fn pending_toast_undo_id(&self) -> Option<u64> {
        self.pending_toast_undo.as_ref().map(|(id, _)| *id)
    }

    pub fn toast_undo_inflight_for_test(&self) -> bool {
        self.toast_undo_inflight
    }

    pub fn last_committed_start_for_test(&self) -> Option<jiff::civil::Date> {
        self.last_committed_start_for_test
    }

    pub fn last_committed_target_for_test(&self) -> Option<jiff::civil::Date> {
        self.last_committed_target_for_test
    }

    pub fn last_bar_hit_for_test(&self) -> Option<BarHitDebug> {
        self.last_bar_hit
    }

    pub fn toast_pointer_for_test(&self) -> bool {
        self.toast_pointer
    }

    pub fn set_form_loading_for_test(&mut self, id: i64, title: impl Into<String>) {
        self.form_loading = Some((id, title.into()));
    }

    pub fn set_form_loading_for_test_clear(&mut self) {
        self.form_loading = None;
    }

    pub fn pending_delete_id(&self) -> Option<i64> {
        self.pending_delete.as_ref().map(|(id, _)| *id)
    }

    pub fn inline_calendar_month(&self, cx: &gpui_kit::App) -> Option<u8> {
        self.inline_dates
            .as_ref()
            .map(|picker| picker.read(cx).current_month())
    }

    pub fn click_inline_calendar_day(&mut self, day: chrono::NaiveDate, cx: &mut Context<Self>) {
        if let Some(picker) = self.inline_dates.clone() {
            picker.update(cx, |state, cx| {
                state.activate_date(day, cx);
            });
        }
        cx.notify();
    }

    pub fn click_form_calendar_day(&mut self, day: chrono::NaiveDate, cx: &mut Context<Self>) {
        if let Some(picker) = self
            .form_dates
            .get(ado_core::START_DATE_FIELD)
            .cloned()
            .or_else(|| self.form_dates.get(ado_core::TARGET_DATE_FIELD).cloned())
        {
            picker.update(cx, |state, cx| {
                state.activate_date(day, cx);
            });
        }
        cx.notify();
    }

    pub fn pending_delete_warning(&self) -> Option<String> {
        let (id, _) = self.pending_delete.as_ref()?;
        let children = crate::gantt::descendants(&self.state.nodes, *id).len();
        Some(if children == 0 {
            "This removes the work item from Azure DevOps.".into()
        } else if children == 1 {
            "This item has 1 child, which stays in Azure DevOps without this parent.".into()
        } else {
            format!(
                "This item has {children} children, which stay in Azure DevOps without this parent."
            )
        })
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if event.is_held
            && (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
            && (key == "z" || key == "y")
        {
            return;
        }
        if key == "escape" {
            if self.pending_delete.take().is_some() {
                cx.notify();
                return;
            }
            if self.inline_edit.take().is_some() {
                cx.notify();
                return;
            }
            self.dismiss_all_overlays();
            cx.notify();
            return;
        }
        if self.pending_delete.is_some() {
            if key == "enter" && !event.is_held {
                self.confirm_delete(cx);
            }
            return;
        }
        if key == "enter" && self.inline_edit.is_some() {
            self.commit_inline(cx);
            return;
        }
        if key == "enter" && self.inline_enter_guard {
            self.inline_enter_guard = false;
            return;
        }
        if self.inline_edit.is_some() {
            if event.keystroke.modifiers.control || event.keystroke.modifiers.platform {
                if key == "z" && event.keystroke.modifiers.shift {
                    self.inline_edit = None;
                    self.redo_last(cx);
                    return;
                }
                if key == "z" {
                    self.inline_edit = None;
                    self.undo_last(cx);
                    return;
                }
                if key == "y" {
                    self.inline_edit = None;
                    self.redo_last(cx);
                    return;
                }
            }
            return;
        }
        if event.keystroke.modifiers.control || event.keystroke.modifiers.platform {
            if key == "z" && event.keystroke.modifiers.shift {
                self.redo_last(cx);
                return;
            }
            if key == "z" {
                self.undo_last(cx);
                return;
            }
            if key == "y" {
                self.redo_last(cx);
                return;
            }
        }
        if matches!(key, "delete" | "backspace")
            && let Some((from_id, to_id)) = self.state.gantt.selected_link
        {
            self.remove_link(from_id, to_id, cx);
            return;
        }
        if matches!(key, "delete" | "backspace")
            && !self.details_overlay()
            && let Some(id) = self.state.gantt.selected_id
        {
            self.request_delete(id, cx);
            return;
        }
        if self.details_overlay() || self.popup_open() {
            return;
        }
        if (event.keystroke.modifiers.control
            || event.keystroke.modifiers.platform
            || event.keystroke.modifiers.alt)
            && key != "left"
            && key != "right"
        {
            return;
        }
        if key == "enter" {
            if let Some(id) = self.state.gantt.selected_id {
                self.open_form(id, cx);
            }
            return;
        }
        let rows = self.state.visible_rows().len();
        let viewport = self.state.gantt.viewport_h;
        if key == "up" {
            self.select_relative_row(-1);
            cx.notify();
            return;
        }
        if key == "down" {
            self.select_relative_row(1);
            cx.notify();
            return;
        }
        let page_rows = ((viewport - TIMELINE_HEADER_H) / ROW_HEIGHT)
            .floor()
            .max(1.0) as i32;
        if key == "pageup" {
            self.select_relative_row(-page_rows);
            cx.notify();
            return;
        }
        if key == "pagedown" {
            self.select_relative_row(page_rows);
            cx.notify();
            return;
        }
        if key == "home" {
            self.cancel_camera_anim();
            self.state.gantt.scroll_y = 0.0;
            if let Some(first) = self.state.visible_rows().first() {
                self.state.gantt.selected_id = Some(first.id);
            }
            cx.notify();
            return;
        }
        if key == "end" {
            self.cancel_camera_anim();
            self.state.gantt.scroll_y = f32::MAX;
            self.state.gantt.clamp_scroll(rows, viewport);
            if let Some(last) = self.state.visible_rows().last() {
                self.state.gantt.selected_id = Some(last.id);
            }
            cx.notify();
            return;
        }
        if key == "left" || key == "right" {
            let delta = if event.keystroke.key == "left" { -1 } else { 1 };
            if self.state.gantt.context_menu.is_none() && !self.details_overlay() {
                if event.keystroke.modifiers.shift {
                    let _ = self.state.nudge(NudgeAxis::Target, delta);
                    self.schedule_nudge_flush(cx);
                } else if event.keystroke.modifiers.alt {
                    let _ = self.state.nudge(NudgeAxis::Start, delta);
                    self.schedule_nudge_flush(cx);
                } else if event.keystroke.modifiers.control {
                    self.state.gantt.nudge_tree_width(delta as f32 * 16.0);
                    self.state.persist_tree_width();
                    cx.notify();
                } else if self.state.gantt.selected_id.is_some() {
                    let _ = self.state.nudge(NudgeAxis::Both, delta);
                    self.schedule_nudge_flush(cx);
                } else {
                    self.state.gantt.nudge_tree_width(delta as f32 * 16.0);
                    self.state.persist_tree_width();
                    cx.notify();
                }
            }
        }
    }

    fn note_toasts_changed(&mut self, cx: &mut Context<Self>) {
        self.clamp_toast_hover();
        self.toast_expiry_gen += 1;
        self.toast_expiry_armed = false;
        self.schedule_toast_expiry(cx);
    }

    fn schedule_toast_expiry(&mut self, cx: &mut Context<Self>) {
        if self.toast_expiry_armed
            || self.state.toasts.items.is_empty()
            || self
                .state
                .toasts
                .items
                .iter()
                .all(|toast| toast.kind == ToastKind::Error)
        {
            return;
        }
        self.toast_expiry_armed = true;
        let expiry_gen = self.toast_expiry_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    if this.toast_expiry_gen != expiry_gen {
                        return;
                    }
                    this.state.toasts.expire();
                    this.toast_expiry_armed = false;
                    if !this.state.toasts.items.is_empty() {
                        this.schedule_toast_expiry(cx);
                    }
                    cx.notify();
                });
                cx.refresh();
            }
        })
        .detach();
    }

    fn render_updater(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(prompt) = self.state.updater.prompt.clone() else {
            return div().into_any_element();
        };
        h_flex()
            .id("updater-prompt")
            .w_full()
            .px_5()
            .py_2()
            .gap_2()
            .bg(cx.theme().accent)
            .child(div().child(format!("Update {} is available", prompt.version)))
            .child(
                Button::new("install-update")
                    .primary()
                    .label("Install")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let runtime = this.state.runtime.clone();
                        let mut updater = this.state.updater.clone();
                        let rx = runtime.spawn_result(async move {
                            tokio::task::spawn_blocking(move || updater.apply())
                                .await
                                .unwrap_or_else(|err| Err(err.to_string()))
                        });
                        cx.spawn(async move |this, cx| {
                            let result = rx.await.ok();
                            let _ = this.update(cx, |this, cx| {
                                match result {
                                    Some(Ok(())) => this.state.updater.prompt = None,
                                    Some(Err(err)) => {
                                        this.state.toasts.push(
                                            ToastKind::Error,
                                            format!("Could not install update: {err}"),
                                        );
                                    }
                                    None => {}
                                }
                                cx.notify();
                            });
                        })
                        .detach();
                    })),
            )
            .child(
                Button::new("snooze-update")
                    .label("Later")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.updater.snooze();
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn scroll_or_select(&mut self, rows: i32, cx: &mut Context<Self>) {
        self.state.gantt.clear_hover();
        self.cancel_camera_anim();
        let before = self.state.gantt.scroll_y;
        let count = self.state.visible_rows().len();
        self.state
            .gantt
            .scroll_rows(rows, self.state.gantt.viewport_h, count);
        if (self.state.gantt.scroll_y - before).abs() < f32::EPSILON {
            self.select_relative_row(rows.signum());
        }
        cx.notify();
    }

    fn select_relative_row(&mut self, delta: i32) {
        let pending = self.state.gantt.pending_nudge.clone();
        let rows = self.state.visible_rows();
        self.state.gantt.select_relative(&rows, delta);
        if let Some(pending) = pending
            && self.state.gantt.selected_id != Some(pending.id)
        {
            let _ = self.state.apply_optimistic_dates(
                pending.id,
                pending.original_start,
                pending.original_target,
            );
        }
    }

    fn schedule_nudge_flush(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(NUDGE_IDLE_MS))
                .await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    if this
                        .state
                        .nudge_is_idle(Duration::from_millis(NUDGE_IDLE_MS))
                    {
                        this.spawn_state(cx, |mut state| async move {
                            let result = state.flush_nudge().await;
                            (state, result)
                        });
                    }
                });
            }
        })
        .detach();
    }
}

fn this_type(state: &PlannerState, id: i64) -> Option<String> {
    state
        .nodes
        .iter()
        .find(|n| n.id == id)
        .map(|n| n.work_item_type.clone())
}

fn this_state(state: &PlannerState, id: i64) -> Option<String> {
    state
        .nodes
        .iter()
        .find(|n| n.id == id)
        .map(|n| n.state.clone())
}

fn jiff_to_naive(date: jiff::civil::Date) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::from_ymd_opt(
        i32::from(date.year()),
        u32::from(date.month() as u8),
        u32::from(date.day() as u8),
    )
}

fn naive_to_jiff(date: chrono::NaiveDate) -> Option<jiff::civil::Date> {
    jiff::civil::Date::new(date.year() as i16, date.month() as i8, date.day() as i8).ok()
}

fn month_name(month: u8) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        _ => "December",
    }
}

/// Map a range picker value (and optional typed ISO text) to Start/Target.
/// A missing side is `None` so the other date can stay as-is.
/// A complete typed range that differs from `typed_seed` wins so typing on a
/// scheduled item is not ignored. An explicit start-only typed date (`Jul 6`)
/// also wins so Target Date is cleared. An incomplete `…` preview after the
/// first calendar click must not override a complete calendar range.
pub fn format_picker_dates(date: PickerDate) -> String {
    match date {
        PickerDate::Range(start, end) => {
            format_date_range(start.and_then(naive_to_jiff), end.and_then(naive_to_jiff))
        }
        PickerDate::Single(day) => format_date_range(day.and_then(naive_to_jiff), None),
    }
}

/// Visible copy for a form DateTime when no picker exists (read-only fields).
pub fn date_field_text(
    id: &str,
    value: &serde_json::Value,
    values: &serde_json::Map<String, serde_json::Value>,
) -> String {
    if id == ado_core::START_DATE_FIELD || id == ado_core::TARGET_DATE_FIELD {
        let start = values
            .get(ado_core::START_DATE_FIELD)
            .and_then(serde_json::Value::as_str)
            .and_then(|text| parse_iso_date(Some(text)));
        let target = values
            .get(ado_core::TARGET_DATE_FIELD)
            .and_then(serde_json::Value::as_str)
            .and_then(|text| parse_iso_date(Some(text)));
        if id == ado_core::START_DATE_FIELD {
            return format_date_range(start, target);
        }
        if id == ado_core::TARGET_DATE_FIELD && start.is_none() {
            return format_date_range(None, target);
        }
    }
    value
        .as_str()
        .and_then(|text| parse_iso_date(Some(text)))
        .map(compact_date)
        .unwrap_or_default()
}

pub fn dates_from_range_picker(
    date: PickerDate,
    typed: &str,
    typed_seed: &str,
) -> Option<(Option<jiff::civil::Date>, Option<jiff::civil::Date>)> {
    dates_from_range_picker_in_years(date, typed, typed_seed, None, None)
}

pub fn dates_from_range_picker_in_years(
    date: PickerDate,
    typed: &str,
    typed_seed: &str,
    start_year: Option<i16>,
    target_year: Option<i16>,
) -> Option<(Option<jiff::civil::Date>, Option<jiff::civil::Date>)> {
    let (picker_start_year, picker_target_year) = years_from_picker(date);
    let start_year = picker_start_year.or(start_year).or(picker_target_year);
    let target_year = picker_target_year.or(target_year).or(start_year);
    let typed_changed = typed_dates_changed(typed, typed_seed);
    let parsed = parse_typed_date_range(typed, start_year, target_year);
    if typed_changed && matches!(parsed, TypedDateRange::Invalid) {
        return None;
    }
    if typed_changed && matches!(parsed, TypedDateRange::Empty) {
        return Some((None, None));
    }
    let typed_edit = match parsed {
        TypedDateRange::Parsed(start, target) if typed_changed => Some((start, target)),
        _ => None,
    };
    if typed_edit
        .as_ref()
        .is_some_and(|(start, target)| start.is_some() && target.is_some())
    {
        return typed_edit;
    }
    if typed_changed
        && typed_edit
            .as_ref()
            .is_some_and(|(start, target)| start.is_some() && target.is_none())
        && !typed_is_pending_range_preview(typed)
    {
        return typed_edit;
    }
    if let TypedDateRange::Parsed(None, Some(target)) = parsed
        && (typed_changed || !matches!(date, PickerDate::Range(Some(_), Some(_))))
    {
        return Some((None, Some(target)));
    }
    match date {
        PickerDate::Range(Some(start), Some(end)) => {
            Some((naive_to_jiff(start), naive_to_jiff(end)))
        }
        PickerDate::Range(Some(start), None) => typed_edit.or(Some((naive_to_jiff(start), None))),
        PickerDate::Range(None, Some(end)) => typed_edit.or(Some((None, naive_to_jiff(end)))),
        PickerDate::Single(Some(day)) => typed_edit.or(Some((naive_to_jiff(day), None))),
        PickerDate::Single(None) | PickerDate::Range(None, None) => {
            typed_edit.or_else(
                || match parse_typed_date_range(typed, start_year, target_year) {
                    TypedDateRange::Parsed(start, target) => Some((start, target)),
                    TypedDateRange::Empty | TypedDateRange::Invalid => None,
                },
            )
        }
    }
}

fn years_from_picker(date: PickerDate) -> (Option<i16>, Option<i16>) {
    match date {
        PickerDate::Range(start, end) => (
            start.map(|day| day.year() as i16),
            end.map(|day| day.year() as i16),
        ),
        PickerDate::Single(day) => (day.map(|day| day.year() as i16), None),
    }
}

fn picker_date_from_node(node: &ado_core::WorkItemNode) -> PickerDate {
    let start = parse_iso_date(node.start_date.as_deref()).and_then(jiff_to_naive);
    let target = parse_iso_date(node.target_date.as_deref()).and_then(jiff_to_naive);
    PickerDate::Range(start, target)
}

/// Auto-save from the calendar observer only once both ends of a range (or a
/// single day) are chosen. Start-only first clicks wait for an explicit commit.
pub fn range_picker_is_complete(date: PickerDate) -> bool {
    matches!(
        date,
        PickerDate::Range(Some(_), Some(_)) | PickerDate::Single(Some(_))
    )
}

/// Typed-field preview while the user is still picking the other end.
pub fn pending_range_typed(date: PickerDate) -> Option<String> {
    match date {
        PickerDate::Range(start, end) if start.is_some() || end.is_some() => Some(
            format_date_range(start.and_then(naive_to_jiff), end.and_then(naive_to_jiff)),
        ),
        PickerDate::Single(Some(day)) => Some(format_date_range(naive_to_jiff(day), None)),
        _ => None,
    }
}

fn range_day_flags(selected: PickerDate, day: chrono::NaiveDate) -> (bool, bool, bool) {
    let start = match selected {
        PickerDate::Single(Some(value)) | PickerDate::Range(Some(value), _) => Some(value),
        _ => None,
    };
    let end = match selected {
        PickerDate::Range(_, Some(value)) => Some(value),
        _ => None,
    };
    let is_start = start == Some(day);
    let is_end = end == Some(day) && start != Some(day);
    let in_range = match (start, end) {
        (Some(a), Some(b)) => {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            day > lo && day < hi
        }
        _ => false,
    };
    (is_start, is_end, in_range)
}

fn typed_dates_changed(typed: &str, seed: &str) -> bool {
    normalize_typed_dates(typed) != normalize_typed_dates(seed)
}

fn typed_is_pending_range_preview(typed: &str) -> bool {
    typed.contains('…')
        || normalize_typed_date_separators(typed)
            .split('/')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .nth(1)
            .is_some()
}

fn normalize_typed_dates(typed: &str) -> String {
    split_typed_date_parts(typed).join(" / ")
}

/// Replace `→`, `->`, `to`, and ` - ` so compact ranges share one splitter.
fn normalize_typed_date_separators(typed: &str) -> String {
    let mut text = typed.replace(['→', '–', '—'], "/").replace("->", "/");
    let mut lower = text.to_ascii_lowercase();
    while let Some(index) = lower.find(" to ") {
        text.replace_range(index..index + 4, " / ");
        lower = text.to_ascii_lowercase();
    }
    text.replace(" - ", "/")
}

fn split_typed_date_parts(typed: &str) -> Vec<String> {
    normalize_typed_date_separators(typed)
        .split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty() && *part != "…")
        .map(ToOwned::to_owned)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TypedDateRange {
    Empty,
    Parsed(Option<jiff::civil::Date>, Option<jiff::civil::Date>),
    Invalid,
}

fn srgb_luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn ink_for_luminance(lum: f32) -> gpui_kit::Hsla {
    if lum > 0.45 {
        gpui_kit::hsla(0., 0., 0.05, 1.)
    } else {
        gpui_kit::hsla(0., 0., 0.98, 1.)
    }
}

fn inner_bar_label_color_darkened(color: u32, darken: f32) -> gpui_kit::Hsla {
    let scale = (1.0 - darken).clamp(0.0, 1.0);
    let r = ((color >> 16) & 0xff) as f32 / 255.0 * scale;
    let g = ((color >> 8) & 0xff) as f32 / 255.0 * scale;
    let b = (color & 0xff) as f32 / 255.0 * scale;
    ink_for_luminance(srgb_luminance(r, g, b))
}

fn painted_bar_title_color(
    bar: &crate::gantt::BarLayout,
    canvas: gpui_kit::Hsla,
) -> gpui_kit::Hsla {
    // Open-ended / summary bars are a wash over the grid. Measure that
    // painted colour, not the saturated type swatch, so orange Cart UI
    // stays dark-ink on a light Day/Week wash.
    if bar.open_ended || bar.summary {
        let alpha = if bar.open_ended { 0.22 } else { 0.55 };
        let painted = canvas.blend(rgb(bar.color).opacity(alpha).into());
        let rgb = painted.to_rgb();
        ink_for_luminance(srgb_luminance(rgb.r, rgb.g, rgb.b))
    } else {
        let darken = if bar.progress.unwrap_or(0.0) > 0.0 {
            0.22
        } else {
            0.0
        };
        inner_bar_label_color_darkened(bar.color, darken)
    }
}

fn strip_target_only_prefix(typed: &str) -> (bool, String) {
    let text = typed.trim();
    for prefix in ['◆', '◇', '♦'] {
        if let Some(rest) = text.strip_prefix(prefix) {
            return (true, rest.trim().to_string());
        }
    }
    (false, text.to_string())
}

fn parse_typed_date_range(
    typed: &str,
    start_year: Option<i16>,
    target_year: Option<i16>,
) -> TypedDateRange {
    let (target_only, text) = strip_target_only_prefix(typed);
    if text.is_empty() {
        return TypedDateRange::Empty;
    }
    let parts = split_typed_date_parts(&text);
    match parts.as_slice() {
        [] => TypedDateRange::Empty,
        [one] if target_only => match parse_user_date(Some(one), target_year.or(start_year)) {
            Some(target) => TypedDateRange::Parsed(None, Some(target)),
            None => TypedDateRange::Invalid,
        },
        [start] => match parse_user_date(Some(start), start_year) {
            Some(start) => TypedDateRange::Parsed(Some(start), None),
            None => TypedDateRange::Invalid,
        },
        [start, target] => {
            match (
                parse_user_date(Some(start), start_year),
                parse_user_date(Some(target), target_year.or(start_year)),
            ) {
                (Some(start), Some(target)) => TypedDateRange::Parsed(Some(start), Some(target)),
                _ => TypedDateRange::Invalid,
            }
        }
        _ => TypedDateRange::Invalid,
    }
}

fn dates_parse_hint(start: Option<jiff::civil::Date>, target: Option<jiff::civil::Date>) -> String {
    let (a, b) = match (start, target) {
        (Some(start), Some(target)) => (compact_date(start), compact_date(target)),
        (Some(start), None) => {
            let target = start.saturating_add(6.days());
            (compact_date(start), compact_date(target))
        }
        (None, Some(target)) => {
            let start = target.saturating_sub(6.days());
            (compact_date(start), compact_date(target))
        }
        (None, None) => ("Jan 6".into(), "Jan 12".into()),
    };
    format!("Could not parse. Use {a} → {b} or a start date.")
}

/// `None` when the Dates field is empty or a recognised range.
pub fn typed_dates_error(typed: &str) -> Option<String> {
    typed_dates_error_in(typed, None, None)
}

/// Parse-hint examples prefer the item's own dates; otherwise a fixed range.
pub fn typed_dates_error_in(
    typed: &str,
    start: Option<jiff::civil::Date>,
    target: Option<jiff::civil::Date>,
) -> Option<String> {
    match parse_typed_date_range(typed, None, None) {
        TypedDateRange::Invalid => Some(dates_parse_hint(start, target)),
        TypedDateRange::Empty | TypedDateRange::Parsed(_, _) => None,
    }
}

pub fn looks_like_pat(value: &str) -> bool {
    let text = value.trim();
    text.len() >= 20
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '=' | '-' | '_'))
        && text.chars().any(|ch| ch.is_ascii_digit())
        && text.chars().any(|ch| ch.is_ascii_alphabetic())
}

impl Focusable for PlannerView {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.form_focus.clone()
    }
}

impl Render for PlannerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(id) = self.pending_inline_title.take()
            && self.state.nodes.iter().any(|node| node.id == id)
        {
            self.open_inline_title(id, window, cx);
            self.pending_focus_planner = false;
        }
        if self.pending_focus_planner {
            self.pending_focus_planner = false;
            self.focus_planner(window, cx);
        }
        if let Some(preview) = self.pending_typed_preview.take() {
            self.inline_input.update(cx, |input, cx| {
                input.set_value(&preview, window, cx);
            });
        }
        let viewport_h = f32::from(window.viewport_size().height);
        let viewport_w = f32::from(window.viewport_size().width);
        self.state.gantt.viewport_h = (viewport_h - 200.0).max(120.0);
        self.state.gantt.viewport_w = viewport_w;
        self.state.gantt.clamp_tree_width();
        self.state.gantt.timeline_origin_x = self
            .timeline_body_origin_x
            .get()
            .unwrap_or(self.state.gantt.tree_width + SPLITTER_WIDTH);
        self.state.gantt.timeline_origin_y = (viewport_h - self.state.gantt.viewport_h).max(0.0);
        if self.state.toasts.items.is_empty() {
            self.toast_hover_depth = 0;
            self.state.toasts.paused = false;
        }
        if !self.state.toasts.paused {
            self.state.toasts.expire();
        }
        self.ensure_form_inputs(window, cx);
        self.flush_pending_form_dates(window, cx);
        window.set_window_title(&self.window_title());
        if let Some(form_id) = self.state.form.as_ref().map(|form| form.model.id) {
            if self.form_title_focused_for != Some(form_id) {
                // Focus the dialog, not Title — auto-focus made accidental edits too easy.
                window.focus(&self.form_focus, cx);
                self.form_title_focused_for = Some(form_id);
            }
        } else {
            self.form_title_focused_for = None;
        }
        self.schedule_toast_expiry(cx);
        div()
            .id("ado-planner")
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .capture_any_mouse_up(cx.listener(|this, event: &MouseUpEvent, window, cx| {
                if this.toast_pointer {
                    this.toast_pointer = false;
                    if this.state.gantt.gesture.is_none()
                        && this.state.gantt.schedule_drag.is_none()
                    {
                        this.state.cancel_gesture();
                        this.state.gantt.schedule_drag = None;
                        this.saw_pressed_drag = false;
                        return;
                    }
                }
                let x: f32 = event.position.x.into();
                let y: f32 = event.position.y.into();
                if this.state.gantt.gesture.is_some() {
                    this.state.gantt.clear_hover();
                    this.release_bar_gesture(x, y, window, cx);
                } else if !this.state.gantt.pointer_in_rows(x, y) {
                    this.clear_canvas_hover(cx);
                }
                if this.state.gantt.splitter_drag.take().is_some() {
                    this.state.persist_tree_width();
                    cx.notify();
                }
                if let Some(id) = this.state.gantt.schedule_drag.take() {
                    let origin = this.schedule_origin_x.take();
                    this.schedule_live_x = None;
                    if this.state.gantt.context_menu.is_some() {
                        this.saw_pressed_drag = false;
                        cx.notify();
                        return;
                    }
                    let x: f32 = event.position.x.into();
                    let dragged = this.saw_pressed_drag
                        || origin.is_some_and(|origin| (x - origin).abs() >= 6.0);
                    this.saw_pressed_drag = false;
                    let _ = this.timeline_local_x(x);
                    if dragged && x > this.state.gantt.timeline_origin_x {
                        let origin_x = origin.unwrap_or(x);
                        let (start, target) =
                            this.state.gantt.schedule_span_from_window_xs(origin_x, x);
                        this.spawn_state(cx, move |mut state| async move {
                            let result = state.schedule_span(id, start, target).await;
                            (state, result)
                        });
                    }
                    cx.notify();
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if this.state.gantt.hover_locked {
                    this.state.gantt.hover_locked = false;
                }
                let x: f32 = event.position.x.into();
                let y: f32 = event.position.y.into();
                this.clear_pinned_tooltip_if_pointer_left(x, y, cx);
                if let Some((origin_x, origin_w)) = this.state.gantt.splitter_drag {
                    this.state.gantt.set_tree_width_for_pane(
                        origin_w + x - origin_x,
                        f32::from(window.viewport_size().width),
                    );
                    cx.notify();
                    return;
                }
                if this.state.gantt.gesture.is_some() {
                    this.on_bar_drag_move(event, window, cx);
                    return;
                }
                if this.state.gantt.schedule_drag.is_some() {
                    this.schedule_live_x = Some(x);
                    if event.dragging() {
                        this.saw_pressed_drag = true;
                    }
                    cx.notify();
                }
                if !this.state.gantt.pointer_in_rows(x, y) {
                    this.clear_canvas_hover(cx);
                }
            }))
            .track_focus(&self.form_focus)
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if this.form_loading.is_some() && key != "escape" {
                    cx.stop_propagation();
                    return;
                }
                if key == "f2"
                    && !this.details_overlay()
                    && this.inline_edit.is_none()
                    && let Some(id) = this.state.gantt.selected_id
                {
                    this.open_inline_title(id, window, cx);
                    cx.stop_propagation();
                    return;
                }
                if key == "escape"
                    && this.details_overlay()
                    && this.form_date_picker_focused(window, cx)
                {
                    return;
                }
                let consume_enter = this.inline_edit.is_some()
                    || this.inline_enter_guard
                    || this.pending_delete.is_some()
                    || (this.state.gantt.selected_id.is_some()
                        && !this.details_overlay()
                        && this.pending_delete.is_none());
                this.on_key(event, cx);
                if key == "escape" || (key == "enter" && consume_enter) {
                    cx.stop_propagation();
                }
            }))
            .when(self.state.session_loading, |el| {
                el.child(
                    v_flex()
                        .id("session-loading")
                        .size_full()
                        .items_center()
                        .justify_center()
                        .child(Label::new("Starting Session…")),
                )
            })
            .when(
                !self.state.session_loading && !self.state.session_info.signed_in,
                |el| el.child(self.render_sign_in(cx)),
            )
            .when(
                !self.state.session_loading && self.state.session_info.signed_in,
                |el| {
                    el.child(
                        v_flex()
                            .size_full()
                            .child(self.render_header(cx))
                            .child(self.render_updater(cx))
                            .child(
                                div()
                                    .flex_1()
                                    .relative()
                                    .child(self.render_gantt(cx))
                                    .child(self.render_error_banner(cx)),
                            )
                            .when(self.state.gantt.context_menu.is_some(), |el| {
                                el.child(
                                    div()
                                        .id("popup-dismiss")
                                        .absolute()
                                        .inset_0()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.state.gantt.schedule_drag = None;
                                                this.state.gantt.context_menu = None;
                                                cx.notify();
                                            }),
                                        )
                                        .on_mouse_up(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        }),
                                )
                            })
                            .child(self.render_context_menu(cx))
                            .child(self.render_dates_popover(cx))
                            .child(self.render_form(cx))
                            .child(self.render_delete_confirm(cx))
                            .child(self.render_toasts(cx)),
                    )
                },
            )
    }
}

pub fn type_abbrev(ty: &str) -> String {
    match ty {
        "Product Backlog Item" => "PBI".into(),
        "User Story" => "US".into(),
        "Test Case" => "TC".into(),
        "Test Suite" => "TS".into(),
        "Test Plan" => "TP".into(),
        other => other.to_string(),
    }
}

pub fn start_app(data_dir: Option<PathBuf>, ado_base: Option<String>, vssps_base: Option<String>) {
    let paths = AppPaths::new(data_dir.unwrap_or_else(AppPaths::default_dir));
    let flavor = crate::prefs::AppPrefs::load(&paths.prefs_file()).flavor;
    let session = ado_session::create_session(&paths.data_dir);
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        apply_flavor(cx, flavor);
        let paths = paths.clone();
        let session = session;
        let ado_base = ado_base.clone();
        let vssps_base = vssps_base.clone();
        gpui_kit::open_window(
            gpui_kit::WindowOptions {
                titlebar: Some(gpui_kit::TitlebarOptions {
                    title: Some("ADO Planner".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            cx,
            move |window, cx| {
                cx.new(move |cx| PlannerView::new(window, cx, session, paths, ado_base, vssps_base))
            },
        )
        .expect("failed to open window");
    });
}

#[cfg(test)]
mod bar_label_tests {
    use super::*;
    use crate::gantt::BarLayout;
    use ado_core::GanttBarKind;

    #[test]
    fn off_screen_markers_do_not_pin_a_label_to_the_right_edge() {
        let bar = BarLayout {
            id: 1,
            kind: GanttBarKind::Unscheduled,
            x: -80.0,
            width: 14.0,
            y: 0.0,
            summary: false,
            milestone: false,
            label: String::new(),
            title: "Unscheduled item".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: true,
            open_ended: false,
        };
        assert!(
            PlannerView::bar_external_label_frame(&bar, 800.0, false, None).is_none(),
            "an off-screen Today diamond must not float a title at the right edge"
        );
    }

    #[test]
    fn clipped_left_bars_place_titles_outside_to_the_right() {
        let bar = BarLayout {
            id: 1,
            kind: GanttBarKind::Scheduled,
            x: -120.0,
            width: 280.0,
            y: 0.0,
            summary: false,
            milestone: false,
            label: String::new(),
            title: "Checkout rewrite".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        assert!(
            !PlannerView::bar_title_fits_inside(&bar),
            "a title wider than the visible fragment must sit outside instead of clipping"
        );
        let (label_x, _) = PlannerView::bar_external_label_frame(&bar, 800.0, false, None)
            .expect("the title must have an outside frame");
        assert!(
            label_x >= 0.0 && label_x >= bar.x + bar.width - 1.0,
            "must not float the title over the grid to the left of a clipped bar, label_x={label_x}"
        );
        let cart = BarLayout {
            id: 1002,
            x: -80.0,
            width: 156.0,
            title: "Cart".into(),
            ..bar
        };
        assert!(
            !PlannerView::bar_title_fits_inside(&cart),
            "a clipped Quarter Cart sliver must not pin its title on top of Checkout rewrite"
        );
    }

    #[test]
    fn wide_leaf_and_summary_bars_place_titles_the_same_way() {
        let leaf = BarLayout {
            id: 1,
            kind: GanttBarKind::Scheduled,
            x: 20.0,
            width: 200.0,
            y: 0.0,
            summary: false,
            milestone: false,
            label: String::new(),
            title: "Cart".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        let summary = BarLayout {
            id: 2,
            summary: true,
            title: "Growth experiments".into(),
            ..leaf.clone()
        };
        assert_eq!(
            PlannerView::bar_title_fits_inside(&leaf),
            PlannerView::bar_title_fits_inside(&summary),
            "title length and summary vs leaf must not disagree on inside vs outside"
        );
        assert!(
            PlannerView::bar_title_fits_inside(&leaf),
            "a wide bar must keep the title inside"
        );
    }

    #[test]
    fn long_title_on_a_narrow_bar_paints_outside() {
        let bar = BarLayout {
            id: 1,
            kind: GanttBarKind::Scheduled,
            x: 20.0,
            width: 80.0,
            y: 0.0,
            summary: false,
            milestone: false,
            label: String::new(),
            title: "Unparented spike".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        assert!(
            !PlannerView::bar_title_fits_inside(&bar),
            "a title wider than the bar must paint outside instead of truncating"
        );
        assert!(
            PlannerView::bar_external_label_frame(&bar, 800.0, false, None).is_some(),
            "the outside label must have a frame on the timeline"
        );
    }

    #[test]
    fn year_view_cart_label_does_not_cross_today() {
        // Year zoom-out: Cart is a short sliver (Jul–Aug) whose outside
        // title would otherwise run across Today (Nov).
        let bar = BarLayout {
            id: 1002,
            kind: GanttBarKind::Scheduled,
            x: 110.0,
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
        };
        let today_x = 184.0;
        let (label_x, max_w) =
            PlannerView::bar_external_label_frame(&bar, 800.0, true, Some(today_x))
                .expect("Cart is too short for an inside title on Year");
        assert!(
            label_x + max_w <= today_x - 4.0 || label_x >= today_x + 4.0,
            "Year Cart label must not sit on the Today line, label={label_x}..{} today={today_x}",
            label_x + max_w
        );
        assert!(
            max_w + 0.1 >= "Cart".chars().count() as f32 * 8.0,
            "Year Cart must keep a full-width outside label, not clip to a letter, max_w={max_w}"
        );
    }

    #[test]
    fn outside_label_stays_on_the_right_after_a_right_edge_grow() {
        let bar = BarLayout {
            id: 1014,
            kind: GanttBarKind::Scheduled,
            x: 620.0,
            width: 140.0,
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
        };
        assert!(
            !PlannerView::bar_title_fits_inside(&bar),
            "precondition: #1014 title is wider than the bar"
        );
        let frame = PlannerView::bar_external_label_frame(&bar, 800.0, true, None);
        if let Some((label_x, max_w)) = frame {
            assert!(
                label_x >= bar.x + bar.width - 1.0,
                "a right-edge resize must not flip the title to the left of the bar, label_x={label_x} bar={}..{}",
                bar.x,
                bar.x + bar.width
            );
            assert!(
                max_w >= 40.0,
                "an outside label must stay readable, not clip to a letter, max_w={max_w}"
            );
        }
    }

    #[test]
    fn green_and_orange_bars_use_dark_ink() {
        for (name, color) in [("User Story", 0xa6e3a1), ("Task", 0xfab387)] {
            let ink = inner_bar_label_color_darkened(color, 0.0);
            assert!(
                ink.l < 0.2,
                "{name} bars must use dark title ink, l={}",
                ink.l
            );
            let progressed = inner_bar_label_color_darkened(color, 0.22);
            assert!(
                progressed.l < 0.2,
                "{name} bars with a progress overlay must stay dark-ink, l={}",
                progressed.l
            );
        }
        let canvas = gpui_kit::hsla(0., 0., 0.96, 1.);
        let spike = BarLayout {
            id: 1012,
            kind: GanttBarKind::Scheduled,
            x: 20.0,
            width: 200.0,
            y: 0.0,
            summary: false,
            milestone: false,
            label: String::new(),
            title: "Unparented spike".into(),
            color: 0xfab387,
            progress: Some(0.4),
            predecessors: Vec::new(),
            marker: false,
            open_ended: false,
        };
        let persist = BarLayout {
            id: 1003,
            title: "Persist cart".into(),
            color: 0xa6e3a1,
            progress: None,
            ..spike.clone()
        };
        assert!(
            painted_bar_title_color(&persist, canvas).l < 0.2,
            "Week/Day Persist cart must not paint white on green"
        );
        assert!(
            painted_bar_title_color(&spike, canvas).l < 0.2,
            "Week/Day Unparented spike must not paint white on orange"
        );
        let cart_ui = BarLayout {
            open_ended: true,
            progress: None,
            ..spike
        };
        assert!(
            painted_bar_title_color(&cart_ui, canvas).l < 0.2,
            "an open-ended orange wash must keep dark ink"
        );
    }

    #[test]
    fn day_view_cart_ui_keeps_a_readable_title() {
        let bar = BarLayout {
            id: 1005,
            kind: GanttBarKind::Scheduled,
            x: 400.0,
            width: 336.0,
            y: 80.0,
            summary: false,
            milestone: false,
            label: String::new(),
            title: "Cart UI".into(),
            color: 0,
            progress: None,
            predecessors: Vec::new(),
            marker: false,
            open_ended: true,
        };
        assert!(
            PlannerView::bar_title_fits_inside(&bar),
            "a Day-view start-only Cart UI bar is wide enough for an inside title"
        );
        assert!(
            PlannerView::bar_external_label_frame(&bar, 800.0, true, None).is_none(),
            "must not float a fragment at the viewport edge when the title fits inside"
        );
        let at_clip = BarLayout {
            x: 760.0,
            width: 48.0,
            ..bar
        };
        let frame = PlannerView::bar_external_label_frame(&at_clip, 800.0, true, None);
        if let Some((label_x, max_w)) = frame {
            assert!(
                max_w >= 40.0,
                "Day-view Cart UI must not crush to a letter at the clip, label_x={label_x} max_w={max_w}"
            );
        }
    }
}
