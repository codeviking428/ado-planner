use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ado_client::{AdoClient, AdoRuntime, TokenSource};
use ado_core::{
    AssigneeFilter, Flavor, HierarchyResult, IdentityValue, IterationNode, Organization,
    OverlayFilter, Project, ScopeSelection, SessionInfo, Team, WorkItemNode, apply_overlays,
    build_clear_dates_patch, build_dates_patch, build_field_patch, choose_available,
    is_assignee_special, iso_date_only, parse_iso_date, resolve_root_types,
    schedule_from_iteration,
};
use ado_session::{LoginCreds, PlannerSession, SharedSession};
use jiff::ToSpan;
use jiff::civil::Date;
use serde_json::{Value, json};

use crate::cache::{CacheFreshness, QueryCache};
use crate::error::AppError;
use crate::form::FormDraft;
use crate::gantt::{GanttView, GestureKind, NudgeAxis, PendingNudge, VisibleRow};
use crate::paths::AppPaths;
use crate::prefs::AppPrefs;
use crate::toasts::{EditHistory, HistoryEntry, ToastKind, Toasts, UndoAction, error_text};
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
        let resolved = resolve_root_types(
            self.prefs.planner.root_types.clone(),
            &types,
            self.hierarchy
                .as_ref()
                .map(|h| h.top_backlog_types.as_slice())
                .unwrap_or(&[]),
        );
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

    pub fn hidden_roots(&self) -> Vec<String> {
        let types = self.types();
        let resolved = resolve_root_types(
            self.prefs.planner.root_types.clone(),
            &types,
            self.hierarchy
                .as_ref()
                .map(|h| h.top_backlog_types.as_slice())
                .unwrap_or(&[]),
        );
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
        self.prefs.planner.team = team;
        self.prefs.planner.iteration_path.clear();
        self.prefs.planner.root_types = None;
        self.persist_prefs();
        self.heal_and_load_iterations().await?;
        self.load_members().await?;
        self.load_hierarchy().await
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
        if let Some(hit) = self.cache.get(&key)
            && let Ok(result) = serde_json::from_value::<HierarchyResult>(hit.value.clone())
        {
            self.apply_hierarchy(result.clone());
            if hit.freshness == CacheFreshness::Fresh && !result.top_backlog_types.is_empty() {
                self.hierarchy_loading = false;
                return Ok(());
            }
        }
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
        self.gantt.reveal_content(&self.nodes, &self.iterations);
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
                self.form = Some(FormDraft::from_model(model));
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
                if let (Some(before), Some(after)) = (
                    before.as_ref(),
                    self.nodes.iter().find(|node| node.id == id),
                ) && (before.start_date != after.start_date
                    || before.target_date != after.target_date)
                {
                    self.record_dates(
                        id,
                        before,
                        after.start_date.clone(),
                        after.target_date.clone(),
                    );
                }
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
                let message = if names.is_empty() {
                    format!("Saved Work Item #{id}")
                } else {
                    format!("Saved {}", names.join(", "))
                };
                self.toasts.push(ToastKind::Success, message);
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
        self.record(HistoryEntry::Dates {
            id,
            before_start: before.start_date.clone(),
            before_target: before.target_date.clone(),
            after_start,
            after_target,
        });
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
        let ops = build_dates_patch(node.rev, Some(start), Some(target));
        match self
            .client
            .patch_work_item(&scope.org, &scope.project, id, &ops)
            .await
        {
            Ok(rev) => {
                if let Some(node) = self.nodes.iter_mut().find(|node| node.id == id) {
                    node.rev = rev;
                }
                self.persist_hierarchy_cache();
                if self.history_recording {
                    self.record_dates(
                        id,
                        &previous,
                        Some(iso_date_only(start)),
                        Some(iso_date_only(target)),
                    );
                    self.toasts
                        .push(ToastKind::Success, "Saved Start Date and Target Date");
                }
                Ok(())
            }
            Err(err) => {
                self.restore_node(previous);
                self.toasts
                    .push(ToastKind::Error, error_text(&err, "Could not save dates"));
                Err(err.into())
            }
        }
    }

    pub async fn schedule_at(&mut self, id: i64, start: Date) -> Result<(), AppError> {
        let target = start.saturating_add(13.days());
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
                let ops = build_dates_patch(node.rev, start, target);
                match self
                    .client
                    .patch_work_item(&scope.org, &scope.project, id, &ops)
                    .await
                {
                    Ok(rev) => {
                        if let Some(node) = self.nodes.iter_mut().find(|node| node.id == id) {
                            node.rev = rev;
                        }
                        self.persist_hierarchy_cache();
                        let after_start = start
                            .map(iso_date_only)
                            .or_else(|| previous.start_date.clone());
                        let after_target = target
                            .map(iso_date_only)
                            .or_else(|| previous.target_date.clone());
                        let message = match (start, target) {
                            (Some(_), None) => "Saved Start Date",
                            (None, Some(_)) => "Saved Target Date",
                            _ => "Saved Start Date and Target Date",
                        };
                        if self.history_recording {
                            self.record_dates(id, &previous, after_start, after_target);
                            self.toasts.push(ToastKind::Success, message);
                        }
                        Ok(())
                    }
                    Err(err) => {
                        self.restore_node(previous);
                        self.toasts
                            .push(ToastKind::Error, error_text(&err, "Could not save dates"));
                        Err(err.into())
                    }
                }
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
                self.persist_hierarchy_cache();
                if self.history_recording {
                    self.record(HistoryEntry::Link {
                        successor_id,
                        predecessor_id,
                        linked: true,
                    });
                    self.toasts.push_undo(
                        format!("Linked #{predecessor_id} → #{successor_id}"),
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
                self.gantt.selected_link = None;
                self.persist_hierarchy_cache();
                if self.history_recording {
                    self.record(HistoryEntry::Link {
                        successor_id,
                        predecessor_id,
                        linked: false,
                    });
                    self.toasts.push_undo(
                        format!("Removed link #{predecessor_id} → #{successor_id}"),
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
        let Some(scope) = self.scope() else {
            return Err(AppError::message("Choose a Team first"));
        };
        let types = self.types();
        let work_item_type = ["User Story", "Task", "Bug", "Feature", "Epic"]
            .into_iter()
            .find(|ty| types.iter().any(|item| item == ty))
            .map(ToString::to_string)
            .or_else(|| types.first().cloned())
            .unwrap_or_else(|| "User Story".into());
        let parent = self
            .gantt
            .selected_id
            .and_then(|id| self.nodes.iter().find(|node| node.id == id));
        let area = parent
            .map(|node| node.area_path.clone())
            .or_else(|| self.nodes.first().map(|node| node.area_path.clone()))
            .unwrap_or_else(|| scope.project.clone());
        let iteration = parent
            .map(|node| node.iteration_path.clone())
            .filter(|path| !path.is_empty())
            .or_else(|| {
                let path = self.prefs.planner.iteration_path.clone();
                (!path.is_empty()).then_some(path)
            })
            .unwrap_or_default();
        let parent_id = parent.map(|node| node.id);
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
            ops.push(ado_core::JsonPatchOp {
                op: ado_core::PatchOp::Add,
                path: "/fields/System.Parent".into(),
                value: Some(json!(parent_id)),
            });
        }
        match self
            .client
            .create_work_item(&scope.org, &scope.project, &work_item_type, &ops)
            .await
        {
            Ok(mut node) => {
                node.parent_id = parent_id;
                let id = node.id;
                self.nodes.push(node);
                self.gantt.selected_id = Some(id);
                self.persist_hierarchy_cache();
                self.toasts
                    .push(ToastKind::Success, format!("Created #{id}"));
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
        let ops = build_field_patch(node.rev, reference, value);
        match self
            .client
            .patch_work_item(&scope.org, &scope.project, id, &ops)
            .await
        {
            Ok(rev) => {
                if let Some(live) = self.nodes.iter_mut().find(|n| n.id == id) {
                    live.rev = rev;
                }
                self.persist_hierarchy_cache();
                self.toasts.push(
                    ToastKind::Success,
                    format!("Saved {}", field_toast_name(reference, None)),
                );
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
        if let Err(message) = self.gantt.begin_gesture(&bar, kind, origin_x, &visible) {
            self.toasts.push(ToastKind::Error, message);
            return Err(AppError::message(message));
        }
        Ok(())
    }

    pub fn update_bar_gesture(&mut self, x: f32) {
        self.gantt.update_gesture(x);
        if let Some(gesture) = &self.gantt.gesture {
            if gesture.kind == GestureKind::Link {
                return;
            }
            let (start, target) = crate::gantt::GanttView::gesture_patch_dates(gesture);
            let _ = self.apply_optimistic_partial(gesture.id, start, target);
        }
    }

    pub fn cancel_gesture(&mut self) {
        if let Some(gesture) = self.gantt.gesture.clone() {
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
            if let Some(to_id) = to_id
                && to_id != from_id
            {
                return self.add_dependency(to_id, from_id).await;
            }
            return Ok(());
        }
        if let Some(gesture) = self.gantt.gesture.clone() {
            if gesture.live_start == gesture.start && gesture.live_target == gesture.target {
                self.gantt.gesture = None;
                return Ok(());
            }
            let (start, target) = crate::gantt::GanttView::gesture_patch_dates(&gesture);
            self.gantt.gesture = None;
            return self.save_partial_dates(gesture.id, start, target).await;
        }
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
        let ops = build_clear_dates_patch(node.rev);
        match self
            .client
            .patch_work_item(&scope.org, &scope.project, id, &ops)
            .await
        {
            Ok(rev) => {
                if let Some(live) = self.nodes.iter_mut().find(|n| n.id == id) {
                    live.rev = rev;
                }
                self.persist_hierarchy_cache();
                if self.history_recording {
                    self.record_dates(id, &previous, None, None);
                    self.toasts.push_undo(
                        "Cleared dates",
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
            self.history.redo.push(entry);
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
                match (
                    parse_iso_date(start.as_deref()),
                    parse_iso_date(target.as_deref()),
                ) {
                    (None, None) => self.clear_dates(*id).await,
                    (start, target) => self.save_partial_dates(*id, start, target).await,
                }
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
        }
    }

    pub async fn apply_undo(&mut self, _action: UndoAction) -> Result<(), AppError> {
        self.undo().await
    }

    pub async fn undo_clear_dates(&mut self, _action: UndoAction) -> Result<(), AppError> {
        self.undo().await
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
