use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use serde_json::{Value, json};
use std::sync::Mutex as StdMutex;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::fixtures::Fixtures;
use ado_core::{START_DATE_FIELD, TARGET_DATE_FIELD};

struct Pending {
    path: String,
    delay_ms: u64,
    failure: Option<(u16, String)>,
}

struct AppState {
    items: Mutex<Vec<Value>>,
    pending: StdMutex<Vec<Pending>>,
    fixtures: Fixtures,
    patches: StdMutex<Vec<String>>,
}

pub struct AdoMock {
    pub url: String,
    state: Arc<AppState>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl AdoMock {
    pub fn delay_next(&self, path: &str, delay_ms: u64) {
        self.state.pending.lock().expect("pending").push(Pending {
            path: path.to_string(),
            delay_ms,
            failure: None,
        });
    }

    pub fn fail_next(&self, path: &str, status: u16, message: &str) {
        self.state.pending.lock().expect("pending").push(Pending {
            path: path.to_string(),
            delay_ms: 0,
            failure: Some((status, message.to_string())),
        });
    }

    pub fn patch_count(&self) -> usize {
        self.state.patches.lock().expect("patches").len()
    }

    pub async fn close(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for AdoMock {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

pub async fn start() -> AdoMock {
    let fixtures = Fixtures::load();
    let items = fixtures.work_items.as_array().cloned().unwrap_or_default();
    let state = Arc::new(AppState {
        items: Mutex::new(items),
        pending: StdMutex::new(Vec::new()),
        fixtures,
        patches: StdMutex::new(Vec::new()),
    });
    let app = Router::new()
        .fallback(any(handle))
        .with_state(state.clone());
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ado-mock");
    let addr = listener.local_addr().expect("local addr");
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("ado-mock server");
    });
    AdoMock {
        url: format!("http://{addr}"),
        state,
        shutdown: Some(shutdown_tx),
    }
}

async fn handle(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    if let Some(pending) = take_pending(&state, &path).await {
        if pending.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(pending.delay_ms)).await;
        }
        if let Some((status, message)) = pending.failure {
            return json_response(
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                json!({ "message": message }),
            );
        }
    }

    match route(&state, &method, &path, req).await {
        Ok(response) => response,
        Err(err) => json_response(StatusCode::INTERNAL_SERVER_ERROR, json!({ "message": err })),
    }
}

async fn take_pending(state: &AppState, path: &str) -> Option<Pending> {
    let mut pending = state.pending.lock().expect("pending");
    let index = pending.iter().position(|row| path.contains(&row.path))?;
    Some(pending.remove(index))
}

async fn route(
    state: &AppState,
    method: &Method,
    path: &str,
    req: Request<Body>,
) -> Result<Response, String> {
    let fixtures = &state.fixtures;
    if path.ends_with("/_apis/connectionData") {
        return Ok(json_response(
            StatusCode::OK,
            fixtures.connection_data.clone(),
        ));
    }
    if path.ends_with("/_apis/profile/profiles/me") {
        return Ok(json_response(StatusCode::OK, fixtures.profile.clone()));
    }
    if path.ends_with("/_apis/accounts") {
        return Ok(json_response(StatusCode::OK, fixtures.accounts.clone()));
    }
    if path.ends_with("/_apis/projects") && *method == Method::GET {
        let body = if path.starts_with("/fabrikam/") {
            fixtures.projects.get("fabrikam").cloned()
        } else {
            fixtures.projects.get("contoso").cloned()
        }
        .unwrap_or(json!({ "value": [] }));
        return Ok(json_response(StatusCode::OK, body));
    }
    if *method == Method::GET && is_project_resource(path) {
        return Ok(json_response(
            StatusCode::OK,
            fixtures
                .process
                .get("project")
                .cloned()
                .unwrap_or(json!({})),
        ));
    }
    if path.contains("/teams/") && path.ends_with("/members") {
        let body = if path.contains("/Delivery/") || path.contains("/Roadmap/") {
            fixtures.members.get("Delivery").cloned()
        } else {
            fixtures.members.get("Platform").cloned()
        }
        .unwrap_or(json!({ "value": [] }));
        return Ok(json_response(StatusCode::OK, body));
    }
    if path.contains("/_apis/projects/") && path.ends_with("/teams") {
        let body = if path.contains("/Roadmap/") {
            fixtures.teams.get("Roadmap").cloned()
        } else {
            fixtures.teams.get("Shop").cloned()
        }
        .unwrap_or(json!({ "value": [] }));
        return Ok(json_response(StatusCode::OK, body));
    }
    if path.contains("/teamfieldvalues") {
        return Ok(json_response(
            StatusCode::OK,
            fixtures.team_field_values.clone(),
        ));
    }
    if path.ends_with("/backlogs") {
        return Ok(json_response(StatusCode::OK, fixtures.backlogs.clone()));
    }
    if path.contains("/teamsettings/iterations") {
        return Ok(json_response(StatusCode::OK, fixtures.iterations.clone()));
    }
    if path.contains("/work/processes/") && path.ends_with("/workItemTypes") {
        return Ok(json_response(
            StatusCode::OK,
            fixtures
                .process
                .get("workItemTypes")
                .cloned()
                .unwrap_or(json!({ "value": [] })),
        ));
    }
    if path.contains("/workitemtypes/") && path.contains("/fields") {
        let ty = work_item_type_from_path(path);
        let with_dates = fixtures
            .type_fields
            .get("withDates")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let types_with_dates = fixtures
            .type_fields
            .get("typesWithDates")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let fields = if types_with_dates
            .iter()
            .any(|name| name.as_str() == Some(&ty))
        {
            with_dates
        } else {
            with_dates
                .into_iter()
                .filter(|field| {
                    let name = field
                        .get("referenceName")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    name != START_DATE_FIELD && name != TARGET_DATE_FIELD
                })
                .collect()
        };
        return Ok(json_response(StatusCode::OK, json!({ "value": fields })));
    }
    if path.contains("/layout") {
        return Ok(json_response(StatusCode::OK, fixtures.layout.clone()));
    }
    if path.contains("/wiql") && *method == Method::POST {
        let items = state.items.lock().await;
        let work_items: Vec<Value> = items
            .iter()
            .filter_map(|item| item.get("id").cloned())
            .map(|id| json!({ "id": id }))
            .collect();
        return Ok(json_response(
            StatusCode::OK,
            json!({ "workItems": work_items }),
        ));
    }
    if path.contains("/workitemsbatch") && *method == Method::POST {
        let body = read_json(req).await?;
        let ids = body
            .get("ids")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let items = state.items.lock().await;
        let selected: Vec<Value> = items
            .iter()
            .filter(|item| ids.contains(item.get("id").unwrap_or(&Value::Null)))
            .cloned()
            .collect();
        return Ok(json_response(StatusCode::OK, json!({ "value": selected })));
    }
    if let Some(id) = work_item_id(path) {
        let mut items = state.items.lock().await;
        let Some(index) = items
            .iter()
            .position(|item| item.get("id") == Some(&json!(id)))
        else {
            return Ok(json_response(
                StatusCode::NOT_FOUND,
                json!({ "message": "not found" }),
            ));
        };
        if *method == Method::PATCH {
            state
                .patches
                .lock()
                .expect("patches")
                .push(path.to_string());
            let document = read_json(req).await?;
            let ops = document.as_array().cloned().unwrap_or_default();
            let current_rev = items[index].get("rev").and_then(Value::as_i64).unwrap_or(0);
            let rev_ok = ops
                .iter()
                .find(|op| {
                    op.get("op").and_then(Value::as_str) == Some("test")
                        && op.get("path").and_then(Value::as_str) == Some("/rev")
                })
                .is_none_or(|rev_test| {
                    rev_test.get("value").and_then(Value::as_i64) == Some(current_rev)
                });
            if !rev_ok {
                return Ok(json_response(
                    StatusCode::PRECONDITION_FAILED,
                    json!({ "message": "rev mismatch" }),
                ));
            }
            for op in ops {
                let is_add = op.get("op").and_then(Value::as_str) == Some("add");
                let field = op
                    .get("path")
                    .and_then(Value::as_str)
                    .and_then(|path| path.strip_prefix("/fields/"));
                if is_add
                    && let Some(field) = field
                    && let Some(fields) = items[index]
                        .get_mut("fields")
                        .and_then(Value::as_object_mut)
                {
                    fields.insert(
                        field.to_string(),
                        op.get("value").cloned().unwrap_or(Value::Null),
                    );
                }
            }
            if let Some(rev) = items[index].get_mut("rev") {
                *rev = json!(current_rev + 1);
            }
        }
        return Ok(json_response(StatusCode::OK, items[index].clone()));
    }
    if path.contains("/identities") {
        return Ok(json_response(StatusCode::OK, fixtures.identities.clone()));
    }
    Ok(json_response(
        StatusCode::NOT_FOUND,
        json!({ "message": format!("unmocked {method} {path}") }),
    ))
}

fn is_project_resource(path: &str) -> bool {
    let Some(idx) = path.find("/_apis/projects/") else {
        return false;
    };
    let rest = &path[idx + "/_apis/projects/".len()..];
    !rest.is_empty() && !rest.contains('/')
}

fn work_item_type_from_path(path: &str) -> String {
    path.split("/workitemtypes/")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .map(percent_decode)
        .unwrap_or_default()
}

fn percent_decode(raw: &str) -> String {
    let mut out = String::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(ch) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(ch as char);
            i += 3;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn work_item_id(path: &str) -> Option<i64> {
    let rest = path.split("/workitems/").nth(1)?;
    let id = rest.split(|ch: char| !ch.is_ascii_digit()).next()?;
    if id.is_empty() { None } else { id.parse().ok() }
}

async fn read_json(req: Request<Body>) -> Result<Value, String> {
    let bytes = axum::body::to_bytes(req.into_body(), 1024 * 1024)
        .await
        .map_err(|err| err.to_string())?;
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&bytes).map_err(|err| err.to_string())
}

fn json_response(status: StatusCode, body: Value) -> Response {
    (
        status,
        [("content-type", "application/json")],
        body.to_string(),
    )
        .into_response()
}
