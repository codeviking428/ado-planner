use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use thiserror::Error;

use ado_core::{
    AreaPathValue, BATCH_SIZE, BacklogLevel, BatchWorkItem, FieldMetadata, HIERARCHY_FIELDS,
    HierarchyResult, IdentityValue, IterationNode, JsonPatchOp, Organization, ProcessLayout,
    Project, ScopeSelection, Team, TeamMemberIdentityRow, WIQL_RESULT_CAP, WorkItemFormModel,
    WorkItemNode, assemble_forest, build_hierarchy_wiql, build_untyped_hierarchy_wiql, chunk_ids,
    flatten_layout, map_batch_work_item, map_team_member_identities, top_backlog_types_from_levels,
    type_has_start_and_target,
};

use crate::auth::{AuthScheme, TokenSource, authorization_header};
use crate::urls::{identities_search_host, resolve_ado_base, resolve_vssps_base};

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Core(#[from] ado_core::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error("{status} {url}: {body}")]
    Status {
        status: StatusCode,
        url: String,
        body: String,
    },
}

impl ClientError {
    pub fn is_precondition_failed(&self) -> bool {
        matches!(self, Self::Status { status, .. } if *status == StatusCode::PRECONDITION_FAILED)
    }

    pub fn is_backlogs_failure(&self) -> bool {
        match self {
            Self::Status { url, body, status } => {
                url.contains("/backlogs")
                    && (body.contains("TF400497")
                        || body.to_ascii_lowercase().contains("iteration path")
                        || *status == StatusCode::INTERNAL_SERVER_ERROR
                        || *status == StatusCode::BAD_REQUEST)
            }
            Self::Message(text) => text.contains("TF400497"),
            _ => false,
        }
    }
}

pub struct AdoClient {
    http: Client,
    tokens: Arc<dyn TokenSource>,
    ado_base: String,
    vssps_base: String,
}

impl AdoClient {
    pub fn new(tokens: Arc<dyn TokenSource>) -> Self {
        let env_ado = std::env::var("ADO_PLANNER_ADO_BASE_URL").ok();
        let env_vssps = std::env::var("ADO_PLANNER_VSSPS_URL").ok();
        let ado_base = resolve_ado_base(None, env_ado.as_deref());
        let vssps_base = resolve_vssps_base(None, env_vssps.as_deref(), &ado_base);
        Self::with_base_urls(tokens, ado_base, vssps_base)
    }

    pub fn with_base_urls(
        tokens: Arc<dyn TokenSource>,
        ado_base: impl Into<String>,
        vssps_base: impl Into<String>,
    ) -> Self {
        Self {
            http: Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_else(|_| Client::new()),
            tokens,
            ado_base: ado_base.into().trim_end_matches('/').to_string(),
            vssps_base: vssps_base.into().trim_end_matches('/').to_string(),
        }
    }

    pub fn ado_base(&self) -> &str {
        &self.ado_base
    }

    fn org_url(&self, org: &str) -> String {
        format!("{}/{org}", self.ado_base)
    }

    async fn request(
        &self,
        method: Method,
        url: &str,
        body: Option<&Value>,
    ) -> Result<Value, ClientError> {
        let token = self.tokens.access_token().await?;
        let mut headers = HeaderMap::new();
        let mut authorization =
            HeaderValue::from_str(&authorization_header(&token, self.tokens.scheme()))
                .map_err(|err| ClientError::Message(err.to_string()))?;
        authorization.set_sensitive(true);
        headers.insert(AUTHORIZATION, authorization);
        if method != Method::GET && method != Method::HEAD {
            let content_type = if method == Method::PATCH || url.contains("/workitems/$") {
                "application/json-patch+json"
            } else {
                "application/json"
            };
            headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
        }
        let mut builder = self.http.request(method.clone(), url).headers(headers);
        if let Some(body) = body {
            let bytes =
                serde_json::to_vec(body).map_err(|err| ClientError::Message(err.to_string()))?;
            builder = builder.body(bytes);
        }
        tracing::debug!(method = %method, url, "ado request");
        let response = builder.send().await?;
        let status = response.status();
        tracing::debug!(method = %method, url, %status, "ado response");
        if status == StatusCode::NO_CONTENT {
            return Ok(Value::Null);
        }
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ClientError::Status {
                status,
                url: url.to_string(),
                body: text.chars().take(400).collect(),
            });
        }
        if text.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|err| ClientError::Message(err.to_string()))
    }

    async fn get(&self, url: &str) -> Result<Value, ClientError> {
        self.request(Method::GET, url, None).await
    }

    pub async fn list_organizations(&self) -> Result<Vec<Organization>, ClientError> {
        if self.tokens.scheme() == AuthScheme::Pat {
            let organization = self.tokens.organization().await?;
            let Some(organization) = organization else {
                return Err(ClientError::Core(ado_core::Error::NoOrganization));
            };
            return Ok(vec![Organization {
                account_name: organization,
            }]);
        }
        let profile = self
            .get(&format!(
                "{}/_apis/profile/profiles/me?api-version=7.1",
                self.vssps_base
            ))
            .await?;
        let qs = profile
            .get("id")
            .and_then(Value::as_str)
            .map(|id| format!("memberId={id}&"))
            .unwrap_or_default();
        let payload = self
            .get(&format!(
                "{}/_apis/accounts?{qs}api-version=7.1",
                self.vssps_base
            ))
            .await?;
        Ok(payload
            .get("value")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|row| {
                row.get("accountName")
                    .and_then(Value::as_str)
                    .map(|name| Organization {
                        account_name: name.to_string(),
                    })
            })
            .collect())
    }

    pub async fn ping_org(&self, org: &str) -> Result<(), ClientError> {
        self.get(&format!(
            "{}/_apis/projects?$top=1&api-version=7.1",
            self.org_url(org)
        ))
        .await?;
        Ok(())
    }

    pub async fn list_projects(&self, org: &str) -> Result<Vec<Project>, ClientError> {
        self.list_named(&format!("{}/_apis/projects", self.org_url(org)))
            .await
    }

    pub async fn list_teams(&self, org: &str, project: &str) -> Result<Vec<Team>, ClientError> {
        Ok(self
            .list_named(&format!(
                "{}/_apis/projects/{}/teams",
                self.org_url(org),
                urlencoding::encode(project)
            ))
            .await?
            .into_iter()
            .map(|row| Team {
                id: row.id,
                name: row.name,
            })
            .collect())
    }

    async fn list_named(&self, base: &str) -> Result<Vec<Project>, ClientError> {
        let mut items = Vec::new();
        let mut skip = 0usize;
        loop {
            let payload = self
                .get(&format!("{base}?$top=200&$skip={skip}&api-version=7.1"))
                .await?;
            let page = map_named(payload.get("value"));
            let count = page.len();
            items.extend(page);
            if count < 200 {
                break;
            }
            skip += count;
        }
        Ok(items)
    }

    pub async fn list_team_members(
        &self,
        org: &str,
        project: &str,
        team: &str,
    ) -> Result<Vec<IdentityValue>, ClientError> {
        let mut rows = Vec::new();
        let mut skip = 0usize;
        loop {
            let payload = self
                .get(&format!(
                    "{}/_apis/projects/{}/teams/{}/members?$top=1000&$skip={skip}&api-version=7.1",
                    self.org_url(org),
                    urlencoding::encode(project),
                    urlencoding::encode(team)
                ))
                .await?;
            let page: Vec<TeamMemberIdentityRow> =
                serde_json::from_value(payload.get("value").cloned().unwrap_or(json!([])))
                    .map_err(|err| ClientError::Message(err.to_string()))?;
            let count = page.len();
            rows.extend(page);
            if count < 1000 {
                break;
            }
            skip += count;
        }
        Ok(map_team_member_identities(&rows))
    }

    pub async fn list_iterations(
        &self,
        org: &str,
        project: &str,
        team: &str,
    ) -> Result<Vec<IterationNode>, ClientError> {
        let payload = self
            .get(&format!(
                "{}/{}/{}/_apis/work/teamsettings/iterations?api-version=7.1",
                self.org_url(org),
                urlencoding::encode(project),
                urlencoding::encode(team)
            ))
            .await?;
        Ok(payload
            .get("value")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|row| IterationNode {
                path: row
                    .get("path")
                    .and_then(Value::as_str)
                    .or_else(|| row.get("name").and_then(Value::as_str))
                    .unwrap_or("")
                    .to_string(),
                name: row
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                start_date: row
                    .pointer("/attributes/startDate")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                finish_date: row
                    .pointer("/attributes/finishDate")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                children: Vec::new(),
            })
            .collect())
    }

    pub async fn load_hierarchy(
        &self,
        scope: &ScopeSelection,
    ) -> Result<HierarchyResult, ClientError> {
        let team_root = format!(
            "{}/{}/{}",
            self.org_url(&scope.org),
            urlencoding::encode(&scope.project),
            urlencoding::encode(&scope.team)
        );
        let project_root = format!(
            "{}/{}",
            self.org_url(&scope.org),
            urlencoding::encode(&scope.project)
        );
        let field_values = self
            .get(&format!(
                "{team_root}/_apis/work/teamsettings/teamfieldvalues?api-version=7.1"
            ))
            .await?;
        let mut warning = None;
        let backlogs = match self
            .get(&format!("{team_root}/_apis/work/backlogs?api-version=7.1"))
            .await
        {
            Ok(value) => value,
            Err(err) if err.is_backlogs_failure() => {
                warning = Some(Self::team_settings_warning(&scope.team, &err));
                json!({ "value": [] })
            }
            Err(err) => return Err(err),
        };
        let levels: Vec<BacklogLevel> =
            serde_json::from_value(backlogs.get("value").cloned().unwrap_or(json!([])))
                .map_err(|err| ClientError::Message(err.to_string()))?;
        let mut types = Vec::new();
        let mut seen = HashSet::new();
        for level in &levels {
            for ty in level.work_item_types.iter().flatten() {
                if let Some(name) = &ty.name
                    && seen.insert(name.clone())
                {
                    types.push(name.clone());
                }
            }
        }
        let top_backlog_types = top_backlog_types_from_levels(&levels);
        if types.is_empty() && warning.is_none() {
            return Ok(HierarchyResult {
                nodes: Vec::new(),
                types,
                top_backlog_types,
                truncated: false,
                warning,
            });
        }
        let areas: Vec<AreaPathValue> = field_values
            .get("values")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|value| AreaPathValue {
                value: value
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                include_children: value
                    .get("includeChildren")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            })
            .collect();
        let query = if types.is_empty() {
            build_untyped_hierarchy_wiql(&scope.project, &areas)
        } else {
            build_hierarchy_wiql(&scope.project, &types, &areas)
        };
        let wiql = self
            .request(
                Method::POST,
                &format!("{project_root}/_apis/wit/wiql?$top={WIQL_RESULT_CAP}&api-version=7.1"),
                Some(&json!({ "query": query })),
            )
            .await?;
        let ids: Vec<i64> = wiql
            .get("workItems")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("id").and_then(Value::as_i64))
            .collect();
        let truncated = ids.len() >= WIQL_RESULT_CAP;
        let mut type_date_fields = HashSet::new();
        let types_for_fields = if types.is_empty() {
            Vec::new()
        } else {
            types.clone()
        };
        for ty in &types_for_fields {
            let fields = self
                .get(&format!(
                    "{project_root}/_apis/wit/workitemtypes/{}/fields?api-version=7.1",
                    urlencoding::encode(ty)
                ))
                .await?;
            let names: Vec<String> = fields
                .get("value")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|field| {
                    field
                        .get("referenceName")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .collect();
            if type_has_start_and_target(&names) {
                type_date_fields.insert(ty.clone());
            }
        }
        let mut nodes: Vec<WorkItemNode> = Vec::new();
        for chunk in chunk_ids(&ids, BATCH_SIZE) {
            let batch = self
                .request(
                    Method::POST,
                    &format!("{project_root}/_apis/wit/workitemsbatch?api-version=7.1"),
                    Some(&json!({
                        "ids": chunk,
                        "fields": HIERARCHY_FIELDS,
                        "errorPolicy": "Omit"
                    })),
                )
                .await?;
            for item in batch
                .get("value")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(id) = item.get("id").and_then(Value::as_i64) else {
                    continue;
                };
                let fields = item.get("fields").and_then(Value::as_object).cloned();
                nodes.push(map_batch_work_item(
                    &BatchWorkItem {
                        id,
                        rev: item.get("rev").and_then(Value::as_i64),
                        fields,
                        relations: item.get("relations").and_then(Value::as_array).cloned(),
                    },
                    &type_date_fields,
                ));
            }
        }
        for chunk in chunk_ids(&ids, BATCH_SIZE) {
            let ids_csv = chunk
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let expanded = self
                .get(&format!(
                    "{project_root}/_apis/wit/workitems?ids={ids_csv}&$expand=relations&errorPolicy=Omit&api-version=7.1"
                ))
                .await;
            let Ok(expanded) = expanded else {
                continue;
            };
            let mut by_id: HashMap<i64, Vec<Value>> = HashMap::new();
            for item in expanded
                .get("value")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(id) = item.get("id").and_then(Value::as_i64) else {
                    continue;
                };
                if let Some(relations) = item.get("relations").and_then(Value::as_array) {
                    by_id.insert(id, relations.clone());
                }
            }
            for node in &mut nodes {
                if let Some(relations) = by_id.get(&node.id) {
                    node.predecessors = ado_core::predecessor_ids(Some(relations.as_slice()));
                }
            }
        }
        if types.is_empty() {
            let mut discovered = Vec::new();
            let mut seen_types = HashSet::new();
            for node in &nodes {
                if seen_types.insert(node.work_item_type.clone()) {
                    discovered.push(node.work_item_type.clone());
                }
            }
            for ty in &discovered {
                let fields = self
                    .get(&format!(
                        "{project_root}/_apis/wit/workitemtypes/{}/fields?api-version=7.1",
                        urlencoding::encode(ty)
                    ))
                    .await?;
                let names: Vec<String> = fields
                    .get("value")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|field| {
                        field
                            .get("referenceName")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                    })
                    .collect();
                if type_has_start_and_target(&names) {
                    type_date_fields.insert(ty.clone());
                }
            }
            for node in &mut nodes {
                node.has_date_fields = type_date_fields.contains(&node.work_item_type);
            }
            types = discovered;
        }
        Ok(HierarchyResult {
            nodes: assemble_forest(&nodes),
            types,
            top_backlog_types,
            truncated,
            warning,
        })
    }

    fn team_settings_warning(team: &str, err: &ClientError) -> String {
        let raw = err.to_string();
        if raw.contains("TF400497") || raw.to_ascii_lowercase().contains("iteration") {
            format!(
                "Team \"{team}\" has invalid backlog settings (the backlog iteration path is not in this team's area). Fix Team Settings in Azure DevOps, or pick another team."
            )
        } else {
            format!(
                "Team \"{team}\" backlog settings could not be loaded. Showing work items from the team's area paths."
            )
        }
    }

    pub async fn patch_work_item(
        &self,
        org: &str,
        project: &str,
        id: i64,
        document: &[JsonPatchOp],
    ) -> Result<i64, ClientError> {
        let updated = self
            .request(
                Method::PATCH,
                &format!(
                    "{}/{}/_apis/wit/workitems/{id}?api-version=7.1",
                    self.org_url(org),
                    urlencoding::encode(project)
                ),
                Some(&serde_json::to_value(document).unwrap_or(Value::Null)),
            )
            .await?;
        Ok(updated.get("rev").and_then(Value::as_i64).unwrap_or(0))
    }

    pub async fn add_dependency(
        &self,
        org: &str,
        project: &str,
        successor_id: i64,
        predecessor_id: i64,
        rev: i64,
    ) -> Result<i64, ClientError> {
        let url = format!(
            "{}/{}/_apis/wit/workitems/{predecessor_id}",
            self.org_url(org),
            urlencoding::encode(project)
        );
        self.patch_work_item(
            org,
            project,
            successor_id,
            &ado_core::build_add_dependency_patch(rev, &url),
        )
        .await
    }

    pub async fn remove_dependency(
        &self,
        org: &str,
        project: &str,
        successor_id: i64,
        predecessor_id: i64,
        rev: i64,
    ) -> Result<i64, ClientError> {
        let item = self
            .get(&format!(
                "{}/{}/_apis/wit/workitems/{successor_id}?$expand=relations&api-version=7.1",
                self.org_url(org),
                urlencoding::encode(project)
            ))
            .await?;
        let relations = item.get("relations").and_then(Value::as_array);
        let Some(index) =
            ado_core::dependency_relation_index(relations.map(|v| v.as_slice()), predecessor_id)
        else {
            return Err(ClientError::Message("dependency not found".into()));
        };
        self.patch_work_item(
            org,
            project,
            successor_id,
            &ado_core::build_remove_dependency_patch(rev, index),
        )
        .await
    }

    pub async fn create_work_item(
        &self,
        org: &str,
        project: &str,
        work_item_type: &str,
        document: &[JsonPatchOp],
    ) -> Result<WorkItemNode, ClientError> {
        let encoded_type = urlencoding::encode(work_item_type);
        let created = self
            .request(
                Method::POST,
                &format!(
                    "{}/{}/_apis/wit/workitems/${encoded_type}?api-version=7.1",
                    self.org_url(org),
                    urlencoding::encode(project)
                ),
                Some(&serde_json::to_value(document).unwrap_or(Value::Null)),
            )
            .await?;
        let item = BatchWorkItem {
            id: created.get("id").and_then(Value::as_i64).unwrap_or(0),
            rev: created.get("rev").and_then(Value::as_i64),
            fields: created.get("fields").and_then(Value::as_object).cloned(),
            relations: created.get("relations").and_then(Value::as_array).cloned(),
        };
        let mut types_with_dates = HashSet::new();
        types_with_dates.insert(work_item_type.to_string());
        Ok(map_batch_work_item(&item, &types_with_dates))
    }

    pub async fn load_form(
        &self,
        org: &str,
        project: &str,
        id: i64,
    ) -> Result<WorkItemFormModel, ClientError> {
        let project_root = format!("{}/{}", self.org_url(org), urlencoding::encode(project));
        let item = self
            .get(&format!(
                "{project_root}/_apis/wit/workitems/{id}?api-version=7.1"
            ))
            .await?;
        let type_name = item
            .pointer("/fields/System.WorkItemType")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let layout = match self.process_layout(org, project, &type_name).await {
            Ok(layout) => layout,
            Err(err) => {
                tracing::warn!("process layout unavailable: {err}");
                ProcessLayout::default()
            }
        };
        let fields = self
            .get(&format!(
                "{project_root}/_apis/wit/workitemtypes/{}/fields?$expand=All&api-version=7.1",
                urlencoding::encode(&type_name)
            ))
            .await?;
        let meta: Vec<FieldMetadata> =
            serde_json::from_value(fields.get("value").cloned().unwrap_or(json!([])))
                .map_err(|err| ClientError::Message(err.to_string()))?;
        let (system_controls, pages) = flatten_layout(&layout, &meta);
        let values = item
            .get("fields")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        Ok(WorkItemFormModel {
            id,
            rev: item.get("rev").and_then(Value::as_i64).unwrap_or(1),
            work_item_type: type_name,
            system_controls,
            pages,
            values,
        })
    }

    async fn process_layout(
        &self,
        org: &str,
        project: &str,
        type_name: &str,
    ) -> Result<ProcessLayout, ClientError> {
        let project_info = self
            .get(&format!(
                "{}/_apis/projects/{}?includeCapabilities=true&api-version=7.1",
                self.org_url(org),
                urlencoding::encode(project)
            ))
            .await?;
        let process_id = project_info
            .pointer("/capabilities/processTemplate/templateTypeId")
            .and_then(Value::as_str)
            .ok_or_else(|| ClientError::Message("project process id missing".into()))?;
        let types = self
            .get(&format!(
                "{}/_apis/work/processes/{process_id}/workItemTypes?api-version=7.1",
                self.org_url(org)
            ))
            .await?;
        let wit_ref = types
            .get("value")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|row| row.get("name").and_then(Value::as_str) == Some(type_name))
            .and_then(|row| row.get("referenceName").and_then(Value::as_str))
            .unwrap_or(type_name);
        let layout = self
            .get(&format!(
                "{}/_apis/work/processes/{process_id}/workItemTypes/{}/layout?api-version=7.1",
                self.org_url(org),
                urlencoding::encode(wit_ref)
            ))
            .await?;
        serde_json::from_value(layout).map_err(|err| ClientError::Message(err.to_string()))
    }

    pub async fn search_identities(
        &self,
        org: &str,
        query: &str,
    ) -> Result<Vec<IdentityValue>, ClientError> {
        let qs = format!(
            "searchFilter=General&filterValue={}&api-version=7.1",
            urlencoding::encode(query)
        );
        let vssps_url = format!(
            "{}/_apis/identities?{qs}",
            identities_search_host(&self.ado_base, org)
        );
        let payload = match self.get(&vssps_url).await {
            Ok(payload) => payload,
            Err(err) => {
                tracing::debug!("identity search on VSSPS failed, falling back to org host: {err}");
                self.get(&format!("{}/_apis/identities?{qs}", self.org_url(org)))
                    .await?
            }
        };
        Ok(parse_identities(&payload))
    }
}

fn parse_identities(payload: &Value) -> Vec<IdentityValue> {
    payload
        .get("value")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let display = row.get("displayName").and_then(Value::as_str);
            let unique = row.get("uniqueName").and_then(Value::as_str);
            if display.is_none() && unique.is_none() {
                return None;
            }
            Some(IdentityValue {
                display_name: display.or(unique).unwrap_or("").to_string(),
                unique_name: unique.or(display).unwrap_or("").to_string(),
                id: row.get("id").and_then(Value::as_str).map(ToOwned::to_owned),
            })
        })
        .collect()
}

fn map_named(value: Option<&Value>) -> Vec<Project> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| Project {
            id: row
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| row.get("name").and_then(Value::as_str))
                .unwrap_or("")
                .to_string(),
            name: row
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        })
        .collect()
}
