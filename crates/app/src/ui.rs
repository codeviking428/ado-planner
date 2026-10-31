use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use ado_core::{
    AuthMode, FLAVORS, FormControlKind, color_for_type, dates_column_text, is_unscheduled,
    iso_date_only, parse_iso_date,
};
use ado_session::PlannerSession;
use chrono::Datelike;
use gpui_kit::base::{Popup, Selectable};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable;
use gpui_kit::component::Sizable;
use gpui_kit::component::Size;
use gpui_kit::component::avatar::Avatar;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::calendar::{Calendar, CalendarEvent, CalendarState, Date as PickerDate};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::date_picker::{DatePicker, DatePickerState};
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
    AnyElement, App, ClipboardItem, Context, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, KeystrokeEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Render, Role, ScrollEvent, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, actions, div, px, rgb,
};

use crate::error::AppError;
use crate::flavor_theme::apply_flavor;
use crate::form::ado_edit_url_on;
use crate::gantt::{
    ASSIGNEE_COL_WIDTH, DATES_COL_WIDTH, DependencyArrow, GanttScale, GestureKind, HANDLE_WIDTH,
    ID_COL_WIDTH, LEAF_BAR_H, LINK_HANDLE, LINK_HANDLE_HIT, MILESTONE_SIZE, MIN_TIMELINE_WIDTH,
    NUDGE_IDLE_MS, NudgeAxis, PLACEHOLDER_BAR_H, ROW_HEIGHT, SUMMARY_BAR_H, TIMELINE_HEADER_H,
    TYPE_COL_WIDTH,
};
use crate::paths::AppPaths;
use crate::state::PlannerState;
use crate::toasts::ToastKind;

actions!(
    ado_planner,
    [Quit, NudgeLeft, NudgeRight, EscapeCancel, OpenSelected]
);

#[derive(Clone, Copy, PartialEq, Eq)]
enum InlineEdit {
    Title(i64),
    Start(i64),
    Target(i64),
    Dates(i64),
    Assignee(i64),
}

pub struct PlannerView {
    pub state: PlannerState,
    org_input: Entity<InputState>,
    pat_input: Entity<InputState>,
    filter_query: Entity<InputState>,
    scope_query: Entity<InputState>,
    inline_input: Entity<InputState>,
    form_inputs: HashMap<String, Entity<InputState>>,
    form_dates: HashMap<String, Entity<DatePickerState>>,
    form_inputs_for: Option<i64>,
    form_title_focused_for: Option<i64>,
    form_focus: FocusHandle,
    open_filter: Option<&'static str>,
    open_scope: Option<&'static str>,
    inline_edit: Option<InlineEdit>,
    inline_dates: Option<Entity<CalendarState>>,
    dates_anchor: Option<(f32, f32)>,
    banner_dismissed: bool,
    io_busy: bool,
    update_checked: bool,
    toast_expiry_armed: bool,
    loading_shown_at: Option<std::time::Instant>,
    _subscriptions: Vec<Subscription>,
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
                let overlay = this.state.form.is_some()
                    || this.state.gantt.context_menu.is_some()
                    || this.popup_open()
                    || this.inline_edit.is_some();
                if key == "escape" && overlay {
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
                let nav = matches!(key, "up" | "down" | "pageup" | "pagedown" | "home" | "end");
                if undo || redo || delete_link {
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
            form_inputs_for: None,
            form_title_focused_for: None,
            form_focus: cx.focus_handle(),
            open_filter: None,
            open_scope: None,
            inline_edit: None,
            inline_dates: None,
            dates_anchor: None,
            banner_dismissed: false,
            io_busy: false,
            update_checked: false,
            toast_expiry_armed: false,
            loading_shown_at: None,
            _subscriptions: vec![keystrokes],
        };
        this.bind_sign_in_events(window, cx);
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
        view
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

    pub fn form_input_value(&self, field: &str, cx: &App) -> Option<String> {
        self.form_inputs
            .get(field)
            .map(|input| input.read(cx).value().to_string())
    }

    fn form_field_focused(&self, window: &Window, cx: &App) -> bool {
        self.form_inputs
            .values()
            .any(|input| input.read(cx).focus_handle(cx).is_focused(window))
            || self
                .form_dates
                .values()
                .any(|picker| picker.read(cx).focus_handle(cx).is_focused(window))
            || self
                .inline_input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
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
    }

    fn spawn_state<F, Fut>(&mut self, cx: &mut Context<Self>, work: F)
    where
        F: FnOnce(PlannerState) -> Fut + Send + 'static,
        Fut: Future<Output = (PlannerState, Result<(), AppError>)> + Send + 'static,
    {
        if self.io_busy {
            return;
        }
        self.io_busy = true;
        let runtime = self.state.runtime.clone();
        let working = self.state.clone();
        let rx = runtime.spawn_result(async move { work(working).await });
        cx.spawn(async move |this, cx| {
            let outcome = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.io_busy = false;
                match outcome {
                    Ok((working, result)) => {
                        let shown_at = this.loading_shown_at;
                        this.state = working;
                        if let Some(at) = shown_at
                            && !this.state.hierarchy_loading
                        {
                            let remain = Duration::from_millis(280).saturating_sub(at.elapsed());
                            if remain > Duration::ZERO {
                                this.state.hierarchy_loading = true;
                                cx.spawn(async move |this, cx| {
                                    cx.background_executor().timer(remain).await;
                                    if let Some(this) = this.upgrade() {
                                        this.update(cx, |this, cx| {
                                            this.state.hierarchy_loading = false;
                                            this.loading_shown_at = None;
                                            cx.notify();
                                        });
                                    }
                                })
                                .detach();
                            } else {
                                this.loading_shown_at = None;
                            }
                        }
                        if let Err(err) = result {
                            tracing::warn!("planner action failed: {err}");
                        }
                    }
                    Err(_) => tracing::warn!("planner action cancelled"),
                }
                if !this.update_checked && this.state.session_info.signed_in {
                    this.update_checked = true;
                    this.spawn_update_check(cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
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
                        this.state = working;
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
        self.inline_edit = None;
        self.inline_dates = None;
        self.dates_anchor = None;
        self.state.gantt.context_menu = None;
    }

    fn dismiss_all_overlays(&mut self) {
        self.dismiss_overlays();
        self.state.close_form();
        self.state.cancel_gesture();
        self.state.gantt.schedule_drag = None;
        self.state.gantt.clear_hover();
        self.dates_anchor = None;
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
        if self.state.gantt.hovered_id != id {
            self.state.gantt.hovered_id = id;
            cx.notify();
        }
    }

    fn begin_inline(
        &mut self,
        edit: InlineEdit,
        initial: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let InlineEdit::Dates(id) = edit {
            let node = self.state.nodes.iter().find(|node| node.id == id);
            let start = node.and_then(|node| {
                parse_iso_date(node.start_date.as_deref()).and_then(jiff_to_naive)
            });
            let target = node.and_then(|node| {
                parse_iso_date(node.target_date.as_deref()).and_then(jiff_to_naive)
            });
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
            cx.subscribe(&picker, move |this, picker, event: &CalendarEvent, cx| {
                if let CalendarEvent::Selected(value) = event
                    && let (Some(start), Some(end)) = (value.start(), value.end())
                    && let (Some(start), Some(target)) = (naive_to_jiff(start), naive_to_jiff(end))
                {
                    this.inline_edit = None;
                    this.inline_dates = None;
                    this.spawn_state(cx, move |mut state| async move {
                        let result = state.save_dates(item_id, start, target).await;
                        (state, result)
                    });
                }
                let _ = picker;
                cx.notify();
            })
            .detach();
            self.inline_dates = Some(picker);
        } else {
            self.inline_dates = None;
        }
        self.inline_edit = Some(edit);
        self.inline_input.update(cx, |input, cx| {
            input.set_value(&initial, window, cx);
            input.select_all(window, cx);
        });
        if !matches!(edit, InlineEdit::Dates(_)) {
            Self::focus_input(&self.inline_input, window, cx);
        }
        cx.notify();
    }

    fn commit_inline(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.inline_edit.take() else {
            return;
        };
        let value = self.inline_input.read(cx).value().to_string();
        match edit {
            InlineEdit::Title(id) => {
                self.spawn_state(cx, move |mut state| async move {
                    let result = state.save_title(id, value).await;
                    (state, result)
                });
            }
            InlineEdit::Dates(id) => {
                if let Some(picker) = self.inline_dates.take() {
                    let date = picker.read(cx).date();
                    if let PickerDate::Range(Some(start), Some(end)) = date
                        && let (Some(start), Some(target)) =
                            (naive_to_jiff(start), naive_to_jiff(end))
                    {
                        self.spawn_state(cx, move |mut state| async move {
                            let result = state.save_dates(id, start, target).await;
                            (state, result)
                        });
                    }
                }
            }
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
        self.state.gantt.context_menu = None;
        self.open_filter = id;
    }

    pub fn popup_is_open(&self) -> bool {
        self.popup_open()
    }

    fn popup_open(&self) -> bool {
        self.open_filter.is_some() || self.open_scope.is_some()
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
        if self.banner_dismissed {
            return div().into_any_element();
        }
        let Some(message) = self.state.team_settings_error.as_ref() else {
            return div().into_any_element();
        };
        h_flex()
            .id("team-settings-error")
            .debug_selector(|| "team-settings-error".into())
            .w_full()
            .max_w(px(720.))
            .flex_shrink_0()
            .items_start()
            .px_3()
            .py_1()
            .gap_2()
            .mx_3()
            .mt_2()
            .rounded_md()
            .bg(cx.theme().muted)
            .text_color(cx.theme().foreground)
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
            .top(px(72.))
            .right_4()
            .w(px(360.))
            .gap_2()
            .children(self.state.toasts.items.iter().map(|toast| {
                let id = toast.id;
                let copyable = toast.copyable;
                let undo = toast.undo.clone();
                let message = toast.message.clone();
                let kind = toast.kind;
                h_flex()
                    .id(("toast", id))
                    .debug_selector(|| "planner-toast".into())
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
                            .flex_1()
                            .min_w(px(0.))
                            .text_sm()
                            .child(message.clone()),
                    )
                    .when(copyable, |el| {
                        el.child(Button::new(("copy-toast", id)).label("Copy").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.state.copy_toast(id);
                                if let Some(text) = this.state.copied.clone() {
                                    this.write_clipboard(cx, text);
                                }
                                cx.notify();
                            }),
                        ))
                    })
                    .when_some(undo.clone(), |el, _action| {
                        el.child(Button::new(("undo-toast", id)).label("Undo").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.undo_last(cx);
                            }),
                        ))
                    })
                    .child(
                        Button::new(("dismiss-toast", id))
                            .label("×")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.state.toasts.dismiss(id);
                                cx.notify();
                            })),
                    )
            }))
    }

    fn undo_last(&mut self, cx: &mut Context<Self>) {
        if self.state.history.undo.is_empty() {
            return;
        }
        self.spawn_state(cx, |mut state| async move {
            let result = state.undo().await;
            (state, result)
        });
    }

    fn redo_last(&mut self, cx: &mut Context<Self>) {
        if self.state.history.redo.is_empty() {
            return;
        }
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
        if mods.control || mods.platform {
            // After invert, wheel-up (negative dy) zooms in, matching the + button.
            let factor = if dy < 0.0 { 1.25 } else { 0.8 };
            let pointer = f32::from(event.position.x) - self.state.gantt.timeline_origin_x;
            self.state.gantt.zoom_at(factor, pointer, 0.0);
        } else {
            self.state.gantt.apply_wheel(dx, dy, mods.shift);
        }
        let rows = self.state.visible_rows().len();
        self.state
            .gantt
            .clamp_scroll(rows, self.state.gantt.viewport_h);
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
            return;
        }
        if self.state.gantt.hovered_id.is_some() || self.state.gantt.hovered_link.is_some() {
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
                    .flex_shrink_0()
                    .child(self.render_filters(cx)),
            )
            .child(self.render_flavor(cx))
            .child(
                div()
                    .id("header-log-out")
                    .debug_selector(|| "header-log-out".into())
                    .child(
                        Button::new("log-out")
                            .ghost()
                            .compact()
                            .label("Log out")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.spawn_state(cx, |mut state| async move {
                                    let result = state.logout().await;
                                    (state, result)
                                });
                            })),
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
        let trigger = Button::new(id)
            .label(if value.is_empty() {
                format!("Select {label}")
            } else {
                value
            })
            .disabled(disabled)
            .on_click(cx.listener(move |this, event, window, cx| {
                this.open_filter = None;
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
            }));
        let option_buttons: Vec<_> = if open {
            options
                .iter()
                .filter(|(val, lab)| {
                    search.is_empty()
                        || lab.to_lowercase().contains(&search.to_lowercase())
                        || val.to_lowercase().contains(&search.to_lowercase())
                })
                .map(|(val, lab)| {
                    let val_click = val.clone();
                    Button::new(SharedString::from(format!("{id}-{val}")))
                        .label(lab.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.choose_scope(id, val_click.clone(), cx);
                        }))
                })
                .collect()
        } else {
            Vec::new()
        };
        let empty_label = match id {
            "assignee" => "No team members loaded",
            _ => "No options",
        };
        let mut popup = Popup::new(format!("{id}-popup"), trigger).offset(px(4.));
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
            "team" => {
                self.state.hierarchy_loading = true;
                self.loading_shown_at = Some(std::time::Instant::now());
                self.banner_dismissed = false;
                self.spawn_state(cx, move |mut state| async move {
                    let result = state.set_team(value).await;
                    (state, result)
                });
            }
            "iteration" => self.state.set_iteration(value),
            "assignee" => self.state.set_assignee(value),
            _ => {}
        }
        cx.notify();
    }

    fn render_filters(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
            ))
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
        let caption = if id == "root-filter" {
            let selected = items.len().saturating_sub(hidden);
            if hidden > 0 {
                format!("{label} ({selected})")
            } else {
                label.to_string()
            }
        } else if hidden > 0 {
            format!("{label} ({hidden})")
        } else {
            label.to_string()
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
        let mut popup = Popup::new(format!("{id}-popup"), trigger).offset(px(4.));
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
        h_flex()
            .id("flavor")
            .gap_1()
            .children(FLAVORS.iter().map(|flavor| {
                let selected = self.state.flavor() == *flavor;
                let flavor = *flavor;
                Button::new(flavor.as_str())
                    .label(flavor.label())
                    .selected(selected)
                    .when(selected, |btn| btn.primary())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.set_flavor(flavor);
                        apply_flavor(cx, flavor);
                        cx.notify();
                    }))
            }))
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
        let rows = self.state.gantt.flatten(&visible);
        let viewport_h = self.state.gantt.viewport_h.max(120.0);
        let content_h = (viewport_h - TIMELINE_HEADER_H).max(80.0);
        let window = self.state.gantt.visible_window(&rows, content_h).to_vec();
        let bars = self
            .state
            .gantt
            .layout_bars(&visible, &rows, &self.state.iterations, content_h);
        v_flex()
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
                    .child(self.render_tree(&visible, &window, cx))
                    .child(
                        div()
                            .id("gantt-splitter")
                            .w(px(6.))
                            .flex_shrink_0()
                            .h_full()
                            .bg(cx.theme().border)
                            .cursor_col_resize()
                            .hover(|el| el.bg(cx.theme().primary))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                    if event.click_count >= 2 {
                                        this.state.gantt.reset_tree_width();
                                        this.state.gantt.splitter_drag = None;
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
                    .child(self.render_timeline(&window, &bars, cx))
                    .when(empty_canvas, |el| {
                        el.child(
                            div()
                                .absolute()
                                .inset_0()
                                .mt(px(TIMELINE_HEADER_H))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    v_flex()
                                        .bg(cx.theme().popover)
                                        .rounded_md()
                                        .p_4()
                                        .shadow_md()
                                        .child(self.render_empty_gantt(cx)),
                                ),
                        )
                    }),
            )
            .when(self.state.hierarchy_loading, |el| {
                el.child(self.render_gantt_loading(cx))
            })
            .into_any_element()
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
            .flex_1()
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
            .child(
                div()
                    .id("gantt-today-btn")
                    .debug_selector(|| "gantt-today-btn".into())
                    .child(
                        Button::new("today")
                            .primary()
                            .label("Today")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.state.gantt.go_today(jiff::Zoned::now().date());
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
                            .tooltip("Create a work item on this team.")
                            .on_click(cx.listener(|this, _, _, cx| {
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
                    .tooltip("Fit every bar into the visible timeline.")
                    .tooltip_placement(Placement::Bottom)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let nodes = this.state.visible_nodes();
                        let width = this.state.gantt.timeline_width.max(MIN_TIMELINE_WIDTH);
                        this.state
                            .gantt
                            .auto_fit(&nodes, &this.state.iterations, width);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("prev-period")
                    .ghost()
                    .compact()
                    .label("‹")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.gantt.step_period(-1);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("next-period")
                    .ghost()
                    .compact()
                    .label("›")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.gantt.step_period(1);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("gantt-range-title")
                    .debug_selector(|| "gantt-range-title".into())
                    .w(px(168.))
                    .flex_shrink_0()
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
                        let label = if selected {
                            scale.button_label(self.state.gantt.zoom)
                        } else {
                            scale.label().to_string()
                        };
                        Button::new(scale.label())
                            .label(label)
                            .compact()
                            .selected(selected)
                            .when(selected, |btn| btn.primary())
                            .when(!selected, |btn| btn.ghost())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.dismiss_overlays();
                                this.state.gantt.scale = scale;
                                cx.notify();
                            }))
                    })),
            )
            .child(
                div()
                    .id("gantt-zoom-out")
                    .debug_selector(|| "gantt-zoom-out".into())
                    .child(
                        Button::new("zoom-out")
                            .ghost()
                            .compact()
                            .label("−")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let pointer = this.state.gantt.timeline_width * 0.5;
                                this.state.gantt.zoom_at(0.8, pointer, 0.0);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("gantt-zoom-in")
                    .debug_selector(|| "gantt-zoom-in".into())
                    .child(
                        Button::new("zoom-in")
                            .ghost()
                            .compact()
                            .label("+")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let pointer = this.state.gantt.timeline_width * 0.5;
                                this.state.gantt.zoom_at(1.25, pointer, 0.0);
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
    }

    fn render_tree(
        &mut self,
        nodes: &[ado_core::WorkItemNode],
        window: &[crate::gantt::VisibleRow],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let by_id: std::collections::HashMap<_, _> =
            nodes.iter().map(|n| (n.id, n.clone())).collect();
        let header_fg = cx.theme().muted_foreground;
        let hover_bg = cx.theme().list_hover;
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
                            .children(window.iter().map(|row| {
                                let node = by_id.get(&row.id).cloned();
                                let Some(node) = node else {
                                    return div().into_any_element();
                                };
                                let id = node.id;
                                let selected = self.state.gantt.selected_id == Some(id);
                                let hovered = self.state.gantt.hovered_id == Some(id);
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
                                let row_selector = format!("gantt-tree-row-{id}");
                                div()
                                    .id(("work-item", id as u64))
                                    .debug_selector(move || row_selector.clone())
                                    .role(Role::Row)
                                    .aria_label(format!("#{id} {}", node.title))
                                    .w_full()
                                    .h(px(ROW_HEIGHT))
                                    .min_h(px(ROW_HEIGHT))
                                    .max_h(px(ROW_HEIGHT))
                                    .flex_basis(px(ROW_HEIGHT))
                                    .flex_shrink_0()
                                    .overflow_hidden()
                                    .bg(if selected {
                                        cx.theme().list_active
                                    } else if hovered {
                                        hover_bg
                                    } else {
                                        cx.theme().background
                                    })
                                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                        this.set_hovered(
                                            if *hovered { Some(id) } else { None },
                                            cx,
                                        );
                                    }))
                                    .on_click(cx.listener(
                                        move |this, event: &gpui_kit::ClickEvent, window, cx| {
                                            this.open_filter = None;
                                            this.open_scope = None;
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
                                            this.state.gantt.selected_id = Some(id);
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
                                                this.state.gantt.selected_id = Some(id);
                                                cx.notify();
                                            }
                                        }),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                            let x: f32 = event.position.x.into();
                                            let y: f32 = event.position.y.into();
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
                                    .child(
                                        h_flex()
                                            .h_full()
                                            .w_full()
                                            .items_center()
                                            .overflow_hidden()
                                            .px_2()
                                            .gap_2()
                                            .pl(px(8.0 + depth as f32 * 16.))
                                            .child(
                                                Button::new(("expand", id as u64))
                                                    .ghost()
                                                    .compact()
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
                                            .when(!editing_title, |el| {
                                                el.when(show_type, |el| {
                                                    el.child(
                                                        self.type_badge(&node.work_item_type, cx),
                                                    )
                                                })
                                                .when(show_assignee, |el| {
                                                    el.child(self.assignee_cell(
                                                        id, &assigned, selected, cx,
                                                    ))
                                                })
                                                .when(show_dates, |el| {
                                                    el.child(self.dates_cell(id, dates, cx))
                                                })
                                            }),
                                    )
                                    .into_any_element()
                            })),
                    ),
            )
    }

    fn type_badge(&self, ty: &str, cx: &Context<Self>) -> impl IntoElement {
        let color = color_for_type(ty);
        let parsed = u32::from_str_radix(color.trim_start_matches('#'), 16).unwrap_or(0x89b4fa);
        h_flex()
            .gap_1()
            .px_1()
            .max_w(px(TYPE_COL_WIDTH))
            .flex_shrink_0()
            .overflow_hidden()
            .rounded_md()
            .bg(cx.theme().muted)
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
                    .child(ty.to_string()),
            )
    }

    fn title_cell(
        &self,
        id: i64,
        title: &str,
        selected: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        if self.inline_edit == Some(InlineEdit::Title(id)) {
            return div()
                .id(("inline-title-wrap", id as u64))
                .debug_selector(|| "gantt-inline-title".into())
                .flex_1()
                .w_full()
                .min_w(px(180.))
                .child(
                    Input::new(&self.inline_input)
                        .id(("inline-title", id as u64))
                        .w_full(),
                )
                .into_any_element();
        }
        let title_owned = title.to_string();
        let tip = title_owned.clone();
        div()
            .id(("title", id as u64))
            .flex_1()
            .min_w(px(120.))
            .truncate()
            .child(title_owned.clone())
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if event.click_count >= 2 {
                        this.open_form(id, cx);
                    }
                }),
            )
            .on_click(
                cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                    this.state.gantt.selected_id = Some(id);
                    if event.click_count() >= 2 {
                        this.open_form(id, cx);
                    } else if selected {
                        this.begin_inline(InlineEdit::Title(id), title_owned.clone(), window, cx);
                    }
                    cx.stop_propagation();
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
                this.state.gantt.selected_id = Some(id);
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
        let display = if let Some(rest) = text.strip_prefix("Unscheduled") {
            let hint = rest.trim_start_matches([' ', '·']).trim().to_string();
            if hint.is_empty() {
                "Unscheduled · drop on timeline".to_string()
            } else {
                format!("Unscheduled · {hint}")
            }
        } else {
            text.clone()
        };
        let tip = display.clone();
        div()
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
            .truncate()
            .text_xs()
            .when(unscheduled, |el| el.text_color(muted))
            .when(editing, |el| el.bg(cx.theme().accent.opacity(0.35)))
            .child(display.clone())
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.state.gantt.selected_id = Some(id);
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
        let (x, y) = self.dates_anchor.unwrap_or((320.0, 160.0));
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
                        .min_w(px(280.))
                        .p_2()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().popover)
                        .shadow_md()
                        .occlude()
                        .child(Calendar::new(&state).number_of_months(1)),
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

    fn render_timeline(
        &mut self,
        window: &[crate::gantt::VisibleRow],
        bars: &[crate::gantt::BarLayout],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let width = self.state.gantt.timeline_width.max(MIN_TIMELINE_WIDTH);
        let ticks = self.state.gantt.timeline_ticks(width);
        let today_x = self.state.gantt.today_x(jiff::Zoned::now().date());
        let header_fg = cx.theme().muted_foreground;
        let muted_fg = cx.theme().foreground;
        let bar_label_fg = gpui_kit::hsla(240. / 360., 0.21, 0.12, 1.);
        let grid = cx.theme().border;
        let weekend = cx.theme().muted;
        let hover_bg = cx.theme().list_hover;
        let selected_bg = cx.theme().list_active;
        let today_color = cx.theme().primary;
        let selected_id = self.state.gantt.selected_id;
        let hovered_id = self.state.gantt.hovered_id;
        let dragging = self.state.gantt.gesture.is_some();
        let show_weekends = self.state.gantt.scale.shows_weekend_bands();
        let weekend_w = self.state.gantt.scale.pixels_per_day(self.state.gantt.zoom);
        let major_labels = self.state.gantt.header_major_labels(width);
        let tick_labels = self.state.gantt.header_tick_labels(width);
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
                        cx.notify();
                    }
                }),
            )
            .child(
                div()
                    .id("gantt-timeline-body")
                    .debug_selector(|| "gantt-timeline-body".into())
                    .absolute()
                    .top(px(TIMELINE_HEADER_H))
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .relative()
                    .overflow_hidden()
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
                    .children(ticks.iter().map(|tick| {
                        div()
                            .absolute()
                            .left(px(tick.x))
                            .top_0()
                            .bottom_0()
                            .w(px(1.))
                            .bg(grid)
                    }))
                    .child(
                        v_flex()
                            .mt(px(-self.state.gantt.row_scroll_remainder()))
                            .children(window.iter().map(|row| {
                                let id = row.id;
                                let selected = selected_id == Some(id);
                                let hovered = hovered_id == Some(id);
                                let bar = bars.iter().find(|bar| bar.id == id).cloned();
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
                                    .bg(if selected {
                                        selected_bg
                                    } else if hovered {
                                        hover_bg
                                    } else {
                                        gpui_kit::hsla(0., 0., 0., 0.)
                                    })
                                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                        this.set_hovered(
                                            if *hovered { Some(id) } else { None },
                                            cx,
                                        );
                                    }))
                                    .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
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
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
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
                                                this.state.gantt.schedule_drag = Some(id);
                                                this.state.gantt.selected_id = Some(id);
                                                this.state.gantt.hovered_id = Some(id);
                                                cx.notify();
                                            }
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
                            })),
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
                        self.state.gantt.gesture.as_ref().and_then(|gesture| {
                            if gesture.kind == GestureKind::Link {
                                return None;
                            }
                            let text = crate::gantt::GanttView::drag_preview_text(gesture);
                            let x = gesture.live_x.max(4.0);
                            let y = bars
                                .iter()
                                .find(|bar| bar.id == gesture.id)
                                .map(|bar| (bar.y - 22.0).max(2.0))
                                .unwrap_or(4.0);
                            Some(
                                div()
                                    .id("gantt-drag-preview")
                                    .debug_selector(|| "gantt-drag-preview".into())
                                    .absolute()
                                    .left(px(x))
                                    .top(px(y))
                                    .px_2()
                                    .py(px(2.))
                                    .rounded_sm()
                                    .bg(cx.theme().popover)
                                    .text_color(cx.theme().popover_foreground)
                                    .text_xs()
                                    .shadow_md()
                                    .child(text),
                            )
                        })
                    })
                    .when(
                        bars.is_empty()
                            && !self.state.nodes.is_empty()
                            && !self.state.filters_hide_work_items(),
                        |el| {
                        el.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .px_6()
                                .child(
                                    div()
                                        .id("unscheduled-empty-hint")
                                        .debug_selector(|| "unscheduled-empty-hint".into())
                                        .max_w(px(280.))
                                        .text_sm()
                                        .text_color(header_fg)
                                        .child(
                                            "No scheduled dates. Drag a work item onto the timeline to set Start and Target.",
                                        ),
                                ),
                        )
                    }),
            )
            .when(today_x >= 0.0 && today_x <= width, |el| {
                el.child(
                    div()
                        .id("gantt-today-label")
                        .debug_selector(|| "gantt-today-label".into())
                        .absolute()
                        .left(px((today_x + 6.0).max(4.0)))
                        .top(px(2.))
                        .text_xs()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(today_color)
                        .child("Today"),
                )
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
                    .children(major_labels.into_iter().filter(|(x, _)| *x < width - 8.0).map(|(x, label)| {
                        div()
                            .absolute()
                            .left(px(x))
                            .top(px(2.))
                            .text_xs()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(muted_fg)
                            .child(label)
                    }))
                    .children(tick_labels.into_iter().enumerate().filter(|(_, (x, _))| *x > -40.0 && *x < width - 8.0).map(|(index, (x, label))| {
                        let selector = format!("gantt-tick-{index}");
                        div()
                            .id(("gantt-tick", index as u64))
                            .debug_selector(move || selector)
                            .absolute()
                            .left(px(x))
                            .top(px(22.))
                            .text_xs()
                            .text_color(header_fg)
                            .child(label)
                    })),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.state.gantt.gesture.is_some() {
                    let x: f32 = event.position.x.into();
                    this.state
                        .update_bar_gesture(x - this.state.gantt.timeline_origin_x);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.commit_gesture(cx);
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
        let id = bar.id;
        let bar_x = bar.x;
        let bar_width = bar.width;
        let summary = bar.summary;
        let unscheduled = bar.kind == ado_core::GanttBarKind::Unscheduled;
        let milestone = bar.milestone;
        let title_width = bar.title.chars().count() as f32 * 6.5 + 16.0;
        let inside = !unscheduled && !milestone && !summary && bar.width >= title_width.max(40.0);
        let selected = self.state.gantt.selected_id == Some(id);
        let hovered = self.state.gantt.hovered_id == Some(id);
        let show_handles = !summary && !unscheduled && !milestone && hovered && !dragging;
        let show_link_handles = !unscheduled && hovered && !dragging;
        let bar_h = if milestone {
            MILESTONE_SIZE
        } else if summary {
            SUMMARY_BAR_H
        } else if unscheduled {
            PLACEHOLDER_BAR_H
        } else {
            LEAF_BAR_H
        };
        let bar_top = ((ROW_HEIGHT - bar_h) / 2.0).max(2.0);
        let label_pad = if show_handles {
            HANDLE_WIDTH + 4.0
        } else {
            6.0
        };
        let progress_value = bar.progress.unwrap_or(0.0);
        let show_progress = summary || progress_value > 0.0;
        let progress_w =
            (bar.width.max(8.0) * progress_value).max(if progress_value > 0.0 { 2.0 } else { 0.0 });
        let bar_selector = format!("gantt-bar-{id}");
        let progress_selector = format!("gantt-bar-progress-{id}");
        let mut items = vec![
            div()
                .id(("gantt-bar", id as u64))
                .debug_selector(move || bar_selector.clone())
                .role(Role::Button)
                .aria_label(format!("Gantt bar #{id} {}", bar.title))
                .absolute()
                .left(px(bar.x))
                .top(px(bar_top))
                .w(px(if milestone {
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
                .when(summary && !milestone, |el| {
                    el.bg(cx.theme().muted).rounded_sm()
                })
                .when(!summary && !milestone, |el| {
                    el.bg(rgb(bar.color))
                        .when(hovered && !dragging, |el| el.opacity(0.92))
                })
                .when(unscheduled && !summary, |el| {
                    el.bg(cx.theme().muted)
                        .opacity(0.95)
                        .border_2()
                        .border_color(cx.theme().primary)
                })
                .text_color(if summary { muted_fg } else { bar_label_fg })
                .when(!milestone && !unscheduled, |el| {
                    el.border_1()
                        .border_color(if selected || (hovered && !dragging) {
                            cx.theme().primary
                        } else {
                            cx.theme().border
                        })
                })
                .overflow_hidden()
                .when(show_progress, |el| {
                    el.child(
                        div()
                            .id(("gantt-bar-progress", id as u64))
                            .debug_selector(move || progress_selector.clone())
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(px(progress_w))
                            .bg(if summary {
                                gpui_kit::hsla(0., 0., 1., 0.45)
                            } else {
                                gpui_kit::hsla(0., 0., 1., 0.28)
                            }),
                    )
                })
                .when(!milestone, |el| {
                    el.child(
                        div()
                            .pl(px(label_pad))
                            .pr(px(label_pad))
                            .truncate()
                            .text_xs()
                            .child(if inside {
                                bar.title.clone()
                            } else {
                                String::new()
                            }),
                    )
                })
                .when(show_handles, |el| {
                    el.child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .h_full()
                            .w(px(HANDLE_WIDTH))
                            .bg(cx.theme().primary),
                    )
                    .child(
                        div()
                            .absolute()
                            .right_0()
                            .top_0()
                            .h_full()
                            .w(px(HANDLE_WIDTH))
                            .bg(cx.theme().primary),
                    )
                })
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    this.set_hovered(if *hovered { Some(id) } else { None }, cx);
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        if event.click_count >= 2 {
                            this.open_form(id, cx);
                            return;
                        }
                        if unscheduled {
                            this.dismiss_overlays();
                            let x: f32 = event.position.x.into();
                            let local_x = x - this.state.gantt.timeline_origin_x;
                            let _ = this.state.begin_bar_gesture(id, GestureKind::Move, local_x);
                            cx.notify();
                            return;
                        }
                        let x: f32 = event.position.x.into();
                        let local_x = x - this.state.gantt.timeline_origin_x;
                        let kind = if milestone {
                            GestureKind::Move
                        } else if local_x <= bar_x + HANDLE_WIDTH {
                            GestureKind::ResizeStart
                        } else if local_x >= bar_x + bar_width - HANDLE_WIDTH {
                            GestureKind::ResizeEnd
                        } else {
                            GestureKind::Move
                        };
                        this.dismiss_overlays();
                        let _ = this.state.begin_bar_gesture(id, kind, local_x);
                        cx.notify();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        let x: f32 = event.position.x.into();
                        let y: f32 = event.position.y.into();
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
        if show_link_handles {
            let start_x = bar.x - LINK_HANDLE_HIT - 4.0;
            let end_x = bar.x + bar.width + 4.0;
            let handle_top = ((ROW_HEIGHT - LINK_HANDLE_HIT) / 2.0).max(0.0);
            for (handle_id, handle_x) in [("link-start", start_x), ("link-end", end_x)] {
                let selector = format!("gantt-link-handle-{id}-{handle_id}");
                let origin = handle_x + LINK_HANDLE_HIT / 2.0;
                items.push(
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
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered {
                                this.set_hovered(Some(id), cx);
                            }
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.dismiss_overlays();
                                let _ = this.state.begin_bar_gesture(id, GestureKind::Link, origin);
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .size(px(LINK_HANDLE))
                                .rounded_full()
                                .bg(cx.theme().background)
                                .border_2()
                                .border_color(cx.theme().primary),
                        )
                        .into_any_element(),
                );
            }
        }
        if !inside {
            let label = bar.title.clone();
            let label_gap = if show_link_handles {
                LINK_HANDLE_HIT + 10.0
            } else {
                10.0
            };
            items.push(
                div()
                    .id(("gantt-bar-label", id as u64))
                    .debug_selector(|| "gantt-bar-label".into())
                    .absolute()
                    .left(px(bar.x + bar.width + label_gap))
                    .top(px(((ROW_HEIGHT - 16.0) / 2.0).max(2.0)))
                    .h(px(16.))
                    .flex()
                    .items_center()
                    .whitespace_nowrap()
                    .text_xs()
                    .text_color(muted_fg)
                    .child(label.clone())
                    .on_click(
                        cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                            this.state.gantt.selected_id = Some(id);
                            if event.click_count() >= 2 {
                                this.open_form(id, cx);
                            } else {
                                this.begin_inline(InlineEdit::Title(id), label.clone(), window, cx);
                            }
                        }),
                    )
                    .into_any_element(),
            );
        }
        div()
            .relative()
            .size_full()
            .children(items)
            .into_any_element()
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
        let min_x = arrow.x1.min(arrow.x2).min(arrow.x_head) - 10.0;
        let max_x = arrow.x1.max(arrow.x2).max(arrow.x_head) + 18.0;
        let min_y = arrow.y1.min(arrow.y2).min(arrow.lane_y) - 10.0;
        let max_y = arrow.y1.max(arrow.y2).max(arrow.lane_y) + 10.0;
        let selector = format!("gantt-dep-arrow-{}-{}", arrow.from_id, arrow.to_id);
        let from_id = arrow.from_id;
        let to_id = arrow.to_id;
        let v1 = (arrow.lane_y - arrow.y1).abs().max(2.0);
        let h_len = (arrow.x2 - arrow.x1).abs().max(2.0);
        let v2 = (arrow.y2 - arrow.lane_y).abs().max(2.0);
        let stub_h = (arrow.x_head - arrow.x2).abs().max(2.0);
        div()
            .absolute()
            .left(px(min_x))
            .top(px(min_y))
            .w(px((max_x - min_x).max(20.0)))
            .h(px((max_y - min_y).max(20.0)))
            .child(
                div()
                    .absolute()
                    .left(px(arrow.x1 - min_x))
                    .top(px(arrow.y1.min(arrow.lane_y) - min_y))
                    .w(px(stroke))
                    .h(px(v1))
                    .bg(color),
            )
            .child(
                div()
                    .absolute()
                    .left(px(arrow.x1.min(arrow.x2) - min_x))
                    .top(px(arrow.lane_y - min_y))
                    .w(px(h_len))
                    .h(px(stroke))
                    .bg(color),
            )
            .child(
                div()
                    .absolute()
                    .left(px(arrow.x2 - min_x))
                    .top(px(arrow.lane_y.min(arrow.y2) - min_y))
                    .w(px(stroke))
                    .h(px(v2))
                    .bg(color),
            )
            .child(
                div()
                    .absolute()
                    .left(px(arrow.x2.min(arrow.x_head) - min_x))
                    .top(px(arrow.y2 - min_y))
                    .w(px(stub_h))
                    .h(px(stroke))
                    .bg(color),
            )
            .child(
                div()
                    .id(SharedString::from(format!(
                        "gantt-dep-arrowhead-{}-{}",
                        arrow.from_id, arrow.to_id
                    )))
                    .debug_selector(|| "gantt-dep-arrowhead".into())
                    .absolute()
                    .left(px(arrow.x_head - min_x - 16.0))
                    .top(px(arrow.y2 - min_y - 10.0))
                    .w(px(18.))
                    .h(px(20.))
                    .text_xl()
                    .line_height(gpui_kit::relative(1.))
                    .text_color(color)
                    .child("▶"),
            )
            .child(
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .absolute()
                    .inset_0()
                    .cursor_pointer()
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
                            this.state.gantt.selected_id = Some(to_id);
                            cx.notify();
                        }),
                    )
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
                    ),
            )
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
                    this.state.gantt.context_menu = None;
                    this.state.gantt.selected_link = None;
                    this.spawn_state(cx, move |mut state| async move {
                        let result = state.remove_dependency(to_id, from_id).await;
                        (state, result)
                    });
                }))
            })
            .child(self.menu_item("Open", cx, move |this, cx| this.open_form(id, cx)))
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
            .child(Button::new(label).label(label))
    }

    fn render_form(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(form) = self.state.form.as_ref() else {
            return div().into_any_element();
        };
        let title = form.title();
        let heading = form.item_title();
        let work_item_type = form.model.work_item_type.clone();
        let pages = form.model.pages.clone();
        let system = form.model.system_controls.clone();
        let active = form.active_page;
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
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key.as_str() == "escape" {
                    this.dismiss_all_overlays();
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.state.close_form();
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .id("work-item-form-dialog")
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
                                    .child(div().text_lg().truncate().child(
                                        if heading.is_empty() {
                                            "Untitled".to_string()
                                        } else {
                                            heading
                                        },
                                    )),
                            )
                            .child(
                                Button::new("close-form")
                                    .label("Close")
                                    .on_click(cx.listener(|this, _, _, cx| {
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
                            .min_h(px(120.))
                            .px_6()
                            .py_3()
                            .overflow_y_scrollbar()
                            .gap_3()
                            .children(system.iter().map(|control| self.render_field(control, cx)))
                            .when(active < pages.len(), |el| {
                                el.child(v_flex().gap_3().children(
                                    pages[active].groups.iter().map(|group| {
                                        v_flex()
                                            .gap_2()
                                            .p_3()
                                            .rounded_md()
                                            .bg(cx.theme().muted)
                                            .child(
                                                Label::new(group.label.clone())
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground),
                                            )
                                            .children(
                                                group
                                                    .controls
                                                    .iter()
                                                    .map(|control| self.render_field(control, cx)),
                                            )
                                    }),
                                ))
                            }),
                    )
                    .child(
                        h_flex()
                            .px_6()
                            .py_3()
                            .gap_2()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(
                                Button::new("save-work-item")
                                    .primary()
                                    .label("Save")
                                    .on_click(cx.listener(|this, _, _, cx| this.save_form(cx))),
                            )
                            .child(Button::new("cancel-form").label("Cancel").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.state.close_form();
                                    cx.notify();
                                }),
                            )),
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
        let has_target = crate::form::all_controls(&form.model)
            .iter()
            .any(|item| item.reference_name == ado_core::TARGET_DATE_FIELD);
        let label = if control.kind == FormControlKind::DateTime
            && control.reference_name == ado_core::START_DATE_FIELD
            && has_target
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
                    if id == ado_core::TARGET_DATE_FIELD
                        && form
                            .model
                            .system_controls
                            .iter()
                            .chain(form.model.pages.iter().flat_map(|page| {
                                page.groups.iter().flat_map(|group| group.controls.iter())
                            }))
                            .any(|control| control.reference_name == ado_core::START_DATE_FIELD)
                    {
                        return div().into_any_element();
                    }
                    let picker = self
                        .form_dates
                        .get(&id)
                        .cloned()
                        .or_else(|| self.form_dates.get(ado_core::START_DATE_FIELD).cloned());
                    v_flex()
                        .id(SharedString::from(format!("form-date-{id}")))
                        .debug_selector(|| "form-date-picker".into())
                        .w_full()
                        .when_some(picker, |el, state| {
                            el.child(DatePicker::new(&state).w_full())
                        })
                        .into_any_element()
                }
                FormControlKind::Identity => {
                    let display = value
                        .get("displayName")
                        .and_then(|v| v.as_str())
                        .or_else(|| value.as_str())
                        .unwrap_or("")
                        .to_string();
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

    fn open_form(&mut self, id: i64, cx: &mut Context<Self>) {
        self.dismiss_overlays();
        self.form_inputs.clear();
        self.form_inputs_for = None;
        self.spawn_state(cx, move |mut state| async move {
            let result = state.open_form(id).await;
            (state, result)
        });
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
                FormControlKind::DateTime => {}
                _ => form.set_string(&control.reference_name, text),
            }
        }
        for (reference, picker) in &self.form_dates {
            match picker.read(cx).date() {
                PickerDate::Single(Some(naive)) => {
                    if let Some(date) = naive_to_jiff(naive) {
                        form.values
                            .insert(reference.clone(), serde_json::json!(iso_date_only(date)));
                    }
                }
                PickerDate::Range(start, end) => {
                    if let Some(start) = start.and_then(naive_to_jiff) {
                        form.values.insert(
                            ado_core::START_DATE_FIELD.into(),
                            serde_json::json!(iso_date_only(start)),
                        );
                    }
                    if let Some(end) = end.and_then(naive_to_jiff) {
                        form.values.insert(
                            ado_core::TARGET_DATE_FIELD.into(),
                            serde_json::json!(iso_date_only(end)),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn save_form(&mut self, cx: &mut Context<Self>) {
        self.sync_form_inputs(cx);
        self.spawn_state(cx, |mut state| async move {
            let result = state.save_form().await;
            (state, result)
        });
    }

    fn commit_gesture(&mut self, cx: &mut Context<Self>) {
        if self.state.gantt.gesture.is_none() {
            return;
        }
        let linking = self
            .state
            .gantt
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.kind == GestureKind::Link);
        if !linking {
            self.state.gantt.clear_hover();
        }
        self.spawn_state(cx, |mut state| async move {
            let result = state.commit_gesture().await;
            (state, result)
        });
    }

    fn ensure_form_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.state.form.as_ref() else {
            self.form_inputs.clear();
            self.form_dates.clear();
            self.form_inputs_for = None;
            return;
        };
        if self.form_inputs_for != Some(form.model.id) {
            self.form_inputs.clear();
            self.form_dates.clear();
            self.form_inputs_for = Some(form.model.id);
        }
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
                if control.reference_name == ado_core::TARGET_DATE_FIELD && has_start {
                    continue;
                }
                if self.form_dates.contains_key(&control.reference_name) {
                    continue;
                }
                let range = control.reference_name == ado_core::START_DATE_FIELD && has_target;
                let start = form
                    .values
                    .get(ado_core::START_DATE_FIELD)
                    .and_then(serde_json::Value::as_str)
                    .and_then(|text| parse_iso_date(Some(text)))
                    .and_then(jiff_to_naive);
                let target = form
                    .values
                    .get(ado_core::TARGET_DATE_FIELD)
                    .and_then(serde_json::Value::as_str)
                    .and_then(|text| parse_iso_date(Some(text)))
                    .and_then(jiff_to_naive);
                let initial = form
                    .values
                    .get(&control.reference_name)
                    .and_then(serde_json::Value::as_str)
                    .and_then(|text| parse_iso_date(Some(text)))
                    .and_then(jiff_to_naive);
                let picker = cx.new(|cx| match (range, start, target) {
                    (true, Some(start), Some(target)) => {
                        let mut state = DatePickerState::range(window, cx).date_format("%Y-%m-%d");
                        state.set_date(PickerDate::Range(Some(start), Some(target)), window, cx);
                        state
                    }
                    (_, Some(start), None) => {
                        let mut state = DatePickerState::new(window, cx).date_format("%Y-%m-%d");
                        state.set_date(PickerDate::Single(Some(start)), window, cx);
                        state
                    }
                    (_, None, Some(target)) => {
                        let mut state = DatePickerState::new(window, cx).date_format("%Y-%m-%d");
                        state.set_date(PickerDate::Single(Some(target)), window, cx);
                        state
                    }
                    (true, None, None) => {
                        DatePickerState::range(window, cx).date_format("%Y-%m-%d")
                    }
                    _ => {
                        let mut state = DatePickerState::new(window, cx).date_format("%Y-%m-%d");
                        if let Some(naive) = initial {
                            state.set_date(PickerDate::Single(Some(naive)), window, cx);
                        }
                        state
                    }
                });
                self.form_dates
                    .insert(control.reference_name.clone(), picker);
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
            self.form_inputs
                .insert(control.reference_name.clone(), input);
        }
    }

    pub fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        self.on_key(event, cx);
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            if self.inline_edit.take().is_some() {
                cx.notify();
                return;
            }
            self.dismiss_all_overlays();
            cx.notify();
            return;
        }
        if key == "enter" && self.inline_edit.is_some() {
            self.commit_inline(cx);
            return;
        }
        if self.inline_edit.is_some() {
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
            && let Some((from_id, to_id)) = self.state.gantt.selected_link.take()
        {
            self.spawn_state(cx, move |mut state| async move {
                let result = state.remove_dependency(to_id, from_id).await;
                (state, result)
            });
            return;
        }
        if self.state.form.is_some() || self.popup_open() {
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
            self.state.gantt.scroll_y = 0.0;
            if let Some(first) = self.state.visible_rows().first() {
                self.state.gantt.selected_id = Some(first.id);
            }
            cx.notify();
            return;
        }
        if key == "end" {
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
            if self.state.gantt.context_menu.is_none() && self.state.form.is_none() {
                if event.keystroke.modifiers.shift {
                    let _ = self.state.nudge(NudgeAxis::Target, delta);
                    self.schedule_nudge_flush(cx);
                } else if event.keystroke.modifiers.alt {
                    let _ = self.state.nudge(NudgeAxis::Start, delta);
                    self.schedule_nudge_flush(cx);
                } else if event.keystroke.modifiers.control {
                    self.state.gantt.nudge_tree_width(delta as f32 * 16.0);
                    cx.notify();
                } else if self.state.gantt.selected_id.is_some() {
                    let _ = self.state.nudge(NudgeAxis::Both, delta);
                    self.schedule_nudge_flush(cx);
                } else {
                    self.state.gantt.nudge_tree_width(delta as f32 * 16.0);
                    cx.notify();
                }
            }
        }
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
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    this.state.toasts.expire();
                    this.toast_expiry_armed = false;
                    if !this.state.toasts.items.is_empty() {
                        this.schedule_toast_expiry(cx);
                    }
                    cx.notify();
                });
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
        let viewport_h = f32::from(window.viewport_size().height);
        let viewport_w = f32::from(window.viewport_size().width);
        self.state.gantt.viewport_h = (viewport_h - 140.0).max(120.0);
        self.state.gantt.viewport_w = viewport_w;
        self.state.gantt.clamp_tree_width();
        self.state.gantt.timeline_origin_x = self.state.gantt.tree_width + 6.0;
        self.state.toasts.expire();
        self.ensure_form_inputs(window, cx);
        if let Some(form_id) = self.state.form.as_ref().map(|form| form.model.id) {
            if self.form_title_focused_for != Some(form_id) {
                if let Some(input) = self.form_inputs.get("System.Title").cloned() {
                    Self::focus_input(&input, window, cx);
                } else {
                    window.focus(&self.form_focus, cx);
                }
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
            .capture_any_mouse_up(cx.listener(|this, event: &MouseUpEvent, _, cx| {
                if this.state.gantt.gesture.is_some() {
                    this.commit_gesture(cx);
                }
                if this.state.gantt.splitter_drag.take().is_some() {
                    cx.notify();
                }
                if let Some(id) = this.state.gantt.schedule_drag.take() {
                    if this.state.gantt.context_menu.is_some() {
                        cx.notify();
                        return;
                    }
                    let x: f32 = event.position.x.into();
                    if x > this.state.gantt.timeline_origin_x {
                        let date = this.state.gantt.drop_date(x);
                        this.spawn_state(cx, move |mut state| async move {
                            let result = state.schedule_at(id, date).await;
                            (state, result)
                        });
                    }
                    cx.notify();
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                let x: f32 = event.position.x.into();
                if let Some((origin_x, origin_w)) = this.state.gantt.splitter_drag {
                    this.state.gantt.set_tree_width_for_pane(
                        origin_w + x - origin_x,
                        f32::from(window.viewport_size().width),
                    );
                    cx.notify();
                    return;
                }
                if this.state.gantt.gesture.is_some() {
                    this.state
                        .update_bar_gesture(x - this.state.gantt.timeline_origin_x);
                    cx.notify();
                }
            }))
            .track_focus(&self.form_focus)
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                this.on_key(event, cx);
                if event.keystroke.key.as_str() == "escape" {
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
                            .child(self.render_error_banner(cx))
                            .child(self.render_updater(cx))
                            .child(div().flex_1().child(self.render_gantt(cx)))
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
                            .child(self.render_toasts(cx)),
                    )
                },
            )
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
        gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, move |window, cx| {
            cx.new(move |cx| PlannerView::new(window, cx, session, paths, ado_base, vssps_base))
        })
        .expect("failed to open window");
    });
}
