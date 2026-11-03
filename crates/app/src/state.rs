use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ado_client::{AdoClient, AdoRuntime, TokenSource};
use ado_core::{
    AssigneeFilter, Flavor, HierarchyResult, IdentityValue, IterationNode, Organization,
    OverlayFilter, Project, ScopeSelection, SessionInfo, Team, WorkItemNode, apply_overlays,
    build_clear_dates_patch, build_dates_patch, build_field_patch, build_replace_dates_patch,
    choose_available, compact_date, identity_from_field, is_assignee_special, iso_date_only,
    parent_relation_op, parse_iso_date, resolve_root_types, schedule_from_iteration,
};
use ado_session::{LoginCreds, PlannerSession, SharedSession};
use jiff::ToSpan;
use jiff::Unit;
use jiff::civil::Date;
use serde_json::{Value, json};

use crate::cache::{CacheFreshness, QueryCache};
use crate::error::AppError;
use crate::form::FormDraft;
use crate::gantt::{
    GanttView, GestureKind, NudgeAxis, PendingNudge, SavedGanttView, VisibleRow,
    insert_index_for_child,
};
use crate::paths::AppPaths;
use crate::prefs::AppPrefs;
use crate::toasts::{
    DateRestore, DatesChange, EditHistory, HistoryEntry, ToastExtra, ToastKind, Toasts, UndoAction,
    error_text,
};
use crate::updater::Updater;

#[derive(Clone)]
pub struct PlannerState {
    pub paths: AppPaths,
    pub runtime: Arc<AdoRuntime>,
    pub session: SharedSession,
    pub client: Arc<AdoClient>,
    pub cache: QueryCache,
    pub prefs: AppPrefs,
    pub session_info: SessionInfo,
    pub session_loading: bool,
    pub sign_in_org: String,
    pub sign_in_pat: String,
    pub orgs: Vec<Organization>,
    pub projects: Vec<Project>,
    pub teams: Vec<Team>,
    pub iterations: Vec<IterationNode>,
    pub members: Vec<IdentityValue>,
    pub hierarchy: Option<HierarchyResult>,
    pub nodes: Vec<WorkItemNode>,
    pub orgs_loading: bool,
    pub projects_loading: bool,
    pub teams_loading: bool,
    pub iterations_loading: bool,
    pub members_loading: bool,
    pub hierarchy_loading: bool,
    pub team_settings_error: Option<String>,
    pub scope_locked: bool,
    pub gantt: GanttView,
    pub form: Option<FormDraft>,
    pub toasts: Toasts,
    pub history: EditHistory,
    pub updater: Updater,
    pub copied: Option<String>,
    last_nudge_at: Option<Instant>,
    history_recording: bool,
    team_views: HashMap<String, SavedGanttView>,
}

/// Why this date PATCH was queued — so a failed persist can put history back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateSaveRole {
    Commit,
    Undo,
    Redo,
    /// Toast Undo already updated history; persist the restored dates.
    Restore,
    /// Shift descendants after a parent move; persist without replacing the parent toast.
    Cascade,
}

/// Date change already applied and recorded locally; persist over HTTP next.
#[derive(Clone)]
pub struct PendingDateSave {
    pub id: i64,
    pub start: Option<Date>,
    pub target: Option<Date>,
    pub after_start: Option<String>,
    pub after_target: Option<String>,
    pub previous: WorkItemNode,
    pub role: DateSaveRole,
    pub rev: i64,
}

impl PlannerState {
    pub fn new(
        paths: AppPaths,
        session: PlannerSession,
        ado_base: Option<String>,
        vssps_base: Option<String>,
    ) -> Self {
        let prefs = AppPrefs::load(&paths.prefs_file());
        let cache = QueryCache::load(paths.cache_file());
        let shared = SharedSession::new(session);
        let client = match (ado_base, vssps_base) {
            (Some(ado), Some(vssps)) => {
                AdoClient::with_base_urls(Arc::new(shared.clone()), ado, vssps)
            }
            _ => AdoClient::new(Arc::new(shared.clone())),
        };
        Self {
            paths,
            runtime: Arc::new(AdoRuntime::default()),
            session: shared,
            client: Arc::new(client),
            cache,
            session_info: SessionInfo {
                signed_in: false,
                display_name: None,
                username: None,
                auth_mode: ado_core::AuthMode::Pat,
            },
            session_loading: true,
            sign_in_org: String::new(),
            sign_in_pat: String::new(),
            orgs: Vec::new(),
            projects: Vec::new(),
            teams: Vec::new(),
            iterations: Vec::new(),
            members: Vec::new(),
            hierarchy: None,
            nodes: Vec::new(),
            orgs_loading: false,
            projects_loading: false,
            teams_loading: false,
            iterations_loading: false,
            members_loading: false,
            hierarchy_loading: false,
            team_settings_error: None,
            scope_locked: false,
            gantt: GanttView::default(),
            form: None,
            toasts: Toasts::default(),
            history: EditHistory::default(),
            updater: Updater::from_env(),
            copied: None,
            last_nudge_at: None,
            history_recording: true,
            team_views: HashMap::new(),
            prefs,
        }
    }

    pub fn flavor(&self) -> Flavor {
        self.prefs.flavor
    }

    pub fn set_flavor(&mut self, flavor: Flavor) {
        self.prefs.flavor = flavor;
        self.persist_prefs();
    }

    pub fn persist_prefs(&self) {
        self.prefs.save(&self.paths.prefs_file());
    }

    pub fn initials(&self) -> String {
        let name = self
            .session_info
            .display_name
            .clone()
            .unwrap_or_else(|| "Signed in".into());
        let parts: Vec<_> = name.split_whitespace().filter(|p| !p.is_empty()).collect();
        match parts.as_slice() {
            [] => "?".into(),
            [one] => one.chars().take(2).collect::<String>().to_uppercase(),
            [first, .., last] => format!(
                "{}{}",
                first.chars().next().unwrap_or('?'),
                last.chars().next().unwrap_or('?')
            )
            .to_uppercase(),
        }
    }

    pub fn scope(&self) -> Option<ScopeSelection> {
        let org = self.prefs.planner.org.clone();
        let project = self.prefs.planner.project.clone();
        let team = self.prefs.planner.team.clone();
        if org.is_empty() || project.is_empty() || team.is_empty() {
            return None;
        }
        Some(ScopeSelection {
            org,
            project,
            team,
            iteration_path: if self.prefs.planner.iteration_path.is_empty() {
                None
            } else {
                Some(self.prefs.planner.iteration_path.clone())
            },
        })
    }

    pub fn overlay(&self) -> OverlayFilter {
        let types = self
            .hierarchy
            .as_ref()
            .map(|h| h.types.clone())
            .unwrap_or_else(|| {
                let mut types: Vec<_> = self
                    .nodes
                    .iter()
                    .map(|n| n.work_item_type.clone())
                    .collect();
                types.sort();
                types.dedup();
                types
            });
        let mut states: Vec<_> = self.nodes.iter().map(|n| n.state.clone()).collect();
        states.sort();
        states.dedup();
        let resolved = self.planner_root_types(&types);
        OverlayFilter {
            types: if self.prefs.planner.hidden_types.is_empty() {
                None
            } else {
                Some(
                    types
                        .iter()
                        .filter(|ty| !self.prefs.planner.hidden_types.contains(ty))
                        .cloned()
                        .collect(),
                )
            },
            states: if self.prefs.planner.hidden_states.is_empty() {
                None
            } else {
                Some(
                    states
                        .iter()
                        .filter(|st| !self.prefs.planner.hidden_states.contains(st))
                        .cloned()
                        .collect(),
                )
            },
            root_types: resolved,
            assignee: self.prefs.planner.assignee.clone(),
            iteration_path: if self.prefs.planner.iteration_path.is_empty() {
                None
            } else {
                Some(self.prefs.planner.iteration_path.clone())
            },
            current_user_unique_name: self.session_info.username.clone(),
        }
    }

    pub fn visible_nodes(&self) -> Vec<WorkItemNode> {
        apply_overlays(&self.nodes, &self.overlay())
    }

    pub fn types(&self) -> Vec<String> {
        let mut types = self
            .hierarchy
            .as_ref()
            .map(|h| h.types.clone())
            .unwrap_or_default();
        if types.is_empty() {
            types = self
                .nodes
                .iter()
                .map(|n| n.work_item_type.clone())
                .collect();
            types.sort();
            types.dedup();
        }
        types
    }

    pub fn states(&self) -> Vec<String> {
        let mut states: Vec<_> = self.nodes.iter().map(|n| n.state.clone()).collect();
        states.sort();
        states.dedup();
        states
    }

    fn planner_root_types(&self, types: &[String]) -> Option<Vec<String>> {
        // Unset Roots prefs mean every loaded type, matching the first-load default.
        // Team switch must not reseed from the team's top backlog (Epic-only).
        resolve_root_types(self.prefs.planner.root_types.clone(), types, &[])
    }

    pub fn hidden_roots(&self) -> Vec<String> {
        let types = self.types();
        let resolved = self.planner_root_types(&types);
        match resolved {
            None => Vec::new(),
            Some(roots) => types.into_iter().filter(|ty| !roots.contains(ty)).collect(),
        }
    }

    pub fn hidden_root_count(&self) -> usize {
        self.hidden_roots().len()
    }

    pub fn selected_root_count(&self) -> usize {
        self.types().len().saturating_sub(self.hidden_root_count())
    }

    pub fn toggle_hidden_root(&mut self, ty: &str) {
        let mut hidden = self.hidden_roots();
        if let Some(index) = hidden.iter().position(|item| item == ty) {
            hidden.remove(index);
        } else {
            hidden.push(ty.to_string());
        }
        self.set_hidden_roots(hidden);
    }

    pub fn set_hidden_roots(&mut self, hidden: Vec<String>) {
        let types = self.types();
        if types.is_empty() {
            return;
        }
        if hidden.is_empty() {
            self.prefs.planner.root_types = Some(None);
        } else if hidden.len() == types.len() {
            self.prefs.planner.root_types = Some(Some(Vec::new()));
        } else {
            self.prefs.planner.root_types = Some(Some(
                types
                    .into_iter()
                    .filter(|ty| !hidden.contains(ty))
                    .collect(),
            ));
        }
        self.persist_prefs();
    }

    pub fn toggle_hidden_type(&mut self, ty: &str) {
        toggle_hidden(&mut self.prefs.planner.hidden_types, ty);
        self.persist_prefs();
    }

    pub fn toggle_hidden_state(&mut self, state: &str) {
        toggle_hidden(&mut self.prefs.planner.hidden_states, state);
        self.persist_prefs();
    }

    pub fn hide_all_types(&mut self) {
        self.prefs.planner.hidden_types = self.types();
        self.persist_prefs();
    }

    pub fn show_all_types(&mut self) {
        self.prefs.planner.hidden_types.clear();
        self.persist_prefs();
    }

    pub fn hide_all_states(&mut self) {
        self.prefs.planner.hidden_states = self.states();
        self.persist_prefs();
    }

    pub fn show_all_states(&mut self) {
        self.prefs.planner.hidden_states.clear();
        self.persist_prefs();
    }

    pub fn hide_all_roots(&mut self) {
        self.set_hidden_roots(self.types());
    }

    pub fn show_all_roots(&mut self) {
        self.set_hidden_roots(Vec::new());
    }

    pub fn assignee_options(&self) -> Vec<(String, String)> {
        let mut options = vec![("anyone".into(), "Anyone".into())];
        if self.session_info.username.is_some() {
            options.push(("me".into(), "Me".into()));
        }
        options.push(("unassigned".into(), "Unassigned".into()));
        for member in &self.members {
            options.push((member.unique_name.clone(), member.display_name.clone()));
        }
        options
    }

    pub fn iteration_options(&self) -> Vec<(String, String)> {
        self.iterations
            .iter()
            .map(|row| (row.path.clone(), row.name.clone()))
            .collect()
    }

    pub async fn bootstrap(&mut self) -> Result<(), AppError> {
        self.session_loading = true;
        let info = self.session.inner().lock().await.session_info().await?;
        self.session_info = info;
        self.session_loading = false;
        if self.session_info.signed_in {
            self.warn_if_identity_missing();
            self.reload_after_sign_in().await?;
        }
        Ok(())
    }

    pub async fn login_pat(&mut self) -> Result<(), AppError> {
        let info = self
            .session
            .inner()
            .lock()
            .await
            .login(Some(LoginCreds {
                pat: Some(self.sign_in_pat.clone()),
                organization: Some(self.sign_in_org.clone()),
            }))
            .await?;
        self.session_info = info;
        if !self.session_info.signed_in {
            self.toasts
                .push(ToastKind::Error, "Sign-in did not create a Session");
            return Ok(());
        }
        self.warn_if_identity_missing();
        self.reload_after_sign_in().await
    }

    pub async fn login_entra(&mut self) -> Result<(), AppError> {
        let info = self.session.inner().lock().await.login(None).await?;
        self.session_info = info;
        if !self.session_info.signed_in {
            self.toasts
                .push(ToastKind::Error, "Sign-in did not create a Session");
            return Ok(());
        }
        self.warn_if_identity_missing();
        self.reload_after_sign_in().await
    }

    fn warn_if_identity_missing(&mut self) {
        if self.session_info.username.is_none() {
            self.toasts.push(
                ToastKind::Warning,
                "Could not load your identity. Assignee → Me is unavailable. The PAT may be missing the Profile scope (vso.profile).",
            );
        }
    }

    pub async fn logout(&mut self) -> Result<(), AppError> {
        self.session.inner().lock().await.logout().await?;
        self.session_info = self.session.inner().lock().await.session_info().await?;
        self.orgs.clear();
        self.projects.clear();
        self.teams.clear();
        self.iterations.clear();
        self.members.clear();
        self.hierarchy = None;
        self.nodes.clear();
        self.cache.clear();
        Ok(())
    }

    async fn reload_after_sign_in(&mut self) -> Result<(), AppError> {
        self.load_orgs().await?;
        self.heal_and_load_projects().await?;
        self.heal_and_load_teams().await?;
        self.heal_and_load_iterations().await?;
        self.load_members().await?;
        self.heal_assignee();
        self.load_hierarchy().await?;
        Ok(())
    }

    pub async fn load_orgs(&mut self) -> Result<(), AppError> {
        self.orgs_loading = true;
        let session_org = self
            .session
            .organization()
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        let key = self.cache_key("orgs", &session_org);
        if let Some(hit) = self.cache.get(&key)
            && let Ok(orgs) = serde_json::from_value::<Vec<Organization>>(hit.value.clone())
        {
            self.orgs = orgs;
            if hit.freshness == CacheFreshness::Fresh {
                self.orgs_loading = false;
                self.heal_org();
                return Ok(());
            }
        }
        match self.client.list_organizations().await {
            Ok(orgs) => {
                self.orgs = orgs;
                if let Ok(value) = serde_json::to_value(&self.orgs) {
                    self.cache.set(&key, &value);
                }
                self.heal_org();
            }
            Err(err) => {
                self.toasts.push(
                    ToastKind::Error,
                    error_text(&err, "Could not load organizations"),
                );
                self.orgs_loading = false;
                return Err(err.into());
            }
        }
        self.orgs_loading = false;
        Ok(())
    }

    fn heal_org(&mut self) {
        let names: Vec<_> = self.orgs.iter().map(|o| o.account_name.clone()).collect();
        let next = choose_available(&self.prefs.planner.org, &names);
        if next != self.prefs.planner.org {
            self.prefs.planner.org = next;
            self.prefs.planner.project.clear();
            self.prefs.planner.team.clear();
            self.prefs.planner.iteration_path.clear();
            self.persist_prefs();
        }
    }

    pub async fn set_org(&mut self, org: String) -> Result<(), AppError> {
        self.scope_locked = true;
        self.prefs.planner.org = org;
        self.prefs.planner.project.clear();
        self.prefs.planner.team.clear();
        self.prefs.planner.iteration_path.clear();
        self.persist_prefs();
        let result = self.reload_from_org().await;
        self.scope_locked = false;
        result
    }

    async fn reload_from_org(&mut self) -> Result<(), AppError> {
        self.heal_and_load_projects().await?;
        self.heal_and_load_teams().await?;
        self.heal_and_load_iterations().await?;
        self.load_members().await?;
        self.heal_assignee();
        self.load_hierarchy().await
    }

    pub async fn heal_and_load_projects(&mut self) -> Result<(), AppError> {
        let org = self.prefs.planner.org.clone();
        if org.is_empty() {
            self.projects.clear();
            return Ok(());
        }
        self.projects_loading = true;
        let key = self.cache_key("projects", &org);
        if let Some(hit) = self.cache.get(&key)
            && let Ok(projects) = serde_json::from_value::<Vec<Project>>(hit.value.clone())
        {
            self.projects = projects;
            if hit.freshness == CacheFreshness::Fresh {
                self.projects_loading = false;
                self.heal_project();
                return Ok(());
            }
        }
        match self.client.list_projects(&org).await {
            Ok(projects) => {
                self.projects = projects;
                if let Ok(value) = serde_json::to_value(&self.projects) {
                    self.cache.set(&key, &value);
                }
                self.heal_project();
            }
            Err(err) => {
                self.toasts.push(
                    ToastKind::Error,
                    error_text(&err, "Could not load projects"),
                );
                self.projects_loading = false;
                return Err(err.into());
            }
        }
        self.projects_loading = false;
        Ok(())
    }

    fn heal_project(&mut self) {
        let names: Vec<_> = self.projects.iter().map(|p| p.name.clone()).collect();
        let next = choose_available(&self.prefs.planner.project, &names);
        if next != self.prefs.planner.project {
            self.prefs.planner.project = next;
            self.prefs.planner.team.clear();
            self.prefs.planner.iteration_path.clear();
            self.persist_prefs();
        }
    }

    pub async fn set_project(&mut self, project: String) -> Result<(), AppError> {
        self.prefs.planner.project = project;
        self.prefs.planner.team.clear();
        self.prefs.planner.iteration_path.clear();
        self.persist_prefs();
        self.heal_and_load_teams().await?;
        self.heal_and_load_iterations().await?;
        self.load_members().await?;
        self.load_hierarchy().await
    }

    pub async fn heal_and_load_teams(&mut self) -> Result<(), AppError> {
        let org = self.prefs.planner.org.clone();
        let project = self.prefs.planner.project.clone();
        if org.is_empty() || project.is_empty() {
            self.teams.clear();
            return Ok(());
        }
        self.teams_loading = true;
        let key = self.cache_key("teams", &format!("{org}:{project}"));
        if let Some(hit) = self.cache.get(&key)
            && let Ok(teams) = serde_json::from_value::<Vec<Team>>(hit.value.clone())
        {
            self.teams = teams;
            if hit.freshness == CacheFreshness::Fresh {
                self.teams_loading = false;
                self.heal_team();
                return Ok(());
            }
        }
        match self.client.list_teams(&org, &project).await {
            Ok(teams) => {
                self.teams = teams;
                if let Ok(value) = serde_json::to_value(&self.teams) {
                    self.cache.set(&key, &value);
                }
                self.heal_team();
            }
            Err(err) => {
                self.toasts
                    .push(ToastKind::Error, error_text(&err, "Could not load Teams"));
                self.teams_loading = false;
                return Err(err.into());
            }
        }
        self.teams_loading = false;
        Ok(())
    }

    fn heal_team(&mut self) {
        let names: Vec<_> = self.teams.iter().map(|t| t.name.clone()).collect();
        let next = choose_available(&self.prefs.planner.team, &names);
        if next != self.prefs.planner.team {
            self.prefs.planner.team = next;
            self.prefs.planner.iteration_path.clear();
            self.persist_prefs();
        }
    }

    pub async fn set_team(&mut self, team: String) -> Result<(), AppError> {
        if team != self.prefs.planner.team {
            self.save_gantt_view();
        }
        self.prefs.planner.team = team;
        self.prefs.planner.iteration_path.clear();
        self.prefs.planner.root_types = Some(None);
        self.gantt.clear_hover();
        self.persist_prefs();
        self.heal_and_load_iterations().await?;
        self.load_members().await?;
        self.load_hierarchy().await
    }

    fn save_gantt_view(&mut self) {
        let team = self.prefs.planner.team.clone();
        if team.is_empty() || self.nodes.is_empty() {
            return;
        }
        self.team_views.insert(team, self.gantt.snapshot());
    }

    pub fn has_saved_gantt_view(&self) -> bool {
        let team = self.prefs.planner.team.as_str();
        !team.is_empty() && self.team_views.contains_key(team)
    }

    /// Replace this state with a spawned clone. Keep live per-team Gantt
    /// snapshots, and keep the live camera when the team did not change so a
    /// date persist cannot wipe a pan the user made while I/O was in flight.
    pub fn adopt_spawned(&mut self, working: PlannerState) {
        self.save_gantt_view();
        let live_views = std::mem::take(&mut self.team_views);
        let live_team = self.prefs.planner.team.clone();
        let live_snap = (!self.nodes.is_empty()).then(|| self.gantt.snapshot());
        let keep_live_gantt = live_snap.is_some() && working.prefs.planner.team == live_team;
        *self = working;
        for (team, snap) in live_views {
            self.team_views.entry(team).or_insert(snap);
        }
        if let Some(snap) = live_snap {
            self.team_views.entry(live_team).or_insert(snap);
            if keep_live_gantt {
                self.gantt.restore(snap);
            }
        }
    }

    /// Drop stale rows and show the new team name before the hierarchy fetch returns.
    pub fn begin_team_switch(&mut self, team: String) {
        self.save_gantt_view();
        self.prefs.planner.team = team;
        self.prefs.planner.iteration_path.clear();
        self.prefs.planner.root_types = Some(None);
        self.nodes.clear();
        self.hierarchy = None;
        self.team_settings_error = None;
        self.hierarchy_loading = true;
        self.gantt.clear_hover();
        self.gantt.selected_id = None;
        self.gantt.hovered_id = None;
        self.gantt.selected_link = None;
    }

    pub fn hidden_by_filters_count(&self) -> usize {
        let visible: HashSet<i64> = self.visible_nodes().iter().map(|node| node.id).collect();
        self.nodes
            .iter()
            .filter(|node| !visible.contains(&node.id))
            .count()
    }

    pub fn filters_hide_work_items(&self) -> bool {
        !self.nodes.is_empty() && self.visible_nodes().is_empty()
    }

    pub async fn heal_and_load_iterations(&mut self) -> Result<(), AppError> {
        let Some(scope) = self.scope() else {
            self.iterations.clear();
            return Ok(());
        };
        self.iterations_loading = true;
        let key = self.cache_key(
            "iterations",
            &format!("{}:{}:{}", scope.org, scope.project, scope.team),
        );
        if let Some(hit) = self.cache.get(&key)
            && let Ok(iterations) = serde_json::from_value::<Vec<IterationNode>>(hit.value.clone())
        {
            self.iterations = iterations;
            if hit.freshness == CacheFreshness::Fresh {
                self.iterations_loading = false;
                self.heal_iteration();
                return Ok(());
            }
        }
        match self
            .client
            .list_iterations(&scope.org, &scope.project, &scope.team)
            .await
        {
            Ok(iterations) => {
                self.iterations = iterations;
                if let Ok(value) = serde_json::to_value(&self.iterations) {
                    self.cache.set(&key, &value);
                }
                self.heal_iteration();
            }
            Err(err) => {
                self.toasts.push(
                    ToastKind::Error,
                    error_text(&err, "Could not load iterations"),
                );
                self.iterations_loading = false;
                return Err(err.into());
            }
        }
        self.iterations_loading = false;
        Ok(())
    }

    fn heal_iteration(&mut self) {
        if self.prefs.planner.iteration_path.is_empty() {
            return;
        }
        let paths: HashSet<_> = self.iterations.iter().map(|i| i.path.as_str()).collect();
        if !paths.contains(self.prefs.planner.iteration_path.as_str()) {
            self.prefs.planner.iteration_path.clear();
            self.persist_prefs();
        }
    }

    pub fn set_iteration(&mut self, path: String) {
        self.prefs.planner.iteration_path = path;
        self.persist_prefs();
    }

    pub async fn load_members(&mut self) -> Result<(), AppError> {
        let Some(scope) = self.scope() else {
            self.members.clear();
            return Ok(());
        };
        self.members_loading = true;
        match self
            .client
            .list_team_members(&scope.org, &scope.project, &scope.team)
            .await
        {
            Ok(members) => self.members = members,
            Err(err) => {
                self.toasts.push(
                    ToastKind::Warning,
                    error_text(&err, "Could not load Team members"),
                );
                self.members.clear();
            }
        }
        self.members_loading = false;
        Ok(())
    }

    fn heal_assignee(&mut self) {
        let assignee = self.prefs.planner.assignee.clone();
        if assignee == "me" && self.session_info.username.is_none() {
            self.prefs.planner.assignee = "anyone".into();
            self.persist_prefs();
            return;
        }
        if is_assignee_special(&assignee) {
            return;
        }
        if self.prefs.planner.team.is_empty() {
            self.prefs.planner.assignee = "anyone".into();
            self.persist_prefs();
            return;
        }
        let names: HashSet<_> = self
            .members
            .iter()
            .map(|m| m.unique_name.to_lowercase())
            .collect();
        if !names.contains(&assignee.to_lowercase()) {
            self.prefs.planner.assignee = "anyone".into();
            self.persist_prefs();
        }
    }

    pub fn set_assignee(&mut self, assignee: AssigneeFilter) {
        self.prefs.planner.assignee = assignee;
        self.persist_prefs();
    }

    pub async fn load_hierarchy(&mut self) -> Result<(), AppError> {
        let Some(scope) = self.scope() else {
            self.hierarchy = None;
            self.nodes.clear();
            return Ok(());
        };
        self.hierarchy_loading = true;
        self.team_settings_error = None;
        let key = self.hierarchy_cache_key(&scope);
        // Always refetch rows. A Fresh cache hit is a per-team snapshot of
        // nodes and can overwrite a date persist that only updated a worker
        // clone's cache. Camera restore stays in apply_hierarchy.
        match self.client.load_hierarchy(&scope).await {
            Ok(result) => {
                if let Ok(value) = serde_json::to_value(&result) {
                    self.cache.set(&key, &value);
                }
                self.apply_hierarchy(result);
            }
            Err(err) => {
                let message = error_text(&err, "Could not load Hierarchy");
                if err.is_backlogs_failure() || message.contains("TF400497") {
                    self.team_settings_error = Some(team_settings_banner(&scope.team, &message));
                    self.nodes.clear();
                    self.hierarchy = None;
                    self.hierarchy_loading = false;
                    return Ok(());
                }
                self.toasts.push(ToastKind::Error, message);
                self.hierarchy_loading = false;
                return Err(err.into());
            }
        }
        self.hierarchy_loading = false;
        Ok(())
    }

    fn apply_hierarchy(&mut self, result: HierarchyResult) {
        if result.truncated {
            self.toasts.push(
                ToastKind::Warning,
                "Hierarchy is incomplete — WIQL hit the 20,000 result cap.",
            );
        }
        self.team_settings_error = result.warning.clone();
        self.nodes = result.nodes.clone();
        self.hierarchy = Some(result);
        if let Some(saved) = self.team_views.get(&self.prefs.planner.team).cloned() {
            self.gantt.restore(saved);
            let rows = self.gantt.flatten(&self.visible_nodes()).len();
            self.gantt.clamp_scroll(rows, self.gantt.viewport_h);
        } else if self.team_views.is_empty() {
            let width = self.gantt.timeline_width.max(800.0);
            let nodes = self.visible_nodes();
            self.gantt.auto_fit(&nodes, &self.iterations, width);
        } else {
            self.gantt.scroll_y = 0.0;
        }
        // A team switch without a saved camera keeps the live date range and scale.
    }

    fn cache_identity(&self) -> String {
        self.session_info
            .username
            .clone()
            .or_else(|| self.session_info.display_name.clone())
            .unwrap_or_default()
    }

    fn cache_key(&self, kind: &str, rest: &str) -> String {
        format!("{kind}:{}:{rest}", self.cache_identity())
    }

    fn hierarchy_cache_key(&self, scope: &ScopeSelection) -> String {
        self.cache_key(
            "hierarchy",
            &format!("{}:{}:{}", scope.org, scope.project, scope.team),
        )
    }

    fn persist_hierarchy_cache(&mut self) {
        let Some(scope) = self.scope() else {
            return;
        };
        let key = self.hierarchy_cache_key(&scope);
        let Some(hierarchy) = self.hierarchy.as_mut() else {
            return;
        };
        hierarchy.nodes = self.nodes.clone();
        if let Ok(value) = serde_json::to_value(&*hierarchy) {
            self.cache.set(&key, &value);
        }
    }

    pub fn visible_rows(&self) -> Vec<VisibleRow> {
        self.gantt.flatten(&self.visible_nodes())
    }

    pub async fn open_form(&mut self, id: i64) -> Result<(), AppError> {
        let Some(scope) = self.scope() else {
            return Ok(());
        };
        match self.client.load_form(&scope.org, &scope.project, id).await {
            Ok(model) => {
                let mut draft = FormDraft::from_model(model);
                if let Some(node) = self.nodes.iter().find(|node| node.id == id) {
                    draft.hydrate_assigned_to(node);
                }
                self.form = Some(draft);
                Ok(())
            }
            Err(err) => {
                self.toasts.push(
                    ToastKind::Error,
                    error_text(&err, "Could not open Work Item"),
                );
                Err(err.into())
            }
        }
    }

    pub fn close_form(&mut self) {
        self.form = None;
        self.gantt.clear_hover();
        self.gantt.hover_locked = false;
    }

    /// Copy the live node's rev and dates into the open details dialog.
    pub fn sync_open_form_from_node(&mut self, id: i64) {
        let Some(node) = self.nodes.iter().find(|node| node.id == id).cloned() else {
            return;
        };
        if let Some(form) = self.form.as_mut() {
            form.adopt_live_node(&node);
        }
    }

    pub async fn save_form(&mut self) -> Result<(), AppError> {
        let Some(scope) = self.scope() else {
            return Ok(());
        };
        let Some(form) = self.form.as_mut() else {
            return Ok(());
        };
        form.validate()?;
        form.saving = true;
        let ops = form.patch_ops();
        let original_values = form.model.values.clone();
        let id = form.model.id;
        let before = self.nodes.iter().find(|node| node.id == id).cloned();
        match self
            .client
            .patch_work_item(&scope.org, &scope.project, id, &ops)
            .await
        {
            Ok(rev) => {
                let labels: std::collections::HashMap<String, String> =
                    if let Some(form) = self.form.as_ref() {
                        ado_core::all_editable_controls(&form.model)
                            .into_iter()
                            .map(|control| (control.reference_name.clone(), control.label.clone()))
                            .collect()
                    } else {
                        std::collections::HashMap::new()
                    };
                if let Some(form) = self.form.as_ref() {
                    form.apply_saved(&mut self.nodes, rev);
                }
                let dates_changed = matches!(
                    (before.as_ref(), self.nodes.iter().find(|node| node.id == id)),
                    (Some(before), Some(after))
                        if before.start_date != after.start_date
                            || before.target_date != after.target_date
                );
                self.form = None;
                self.persist_hierarchy_cache();
                let names: Vec<String> = ops
                    .iter()
                    .filter_map(|op| {
                        op.path.strip_prefix("/fields/").map(|reference| {
                            field_toast_name(reference, labels.get(reference).map(String::as_str))
                        })
                    })
                    .collect();
                let only_dates = !names.is_empty()
                    && names
                        .iter()
                        .all(|name| name == "start date" || name == "target date");
                let field_changes = field_changes_from_ops(&ops, &original_values);
                let has_non_date = field_changes
                    .iter()
                    .any(|(reference, _, _)| !is_scheduling_date(reference));
                if dates_changed && !has_non_date {
                    if let (Some(before), Some(after)) = (
                        before.as_ref(),
                        self.nodes.iter().find(|node| node.id == id),
                    ) {
                        self.record_dates(
                            id,
                            before,
                            after.start_date.clone(),
                            after.target_date.clone(),
                        );
                    }
                } else if !field_changes.is_empty() {
                    self.record(HistoryEntry::Fields {
                        id,
                        changes: field_changes.clone(),
                    });
                }
                let message = if names.is_empty() {
                    format!("Saved Work Item #{id}")
                } else if only_dates {
                    let title = self.item_title(id);
                    let (start, target) = self
                        .nodes
                        .iter()
                        .find(|node| node.id == id)
                        .map(|node| {
                            (
                                parse_iso_date(node.start_date.as_deref()),
                                parse_iso_date(node.target_date.as_deref()),
                            )
                        })
                        .unwrap_or((None, None));
                    dates_moved_message(&title, start, target)
                } else if names.len() == 1 && names[0] == "title" {
                    let old = before
                        .as_ref()
                        .map(|node| node.title.trim())
                        .filter(|title| !title.is_empty())
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| format!("#{id}"));
                    renamed_message(&old, &self.item_title(id))
                } else {
                    format!(
                        "Saved {} on {}",
                        names.join(", "),
                        quote_name(&self.item_title(id))
                    )
                };
                if only_dates {
                    if let Some(before) = before.as_ref() {
                        self.toasts.push_undo(
                            message,
                            UndoAction::RestoreDates {
                                id,
                                start_date: before.start_date.clone(),
                                target_date: before.target_date.clone(),
                                rev: before.rev,
                            },
                        );
                    } else {
                        self.toasts.push(ToastKind::Success, message);
                    }
                } else if !field_changes.is_empty() {
                    self.toasts.push_undo(
                        message,
                        UndoAction::RestoreFields {
                            id,
                            fields: field_changes
                                .into_iter()
                                .map(|(reference, before, _)| (reference, before))
                                .collect(),
                        },
                    );
                } else {
                    self.toasts.push(ToastKind::Success, message);
                }
                Ok(())
            }
            Err(err) if err.is_precondition_failed() => {
                self.toasts.push(
                    ToastKind::Error,
                    "Work Item changed in Azure DevOps. Your edits are kept — review and save again.",
                );
                let draft = self.form.as_ref().map(|form| form.values.clone());
                if let Some(form) = self.form.as_mut() {
                    form.saving = false;
                }
                let _ = self.open_form(id).await;
                if let (Some(draft), Some(form)) = (draft, self.form.as_mut()) {
                    form.values = draft;
                }
                Err(err.into())
            }
            Err(err) => {
                if let Some(form) = self.form.as_mut() {
                    form.saving = false;
                }
                self.toasts.push(
                    ToastKind::Error,
                    error_text(&err, "Could not save Work Item"),
                );
                Err(err.into())
            }
        }
    }

    pub async fn search_identities(&mut self, query: String) -> Result<(), AppError> {
        let Some(scope) = self.scope() else {
            return Ok(());
        };
        if let Some(form) = self.form.as_mut() {
            form.identity_query = query.clone();
        }
        if query.trim().is_empty() {
            return Ok(());
        }
        let results = self.client.search_identities(&scope.org, &query).await?;
        if let Some(form) = self.form.as_mut() {
            form.identity_results = results;
        }
        Ok(())
    }

    pub fn apply_optimistic_dates(
        &mut self,
        id: i64,
        start: Date,
        target: Date,
    ) -> Option<WorkItemNode> {
        self.apply_optimistic_partial(id, Some(start), Some(target))
    }

    pub fn apply_optimistic_partial(
        &mut self,
        id: i64,
        start: Option<Date>,
        target: Option<Date>,
    ) -> Option<WorkItemNode> {
        let node = self.nodes.iter_mut().find(|node| node.id == id)?;
        let previous = node.clone();
        if let Some(start) = start {
            node.start_date = Some(iso_date_only(start));
        }
        if let Some(target) = target {
            node.target_date = Some(iso_date_only(target));
        }
        Some(previous)
    }

    fn restore_partial_dates(&mut self, id: i64, start: Option<Date>, target: Option<Date>) {
        if let Some(node) = self.nodes.iter_mut().find(|node| node.id == id) {
            node.start_date = start.map(iso_date_only);
            node.target_date = target.map(iso_date_only);
        }
    }

    fn restore_nudge(&mut self, pending: &PendingNudge) {
        self.restore_partial_dates(
            pending.id,
            pending.had_start.then_some(pending.original_start),
            pending.had_target.then_some(pending.original_target),
        );
    }

    pub fn restore_node(&mut self, previous: WorkItemNode) {
        if let Some(node) = self.nodes.iter_mut().find(|node| node.id == previous.id) {
            *node = previous;
        }
    }

    fn record(&mut self, entry: HistoryEntry) {
        if self.history_recording {
            self.history.push(entry);
        }
    }

    fn record_dates(
        &mut self,
        id: i64,
        before: &WorkItemNode,
        after_start: Option<String>,
        after_target: Option<String>,
    ) {
        if before.start_date == after_start && before.target_date == after_target {
            return;
        }
        self.record(HistoryEntry::Dates {
            id,
            before_start: before.start_date.clone(),
            before_target: before.target_date.clone(),
            after_start,
            after_target,
        });
    }

    pub fn discard_last_dates(&mut self, id: i64) {
        match self.history.undo.last() {
            Some(HistoryEntry::Dates { id: entry_id, .. }) if *entry_id == id => {
                self.history.undo.pop();
            }
            Some(HistoryEntry::DatesGroup { items }) if items.iter().any(|item| item.id == id) => {
                self.history.undo.pop();
            }
            _ => {}
        }
    }

    /// Drop the matching Dates undo entry, not merely the last one for `id`.
    /// A failed second persist must not swallow the first drag.
    pub fn discard_dates_after(
        &mut self,
        id: i64,
        after_start: &Option<String>,
        after_target: &Option<String>,
    ) {
        if let Some(index) = self.history.undo.iter().rposition(|entry| {
            matches!(
                entry,
                HistoryEntry::Dates {
                    id: entry_id,
                    after_start: start,
                    after_target: target,
                    ..
                } if *entry_id == id && start == after_start && target == after_target
            )
        }) {
            self.history.undo.remove(index);
        }
    }

    pub fn revert_date_save_history(&mut self, pending: &PendingDateSave) {
        match pending.role {
            DateSaveRole::Commit => {
                self.discard_dates_after(pending.id, &pending.after_start, &pending.after_target);
            }
            DateSaveRole::Undo => {
                if let Some(entry) = self.history.redo.pop() {
                    self.history.undo.push(entry);
                }
            }
            DateSaveRole::Redo => {
                if let Some(entry) = self.history.undo.pop() {
                    self.history.redo.push(entry);
                }
            }
            DateSaveRole::Restore => {}
            DateSaveRole::Cascade => {
                self.discard_dates_after(pending.id, &pending.after_start, &pending.after_target);
            }
        }
    }

    /// Drop the matching Dates undo entry after a toast Restore persist succeeds.
    pub fn complete_restore_date_save(&mut self, pending: &PendingDateSave) {
        if pending.role != DateSaveRole::Restore {
            return;
        }
        self.discard_history_for_undo(&UndoAction::RestoreDates {
            id: pending.id,
            start_date: pending.after_start.clone(),
            target_date: pending.after_target.clone(),
            rev: pending.rev,
        });
    }

    /// Re-offer toast Undo when a Restore persist fails. History was left in
    /// place so Ctrl+Z still sees the original Dates entry.
    pub fn restore_date_save_failed(&mut self, pending: &PendingDateSave) {
        if pending.role != DateSaveRole::Restore {
            return;
        }
        self.toasts.push_undo(
            "Cleared dates",
            UndoAction::RestoreDates {
                id: pending.id,
                start_date: pending.after_start.clone(),
                target_date: pending.after_target.clone(),
                rev: pending.previous.rev,
            },
        );
    }

    /// A later local edit landed while this persist was in flight. Queue the
    /// live dates, rolling back to the just-persisted node if that PATCH fails.
    pub fn follow_up_date_save(live: &WorkItemNode, persisted: &WorkItemNode) -> PendingDateSave {
        PendingDateSave {
            id: live.id,
            start: parse_iso_date(live.start_date.as_deref()),
            target: parse_iso_date(live.target_date.as_deref()),
            after_start: live.start_date.clone(),
            after_target: live.target_date.clone(),
            rev: persisted.rev,
            previous: persisted.clone(),
            role: DateSaveRole::Commit,
        }
    }

    /// Apply picker dates locally and queue a persist. `None` clears that
    /// side, so start-only and an empty Enter (clear both) are exact writes.
    pub fn commit_range_dates_now(
        &mut self,
        id: i64,
        start: Option<Date>,
        target: Option<Date>,
    ) -> Option<PendingDateSave> {
        let previous = self.nodes.iter().find(|node| node.id == id).cloned()?;
        let after_start = start.map(iso_date_only);
        let after_target = target.map(iso_date_only);
        if after_start == previous.start_date && after_target == previous.target_date {
            return None;
        }
        self.restore_partial_dates(id, start, target);
        if self.history_recording {
            self.record_dates(id, &previous, after_start.clone(), after_target.clone());
        }
        Some(PendingDateSave {
            id,
            start,
            target,
            after_start,
            after_target,
            rev: previous.rev,
            previous,
            role: DateSaveRole::Commit,
        })
    }

    /// Apply the live gesture dates and push undo immediately, before PATCH.
    pub fn commit_date_gesture_now(&mut self) -> Option<PendingDateSave> {
        let gesture = self.gantt.gesture.take()?;
        if gesture.kind == GestureKind::Link {
            self.gantt.gesture = Some(gesture);
            return None;
        }
        self.gantt.clear_hover();
        if gesture.live_start == gesture.start && gesture.live_target == gesture.target {
            return None;
        }
        let (start, target) = crate::gantt::GanttView::gesture_patch_dates(&gesture);
        let previous = self
            .nodes
            .iter()
            .find(|node| node.id == gesture.id)
            .cloned()?;
        let after_start = start
            .map(iso_date_only)
            .or_else(|| previous.start_date.clone());
        let after_target = target
            .map(iso_date_only)
            .or_else(|| previous.target_date.clone());
        let _ = self.apply_optimistic_partial(gesture.id, start, target);
        if self.history_recording {
            self.record_dates(
                gesture.id,
                &previous,
                after_start.clone(),
                after_target.clone(),
            );
        }
        Some(PendingDateSave {
            id: gesture.id,
            start,
            target,
            after_start,
            after_target,
            rev: previous.rev,
            previous,
            role: DateSaveRole::Commit,
        })
    }

    /// Apply the last Dates undo locally so a second Ctrl+Z is not dropped
    /// while the first PATCH is in flight. Returns the persist to queue.
    pub fn undo_dates_now(&mut self) -> Option<PendingDateSave> {
        self.undo_date_stack_now().into_iter().next()
    }

    /// Undo a Dates or DatesGroup entry. A group restores parent + children
    /// in one step so "Move children too" is not a separate Undo.
    pub fn undo_date_stack_now(&mut self) -> Vec<PendingDateSave> {
        match self.history.undo.last().cloned() {
            Some(HistoryEntry::Dates {
                id,
                before_start,
                before_target,
                after_start,
                after_target,
            }) => {
                let Some(previous) = self.nodes.iter().find(|node| node.id == id).cloned() else {
                    return Vec::new();
                };
                let _ = self.history.pop_undo();
                if let Some(node) = self.nodes.iter_mut().find(|node| node.id == id) {
                    node.start_date = before_start.clone();
                    node.target_date = before_target.clone();
                }
                let entry = HistoryEntry::Dates {
                    id,
                    before_start: before_start.clone(),
                    before_target: before_target.clone(),
                    after_start,
                    after_target,
                };
                self.history.redo.push(entry.clone());
                self.push_history_toast(&entry, true);
                self.sync_open_form_from_node(id);
                vec![PendingDateSave {
                    id,
                    start: parse_iso_date(before_start.as_deref()),
                    target: parse_iso_date(before_target.as_deref()),
                    after_start: before_start,
                    after_target: before_target,
                    rev: previous.rev,
                    previous,
                    role: DateSaveRole::Undo,
                }]
            }
            Some(HistoryEntry::DatesGroup { items }) => {
                if items.is_empty() {
                    return Vec::new();
                }
                let _ = self.history.pop_undo();
                self.history.redo.push(HistoryEntry::DatesGroup {
                    items: items.clone(),
                });
                if let Some(first) = items.first() {
                    let entry = HistoryEntry::Dates {
                        id: first.id,
                        before_start: first.before_start.clone(),
                        before_target: first.before_target.clone(),
                        after_start: first.after_start.clone(),
                        after_target: first.after_target.clone(),
                    };
                    self.push_history_toast(&entry, true);
                }
                self.pending_from_dates_group(items, true)
            }
            _ => Vec::new(),
        }
    }

    /// Apply the last Dates redo locally so a second Ctrl+Shift+Z is not dropped.
    pub fn redo_dates_now(&mut self) -> Option<PendingDateSave> {
        self.redo_date_stack_now().into_iter().next()
    }

    pub fn redo_date_stack_now(&mut self) -> Vec<PendingDateSave> {
        match self.history.redo.last().cloned() {
            Some(HistoryEntry::Dates {
                id,
                before_start,
                before_target,
                after_start,
                after_target,
            }) => {
                let Some(previous) = self.nodes.iter().find(|node| node.id == id).cloned() else {
                    return Vec::new();
                };
                let _ = self.history.pop_redo();
                if let Some(node) = self.nodes.iter_mut().find(|node| node.id == id) {
                    node.start_date = after_start.clone();
                    node.target_date = after_target.clone();
                }
                let entry = HistoryEntry::Dates {
                    id,
                    before_start,
                    before_target,
                    after_start: after_start.clone(),
                    after_target: after_target.clone(),
                };
                self.history.undo.push(entry.clone());
                self.push_history_toast(&entry, false);
                self.sync_open_form_from_node(id);
                vec![PendingDateSave {
                    id,
                    start: parse_iso_date(after_start.as_deref()),
                    target: parse_iso_date(after_target.as_deref()),
                    after_start,
                    after_target,
                    rev: previous.rev,
                    previous,
                    role: DateSaveRole::Redo,
                }]
            }
            Some(HistoryEntry::DatesGroup { items }) => {
                if items.is_empty() {
                    return Vec::new();
                }
                let _ = self.history.pop_redo();
                self.history.undo.push(HistoryEntry::DatesGroup {
                    items: items.clone(),
                });
                if let Some(first) = items.first() {
                    let entry = HistoryEntry::Dates {
                        id: first.id,
                        before_start: first.before_start.clone(),
                        before_target: first.before_target.clone(),
                        after_start: first.after_start.clone(),
                        after_target: first.after_target.clone(),
                    };
                    self.push_history_toast(&entry, false);
                }
                self.pending_from_dates_group(items, false)
            }
            _ => Vec::new(),
        }
    }

    fn pending_from_dates_group(
        &mut self,
        items: Vec<DatesChange>,
        is_undo: bool,
    ) -> Vec<PendingDateSave> {
        let mut out = Vec::new();
        for (index, change) in items.into_iter().enumerate() {
            let Some(previous) = self.nodes.iter().find(|node| node.id == change.id).cloned()
            else {
                continue;
            };
            let (start_s, target_s) = if is_undo {
                (change.before_start, change.before_target)
            } else {
                (change.after_start, change.after_target)
            };
            if let Some(node) = self.nodes.iter_mut().find(|node| node.id == change.id) {
                node.start_date = start_s.clone();
                node.target_date = target_s.clone();
            }
            self.sync_open_form_from_node(change.id);
            let role = if index == 0 {
                if is_undo {
                    DateSaveRole::Undo
                } else {
                    DateSaveRole::Redo
                }
            } else {
                DateSaveRole::Cascade
            };
            out.push(PendingDateSave {
                id: change.id,
                start: parse_iso_date(start_s.as_deref()),
                target: parse_iso_date(target_s.as_deref()),
                after_start: start_s,
                after_target: target_s,
                rev: previous.rev,
                previous,
                role,
            });
        }
        out
    }

    pub async fn persist_pending_dates(
        &mut self,
        pending: PendingDateSave,
    ) -> Result<(), AppError> {
        let Some(scope) = self.scope() else {
            return Ok(());
        };
        let Some(node) = self
            .nodes
            .iter()
            .find(|node| node.id == pending.id)
            .cloned()
        else {
            return Ok(());
        };
        if node.start_date != pending.after_start || node.target_date != pending.after_target {
            return Ok(());
        }
        if !node.has_date_fields {
            return Err(AppError::message("no date fields"));
        }
        let start = parse_iso_date(pending.after_start.as_deref());
        let target = parse_iso_date(pending.after_target.as_deref());
        match self
            .client
            .patch_work_item_with_rev_retry(
                &scope.org,
                &scope.project,
                pending.id,
                pending.rev,
                |rev| build_replace_dates_patch(rev, start, target),
            )
            .await
        {
            Ok(rev) => {
                if let Some(live) = self.nodes.iter_mut().find(|node| node.id == pending.id) {
                    live.rev = rev;
                }
                self.sync_open_form_from_node(pending.id);
                self.persist_hierarchy_cache();
                if pending.role == DateSaveRole::Commit {
                    self.push_dates_moved_toast(
                        pending.id,
                        parse_iso_date(pending.after_start.as_deref()),
                        parse_iso_date(pending.after_target.as_deref()),
                        &pending.previous,
                    );
                }
                self.complete_restore_date_save(&pending);
                Ok(())
            }
            Err(err) => {
                if self.nodes.iter().any(|node| {
                    node.id == pending.id
                        && node.start_date == pending.after_start
                        && node.target_date == pending.after_target
                }) {
                    self.restore_node(pending.previous.clone());
                    self.revert_date_save_history(&pending);
                    self.restore_date_save_failed(&pending);
                }
                self.toasts
                    .push(ToastKind::Error, error_text(&err, "Could not save dates"));
                Err(err.into())
            }
        }
    }

    pub async fn save_dates(&mut self, id: i64, start: Date, target: Date) -> Result<(), AppError> {
        let Some(scope) = self.scope() else {
            return Ok(());
        };
        let Some(node) = self.nodes.iter().find(|node| node.id == id).cloned() else {
            return Err(AppError::message("Work Item not found"));
        };
        if !node.has_date_fields {
            self.toasts.push(
                ToastKind::Error,
                "This Work Item has no Start Date and Target Date fields.",
            );
            return Err(AppError::message("no date fields"));
        }
        let Some(previous) = self.apply_optimistic_dates(id, start, target) else {
            return Ok(());
        };
        if self.history_recording {
            self.record_dates(
                id,
                &previous,
                Some(iso_date_only(start)),
                Some(iso_date_only(target)),
            );
        }
        match self
            .client
            .patch_work_item_with_rev_retry(&scope.org, &scope.project, id, node.rev, |rev| {
                build_dates_patch(rev, Some(start), Some(target))
            })
            .await
        {
            Ok(rev) => {
                if let Some(node) = self.nodes.iter_mut().find(|node| node.id == id) {
                    node.rev = rev;
                }
                self.persist_hierarchy_cache();
                if self.history_recording {
                    self.push_dates_moved_toast(id, Some(start), Some(target), &previous);
                }
                Ok(())
            }
            Err(err) => {
                self.restore_node(previous);
                if self.history_recording {
                    self.discard_last_dates(id);
                }
                self.toasts
                    .push(ToastKind::Error, error_text(&err, "Could not save dates"));
                Err(err.into())
            }
        }
    }

    pub async fn schedule_at(&mut self, id: i64, start: Date) -> Result<(), AppError> {
        self.schedule_span(id, start, start.saturating_add(13.days()))
            .await
    }

    pub async fn schedule_span(
        &mut self,
        id: i64,
        start: Date,
        target: Date,
    ) -> Result<(), AppError> {
        let (start, target) = if start <= target {
            (start, target)
        } else {
            (target, start)
        };
        let target = if target == start {
            start.saturating_add(13.days())
        } else {
            target
        };
        self.save_dates(id, start, target).await
    }

    pub async fn save_partial_dates(
        &mut self,
        id: i64,
        start: Option<Date>,
        target: Option<Date>,
    ) -> Result<(), AppError> {
        match (start, target) {
            (Some(start), Some(target)) => self.save_dates(id, start, target).await,
            (None, None) => Ok(()),
            (start, target) => {
                let Some(scope) = self.scope() else {
                    return Ok(());
                };
                let Some(node) = self.nodes.iter().find(|node| node.id == id).cloned() else {
                    return Err(AppError::message("Work Item not found"));
                };
                if !node.has_date_fields {
                    self.toasts.push(
                        ToastKind::Error,
                        "This Work Item has no Start Date and Target Date fields.",
                    );
                    return Err(AppError::message("no date fields"));
                }
                let Some(previous) = self.apply_optimistic_partial(id, start, target) else {
                    return Ok(());
                };
                let after_start = start
                    .map(iso_date_only)
                    .or_else(|| previous.start_date.clone());
                let after_target = target
                    .map(iso_date_only)
                    .or_else(|| previous.target_date.clone());
                if self.history_recording {
                    self.record_dates(id, &previous, after_start.clone(), after_target.clone());
                }
                match self
                    .client
                    .patch_work_item_with_rev_retry(
                        &scope.org,
                        &scope.project,
                        id,
                        node.rev,
                        |rev| build_dates_patch(rev, start, target),
                    )
                    .await
                {
                    Ok(rev) => {
                        if let Some(node) = self.nodes.iter_mut().find(|node| node.id == id) {
                            node.rev = rev;
                        }
                        self.persist_hierarchy_cache();
                        if self.history_recording {
                            self.push_dates_moved_toast(
                                id,
                                parse_iso_date(after_start.as_deref()),
                                parse_iso_date(after_target.as_deref()),
                                &previous,
                            );
                        }
                        Ok(())
                    }
                    Err(err) => {
                        self.restore_node(previous);
                        if self.history_recording {
                            self.discard_last_dates(id);
                        }
                        self.toasts
                            .push(ToastKind::Error, error_text(&err, "Could not save dates"));
                        Err(err.into())
                    }
                }
            }
        }
    }

    /// Write both date fields, using JSON null to clear a side.
    pub async fn save_exact_dates(
        &mut self,
        id: i64,
        start: Option<Date>,
        target: Option<Date>,
    ) -> Result<(), AppError> {
        if start.is_none() && target.is_none() {
            return self.clear_dates(id).await;
        }
        let Some(scope) = self.scope() else {
            return Ok(());
        };
        let Some(node) = self.nodes.iter().find(|node| node.id == id).cloned() else {
            return Err(AppError::message("Work Item not found"));
        };
        if !node.has_date_fields {
            self.toasts.push(
                ToastKind::Error,
                "This Work Item has no Start Date and Target Date fields.",
            );
            return Err(AppError::message("no date fields"));
        }
        let previous = node.clone();
        if let Some(live) = self.nodes.iter_mut().find(|item| item.id == id) {
            live.start_date = start.map(iso_date_only);
            live.target_date = target.map(iso_date_only);
        }
        let after_start = start.map(iso_date_only);
        let after_target = target.map(iso_date_only);
        if self.history_recording {
            self.record_dates(id, &previous, after_start.clone(), after_target.clone());
        }
        match self
            .client
            .patch_work_item_with_rev_retry(&scope.org, &scope.project, id, node.rev, |rev| {
                build_replace_dates_patch(rev, start, target)
            })
            .await
        {
            Ok(rev) => {
                if let Some(live) = self.nodes.iter_mut().find(|item| item.id == id) {
                    live.rev = rev;
                }
                self.sync_open_form_from_node(id);
                self.persist_hierarchy_cache();
                if self.history_recording {
                    self.push_dates_moved_toast(id, start, target, &previous);
                }
                Ok(())
            }
            Err(err) => {
                self.restore_node(previous);
                if self.history_recording {
                    self.discard_last_dates(id);
                }
                self.toasts
                    .push(ToastKind::Error, error_text(&err, "Could not save dates"));
                Err(err.into())
            }
        }
    }

    pub async fn add_dependency(
        &mut self,
        successor_id: i64,
        predecessor_id: i64,
    ) -> Result<(), AppError> {
        if successor_id == predecessor_id {
            return Ok(());
        }
        let Some(scope) = self.scope() else {
            return Ok(());
        };
        let Some(node) = self
            .nodes
            .iter()
            .find(|node| node.id == successor_id)
            .cloned()
        else {
            return Err(AppError::message("Work Item not found"));
        };
        if node.predecessors.contains(&predecessor_id) {
            return Ok(());
        }
        let previous = node.clone();
        if let Some(live) = self.nodes.iter_mut().find(|n| n.id == successor_id) {
            live.predecessors.push(predecessor_id);
        }
        match self
            .client
            .add_dependency(
                &scope.org,
                &scope.project,
                successor_id,
                predecessor_id,
                node.rev,
            )
            .await
        {
            Ok(rev) => {
                if let Some(live) = self.nodes.iter_mut().find(|n| n.id == successor_id) {
                    live.rev = rev;
                }
                self.sync_open_form_from_node(successor_id);
                self.persist_hierarchy_cache();
                if self.history_recording {
                    self.record(HistoryEntry::Link {
                        successor_id,
                        predecessor_id,
                        linked: true,
                    });
                    self.toasts.push_undo(
                        self.link_changed_message("Added", predecessor_id, successor_id),
                        UndoAction::RemoveLink {
                            successor_id,
                            predecessor_id,
                        },
                    );
                }
                Ok(())
            }
            Err(err) => {
                self.restore_node(previous);
                self.toasts.push(
                    ToastKind::Error,
                    error_text(&err, "Could not add dependency"),
                );
                Err(err.into())
            }
        }
    }

    pub async fn remove_dependency(
        &mut self,
        successor_id: i64,
        predecessor_id: i64,
    ) -> Result<(), AppError> {
        if successor_id == predecessor_id {
            return Ok(());
        }
        let Some(scope) = self.scope() else {
            return Ok(());
        };
        let Some(node) = self
            .nodes
            .iter()
            .find(|node| node.id == successor_id)
            .cloned()
        else {
            return Err(AppError::message("Work Item not found"));
        };
        if !node.predecessors.contains(&predecessor_id) {
            return Ok(());
        }
        let previous = node.clone();
        if let Some(live) = self.nodes.iter_mut().find(|n| n.id == successor_id) {
            live.predecessors.retain(|id| *id != predecessor_id);
        }
        match self
            .client
            .remove_dependency(
                &scope.org,
                &scope.project,
                successor_id,
                predecessor_id,
                node.rev,
            )
            .await
        {
            Ok(rev) => {
                if let Some(live) = self.nodes.iter_mut().find(|n| n.id == successor_id) {
                    live.rev = rev;
                }
                self.sync_open_form_from_node(successor_id);
                self.gantt.selected_link = None;
                self.gantt.clear_hover();
                self.persist_hierarchy_cache();
                if self.history_recording {
                    self.record(HistoryEntry::Link {
                        successor_id,
                        predecessor_id,
                        linked: false,
                    });
                    self.toasts.push_undo(
                        self.link_changed_message("Removed", predecessor_id, successor_id),
                        UndoAction::RestoreLink {
                            successor_id,
                            predecessor_id,
                        },
                    );
                }
                Ok(())
            }
            Err(err) => {
                self.restore_node(previous);
                self.toasts.push(
                    ToastKind::Error,
                    error_text(&err, "Could not remove dependency"),
                );
                Err(err.into())
            }
        }
    }

    pub async fn add_item(&mut self) -> Result<(), AppError> {
        self.add_item_under(self.gantt.selected_id).await
    }

    pub async fn add_item_under(&mut self, parent_id: Option<i64>) -> Result<(), AppError> {
        let Some(scope) = self.scope() else {
            return Err(AppError::message("Choose a Team first"));
        };
        let types = self.types();
        let (parent_id, work_item_type, area, iteration) = {
            let selected = parent_id.and_then(|id| self.nodes.iter().find(|node| node.id == id));
            let parent_id = selected.map(|node| node.id);
            let work_item_type =
                child_work_item_type(selected.map(|node| node.work_item_type.as_str()), &types);
            let area = selected
                .map(|node| node.area_path.clone())
                .or_else(|| self.nodes.first().map(|node| node.area_path.clone()))
                .unwrap_or_else(|| scope.project.clone());
            let iteration = selected
                .map(|node| node.iteration_path.clone())
                .filter(|path| !path.is_empty())
                .or_else(|| {
                    let path = self.prefs.planner.iteration_path.clone();
                    (!path.is_empty()).then_some(path)
                })
                .unwrap_or_default();
            (parent_id, work_item_type, area, iteration)
        };
        let mut ops = vec![
            ado_core::JsonPatchOp {
                op: ado_core::PatchOp::Add,
                path: "/fields/System.Title".into(),
                value: Some(json!("New work item")),
            },
            ado_core::JsonPatchOp {
                op: ado_core::PatchOp::Add,
                path: "/fields/System.AreaPath".into(),
                value: Some(json!(area)),
            },
        ];
        if !iteration.is_empty() {
            ops.push(ado_core::JsonPatchOp {
                op: ado_core::PatchOp::Add,
                path: "/fields/System.IterationPath".into(),
                value: Some(json!(iteration)),
            });
        }
        if let Some(parent_id) = parent_id {
            let url = self
                .client
                .work_item_api_url(&scope.org, &scope.project, parent_id);
            ops.push(parent_relation_op(&url));
        }
        match self
            .client
            .create_work_item(&scope.org, &scope.project, &work_item_type, &ops)
            .await
        {
            Ok(mut node) => {
                if parent_id.is_some() {
                    node.parent_id = parent_id;
                }
                let id = node.id;
                let title = node.title.clone();
                let insert_at = insert_index_for_child(&self.nodes, parent_id);
                self.nodes.insert(insert_at, node);
                if let Some(pid) = parent_id {
                    self.gantt.collapsed.remove(&pid);
                }
                self.gantt.selected_id = Some(id);
                let rows = self.gantt.flatten(&self.nodes);
                self.gantt.select_relative(&rows, 0);
                self.persist_hierarchy_cache();
                let work_item_type = self
                    .nodes
                    .iter()
                    .find(|node| node.id == id)
                    .map(|node| node.work_item_type.clone())
                    .unwrap_or_else(|| "work item".into());
                let placement = match parent_id.and_then(|pid| {
                    self.nodes.iter().find(|node| node.id == pid).map(|parent| {
                        format!(
                            "under {} {}",
                            parent.work_item_type,
                            quote_name(&parent.title)
                        )
                    })
                }) {
                    Some(under) => under,
                    None => "at the root".into(),
                };
                self.record(HistoryEntry::Created {
                    id,
                    title: title.clone(),
                    work_item_type: work_item_type.clone(),
                    placement: placement.clone(),
                });
                self.toasts.push_undo(
                    created_message(&work_item_type, &title, &placement),
                    UndoAction::DeleteCreated {
                        id,
                        title,
                        work_item_type,
                        placement,
                    },
                );
                Ok(())
            }
            Err(err) => {
                self.toasts.push(
                    ToastKind::Error,
                    error_text(&err, "Could not create work item"),
                );
                Err(err.into())
            }
        }
    }

    pub async fn save_title(&mut self, id: i64, title: String) -> Result<(), AppError> {
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err(AppError::message("Title cannot be empty"));
        }
        self.patch_node_field(id, "System.Title", json!(title.clone()), move |node| {
            node.title = title;
        })
        .await
    }

    pub async fn save_assignee(
        &mut self,
        id: i64,
        identity: Option<IdentityValue>,
    ) -> Result<(), AppError> {
        let value = identity
            .as_ref()
            .map(crate::form::identity_json)
            .unwrap_or(Value::Null);
        self.patch_node_field(id, "System.AssignedTo", value, move |node| {
            node.assigned_to = identity;
        })
        .await
    }

    async fn patch_node_field(
        &mut self,
        id: i64,
        reference: &str,
        value: Value,
        apply: impl FnOnce(&mut WorkItemNode),
    ) -> Result<(), AppError> {
        let Some(scope) = self.scope() else {
            return Ok(());
        };
        let Some(node) = self.nodes.iter().find(|node| node.id == id).cloned() else {
            return Err(AppError::message("Work Item not found"));
        };
        let previous = node.clone();
        if let Some(live) = self.nodes.iter_mut().find(|n| n.id == id) {
            apply(live);
        }
        match self
            .client
            .patch_work_item_with_rev_retry(&scope.org, &scope.project, id, node.rev, |rev| {
                build_field_patch(rev, reference, value.clone())
            })
            .await
        {
            Ok(rev) => {
                if let Some(live) = self.nodes.iter_mut().find(|n| n.id == id) {
                    live.rev = rev;
                }
                self.sync_open_form_from_node(id);
                self.persist_hierarchy_cache();
                if (reference == ado_core::START_DATE_FIELD
                    || reference == ado_core::TARGET_DATE_FIELD)
                    && self.history_recording
                {
                    let start = self
                        .nodes
                        .iter()
                        .find(|n| n.id == id)
                        .and_then(|n| parse_iso_date(n.start_date.as_deref()));
                    let target = self
                        .nodes
                        .iter()
                        .find(|n| n.id == id)
                        .and_then(|n| parse_iso_date(n.target_date.as_deref()));
                    self.push_dates_moved_toast(id, start, target, &previous);
                } else if self.history_recording {
                    let before_value = node_field_json(&previous, reference);
                    self.record(HistoryEntry::Fields {
                        id,
                        changes: vec![(reference.to_string(), before_value.clone(), value.clone())],
                    });
                    let message = if reference == "System.Title" {
                        renamed_message(&previous.title, &self.item_title(id))
                    } else {
                        format!(
                            "Saved {} on {}",
                            field_toast_name(reference, None),
                            quote_name(&self.item_title(id))
                        )
                    };
                    self.toasts.push_undo(
                        message,
                        UndoAction::RestoreFields {
                            id,
                            fields: vec![(reference.to_string(), before_value)],
                        },
                    );
                }
                Ok(())
            }
            Err(err) => {
                self.restore_node(previous);
                self.toasts
                    .push(ToastKind::Error, error_text(&err, "Could not save field"));
                Err(err.into())
            }
        }
    }

    pub fn begin_bar_gesture(
        &mut self,
        bar_id: i64,
        kind: GestureKind,
        origin_x: f32,
    ) -> Result<(), AppError> {
        let visible = self.visible_nodes();
        let rows = self.gantt.flatten(&visible);
        let bars = self
            .gantt
            .layout_bars(&visible, &rows, &self.iterations, 10_000.0);
        let Some(bar) = bars.into_iter().find(|bar| bar.id == bar_id) else {
            return Err(AppError::message("bar not found"));
        };
        if let Err(message) =
            self.gantt
                .begin_gesture(&bar, kind, origin_x, &visible, &self.iterations)
        {
            self.toasts.push(ToastKind::Error, message);
            return Err(AppError::message(message));
        }
        Ok(())
    }

    pub fn update_bar_gesture(&mut self, x: f32) {
        self.gantt.update_gesture(x);
    }

    pub fn cancel_gesture(&mut self) {
        if let Some(gesture) = self.gantt.gesture.clone()
            && gesture.kind != crate::gantt::GestureKind::Link
        {
            self.restore_partial_dates(
                gesture.id,
                gesture.had_start.then_some(gesture.start),
                gesture.had_target.then_some(gesture.target),
            );
        }
        if let Some(nudge) = self.gantt.pending_nudge.clone() {
            self.restore_nudge(&nudge);
        }
        self.gantt.cancel_gesture();
    }

    pub async fn commit_gesture(&mut self) -> Result<(), AppError> {
        if let Some((from_id, to_id)) = self.gantt.take_link_gesture() {
            self.gantt.clear_hover();
            if let Some(to_id) = to_id
                && to_id != from_id
            {
                return self.add_dependency(to_id, from_id).await;
            }
            return Ok(());
        }
        if let Some(pending) = self.commit_date_gesture_now() {
            return self.persist_pending_dates(pending).await;
        }
        self.gantt.clear_hover();
        Ok(())
    }

    pub fn nudge(&mut self, axis: NudgeAxis, delta: i32) -> Result<(), AppError> {
        let visible = self.visible_nodes();
        match self.gantt.apply_nudge(&visible, axis, delta) {
            Ok(pending) => {
                let start = pending.had_start.then_some(pending.start);
                let target = pending.had_target.then_some(pending.target);
                let _ = self.apply_optimistic_partial(pending.id, start, target);
                self.last_nudge_at = Some(Instant::now());
                Ok(())
            }
            Err(message) => {
                self.toasts.push(ToastKind::Error, message);
                Err(AppError::message(message))
            }
        }
    }

    pub fn nudge_is_idle(&self, idle: Duration) -> bool {
        self.last_nudge_at.is_some_and(|at| at.elapsed() >= idle)
            && self.gantt.pending_nudge.is_some()
    }

    pub async fn flush_nudge(&mut self) -> Result<(), AppError> {
        let Some(pending) = self.gantt.pending_nudge.take() else {
            return Ok(());
        };
        self.last_nudge_at = None;
        self.restore_nudge(&pending);
        let start = pending.had_start.then_some(pending.start);
        let target = pending.had_target.then_some(pending.target);
        self.save_partial_dates(pending.id, start, target).await
    }

    pub fn discard_nudge(&mut self) {
        if let Some(nudge) = self.gantt.pending_nudge.take() {
            self.restore_nudge(&nudge);
        }
        self.last_nudge_at = None;
    }

    pub async fn clear_dates(&mut self, id: i64) -> Result<(), AppError> {
        let Some(scope) = self.scope() else {
            return Ok(());
        };
        let Some(node) = self.nodes.iter().find(|node| node.id == id).cloned() else {
            return Ok(());
        };
        if !node.has_date_fields {
            self.toasts.push(
                ToastKind::Error,
                "This Work Item has no Start Date and Target Date fields.",
            );
            return Err(AppError::message("no date fields"));
        }
        let previous = node.clone();
        if let Some(live) = self.nodes.iter_mut().find(|n| n.id == id) {
            live.start_date = None;
            live.target_date = None;
        }
        match self
            .client
            .patch_work_item_with_rev_retry(&scope.org, &scope.project, id, node.rev, |rev| {
                build_clear_dates_patch(rev)
            })
            .await
        {
            Ok(rev) => {
                if let Some(live) = self.nodes.iter_mut().find(|n| n.id == id) {
                    live.rev = rev;
                }
                self.persist_hierarchy_cache();
                self.sync_open_form_from_node(id);
                if self.history_recording {
                    self.record_dates(id, &previous, None, None);
                    self.toasts.push_undo(
                        format!("Cleared dates on {}", quote_name(&self.item_title(id))),
                        UndoAction::RestoreDates {
                            id,
                            start_date: previous.start_date,
                            target_date: previous.target_date,
                            rev,
                        },
                    );
                }
                Ok(())
            }
            Err(err) => {
                self.restore_node(previous);
                self.toasts
                    .push(ToastKind::Error, error_text(&err, "Could not clear dates"));
                Err(err.into())
            }
        }
    }

    pub async fn undo(&mut self) -> Result<(), AppError> {
        let Some(entry) = self.history.pop_undo() else {
            return Ok(());
        };
        self.history_recording = false;
        let result = self.apply_history_entry(&entry, true).await;
        self.history_recording = true;
        if result.is_ok() {
            self.push_history_toast(&entry, true);
            if !matches!(entry, HistoryEntry::Created { .. }) {
                self.history.redo.push(entry);
            }
            self.toasts.items.retain(|toast| toast.undo.is_none());
        } else {
            self.history.undo.push(entry);
        }
        result
    }

    pub async fn redo(&mut self) -> Result<(), AppError> {
        let Some(entry) = self.history.pop_redo() else {
            return Ok(());
        };
        self.history_recording = false;
        let result = self.apply_history_entry(&entry, false).await;
        self.history_recording = true;
        if result.is_ok() {
            self.push_history_toast(&entry, false);
            self.history.undo.push(entry);
        } else {
            self.history.redo.push(entry);
        }
        result
    }

    async fn apply_history_entry(
        &mut self,
        entry: &HistoryEntry,
        is_undo: bool,
    ) -> Result<(), AppError> {
        match entry {
            HistoryEntry::Dates {
                id,
                before_start,
                before_target,
                after_start,
                after_target,
            } => {
                let (start, target) = if is_undo {
                    (before_start, before_target)
                } else {
                    (after_start, after_target)
                };
                self.save_exact_dates(
                    *id,
                    parse_iso_date(start.as_deref()),
                    parse_iso_date(target.as_deref()),
                )
                .await
            }
            HistoryEntry::DatesGroup { items } => {
                let mut result = Ok(());
                for change in items {
                    let (start, target) = if is_undo {
                        (&change.before_start, &change.before_target)
                    } else {
                        (&change.after_start, &change.after_target)
                    };
                    if let Err(err) = self
                        .save_exact_dates(
                            change.id,
                            parse_iso_date(start.as_deref()),
                            parse_iso_date(target.as_deref()),
                        )
                        .await
                    {
                        result = Err(err);
                        break;
                    }
                }
                result
            }
            HistoryEntry::Link {
                successor_id,
                predecessor_id,
                linked,
            } => {
                let should_link = if is_undo { !*linked } else { *linked };
                if should_link {
                    self.add_dependency(*successor_id, *predecessor_id).await
                } else {
                    self.remove_dependency(*successor_id, *predecessor_id).await
                }
            }
            HistoryEntry::Created { id, .. } => {
                if is_undo {
                    self.delete_item(*id).await
                } else {
                    Ok(())
                }
            }
            HistoryEntry::Fields { id, changes } => {
                let fields: Vec<(String, Value)> = changes
                    .iter()
                    .map(|(reference, before, after)| {
                        if is_undo {
                            (reference.clone(), before.clone())
                        } else {
                            (reference.clone(), after.clone())
                        }
                    })
                    .collect();
                self.restore_fields(*id, &fields).await
            }
        }
    }

    pub async fn apply_undo(&mut self, action: UndoAction) -> Result<(), AppError> {
        self.history_recording = false;
        let result = match &action {
            UndoAction::RestoreDates {
                id,
                start_date,
                target_date,
                ..
            } => {
                self.save_exact_dates(
                    *id,
                    parse_iso_date(start_date.as_deref()),
                    parse_iso_date(target_date.as_deref()),
                )
                .await
            }
            UndoAction::RestoreDatesBatch { items } => {
                let mut result = Ok(());
                for item in items {
                    if let Err(err) = self
                        .save_exact_dates(
                            item.id,
                            parse_iso_date(item.start_date.as_deref()),
                            parse_iso_date(item.target_date.as_deref()),
                        )
                        .await
                    {
                        result = Err(err);
                        break;
                    }
                }
                result
            }
            UndoAction::RemoveLink {
                successor_id,
                predecessor_id,
            } => self.remove_dependency(*successor_id, *predecessor_id).await,
            UndoAction::RestoreLink {
                successor_id,
                predecessor_id,
            } => self.add_dependency(*successor_id, *predecessor_id).await,
            UndoAction::DeleteCreated { id, .. } => self.delete_item(*id).await,
            UndoAction::RestoreFields { id, fields } => self.restore_fields(*id, fields).await,
        };
        self.history_recording = true;
        if result.is_ok() {
            self.discard_history_for_undo(&action);
            self.toasts.dismiss_undo();
        }
        result
    }

    /// Apply a toast RestoreDates locally and queue a persist. History stays
    /// until the persist succeeds so a failed PATCH can still be undone.
    pub fn apply_toast_restore_dates(&mut self, action: UndoAction) -> Option<PendingDateSave> {
        self.apply_toast_restore_dates_all(action)
            .into_iter()
            .next()
    }

    fn apply_toast_restore_dates_all(&mut self, action: UndoAction) -> Vec<PendingDateSave> {
        let items: Vec<DateRestore> = match action {
            UndoAction::RestoreDates {
                id,
                start_date,
                target_date,
                rev,
            } => vec![DateRestore {
                id,
                start_date,
                target_date,
                rev,
            }],
            UndoAction::RestoreDatesBatch { items } => items,
            _ => return Vec::new(),
        };
        let mut out = Vec::new();
        for (index, item) in items.into_iter().enumerate() {
            let Some(previous) = self.nodes.iter().find(|node| node.id == item.id).cloned() else {
                continue;
            };
            if let Some(node) = self.nodes.iter_mut().find(|node| node.id == item.id) {
                node.start_date = item.start_date.clone();
                node.target_date = item.target_date.clone();
            }
            self.sync_open_form_from_node(item.id);
            let role = if index == 0 {
                DateSaveRole::Restore
            } else {
                DateSaveRole::Cascade
            };
            out.push(PendingDateSave {
                id: item.id,
                start: parse_iso_date(item.start_date.as_deref()),
                target: parse_iso_date(item.target_date.as_deref()),
                after_start: item.start_date,
                after_target: item.target_date,
                rev: previous.rev,
                previous,
                role,
            });
        }
        out
    }

    /// Copy predecessors (and rev when dates were not edited) for a toast
    /// link Undo so live date edits on those rows survive the HTTP round-trip.
    pub fn merge_undo_action(&mut self, working: &PlannerState, action: &UndoAction) {
        let ids: Vec<i64> = match action {
            UndoAction::RestoreDates { id, .. }
            | UndoAction::DeleteCreated { id, .. }
            | UndoAction::RestoreFields { id, .. } => {
                vec![*id]
            }
            UndoAction::RestoreDatesBatch { items } => items.iter().map(|item| item.id).collect(),
            UndoAction::RemoveLink {
                successor_id,
                predecessor_id,
            }
            | UndoAction::RestoreLink {
                successor_id,
                predecessor_id,
            } => vec![*successor_id, *predecessor_id],
        };
        if let UndoAction::DeleteCreated { id, .. } = action {
            self.nodes.retain(|node| node.id != *id);
            if self.gantt.selected_id == Some(*id) {
                self.gantt.selected_id = None;
            }
        }
        for id in ids {
            let Some(src) = working.nodes.iter().find(|node| node.id == id) else {
                continue;
            };
            if let Some(dst) = self.nodes.iter_mut().find(|node| node.id == id) {
                dst.predecessors = src.predecessors.clone();
                dst.title = src.title.clone();
                dst.assigned_to = src.assigned_to.clone();
                dst.state = src.state.clone();
                dst.iteration_path = src.iteration_path.clone();
                dst.area_path = src.area_path.clone();
                dst.fields = src.fields.clone();
                let copy_dates = matches!(
                    action,
                    UndoAction::RestoreFields { fields, .. }
                        if fields.iter().any(|(reference, _)| is_scheduling_date(reference))
                );
                if copy_dates {
                    dst.start_date = src.start_date.clone();
                    dst.target_date = src.target_date.clone();
                }
                if dst.start_date == src.start_date && dst.target_date == src.target_date {
                    dst.rev = src.rev;
                }
            }
        }
        self.discard_history_for_undo(action);
        self.persist_hierarchy_cache();
        if let Some(msg) = working.toasts.last_error_message() {
            self.toasts.push(ToastKind::Error, msg.to_string());
        }
    }

    fn discard_history_for_undo(&mut self, action: &UndoAction) {
        match action {
            UndoAction::RestoreDates {
                id,
                start_date,
                target_date,
                ..
            } => {
                if let Some(index) = self.history.undo.iter().position(|entry| match entry {
                    HistoryEntry::Dates {
                        id: entry_id,
                        before_start,
                        before_target,
                        ..
                    } => {
                        *entry_id == *id
                            && before_start == start_date
                            && before_target == target_date
                    }
                    HistoryEntry::DatesGroup { items } => items.first().is_some_and(|item| {
                        item.id == *id
                            && item.before_start == *start_date
                            && item.before_target == *target_date
                    }),
                    _ => false,
                }) {
                    self.history.undo.remove(index);
                }
            }
            UndoAction::RestoreDatesBatch { items } => {
                let Some(first) = items.first() else {
                    return;
                };
                if let Some(index) = self.history.undo.iter().position(|entry| match entry {
                    HistoryEntry::DatesGroup { items: group } => {
                        group.first().is_some_and(|item| item.id == first.id)
                    }
                    HistoryEntry::Dates { id, .. } => *id == first.id,
                    _ => false,
                }) {
                    self.history.undo.remove(index);
                }
            }
            UndoAction::RemoveLink {
                successor_id,
                predecessor_id,
            }
            | UndoAction::RestoreLink {
                successor_id,
                predecessor_id,
            } => {
                if let Some(index) = self.history.undo.iter().rposition(|entry| {
                    matches!(
                        entry,
                        HistoryEntry::Link {
                            successor_id: sid,
                            predecessor_id: pid,
                            ..
                        } if *sid == *successor_id && *pid == *predecessor_id
                    )
                }) {
                    self.history.undo.remove(index);
                }
            }
            UndoAction::DeleteCreated { id, .. } => {
                self.drop_history_for_item(*id);
            }
            UndoAction::RestoreFields { id, fields } => {
                if let Some(index) = self.history.undo.iter().rposition(|entry| {
                    matches!(
                        entry,
                        HistoryEntry::Fields {
                            id: entry_id,
                            changes,
                        } if *entry_id == *id
                            && changes.iter().all(|(reference, before, _)| {
                                fields.iter().any(|(name, value)| {
                                    name == reference && value == before
                                })
                            })
                    )
                }) {
                    self.history.undo.remove(index);
                }
            }
        }
    }

    fn drop_history_for_item(&mut self, id: i64) {
        let keep = |entry: &HistoryEntry| match entry {
            HistoryEntry::Created { id: entry_id, .. }
            | HistoryEntry::Dates { id: entry_id, .. }
            | HistoryEntry::Fields { id: entry_id, .. } => *entry_id != id,
            HistoryEntry::DatesGroup { items } => items.iter().all(|item| item.id != id),
            HistoryEntry::Link {
                successor_id,
                predecessor_id,
                ..
            } => *successor_id != id && *predecessor_id != id,
        };
        self.history.undo.retain(keep);
        self.history.redo.retain(keep);
    }

    async fn restore_fields(
        &mut self,
        id: i64,
        fields: &[(String, Value)],
    ) -> Result<(), AppError> {
        let mut start: Option<Option<Date>> = None;
        let mut target: Option<Option<Date>> = None;
        let mut others: Vec<(String, Value)> = Vec::new();
        for (reference, value) in fields {
            if reference == ado_core::START_DATE_FIELD {
                start = Some(parse_iso_date(value.as_str()));
            } else if reference == ado_core::TARGET_DATE_FIELD {
                target = Some(parse_iso_date(value.as_str()));
            } else {
                others.push((reference.clone(), value.clone()));
            }
        }
        if start.is_some() || target.is_some() {
            let node = self.nodes.iter().find(|node| node.id == id);
            let next_start = start.unwrap_or_else(|| {
                node.and_then(|node| parse_iso_date(node.start_date.as_deref()))
            });
            let next_target = target.unwrap_or_else(|| {
                node.and_then(|node| parse_iso_date(node.target_date.as_deref()))
            });
            self.save_exact_dates(id, next_start, next_target).await?;
        }
        for (reference, value) in others {
            let apply_value = value.clone();
            let apply_reference = reference.clone();
            self.patch_node_field(id, &reference, value, move |node| {
                apply_node_field(node, &apply_reference, &apply_value);
            })
            .await?;
        }
        Ok(())
    }

    pub async fn delete_item(&mut self, id: i64) -> Result<(), AppError> {
        let Some(scope) = self.scope() else {
            return Err(AppError::message("Choose a Team first"));
        };
        let title = self.item_title(id);
        if let Err(err) = self
            .client
            .delete_work_item(&scope.org, &scope.project, id)
            .await
        {
            self.toasts.push(
                ToastKind::Error,
                error_text(&err, "Could not delete work item"),
            );
            return Err(err.into());
        }
        self.nodes.retain(|node| node.id != id);
        if self.gantt.selected_id == Some(id) {
            self.gantt.selected_id = None;
        }
        self.drop_history_for_item(id);
        self.toasts.dismiss_undo_for_item(id);
        if self.history_recording {
            self.toasts
                .push(ToastKind::Success, format!("Deleted #{id} {title}"));
        }
        self.persist_hierarchy_cache();
        Ok(())
    }

    pub async fn undo_clear_dates(&mut self, action: UndoAction) -> Result<(), AppError> {
        self.apply_undo(action).await
    }

    pub async fn schedule_to_iteration(&mut self, id: i64) -> Result<(), AppError> {
        let Some(node) = self.nodes.iter().find(|node| node.id == id).cloned() else {
            return Ok(());
        };
        if !node.has_date_fields {
            self.toasts.push(
                ToastKind::Error,
                "This Work Item has no Start Date and Target Date fields.",
            );
            return Err(AppError::message("no date fields"));
        }
        let Some(hint) = crate::gantt::iteration_for(&node, &self.iterations) else {
            self.toasts.push(ToastKind::Error, "Iteration has no dates");
            return Err(AppError::message("no iteration dates"));
        };
        let Some((start, target)) = schedule_from_iteration(&hint) else {
            self.toasts.push(ToastKind::Error, "Iteration has no dates");
            return Err(AppError::message("no iteration dates"));
        };
        self.save_dates(id, start, target).await
    }

    pub fn copy_id(&mut self, id: i64) -> String {
        let text = id.to_string();
        self.copied = Some(text.clone());
        self.toasts
            .push(ToastKind::Success, format!("Copied #{id}"));
        text
    }

    pub fn copy_link(&mut self, id: i64) -> Option<String> {
        let scope = self.scope()?;
        let url =
            crate::form::ado_edit_url_on(self.client.ado_base(), &scope.org, &scope.project, id);
        self.copied = Some(url.clone());
        self.toasts.push(ToastKind::Success, "Copied link");
        Some(url)
    }

    pub fn open_in_ado(&self, id: i64) {
        if let Some(scope) = self.scope() {
            let url = crate::form::ado_edit_url_on(
                self.client.ado_base(),
                &scope.org,
                &scope.project,
                id,
            );
            let _ = open::that(url);
        }
    }

    pub fn copy_toast(&mut self, id: u64) {
        if let Some(toast) = self.toasts.items.iter().find(|t| t.id == id) {
            self.copied = Some(toast.message.clone());
        }
    }

    pub fn work_item_url(org: &str, project: &str, id: i64) -> String {
        crate::form::ado_edit_url(org, project, id)
    }

    pub fn filter_options(&self, items: &[String], query: &str) -> Vec<String> {
        let needle = query.trim().to_lowercase();
        items
            .iter()
            .filter(|item| needle.is_empty() || item.to_lowercase().contains(&needle))
            .cloned()
            .collect()
    }

    pub fn filter_scope_options(
        options: &[(String, String)],
        query: &str,
    ) -> Vec<(String, String)> {
        let needle = query.trim().to_lowercase();
        options
            .iter()
            .filter(|(value, label)| {
                needle.is_empty()
                    || label.to_lowercase().contains(&needle)
                    || value.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect()
    }

    fn push_dates_moved_toast(
        &mut self,
        id: i64,
        start: Option<Date>,
        target: Option<Date>,
        previous: &WorkItemNode,
    ) {
        self.toasts.push_undo_extra(
            dates_changed_message(
                &self.item_title(id),
                parse_iso_date(previous.start_date.as_deref()),
                parse_iso_date(previous.target_date.as_deref()),
                start,
                target,
            ),
            UndoAction::RestoreDates {
                id,
                start_date: previous.start_date.clone(),
                target_date: previous.target_date.clone(),
                rev: previous.rev,
            },
            parent_move_extra(&self.nodes, previous, start, target),
        );
    }

    /// Shift every descendant's dates by `delta_days` and queue persists.
    pub fn shift_descendants_now(
        &mut self,
        parent_id: i64,
        delta_days: i32,
    ) -> Vec<PendingDateSave> {
        if delta_days == 0 {
            return Vec::new();
        }
        let ids = descendant_ids(&self.nodes, parent_id);
        let mut pending = Vec::new();
        let mut child_changes = Vec::new();
        for id in ids {
            let Some(previous) = self.nodes.iter().find(|node| node.id == id).cloned() else {
                continue;
            };
            if !previous.has_date_fields {
                continue;
            }
            let start = parse_iso_date(previous.start_date.as_deref())
                .map(|date| date.saturating_add(delta_days.days()));
            let target = parse_iso_date(previous.target_date.as_deref())
                .map(|date| date.saturating_add(delta_days.days()));
            if start.is_none() && target.is_none() {
                continue;
            }
            let after_start = start
                .map(iso_date_only)
                .or_else(|| previous.start_date.clone());
            let after_target = target
                .map(iso_date_only)
                .or_else(|| previous.target_date.clone());
            if after_start == previous.start_date && after_target == previous.target_date {
                continue;
            }
            let _ = self.apply_optimistic_partial(id, start, target);
            let change = DatesChange {
                id,
                before_start: previous.start_date.clone(),
                before_target: previous.target_date.clone(),
                after_start: after_start.clone(),
                after_target: after_target.clone(),
            };
            pending.push(PendingDateSave {
                id,
                start,
                target,
                after_start,
                after_target,
                rev: previous.rev,
                previous,
                role: DateSaveRole::Cascade,
            });
            child_changes.push(change);
        }
        if self.history_recording {
            self.fold_descendant_dates(parent_id, child_changes);
        }
        self.attach_children_to_move_toast(parent_id, &pending);
        pending
    }

    /// Fold descendant date shifts into the parent move so one Undo reverts both.
    fn fold_descendant_dates(&mut self, parent_id: i64, children: Vec<DatesChange>) {
        if children.is_empty() {
            return;
        }
        match self.history.undo.last() {
            Some(HistoryEntry::Dates { id, .. }) if *id == parent_id => {
                if let Some(HistoryEntry::Dates {
                    id,
                    before_start,
                    before_target,
                    after_start,
                    after_target,
                }) = self.history.undo.pop()
                {
                    let mut items = vec![DatesChange {
                        id,
                        before_start,
                        before_target,
                        after_start,
                        after_target,
                    }];
                    items.extend(children);
                    self.history.undo.push(HistoryEntry::DatesGroup { items });
                }
            }
            Some(HistoryEntry::DatesGroup { items })
                if items.first().is_some_and(|item| item.id == parent_id) =>
            {
                if let Some(HistoryEntry::DatesGroup { mut items }) = self.history.undo.pop() {
                    items.extend(children);
                    self.history.undo.push(HistoryEntry::DatesGroup { items });
                }
            }
            _ => {
                self.record(HistoryEntry::DatesGroup { items: children });
            }
        }
    }

    fn attach_children_to_move_toast(&mut self, parent_id: i64, children: &[PendingDateSave]) {
        if children.is_empty() {
            return;
        }
        for toast in &mut self.toasts.items {
            let Some(UndoAction::RestoreDates {
                id,
                start_date,
                target_date,
                rev,
            }) = &toast.undo
            else {
                continue;
            };
            if *id != parent_id {
                continue;
            }
            let mut items = vec![DateRestore {
                id: *id,
                start_date: start_date.clone(),
                target_date: target_date.clone(),
                rev: *rev,
            }];
            for child in children {
                items.push(DateRestore {
                    id: child.id,
                    start_date: child.previous.start_date.clone(),
                    target_date: child.previous.target_date.clone(),
                    rev: child.previous.rev,
                });
            }
            toast.undo = Some(UndoAction::RestoreDatesBatch { items });
            break;
        }
    }

    pub(crate) fn item_title(&self, id: i64) -> String {
        self.nodes
            .iter()
            .find(|node| node.id == id)
            .map(|node| node.title.trim())
            .filter(|title| !title.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("#{id}"))
    }

    fn link_changed_message(&self, verb: &str, predecessor_id: i64, successor_id: i64) -> String {
        format!(
            "{verb} link: {} → {}",
            quote_name(&self.item_title(predecessor_id)),
            quote_name(&self.item_title(successor_id))
        )
    }

    /// Toast Undo for dates: prefer the Ctrl+Z path so history and the toast
    /// stay in sync. Fall back to a local restore when that stack entry is gone.
    pub fn toast_restore_or_undo_dates(&mut self, action: UndoAction) -> Vec<PendingDateSave> {
        let matches_stack = match &action {
            UndoAction::RestoreDates { id, .. } => {
                matches!(
                    self.history.undo.last(),
                    Some(HistoryEntry::Dates { id: hid, .. }) if *hid == *id
                ) || matches!(
                    self.history.undo.last(),
                    Some(HistoryEntry::DatesGroup { items }) if items.first().is_some_and(|item| item.id == *id)
                )
            }
            UndoAction::RestoreDatesBatch { items } => items.first().is_some_and(|first| {
                matches!(
                    self.history.undo.last(),
                    Some(HistoryEntry::Dates { id, .. }) if *id == first.id
                ) || matches!(
                    self.history.undo.last(),
                    Some(HistoryEntry::DatesGroup { items: group })
                        if group.first().is_some_and(|item| item.id == first.id)
                )
            }),
            _ => false,
        };
        if matches_stack {
            return self.undo_date_stack_now();
        }
        self.apply_toast_restore_dates_all(action)
    }

    pub(crate) fn history_action_message_for_undo(&self, action: &UndoAction) -> String {
        match action {
            UndoAction::RestoreDates {
                id,
                start_date,
                target_date,
                ..
            } => {
                let live = self.nodes.iter().find(|node| node.id == *id);
                dates_undo_message(
                    &self.item_title(*id),
                    parse_iso_date(start_date.as_deref()),
                    parse_iso_date(target_date.as_deref()),
                    live.and_then(|node| parse_iso_date(node.start_date.as_deref())),
                    live.and_then(|node| parse_iso_date(node.target_date.as_deref())),
                )
            }
            UndoAction::RestoreDatesBatch { items } => {
                let Some(first) = items.first() else {
                    return "Moved dates".into();
                };
                let live = self.nodes.iter().find(|node| node.id == first.id);
                dates_undo_message(
                    &self.item_title(first.id),
                    parse_iso_date(first.start_date.as_deref()),
                    parse_iso_date(first.target_date.as_deref()),
                    live.and_then(|node| parse_iso_date(node.start_date.as_deref())),
                    live.and_then(|node| parse_iso_date(node.target_date.as_deref())),
                )
            }
            UndoAction::RestoreFields { id, fields } => {
                let title = self.item_title(*id);
                match fields.as_slice() {
                    [(reference, before)] if reference == "System.Title" => {
                        renamed_message(&json_plain(before), &title)
                    }
                    [(reference, _)] => {
                        format!(
                            "Saved {} on {}",
                            field_toast_name(reference, None),
                            quote_name(&title)
                        )
                    }
                    _ => format!("Saved fields on {}", quote_name(&title)),
                }
            }
            UndoAction::DeleteCreated {
                title,
                work_item_type,
                placement,
                ..
            } => created_message(work_item_type, title, placement),
            UndoAction::RemoveLink {
                successor_id,
                predecessor_id,
            } => self.link_changed_message("Added", *predecessor_id, *successor_id),
            UndoAction::RestoreLink {
                successor_id,
                predecessor_id,
            } => self.link_changed_message("Removed", *predecessor_id, *successor_id),
        }
    }

    fn history_action_message(&self, entry: &HistoryEntry) -> String {
        match entry {
            HistoryEntry::Dates {
                id,
                before_start,
                before_target,
                after_start,
                after_target,
                ..
            } => dates_changed_message(
                &self.item_title(*id),
                parse_iso_date(before_start.as_deref()),
                parse_iso_date(before_target.as_deref()),
                parse_iso_date(after_start.as_deref()),
                parse_iso_date(after_target.as_deref()),
            ),
            HistoryEntry::DatesGroup { items } => {
                let Some(first) = items.first() else {
                    return "Moved dates".into();
                };
                dates_changed_message(
                    &self.item_title(first.id),
                    parse_iso_date(first.before_start.as_deref()),
                    parse_iso_date(first.before_target.as_deref()),
                    parse_iso_date(first.after_start.as_deref()),
                    parse_iso_date(first.after_target.as_deref()),
                )
            }
            HistoryEntry::Link {
                successor_id,
                predecessor_id,
                linked,
            } => {
                let verb = if *linked { "Added" } else { "Removed" };
                self.link_changed_message(verb, *predecessor_id, *successor_id)
            }
            HistoryEntry::Created {
                title,
                work_item_type,
                placement,
                ..
            } => created_message(work_item_type, title, placement),
            HistoryEntry::Fields { id, changes } => {
                field_changes_message(&self.item_title(*id), changes)
            }
        }
    }

    pub fn push_history_toast(&mut self, entry: &HistoryEntry, is_undo: bool) {
        let original = if is_undo {
            self.history_undo_message(entry)
        } else {
            self.history_action_message(entry)
        };
        let message = if is_undo {
            undone_message(&original)
        } else {
            format!("Redid: {original}")
        };
        self.toasts.push(ToastKind::Success, message);
    }

    fn history_undo_message(&self, entry: &HistoryEntry) -> String {
        match entry {
            HistoryEntry::Dates {
                id,
                before_start,
                before_target,
                after_start,
                after_target,
                ..
            } => dates_undo_message(
                &self.item_title(*id),
                parse_iso_date(before_start.as_deref()),
                parse_iso_date(before_target.as_deref()),
                parse_iso_date(after_start.as_deref()),
                parse_iso_date(after_target.as_deref()),
            ),
            HistoryEntry::DatesGroup { items } => {
                let Some(first) = items.first() else {
                    return "Moved dates".into();
                };
                dates_undo_message(
                    &self.item_title(first.id),
                    parse_iso_date(first.before_start.as_deref()),
                    parse_iso_date(first.before_target.as_deref()),
                    parse_iso_date(first.after_start.as_deref()),
                    parse_iso_date(first.after_target.as_deref()),
                )
            }
            other => self.history_action_message(other),
        }
    }
}

pub(crate) fn undone_message(original: &str) -> String {
    let mut chars = original.chars();
    match chars.next() {
        Some(first) => format!("Undone {}{}", first.to_lowercase(), chars.as_str()),
        None => "Undone".into(),
    }
}

fn toast_date(date: Date) -> String {
    compact_date(date).replace(' ', "\u{00a0}")
}

pub(crate) fn quote_name(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        "\"item\"".into()
    } else {
        format!("\"{title}\"")
    }
}

pub(crate) fn dates_moved_message(
    title: &str,
    start: Option<Date>,
    target: Option<Date>,
) -> String {
    dates_range_message("Moved", title, start, target)
}

fn dates_resized_message(title: &str, start: Option<Date>, target: Option<Date>) -> String {
    dates_range_message("Resized", title, start, target)
}

fn dates_range_message(
    verb: &str,
    title: &str,
    start: Option<Date>,
    target: Option<Date>,
) -> String {
    let title = quote_name(title);
    match (start, target) {
        (Some(start), Some(target)) => format!(
            "{verb} {title} to {} → {}",
            toast_date(start),
            toast_date(target)
        ),
        (Some(start), None) => format!("{verb} {title} to {}", toast_date(start)),
        (None, Some(target)) => format!("{verb} {title} to {}", toast_date(target)),
        (None, None) => format!("Cleared dates on {title}"),
    }
}

fn descendant_ids(nodes: &[WorkItemNode], parent_id: i64) -> Vec<i64> {
    let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
    for node in nodes {
        if let Some(pid) = node.parent_id {
            children.entry(pid).or_default().push(node.id);
        }
    }
    let mut out = Vec::new();
    let mut stack = children.get(&parent_id).cloned().unwrap_or_default();
    while let Some(id) = stack.pop() {
        out.push(id);
        if let Some(kids) = children.get(&id) {
            stack.extend(kids.iter().copied());
        }
    }
    out
}

fn day_delta(from: Date, to: Date) -> Option<i32> {
    to.since((Unit::Day, from)).ok().map(|span| span.get_days())
}

fn parent_move_extra(
    nodes: &[WorkItemNode],
    previous: &WorkItemNode,
    after_start: Option<Date>,
    after_target: Option<Date>,
) -> Option<ToastExtra> {
    if !nodes.iter().any(|node| node.parent_id == Some(previous.id)) {
        return None;
    }
    let before_start = parse_iso_date(previous.start_date.as_deref())?;
    let before_target = parse_iso_date(previous.target_date.as_deref())?;
    let after_start = after_start?;
    let after_target = after_target?;
    if dates_is_resize(
        Some(before_start),
        Some(before_target),
        Some(after_start),
        Some(after_target),
    ) {
        return None;
    }
    let start_delta = day_delta(before_start, after_start)?;
    let target_delta = day_delta(before_target, after_target)?;
    if start_delta == 0 || start_delta != target_delta {
        return None;
    }
    Some(ToastExtra::MoveChildren {
        parent_id: previous.id,
        delta_days: start_delta,
    })
}

fn dates_is_resize(
    before_start: Option<Date>,
    before_target: Option<Date>,
    after_start: Option<Date>,
    after_target: Option<Date>,
) -> bool {
    let xor = (before_start != after_start) != (before_target != after_target);
    if !xor {
        return false;
    }
    let one_sided_move = (before_start.is_some() != before_target.is_some())
        && (after_start.is_none() == before_start.is_none())
        && (after_target.is_none() == before_target.is_none());
    !one_sided_move
}

fn dates_changed_message(
    title: &str,
    before_start: Option<Date>,
    before_target: Option<Date>,
    after_start: Option<Date>,
    after_target: Option<Date>,
) -> String {
    let was_clear = before_start.is_none() && before_target.is_none();
    let now_clear = after_start.is_none() && after_target.is_none();
    if was_clear && !now_clear {
        let title = quote_name(title);
        match (after_start, after_target) {
            (Some(start), Some(target)) => format!(
                "Scheduled {title} {} → {}",
                toast_date(start),
                toast_date(target)
            ),
            (Some(start), None) => format!("Scheduled {title} {}", toast_date(start)),
            (None, Some(target)) => format!("Scheduled {title} {}", toast_date(target)),
            _ => format!("Scheduled {title}"),
        }
    } else if dates_is_resize(before_start, before_target, after_start, after_target) {
        dates_resized_message(title, after_start, after_target)
    } else {
        dates_moved_message(title, after_start, after_target)
    }
}

fn dates_undo_message(
    title: &str,
    before_start: Option<Date>,
    before_target: Option<Date>,
    after_start: Option<Date>,
    after_target: Option<Date>,
) -> String {
    let was_clear = before_start.is_none() && before_target.is_none();
    let now_clear = after_start.is_none() && after_target.is_none();
    if was_clear {
        dates_changed_message(
            title,
            before_start,
            before_target,
            after_start,
            after_target,
        )
    } else if now_clear {
        format!("Cleared dates on {}", quote_name(title))
    } else if dates_is_resize(before_start, before_target, after_start, after_target) {
        dates_resized_message(title, before_start, before_target)
    } else {
        dates_moved_message(title, before_start, before_target)
    }
}

fn created_message(work_item_type: &str, title: &str, placement: &str) -> String {
    let ty = work_item_type.trim();
    let ty = if ty.is_empty() { "work item" } else { ty };
    format!("Created {ty} {} {placement}", quote_name(title))
}

fn renamed_message(old: &str, new: &str) -> String {
    format!("Renamed \"{old}\" to \"{new}\"")
}

fn field_changes_message(title: &str, changes: &[(String, Value, Value)]) -> String {
    match changes {
        [(reference, before, after)] if reference == "System.Title" => {
            renamed_message(&json_plain(before), &json_plain(after))
        }
        [(reference, _, _)] => format!(
            "Saved {} on {}",
            field_toast_name(reference, None),
            quote_name(title)
        ),
        _ => format!("Saved fields on {}", quote_name(title)),
    }
}

fn json_plain(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn is_scheduling_date(reference: &str) -> bool {
    reference == ado_core::START_DATE_FIELD || reference == ado_core::TARGET_DATE_FIELD
}

fn field_changes_from_ops(
    ops: &[ado_core::JsonPatchOp],
    original: &serde_json::Map<String, Value>,
) -> Vec<(String, Value, Value)> {
    ops.iter()
        .filter_map(|op| {
            let reference = op.path.strip_prefix("/fields/")?;
            let after = op.value.clone().unwrap_or(Value::Null);
            let before = original.get(reference).cloned().unwrap_or(Value::Null);
            if before == after {
                return None;
            }
            Some((reference.to_string(), before, after))
        })
        .collect()
}

fn node_field_json(node: &WorkItemNode, reference: &str) -> Value {
    match reference {
        "System.Title" => json!(node.title),
        "System.AssignedTo" => node
            .assigned_to
            .as_ref()
            .map(crate::form::identity_json)
            .unwrap_or(Value::Null),
        "System.State" => json!(node.state),
        "System.IterationPath" => json!(node.iteration_path),
        "System.AreaPath" => json!(node.area_path),
        ado_core::START_DATE_FIELD => node
            .start_date
            .as_ref()
            .map(|text| json!(text))
            .unwrap_or(Value::Null),
        ado_core::TARGET_DATE_FIELD => node
            .target_date
            .as_ref()
            .map(|text| json!(text))
            .unwrap_or(Value::Null),
        other => node.fields.get(other).cloned().unwrap_or(Value::Null),
    }
}

fn write_node_string(node: &mut WorkItemNode, reference: &str, value: &Value) {
    let text = value.as_str().unwrap_or_default().to_string();
    match reference {
        "System.Title" => node.title = text,
        "System.State" => node.state = text,
        "System.IterationPath" => node.iteration_path = text,
        "System.AreaPath" => node.area_path = text,
        _ => {}
    }
    if value.is_null() {
        node.fields.remove(reference);
    } else {
        node.fields.insert(reference.to_string(), value.clone());
    }
}

fn apply_node_field(node: &mut WorkItemNode, reference: &str, value: &Value) {
    match reference {
        "System.Title" | "System.State" | "System.IterationPath" | "System.AreaPath" => {
            write_node_string(node, reference, value);
        }
        "System.AssignedTo" => {
            node.assigned_to = identity_from_field(value);
            if value.is_null() {
                node.fields.remove(reference);
            } else {
                node.fields.insert(reference.to_string(), value.clone());
            }
        }
        ado_core::START_DATE_FIELD => {
            node.start_date = value
                .as_str()
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned);
            if value.is_null() || node.start_date.is_none() {
                node.fields.remove(reference);
            } else {
                node.fields.insert(reference.to_string(), value.clone());
            }
        }
        ado_core::TARGET_DATE_FIELD => {
            node.target_date = value
                .as_str()
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned);
            if value.is_null() || node.target_date.is_none() {
                node.fields.remove(reference);
            } else {
                node.fields.insert(reference.to_string(), value.clone());
            }
        }
        other => {
            if value.is_null() {
                node.fields.remove(other);
            } else {
                node.fields.insert(other.to_string(), value.clone());
            }
        }
    }
}

pub(crate) fn field_toast_name(reference: &str, label: Option<&str>) -> String {
    if let Some(label) = label.map(str::trim).filter(|text| !text.is_empty()) {
        return label.to_lowercase();
    }
    match reference {
        "System.Title" => "title".into(),
        "System.AssignedTo" => "assignee".into(),
        ado_core::START_DATE_FIELD => "start date".into(),
        ado_core::TARGET_DATE_FIELD => "target date".into(),
        other => {
            let leaf = other.rsplit('.').next().unwrap_or(other);
            let mut out = String::new();
            for (i, ch) in leaf.chars().enumerate() {
                if i > 0 && ch.is_uppercase() {
                    out.push(' ');
                }
                out.extend(ch.to_lowercase());
            }
            out
        }
    }
}

pub(crate) fn child_work_item_type(parent_type: Option<&str>, types: &[String]) -> String {
    let preferred: &[&str] = match parent_type {
        Some("Epic") => &[
            "Feature",
            "User Story",
            "Product Backlog Item",
            "Requirement",
            "Issue",
            "Story",
            "Task",
        ],
        Some("Feature") => &[
            "User Story",
            "Product Backlog Item",
            "Requirement",
            "Issue",
            "Story",
            "Task",
            "Bug",
        ],
        Some("User Story")
        | Some("Product Backlog Item")
        | Some("Requirement")
        | Some("Issue")
        | Some("Story") => &["Task", "Bug"],
        Some("Task") | Some("Bug") => &["Task", "Bug"],
        _ => &[
            "Epic",
            "Feature",
            "User Story",
            "Product Backlog Item",
            "Requirement",
            "Issue",
            "Story",
            "Task",
            "Bug",
        ],
    };
    preferred
        .iter()
        .find(|ty| types.iter().any(|item| item == **ty))
        .map(|ty| (*ty).to_string())
        .or_else(|| types.first().cloned())
        .unwrap_or_else(|| "User Story".into())
}

fn toggle_hidden(list: &mut Vec<String>, value: &str) {
    if let Some(index) = list.iter().position(|item| item == value) {
        list.remove(index);
    } else {
        list.push(value.to_string());
    }
}

pub fn fake_hierarchy(count: usize) -> Vec<WorkItemNode> {
    (0..count)
        .map(|i| WorkItemNode {
            id: i as i64 + 1,
            rev: 1,
            title: format!("Work Item {i}"),
            work_item_type: if i % 20 == 0 { "Epic" } else { "Feature" }.into(),
            state: "New".into(),
            assigned_to: None,
            parent_id: if i % 20 == 0 {
                None
            } else {
                Some(((i / 20) * 20 + 1) as i64)
            },
            area_path: "Shop".into(),
            iteration_path: "Shop\\Sprint 12".into(),
            start_date: Some("2026-07-06T00:00:00Z".into()),
            target_date: Some("2026-07-20T00:00:00Z".into()),
            has_date_fields: true,
            predecessors: Vec::new(),
            fields: serde_json::Map::new(),
        })
        .collect()
}

/// Backlog shaped like a real ADO team: a deep Epic → Feature → Story → Task/Test
/// Case tree plus a long unscheduled tail and a truncated Product Backlog Item.
pub fn fake_ado_shaped_backlog() -> Vec<WorkItemNode> {
    fn item(id: i64, parent: Option<i64>, ty: &str, title: &str, scheduled: bool) -> WorkItemNode {
        WorkItemNode {
            id,
            rev: 1,
            title: title.into(),
            work_item_type: ty.into(),
            state: "New".into(),
            assigned_to: None,
            parent_id: parent,
            area_path: "Shop".into(),
            iteration_path: if scheduled {
                "Shop\\Sprint 12".into()
            } else {
                "Shop".into()
            },
            start_date: scheduled.then(|| "2026-07-06T00:00:00Z".into()),
            target_date: scheduled.then(|| "2026-07-20T00:00:00Z".into()),
            has_date_fields: true,
            predecessors: Vec::new(),
            fields: serde_json::Map::new(),
        }
    }
    let mut nodes = vec![
        item(1, None, "Epic", "Shop modernization", true),
        item(2, Some(1), "Feature", "Product Backlog Item catalog", true),
        item(
            3,
            Some(2),
            "User Story",
            "Persist cart across sessions",
            true,
        ),
        item(4, Some(3), "Task", "Wire Redis session store", true),
        item(5, Some(3), "Test Case", "Cart survives refresh", false),
        item(
            6,
            Some(2),
            "Product Backlog Item",
            "Product Backlog Item for checkout that is quite long",
            false,
        ),
    ];
    for i in 20..55 {
        nodes.push(item(
            i,
            Some(2),
            "User Story",
            &format!("Unscheduled backlog {i}"),
            false,
        ));
    }
    nodes
}

pub fn json_scope(org: &str, project: &str, team: &str) -> Value {
    json!({ "org": org, "project": project, "team": team })
}

fn team_settings_banner(team: &str, raw: &str) -> String {
    if raw.contains("TF400497") || raw.to_ascii_lowercase().contains("iteration path") {
        format!(
            "Team \"{team}\" has invalid backlog settings (the backlog iteration path is not in this team's area). Fix Team Settings in Azure DevOps, or pick another team."
        )
    } else {
        format!("Could not load backlog settings for \"{team}\". {raw}")
    }
}

#[allow(dead_code)]
pub(crate) fn pending_nudge_rev(pending: &PendingNudge) -> i64 {
    pending.original_rev
}

#[cfg(test)]
mod child_type_tests {
    use super::child_work_item_type;

    #[test]
    fn root_add_item_prefers_epic() {
        let types = vec![
            "Bug".into(),
            "Epic".into(),
            "Feature".into(),
            "Task".into(),
            "User Story".into(),
        ];
        assert_eq!(child_work_item_type(None, &types), "Epic");
        assert_eq!(child_work_item_type(Some("Epic"), &types), "Feature");
        assert_eq!(child_work_item_type(Some("User Story"), &types), "Task");
    }

    #[test]
    fn scrum_feature_prefers_product_backlog_item() {
        let types = vec![
            "Bug".into(),
            "Epic".into(),
            "Feature".into(),
            "Product Backlog Item".into(),
            "Task".into(),
        ];
        assert_eq!(
            child_work_item_type(Some("Feature"), &types),
            "Product Backlog Item"
        );
        assert_eq!(
            child_work_item_type(Some("Product Backlog Item"), &types),
            "Task"
        );
        assert_eq!(child_work_item_type(None, &types), "Epic");
    }

    #[test]
    fn cmmi_feature_prefers_requirement() {
        let types = vec![
            "Bug".into(),
            "Epic".into(),
            "Feature".into(),
            "Issue".into(),
            "Requirement".into(),
            "Task".into(),
        ];
        assert_eq!(child_work_item_type(Some("Feature"), &types), "Requirement");
        assert_eq!(child_work_item_type(Some("Requirement"), &types), "Task");
        assert_eq!(child_work_item_type(Some("Issue"), &types), "Task");
        assert_eq!(child_work_item_type(Some("Epic"), &types), "Feature");
    }
}

#[cfg(test)]
mod node_field_tests {
    use super::{apply_node_field, node_field_json};
    use ado_core::WorkItemNode;
    use serde_json::json;

    fn sample() -> WorkItemNode {
        WorkItemNode {
            id: 1,
            rev: 1,
            title: "Cart".into(),
            work_item_type: "User Story".into(),
            state: "New".into(),
            assigned_to: None,
            parent_id: None,
            area_path: "Shop".into(),
            iteration_path: "Shop\\Sprint 12".into(),
            start_date: None,
            target_date: None,
            has_date_fields: true,
            predecessors: Vec::new(),
            fields: serde_json::Map::new(),
        }
    }

    #[test]
    fn apply_node_field_writes_canonical_state_iteration_and_area() {
        let mut node = sample();
        apply_node_field(&mut node, "System.State", &json!("Active"));
        apply_node_field(&mut node, "System.IterationPath", &json!("Shop\\Sprint 13"));
        apply_node_field(&mut node, "System.AreaPath", &json!("Shop\\Platform"));
        assert_eq!(node.state, "Active");
        assert_eq!(node.iteration_path, "Shop\\Sprint 13");
        assert_eq!(node.area_path, "Shop\\Platform");
        assert_eq!(node_field_json(&node, "System.State"), json!("Active"));
        assert_eq!(
            node_field_json(&node, "System.IterationPath"),
            json!("Shop\\Sprint 13")
        );
        assert_eq!(
            node_field_json(&node, "System.AreaPath"),
            json!("Shop\\Platform")
        );
    }
}

#[cfg(test)]
mod toast_copy_tests {
    use super::dates_moved_message;
    use jiff::civil::Date;

    #[test]
    fn moved_toast_names_dates_of_the_item() {
        let start = Date::new(2026, 7, 6).unwrap();
        let target = Date::new(2026, 8, 18).unwrap();
        let message = dates_moved_message("Cart", Some(start), Some(target));
        assert!(
            message.starts_with("Moved \"Cart\" to"),
            "move toasts must quote the item then the range, got {message}"
        );
        assert!(
            message.contains('\u{00a0}'),
            "the range must use a non-breaking space so '18' cannot wrap alone, got {message}"
        );
    }

    #[test]
    fn left_edge_resize_toast_says_resized() {
        use super::dates_changed_message;
        let start = Date::new(2026, 7, 6).unwrap();
        let target = Date::new(2026, 8, 14).unwrap();
        let resized_start = Date::new(2026, 7, 9).unwrap();
        let message = dates_changed_message(
            "Cart",
            Some(start),
            Some(target),
            Some(resized_start),
            Some(target),
        );
        assert!(
            message.starts_with("Resized \"Cart\" to"),
            "a left-edge resize must not read as a move, got {message}"
        );
        let resized_end = Date::new(2026, 8, 17).unwrap();
        let end_message = dates_changed_message(
            "Cart",
            Some(start),
            Some(target),
            Some(start),
            Some(resized_end),
        );
        assert!(
            end_message.starts_with("Resized \"Cart\" to"),
            "a right-edge resize must not read as a move, got {end_message}"
        );
        let moved = dates_changed_message(
            "Cart",
            Some(start),
            Some(target),
            Some(resized_start),
            Some(resized_end),
        );
        assert!(
            moved.starts_with("Moved \"Cart\" to"),
            "shifting both dates must still say Moved, got {moved}"
        );
        let start_only =
            dates_changed_message("Cart UI", Some(start), None, Some(resized_start), None);
        assert!(
            start_only.starts_with("Moved \"Cart UI\" to"),
            "moving a start-only bar must not read as a resize, got {start_only}"
        );
    }

    #[test]
    fn undo_prefix_has_no_double_colon() {
        use super::undone_message;
        let message = undone_message("Added link: \"A\" → \"B\"");
        assert_eq!(message, "Undone added link: \"A\" → \"B\"");
        assert_eq!(message.matches(':').count(), 1);
        assert_eq!(
            undone_message("Renamed \"Cart\" to \"Basket\""),
            "Undone renamed \"Cart\" to \"Basket\""
        );
    }

    #[test]
    fn undo_clear_dates_names_the_clear() {
        use super::{dates_undo_message, undone_message};
        let start = Date::new(2026, 7, 21).unwrap();
        let original = dates_undo_message("Cart UI", Some(start), None, None, None);
        assert_eq!(original, "Cleared dates on \"Cart UI\"");
        assert_eq!(
            undone_message(&original),
            "Undone cleared dates on \"Cart UI\""
        );
    }

    #[test]
    fn undo_of_a_resize_reuses_the_resized_verb() {
        use super::{dates_undo_message, undone_message};
        let start = Date::new(2026, 7, 6).unwrap();
        let target = Date::new(2026, 8, 14).unwrap();
        let resized_start = Date::new(2026, 7, 9).unwrap();
        let resized_end = Date::new(2026, 8, 17).unwrap();
        let original = dates_undo_message(
            "Cart",
            Some(start),
            Some(target),
            Some(start),
            Some(resized_end),
        );
        assert!(
            original.starts_with("Resized \"Cart\" to"),
            "undo of a resize must keep Resized, got {original}"
        );
        assert!(
            original.contains("14"),
            "undo must name the restored dates, got {original}"
        );
        let undid = undone_message(&original);
        assert!(
            undid.starts_with("Undone resized"),
            "undo toast must reuse the resize verb, got {undid}"
        );
        let moved = dates_undo_message(
            "Cart",
            Some(start),
            Some(target),
            Some(resized_start),
            Some(resized_end),
        );
        assert!(
            moved.starts_with("Moved \"Cart\" to"),
            "undo of a two-sided move must keep Moved, got {moved}"
        );
    }

    #[test]
    fn parent_move_offers_shift_children() {
        use super::parent_move_extra;
        use crate::toasts::ToastExtra;
        let mut nodes = super::fake_hierarchy(3);
        let previous = nodes[0].clone();
        let start = Date::new(2026, 7, 9).unwrap();
        let target = Date::new(2026, 7, 23).unwrap();
        match parent_move_extra(&nodes, &previous, Some(start), Some(target)) {
            Some(ToastExtra::MoveChildren {
                parent_id,
                delta_days,
            }) => {
                assert_eq!(parent_id, 1);
                assert_eq!(delta_days, 3);
            }
            other => panic!("expected MoveChildren, got {other:?}"),
        }
        nodes[0].parent_id = Some(99);
        nodes[1].parent_id = Some(99);
        nodes[2].parent_id = Some(99);
        assert!(
            parent_move_extra(&nodes, &previous, Some(start), Some(target)).is_none(),
            "a leaf move must not offer Move children too"
        );
    }

    #[test]
    fn shift_descendants_moves_child_dates() {
        let mut state = super::PlannerState::new(
            crate::paths::AppPaths::new(std::env::temp_dir().join("ado-planner-shift-test")),
            ado_session::PlannerSession::pat(ado_session::EncryptedStore::new(
                Box::new(ado_session::MemoryStore::new(false)),
                None,
            )),
            None,
            None,
        );
        state.nodes = super::fake_hierarchy(3);
        state.history_recording = true;
        let pending = state.shift_descendants_now(1, 3);
        assert!(
            pending.iter().any(|save| save.id == 2),
            "direct children must shift, got {:?}",
            pending.iter().map(|save| save.id).collect::<Vec<_>>()
        );
        let child = state.nodes.iter().find(|node| node.id == 2).expect("child");
        assert_eq!(child.start_date.as_deref(), Some("2026-07-09T00:00:00Z"));
        assert_eq!(child.target_date.as_deref(), Some("2026-07-23T00:00:00Z"));
        assert_eq!(
            pending[0].role,
            super::DateSaveRole::Cascade,
            "children persist must not replace the parent undo toast"
        );
    }

    #[test]
    fn move_children_too_is_one_undo_with_the_parent() {
        use crate::toasts::{HistoryEntry, UndoAction};
        let mut state = super::PlannerState::new(
            crate::paths::AppPaths::new(std::env::temp_dir().join("ado-planner-batch-undo")),
            ado_session::PlannerSession::pat(ado_session::EncryptedStore::new(
                Box::new(ado_session::MemoryStore::new(false)),
                None,
            )),
            None,
            None,
        );
        state.nodes = super::fake_hierarchy(3);
        state.history_recording = true;
        let parent_before = state.nodes[0].clone();
        let _ = state.apply_optimistic_partial(
            1,
            Some(Date::new(2026, 7, 9).unwrap()),
            Some(Date::new(2026, 7, 23).unwrap()),
        );
        state.record_dates(
            1,
            &parent_before,
            Some("2026-07-09T00:00:00Z".into()),
            Some("2026-07-23T00:00:00Z".into()),
        );
        state.toasts.push_undo_extra(
            "Moved \"Work Item 0\" to Jul 9 → Jul 23",
            UndoAction::RestoreDates {
                id: 1,
                start_date: parent_before.start_date.clone(),
                target_date: parent_before.target_date.clone(),
                rev: parent_before.rev,
            },
            Some(crate::toasts::ToastExtra::MoveChildren {
                parent_id: 1,
                delta_days: 3,
            }),
        );
        let _ = state.shift_descendants_now(1, 3);
        assert_eq!(
            state.history.undo.len(),
            1,
            "parent move + children must be one history step, got {:?}",
            state.history.undo
        );
        match state.history.undo.last() {
            Some(HistoryEntry::DatesGroup { items }) => {
                assert!(
                    items.iter().any(|item| item.id == 1) && items.iter().any(|item| item.id == 2),
                    "group must include parent and child, got {items:?}"
                );
            }
            other => panic!("expected DatesGroup, got {other:?}"),
        }
        assert!(
            matches!(
                state.toasts.items.last().and_then(|toast| toast.undo.as_ref()),
                Some(UndoAction::RestoreDatesBatch { items }) if items.len() >= 2
            ),
            "toast Undo must restore parent and children together, got {:?}",
            state.toasts.items.last().map(|toast| &toast.undo)
        );
        let pending = state.undo_date_stack_now();
        assert!(
            pending.iter().any(|save| save.id == 1) && pending.iter().any(|save| save.id == 2),
            "one Undo must restore parent and child, got {:?}",
            pending.iter().map(|save| save.id).collect::<Vec<_>>()
        );
        let parent = state.nodes.iter().find(|node| node.id == 1).unwrap();
        let child = state.nodes.iter().find(|node| node.id == 2).unwrap();
        assert_eq!(parent.start_date.as_deref(), Some("2026-07-06T00:00:00Z"));
        assert_eq!(child.start_date.as_deref(), Some("2026-07-06T00:00:00Z"));
    }
}
