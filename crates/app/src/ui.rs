use std::path::PathBuf;
use std::time::Duration;

use ado_core::{AuthMode, FLAVORS, FormControlKind, color_for_type, dates_column_text};
use ado_session::PlannerSession;
use gpui_kit::base::Selectable;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::component::text::TextView;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Entity, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, ParentElement, Render, Role, ScrollEvent, SharedString,
    StatefulInteractiveElement, Styled, Window, actions, div, px, rgb,
};

use crate::flavor_theme::apply_flavor;
use crate::form::ado_edit_url;
use crate::gantt::{GanttScale, GestureKind, HANDLE_WIDTH, NUDGE_IDLE_MS, NudgeAxis, ROW_HEIGHT};
use crate::paths::AppPaths;
use crate::state::PlannerState;
use crate::toasts::{ToastKind, UndoAction};

actions!(
    ado_planner,
    [Quit, NudgeLeft, NudgeRight, EscapeCancel, OpenSelected]
);

macro_rules! go {
    ($this:expr, $cx:expr, $call:expr) => {{
        let handle = $this.state.runtime.handle();
        if let Err(err) = handle.block_on($call) {
            tracing::warn!("planner action failed: {err}");
        }
        $cx.notify();
    }};
}

pub struct PlannerView {
    pub state: PlannerState,
    org_input: Entity<InputState>,
    pat_input: Entity<InputState>,
    filter_query: Entity<InputState>,
    form_focus: FocusHandle,
    open_filter: Option<&'static str>,
    open_scope: Option<&'static str>,
    scope_search: String,
    filter_search: String,
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
        let mut view = Self {
            state,
            org_input,
            pat_input,
            filter_query,
            form_focus: cx.focus_handle(),
            open_filter: None,
            open_scope: None,
            scope_search: String::new(),
            filter_search: String::new(),
        };
        go!(view, cx, view.state.bootstrap());
        view
    }

    fn submit_pat(&mut self, cx: &mut Context<Self>) {
        self.state.sign_in_org = self.org_input.read(cx).value().to_string();
        self.state.sign_in_pat = self.pat_input.read(cx).value().to_string();
        go!(self, cx, self.state.login_pat());
    }

    fn render_toasts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .absolute()
            .top_4()
            .right_4()
            .gap_2()
            .children(self.state.toasts.items.iter().map(|toast| {
                let id = toast.id;
                let copyable = toast.copyable;
                let undo = toast.undo.clone();
                let message = toast.message.clone();
                let kind = toast.kind;
                h_flex()
                    .id(("toast", id))
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(match kind {
                        ToastKind::Error => cx.theme().danger,
                        ToastKind::Warning => cx.theme().warning,
                        ToastKind::Success => cx.theme().success,
                    })
                    .text_color(cx.theme().background)
                    .child(div().id(("toast-title", id)).child(message.clone()))
                    .when(copyable, |el| {
                        el.child(Button::new(("copy-toast", id)).label("Copy").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.state.copy_toast(id);
                                cx.notify();
                            }),
                        ))
                    })
                    .when_some(undo.clone(), |el, action| {
                        el.child(Button::new(("undo-toast", id)).label("Undo").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.undo(action.clone(), cx);
                            }),
                        ))
                    })
            }))
    }

    fn undo(&mut self, action: UndoAction, cx: &mut Context<Self>) {
        go!(self, cx, self.state.undo_clear_dates(action));
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
                                go!(this, cx, this.state.login_entra());
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
            .w_full()
            .px_5()
            .py_3()
            .gap_4()
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
            .child(self.render_filters(cx))
            .child(self.render_flavor(cx))
            .child(
                Button::new("log-out")
                    .label("Log out")
                    .on_click(cx.listener(|this, _, _, cx| {
                        go!(this, cx, this.state.logout());
                    })),
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
        let search = self.scope_search.clone();
        v_flex()
            .gap_1()
            .child(
                Label::new(label)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                Button::new(id)
                    .label(if value.is_empty() {
                        format!("Select {label}")
                    } else {
                        value
                    })
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_scope = if this.open_scope == Some(id) {
                            None
                        } else {
                            Some(id)
                        };
                        this.scope_search.clear();
                        cx.notify();
                    })),
            )
            .when(open, |el| {
                el.child(
                    v_flex()
                        .absolute()
                        .mt(px(48.))
                        .w(px(220.))
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().popover)
                        .rounded_md()
                        .p_1()
                        .shadow_md()
                        .child(Input::new(&self.filter_query).id(format!("{id}-search")))
                        .children(options.into_iter().filter_map(move |(val, lab)| {
                            if !search.is_empty()
                                && !lab.to_lowercase().contains(&search.to_lowercase())
                                && !val.to_lowercase().contains(&search.to_lowercase())
                            {
                                return None;
                            }
                            let val_click = val.clone();
                            Some(
                                Button::new(SharedString::from(format!("{id}-{val}")))
                                    .label(lab)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.choose_scope(id, val_click.clone(), cx);
                                    })),
                            )
                        })),
                )
            })
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
                go!(self, cx, self.state.set_org(value));
            }
            "project" => {
                go!(self, cx, self.state.set_project(value));
            }
            "team" => {
                go!(self, cx, self.state.set_team(value));
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
            .child(self.filter_button("root-filter", "Roots", self.state.hidden_root_count(), cx))
            .child(self.filter_button(
                "type-filter",
                "Types",
                self.state.prefs.planner.hidden_types.len(),
                cx,
            ))
            .child(self.filter_button(
                "state-filter",
                "States",
                self.state.prefs.planner.hidden_states.len(),
                cx,
            ))
    }

    fn filter_button(
        &mut self,
        id: &'static str,
        label: &'static str,
        hidden: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let caption = if hidden > 0 {
            format!("{label} {hidden} hidden")
        } else {
            label.to_string()
        };
        let open = self.open_filter == Some(id);
        let items = match id {
            "root-filter" | "type-filter" => self.state.types(),
            _ => self.state.states(),
        };
        let hidden_set = match id {
            "type-filter" => self.state.prefs.planner.hidden_types.clone(),
            "state-filter" => self.state.prefs.planner.hidden_states.clone(),
            _ => {
                let types = self.state.types();
                let shown = types.len().saturating_sub(self.state.hidden_root_count());
                types.into_iter().skip(shown).collect()
            }
        };
        v_flex()
            .child(
                Button::new(id)
                    .label(caption)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_filter = if this.open_filter == Some(id) {
                            None
                        } else {
                            Some(id)
                        };
                        this.filter_search.clear();
                        cx.notify();
                    })),
            )
            .when(open, |el| {
                el.child(self.filter_menu(id, items, hidden_set, cx))
            })
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
        v_flex()
            .absolute()
            .mt(px(36.))
            .w(px(240.))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .rounded_md()
            .p_1()
            .child(Input::new(&self.filter_query).id(format!("{id}-search")))
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
            .children(filtered.into_iter().map(|item| {
                let checked = !hidden.contains(&item);
                let item_click = item.clone();
                h_flex()
                    .id(SharedString::from(format!("filter-item-{item}")))
                    .gap_2()
                    .child(Checkbox::new(SharedString::from(format!("cb-{item}"))).checked(checked))
                    .child(item.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        match id {
                            "type-filter" => this.state.toggle_hidden_type(&item_click),
                            "state-filter" => this.state.toggle_hidden_state(&item_click),
                            _ => {
                                let mut hidden = this.state.types();
                                hidden.retain(|ty| ty != &item_click);
                                // toggle membership in hidden-roots by inverting current
                                let currently_hidden = this.state.hidden_root_count() > 0;
                                let _ = currently_hidden;
                                this.state.toggle_hidden_type(&item_click);
                            }
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
        if visible.is_empty() {
            return v_flex()
                .id("empty-gantt")
                .size_full()
                .items_center()
                .justify_center()
                .child(Label::new("No Work Items match…"))
                .into_any_element();
        }
        let rows = self.state.gantt.flatten(&visible);
        let window = self.state.gantt.visible_window(&rows, 720.0).to_vec();
        let bars = self
            .state
            .gantt
            .layout_bars(&visible, &rows, &self.state.iterations, 720.0);
        v_flex()
            .id("gantt")
            .size_full()
            .child(self.render_gantt_nav(cx))
            .child(
                h_flex()
                    .id("gantt-body")
                    .flex_1()
                    .child(self.render_tree(&visible, &window, cx))
                    .child(
                        div()
                            .id("gantt-splitter")
                            .w(px(6.))
                            .h_full()
                            .bg(cx.theme().border)
                            .cursor_col_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                    if event.click_count >= 2 {
                                        this.state.gantt.reset_tree_width();
                                    }
                                    cx.notify();
                                }),
                            ),
                    )
                    .child(self.render_timeline(&bars, cx)),
            )
            .when(self.state.hierarchy_loading, |el| {
                el.child(
                    div()
                        .id("gantt-loading")
                        .absolute()
                        .inset_0()
                        .bg(cx.theme().overlay)
                        .child(Label::new("Loading Hierarchy…")),
                )
            })
            .into_any_element()
    }

    fn render_gantt_nav(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .id("gantt-nav")
            .gap_2()
            .p_2()
            .child(
                Button::new("today")
                    .label("Today")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.gantt.go_today(jiff::Zoned::now().date());
                        cx.notify();
                    })),
            )
            .child(
                Button::new("prev-period")
                    .label("‹")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.gantt.step_period(-1);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("next-period")
                    .label("›")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.gantt.step_period(1);
                        cx.notify();
                    })),
            )
            .child(div().child(self.state.gantt.scale.range_title(self.state.gantt.origin)))
            .children(GanttScale::ALL.iter().map(|scale| {
                let scale = *scale;
                Button::new(scale.label())
                    .label(scale.label())
                    .selected(self.state.gantt.scale == scale)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.gantt.scale = scale;
                        cx.notify();
                    }))
            }))
            .child(
                Button::new("zoom-out")
                    .label("−")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.gantt.zoom_at(0.8, 0.0, 0.0);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("zoom-in")
                    .label("+")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.gantt.zoom_at(1.25, 0.0, 0.0);
                        cx.notify();
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
        v_flex()
            .id("gantt-tree")
            .w(px(self.state.gantt.tree_width))
            .h_full()
            .overflow_hidden()
            .children(window.iter().map(|row| {
                let node = by_id.get(&row.id).cloned();
                let Some(node) = node else {
                    return div().into_any_element();
                };
                let id = node.id;
                let selected = self.state.gantt.selected_id == Some(id);
                let hint = crate::gantt::iteration_for(&node, &self.state.iterations);
                let dates = dates_column_text(&node, nodes, hint.as_ref());
                let depth = row.depth;
                let expanded = row.expanded;
                let has_children = row.has_children;
                h_flex()
                    .id(("work-item", id as u64))
                    .role(Role::Row)
                    .aria_label(format!("#{id} {}", node.title))
                    .h(px(ROW_HEIGHT))
                    .px_2()
                    .gap_2()
                    .ml(px(depth as f32 * 16.))
                    .bg(if selected {
                        cx.theme().list_active
                    } else {
                        cx.theme().background
                    })
                    .child(
                        Button::new(("expand", id as u64))
                            .label(if !has_children {
                                " "
                            } else if expanded {
                                "▾"
                            } else {
                                "▸"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.state.gantt.toggle_expand(id);
                                cx.notify();
                            })),
                    )
                    .child(div().child(format!("#{id}")))
                    .child(div().flex_1().child(node.title.clone()))
                    .child(self.type_badge(&node.work_item_type))
                    .child(div().id(("dates", id as u64)).child(dates))
                    .on_click(
                        cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| {
                            this.state.gantt.selected_id = Some(id);
                            if event.click_count() >= 2 {
                                this.open_form(id, cx);
                            }
                            cx.notify();
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, _, cx| {
                            this.state.gantt.context_menu = Some(crate::gantt::ContextTarget {
                                id,
                                from_bar: false,
                            });
                            cx.notify();
                        }),
                    )
                    .into_any_element()
            }))
    }

    fn type_badge(&self, ty: &str) -> impl IntoElement {
        let color = color_for_type(ty);
        let parsed = u32::from_str_radix(color.trim_start_matches('#'), 16).unwrap_or(0x89b4fa);
        h_flex()
            .gap_1()
            .px_1()
            .rounded_md()
            .child(div().size(px(8.)).rounded_full().bg(rgb(parsed)))
            .child(div().text_xs().child(ty.to_string()))
    }

    fn render_timeline(
        &mut self,
        bars: &[crate::gantt::BarLayout],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("gantt-timeline")
            .flex_1()
            .h_full()
            .relative()
            .overflow_hidden()
            .bg(cx.theme().background)
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                let mods = event.modifiers;
                if mods.control || mods.platform {
                    let factor = if event.delta.pixel_delta(px(12.)).y > px(0.) {
                        1.1
                    } else {
                        0.9
                    };
                    this.state.gantt.zoom_at(factor, 0.0, 0.0);
                } else {
                    this.state.gantt.scroll_x += f32::from(event.delta.pixel_delta(px(12.)).x);
                    this.state.gantt.scroll_y += f32::from(event.delta.pixel_delta(px(12.)).y);
                }
                cx.notify();
            }))
            .children(bars.iter().map(|bar| {
                let id = bar.id;
                let bar_x = bar.x;
                let bar_width = bar.width;
                let summary = bar.summary;
                div()
                    .id(("gantt-bar", id as u64))
                    .role(Role::Button)
                    .aria_label(format!("Gantt bar #{id}"))
                    .absolute()
                    .left(px(bar.x.max(0.0)))
                    .top(px(bar.y))
                    .w(px(bar.width))
                    .h(px(ROW_HEIGHT - 8.0))
                    .mt(px(4.))
                    .rounded_md()
                    .bg(if bar.summary {
                        cx.theme().muted
                    } else {
                        cx.theme().primary
                    })
                    .border_1()
                    .border_color(cx.theme().border)
                    .overflow_hidden()
                    .child(div().px_1().text_xs().child(bar.label.clone()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            if event.click_count >= 2 {
                                this.open_form(id, cx);
                                return;
                            }
                            if summary {
                                this.state.gantt.selected_id = Some(id);
                                cx.notify();
                                return;
                            }
                            let x: f32 = event.position.x.into();
                            let kind = if x <= bar_x + HANDLE_WIDTH {
                                GestureKind::ResizeStart
                            } else if x >= bar_x + bar_width - HANDLE_WIDTH {
                                GestureKind::ResizeEnd
                            } else {
                                GestureKind::Move
                            };
                            let _ = this.state.begin_bar_gesture(id, kind, x);
                            cx.notify();
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, _, cx| {
                            this.state.gantt.context_menu =
                                Some(crate::gantt::ContextTarget { id, from_bar: true });
                            cx.notify();
                        }),
                    )
            }))
            .on_mouse_move(
                cx.listener(|this, event: &gpui_kit::MouseMoveEvent, _, cx| {
                    if this.state.gantt.gesture.is_some() {
                        this.state.update_bar_gesture(event.position.x.into());
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.commit_gesture(cx);
                }),
            )
            .child(self.render_context_menu(cx))
    }

    fn render_context_menu(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(target) = self.state.gantt.context_menu else {
            return div().into_any_element();
        };
        let id = target.id;
        let from_row = !target.from_bar;
        v_flex()
            .id("gantt-context-menu")
            .absolute()
            .left(px(40.))
            .top(px(40.))
            .w(px(240.))
            .p_1()
            .gap_1()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .rounded_md()
            .shadow_md()
            .child(self.menu_item("Open", cx, move |this, cx| this.open_form(id, cx)))
            .child(
                self.menu_item("Open in Azure DevOps", cx, move |this, _cx| {
                    this.state.open_in_ado(id);
                }),
            )
            .child(self.menu_item("Copy ID", cx, move |this, _cx| this.state.copy_id(id)))
            .child(self.menu_item("Copy link", cx, move |this, _cx| this.state.copy_link(id)))
            .child(self.menu_item("Clear dates", cx, move |this, cx| {
                go!(this, cx, this.state.clear_dates(id));
            }))
            .child(
                self.menu_item("Schedule to iteration", cx, move |this, cx| {
                    go!(this, cx, this.state.schedule_to_iteration(id));
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
        Button::new(label)
            .label(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                on_click(this, cx);
                this.state.gantt.context_menu = None;
                cx.notify();
            }))
    }

    fn render_form(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(form) = self.state.form.as_ref() else {
            return div().into_any_element();
        };
        let title = form.title();
        let pages = form.model.pages.clone();
        let system = form.model.system_controls.clone();
        let active = form.active_page;
        v_flex()
            .id("work-item-form")
            .absolute()
            .inset_0()
            .bg(cx.theme().overlay)
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(720.))
                    .max_h(px(720.))
                    .bg(cx.theme().popover)
                    .rounded_xl()
                    .p_6()
                    .gap_3()
                    .child(div().text_lg().child(title))
                    .child(
                        h_flex()
                            .gap_2()
                            .children(pages.iter().enumerate().map(|(i, page)| {
                                Button::new(("page", i as u32))
                                    .label(page.label.clone())
                                    .selected(i == active)
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
                            .gap_2()
                            .children(system.iter().map(|control| self.render_field(control, cx))),
                    )
                    .when(active < pages.len(), |el| {
                        el.child(v_flex().gap_3().children(pages[active].groups.iter().map(
                            |group| {
                                v_flex()
                                    .gap_2()
                                    .child(Label::new(group.label.clone()))
                                    .children(
                                        group
                                            .controls
                                            .iter()
                                            .map(|control| self.render_field(control, cx)),
                                    )
                            },
                        )))
                    })
                    .child(
                        h_flex()
                            .gap_2()
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
        let label = control.label.clone();
        let id = control.reference_name.clone();
        v_flex()
            .gap_1()
            .child(Label::new(label))
            .child(match control.kind {
                FormControlKind::Html if !form.is_plain_html(&id) => {
                    let url = self
                        .state
                        .scope()
                        .map(|scope| ado_edit_url(&scope.org, &scope.project, form.model.id))
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
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .id(SharedString::from(format!("md-{id}")))
                                .border_1()
                                .border_color(cx.theme().border)
                                .px_2()
                                .py_1()
                                .child(markdown.clone()),
                        )
                        .child(TextView::markdown(
                            SharedString::from(format!("md-preview-{id}")),
                            markdown,
                        ))
                        .into_any_element()
                }
                FormControlKind::Boolean => Checkbox::new(SharedString::from(id))
                    .checked(value.as_bool().unwrap_or(false))
                    .into_any_element(),
                FormControlKind::Identity => {
                    let display = value
                        .get("displayName")
                        .and_then(|v| v.as_str())
                        .or_else(|| value.as_str())
                        .unwrap_or("")
                        .to_string();
                    let results = form.identity_results.clone();
                    v_flex()
                        .gap_1()
                        .child(div().child(display))
                        .children(results.into_iter().map(|identity| {
                            let label = identity.display_name.clone();
                            let unique = identity.unique_name.clone();
                            let field = id.clone();
                            Button::new(SharedString::from(format!("ident-{field}-{unique}")))
                                .label(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(form) = this.state.form.as_mut() {
                                        form.values.insert(
                                            field.clone(),
                                            crate::form::identity_json(&identity),
                                        );
                                    }
                                    cx.notify();
                                }))
                        }))
                        .into_any_element()
                }
                FormControlKind::Picklist => {
                    let options = control.options.clone().unwrap_or_default();
                    let selected = value.as_str().unwrap_or("").to_string();
                    h_flex()
                        .gap_1()
                        .children(options.into_iter().map(|opt| {
                            let field = id.clone();
                            let value = opt.clone();
                            Button::new(SharedString::from(format!("opt-{field}-{opt}")))
                                .label(opt)
                                .selected(value == selected)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(form) = this.state.form.as_mut() {
                                        form.set_string(&field, value.clone());
                                    }
                                    cx.notify();
                                }))
                        }))
                        .into_any_element()
                }
                _ => div()
                    .id(SharedString::from(id))
                    .border_1()
                    .border_color(cx.theme().border)
                    .px_2()
                    .py_1()
                    .child(value.as_str().unwrap_or("").to_string())
                    .into_any_element(),
            })
            .into_any_element()
    }

    fn open_form(&mut self, id: i64, cx: &mut Context<Self>) {
        go!(self, cx, self.state.open_form(id));
    }

    fn save_form(&mut self, cx: &mut Context<Self>) {
        go!(self, cx, self.state.save_form());
    }

    fn commit_gesture(&mut self, cx: &mut Context<Self>) {
        go!(self, cx, self.state.commit_gesture());
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            self.state.cancel_gesture();
            self.state.gantt.context_menu = None;
            cx.notify();
            return;
        }
        if event.keystroke.key == "enter" {
            if let Some(id) = self.state.gantt.selected_id {
                self.open_form(id, cx);
            }
            return;
        }
        if event.keystroke.key == "up" {
            let rows = self.state.visible_rows();
            self.state.gantt.select_relative(&rows, -1);
            cx.notify();
            return;
        }
        if event.keystroke.key == "down" {
            let rows = self.state.visible_rows();
            self.state.gantt.select_relative(&rows, 1);
            cx.notify();
            return;
        }
        if event.keystroke.key == "left" || event.keystroke.key == "right" {
            let delta = if event.keystroke.key == "left" { -1 } else { 1 };
            if self.state.gantt.context_menu.is_none() && self.state.form.is_none() {
                if event.keystroke.modifiers.shift {
                    go!(self, cx, self.state.nudge(NudgeAxis::Target, delta));
                    self.schedule_nudge_flush(cx);
                } else if event.keystroke.modifiers.alt {
                    go!(self, cx, self.state.nudge(NudgeAxis::Start, delta));
                    self.schedule_nudge_flush(cx);
                } else if event.keystroke.modifiers.control {
                    self.state.gantt.nudge_tree_width(delta as f32 * 16.0);
                    cx.notify();
                } else if self.state.gantt.selected_id.is_some() {
                    go!(self, cx, self.state.nudge(NudgeAxis::Both, delta));
                    self.schedule_nudge_flush(cx);
                } else {
                    self.state.gantt.nudge_tree_width(delta as f32 * 16.0);
                    cx.notify();
                }
            }
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
                        go!(this, cx, this.state.flush_nudge());
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

impl Render for PlannerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("ado-planner")
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .track_focus(&self.form_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.on_key(event, cx)))
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
                            .child(div().flex_1().child(self.render_gantt(cx)))
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
