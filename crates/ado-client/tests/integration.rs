use std::sync::Arc;

use ado_client::{AdoClient, AuthScheme, StaticTokens};
use ado_core::{
    JsonPatchOp, PatchOp, ScopeSelection, build_replace_dates_patch, build_start_target_patch,
};
use serde_json::json;

fn tokens(scheme: AuthScheme, org: Option<&str>) -> Arc<StaticTokens> {
    Arc::new(StaticTokens {
        token: "pat-secret-value".into(),
        organization: org.map(ToOwned::to_owned),
        scheme,
    })
}

#[tokio::test]
async fn every_call_against_ado_mock() {
    let mock = ado_mock::start().await;
    let ado = AdoClient::with_base_urls(
        tokens(AuthScheme::Pat, Some("contoso")),
        &mock.url,
        &mock.url,
    );

    let orgs = ado.list_organizations().await.expect("orgs");
    assert_eq!(orgs[0].account_name, "contoso");

    let entra = AdoClient::with_base_urls(tokens(AuthScheme::Bearer, None), &mock.url, &mock.url);
    let entra_orgs = entra.list_organizations().await.expect("entra orgs");
    assert!(entra_orgs.iter().any(|org| org.account_name == "contoso"));

    let projects = ado.list_projects("contoso").await.expect("projects");
    assert_eq!(projects[0].name, "Shop");

    let teams = ado.list_teams("contoso", "Shop").await.expect("teams");
    assert_eq!(teams[0].name, "Platform");

    let members = ado
        .list_team_members("contoso", "Shop", "Platform")
        .await
        .expect("members");
    assert!(
        members
            .iter()
            .any(|row| row.unique_name == "ada@contoso.com")
    );

    let iterations = ado
        .list_iterations("contoso", "Shop", "Platform")
        .await
        .expect("iterations");
    assert!(iterations.iter().any(|row| row.name == "Sprint 10"));

    let hierarchy = ado
        .load_hierarchy(&ScopeSelection {
            org: "contoso".into(),
            project: "Shop".into(),
            team: "Platform".into(),
            iteration_path: None,
        })
        .await
        .expect("hierarchy");
    assert!(!hierarchy.truncated);
    assert!(hierarchy.nodes.iter().any(|node| node.id == 1003));
    assert!(hierarchy.types.contains(&"Epic".to_string()));
    let spike = hierarchy
        .nodes
        .iter()
        .find(|node| node.id == 1012)
        .expect("unparented spike");
    assert_eq!(
        spike.predecessors,
        vec![1003],
        "hierarchy fetch should load System.LinkTypes.Dependency predecessors"
    );

    let identities = ado
        .search_identities("contoso", "Ada")
        .await
        .expect("identities");
    assert_eq!(identities[0].display_name, "Ada Lovelace");

    let form = ado.load_form("contoso", "Shop", 1003).await.expect("form");
    assert_eq!(form.work_item_type, "User Story");
    assert!(!form.system_controls.is_empty());

    let start: jiff::civil::Date = "2026-07-07".parse().unwrap();
    let target: jiff::civil::Date = "2026-07-21".parse().unwrap();
    let rev = ado
        .patch_work_item(
            "contoso",
            "Shop",
            1003,
            &build_start_target_patch(form.rev, start, target),
        )
        .await
        .expect("patch");
    assert_eq!(rev, form.rev + 1);
    assert_eq!(
        mock.last_patch_content_type().as_deref(),
        Some("application/json-patch+json")
    );
}

#[tokio::test]
async fn patch_rev_conflict() {
    let mock = ado_mock::start().await;
    let ado = AdoClient::with_base_urls(
        tokens(AuthScheme::Pat, Some("contoso")),
        &mock.url,
        &mock.url,
    );
    let err = ado
        .patch_work_item(
            "contoso",
            "Shop",
            1003,
            &build_start_target_patch(
                1,
                "2026-07-07".parse().unwrap(),
                "2026-07-21".parse().unwrap(),
            ),
        )
        .await
        .expect_err("conflict");
    assert!(err.is_precondition_failed());
}

#[tokio::test]
async fn patch_with_rev_retry_recovers_from_stale_rev() {
    let mock = ado_mock::start().await;
    let ado = AdoClient::with_base_urls(
        tokens(AuthScheme::Pat, Some("contoso")),
        &mock.url,
        &mock.url,
    );
    let start = "2026-07-07".parse().unwrap();
    let target = "2026-07-21".parse().unwrap();
    let rev = ado
        .patch_work_item_with_rev_retry("contoso", "Shop", 1003, 0, |rev| {
            build_replace_dates_patch(rev, Some(start), Some(target))
        })
        .await
        .expect("retry");
    assert!(rev > 0);
}

#[tokio::test]
async fn remove_dependency_uses_the_server_rev() {
    let mock = ado_mock::start().await;
    let ado = AdoClient::with_base_urls(
        tokens(AuthScheme::Pat, Some("contoso")),
        &mock.url,
        &mock.url,
    );
    ado.add_dependency("contoso", "Shop", 1005, 1003, 1)
        .await
        .expect("link");
    let start = "2026-07-22".parse().unwrap();
    ado.patch_work_item_with_rev_retry("contoso", "Shop", 1005, 1, |rev| {
        build_replace_dates_patch(rev, Some(start), None)
    })
    .await
    .expect("bump rev");
    ado.remove_dependency("contoso", "Shop", 1005, 1003, 1)
        .await
        .expect("stale caller rev must not 412");
}

#[tokio::test]
async fn backoff_retries_server_errors_and_429_then_succeeds() {
    let mock = ado_mock::start().await;
    let ado = pat_client(&mock);
    let start = "2026-07-08".parse().unwrap();
    mock.fail_next("/workitems/1003", 503, "busy");
    mock.fail_next("/workitems/1003", 429, "slow down");
    let rev = ado
        .patch_work_item_with_backoff("contoso", "Shop", 1003, 5, |rev| {
            ado_core::build_dates_patch(rev, Some(start), None)
        })
        .await
        .expect("third attempt lands");
    assert_eq!(rev, 6);
    assert_eq!(
        mock.patch_count(),
        1,
        "the failed attempts never reached the store"
    );
}

#[tokio::test]
async fn backoff_waits_for_retry_after() {
    let mock = ado_mock::start().await;
    let ado = pat_client(&mock);
    let start = "2026-07-08".parse().unwrap();
    mock.fail_next_retry_after("/workitems/1003", 429, "slow down", 1);
    let began = std::time::Instant::now();
    ado.patch_work_item_with_backoff("contoso", "Shop", 1003, 5, |rev| {
        ado_core::build_dates_patch(rev, Some(start), None)
    })
    .await
    .expect("retry after the wait");
    assert!(
        began.elapsed() >= std::time::Duration::from_secs(1),
        "Retry-After: 1 must hold the retry for a second, waited {:?}",
        began.elapsed()
    );
}

#[tokio::test]
async fn backoff_gives_up_on_client_errors_and_after_four_attempts() {
    let mock = ado_mock::start().await;
    let ado = pat_client(&mock);
    let start = "2026-07-08".parse().unwrap();
    mock.fail_next("/workitems/1003", 400, "TF401320: Rule error");
    let err = ado
        .patch_work_item_with_backoff("contoso", "Shop", 1003, 5, |rev| {
            ado_core::build_dates_patch(rev, Some(start), None)
        })
        .await
        .expect_err("400 is permanent");
    assert_eq!(err.reason(), "TF401320: Rule error");
    assert_eq!(mock.request_count("/workitems/1003"), 1, "no retry on 400");
    for _ in 0..4 {
        mock.fail_next("/workitems/1003", 500, "down");
    }
    let err = ado
        .patch_work_item_with_backoff("contoso", "Shop", 1003, 5, |rev| {
            ado_core::build_dates_patch(rev, Some(start), None)
        })
        .await
        .expect_err("still down after four attempts");
    assert_eq!(err.reason(), "down");
    assert_eq!(mock.request_count("/workitems/1003"), 5);
}

fn pat_client(mock: &ado_mock::AdoMock) -> AdoClient {
    AdoClient::with_base_urls(
        tokens(AuthScheme::Pat, Some("contoso")),
        &mock.url,
        &mock.url,
    )
}

fn add_field(field: &str, value: serde_json::Value) -> JsonPatchOp {
    JsonPatchOp {
        op: PatchOp::Add,
        path: format!("/fields/{field}"),
        value: Some(value),
    }
}

#[tokio::test]
async fn ping_org_and_get_work_item_rev() {
    let mock = ado_mock::start().await;
    let ado = pat_client(&mock);
    ado.ping_org("contoso").await.expect("ping");
    assert_eq!(mock.request_count("/contoso/_apis/projects?$top=1"), 1);

    let rev = ado
        .get_work_item_rev("contoso", "Shop", 1003)
        .await
        .expect("rev");
    assert_eq!(rev, 5);

    let err = ado
        .get_work_item_rev("contoso", "Shop", 9999)
        .await
        .expect_err("missing item");
    assert!(err.to_string().contains("404"), "{err}");
}

#[tokio::test]
async fn create_work_item_with_parent() {
    let mock = ado_mock::start().await;
    let ado = pat_client(&mock);
    let parent_url = ado.work_item_api_url("contoso", "Shop", 1002);
    let node = ado
        .create_work_item(
            "contoso",
            "Shop",
            "User Story",
            &[
                add_field("System.Title", json!("Checkout redesign")),
                add_field("System.AreaPath", json!("Shop\\Platform")),
                add_field("System.IterationPath", json!("Shop\\Sprint 10")),
                JsonPatchOp {
                    op: PatchOp::Add,
                    path: "/relations/-".into(),
                    value: Some(json!({
                        "rel": "System.LinkTypes.Hierarchy-Reverse",
                        "url": parent_url,
                    })),
                },
            ],
        )
        .await
        .expect("create");

    assert!(node.id > 2003, "id after the fixtures, got {}", node.id);
    assert_eq!(node.rev, 1);
    assert_eq!(node.title, "Checkout redesign");
    assert_eq!(node.work_item_type, "User Story");
    assert_eq!(node.area_path, "Shop\\Platform");
    assert_eq!(node.iteration_path, "Shop\\Sprint 10");
    assert_eq!(node.parent_id, Some(1002));
    assert_eq!(
        mock.mutating_writes(),
        vec![(
            "POST".to_string(),
            "/contoso/Shop/_apis/wit/workitems/$User%20Story".to_string()
        )]
    );

    let stored = mock.work_item(node.id).await.expect("stored");
    assert_eq!(stored["fields"]["System.Title"], "Checkout redesign");
    assert_eq!(stored["fields"]["System.Parent"], 1002);
}

#[tokio::test]
async fn create_work_item_injected_failure() {
    let mock = ado_mock::start().await;
    mock.fail_next(
        "/workitems/$Task",
        400,
        "TF401320: Rule error for field Title",
    );
    let ado = pat_client(&mock);
    let err = ado
        .create_work_item(
            "contoso",
            "Shop",
            "Task",
            &[add_field("System.Title", json!(""))],
        )
        .await
        .expect_err("injected");
    let text = err.to_string();
    assert!(text.contains("400"), "{text}");
    assert!(text.contains("TF401320"), "{text}");
    assert!(!err.is_precondition_failed());
}

#[tokio::test]
async fn delete_work_item_removes_it() {
    let mock = ado_mock::start().await;
    let ado = pat_client(&mock);
    ado.delete_work_item("contoso", "Shop", 1014)
        .await
        .expect("delete");
    assert!(mock.work_item(1014).await.is_none());
    assert_eq!(
        mock.mutating_writes(),
        vec![(
            "DELETE".to_string(),
            "/contoso/Shop/_apis/wit/workitems/1014".to_string()
        )]
    );

    let err = ado
        .delete_work_item("contoso", "Shop", 1014)
        .await
        .expect_err("already deleted");
    assert!(err.to_string().contains("404"), "{err}");
}

#[tokio::test]
async fn delete_work_item_injected_failure() {
    let mock = ado_mock::start().await;
    mock.fail_next(
        "/workitems/1014",
        403,
        "TF401027: You need Delete work items",
    );
    let ado = pat_client(&mock);
    let err = ado
        .delete_work_item("contoso", "Shop", 1014)
        .await
        .expect_err("injected");
    let text = err.to_string();
    assert!(text.contains("403"), "{text}");
    assert!(text.contains("TF401027"), "{text}");
    assert!(
        mock.work_item(1014).await.is_some(),
        "a failed delete must leave the item in place"
    );
}

#[tokio::test]
async fn injected_failure() {
    let mock = ado_mock::start().await;
    mock.fail_next("/fabrikam/_apis/projects", 503, "Projects unavailable");
    let ado = AdoClient::with_base_urls(
        tokens(AuthScheme::Pat, Some("contoso")),
        &mock.url,
        &mock.url,
    );
    let err = ado.list_projects("fabrikam").await.expect_err("injected");
    let text = err.to_string();
    assert!(text.contains("503"));
    assert!(text.contains("Projects unavailable"));
}

#[tokio::test]
async fn invalid_backlog_settings_still_load_area_work_items() {
    let mock = ado_mock::start().await;
    mock.fail_next(
        "/backlogs",
        500,
        "TF400497: The backlog iteration path is not valid.",
    );
    let ado = AdoClient::with_base_urls(
        tokens(AuthScheme::Pat, Some("contoso")),
        &mock.url,
        &mock.url,
    );
    let hierarchy = ado
        .load_hierarchy(&ScopeSelection {
            org: "contoso".into(),
            project: "Shop".into(),
            team: "Platform".into(),
            iteration_path: None,
        })
        .await
        .expect("hierarchy despite TF400497");
    assert!(
        hierarchy
            .warning
            .as_deref()
            .is_some_and(|text| text.contains("TF400497") || text.contains("backlog")),
        "warning: {:?}",
        hierarchy.warning
    );
    assert!(hierarchy.nodes.iter().any(|node| node.id == 1003));
}
