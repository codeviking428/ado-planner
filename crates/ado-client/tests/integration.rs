use std::sync::Arc;

use ado_client::{AdoClient, AuthScheme, StaticTokens};
use ado_core::{ScopeSelection, build_start_target_patch};

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
