use ado_core::{START_DATE_FIELD, TARGET_DATE_FIELD};
use serde_json::{Value, json};

fn fixtures(name: &str) -> Value {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../e2e/fixtures");
    let text = std::fs::read_to_string(dir.join(name)).expect("fixture");
    serde_json::from_str(&text).expect("json")
}

async fn get(client: &reqwest::Client, url: &str, path: &str) -> (u16, Value) {
    let response = client
        .get(format!("{url}{path}"))
        .send()
        .await
        .expect("get");
    let status = response.status().as_u16();
    let body = response.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn post(client: &reqwest::Client, url: &str, path: &str, body: Value) -> (u16, Value) {
    let response = client
        .post(format!("{url}{path}"))
        .json(&body)
        .send()
        .await
        .expect("post");
    let status = response.status().as_u16();
    let body = response.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn patch(client: &reqwest::Client, url: &str, path: &str, body: Value) -> (u16, Value) {
    let response = client
        .patch(format!("{url}{path}"))
        .header("content-type", "application/json-patch+json")
        .body(body.to_string())
        .send()
        .await
        .expect("patch");
    let status = response.status().as_u16();
    let body = response.json().await.unwrap_or(Value::Null);
    (status, body)
}

#[tokio::test]
async fn snapshots_each_route() {
    let mock = ado_mock::start().await;
    let client = reqwest::Client::new();
    let url = &mock.url;

    let (status, body) = get(&client, url, "/contoso/_apis/connectionData").await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("connection-data.json"));

    let (status, body) = get(&client, url, "/_apis/profile/profiles/me").await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("profile.json"));

    let (status, body) = get(&client, url, "/_apis/accounts").await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("accounts.json"));

    let (status, body) = get(&client, url, "/contoso/_apis/projects").await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("projects.json")["contoso"]);

    let (status, body) = get(&client, url, "/fabrikam/_apis/projects").await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("projects.json")["fabrikam"]);

    let (status, body) = get(&client, url, "/contoso/_apis/projects/Shop").await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("process.json")["project"]);

    let (status, body) = get(&client, url, "/contoso/_apis/projects/Shop/teams").await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("teams.json")["Shop"]);

    let (status, body) = get(&client, url, "/contoso/_apis/projects/Roadmap/teams").await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("teams.json")["Roadmap"]);

    let (status, body) = get(
        &client,
        url,
        "/contoso/_apis/projects/Shop/teams/Platform/members",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("members.json")["Platform"]);

    let (status, body) = get(
        &client,
        url,
        "/contoso/_apis/projects/Roadmap/teams/Delivery/members",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("members.json")["Delivery"]);

    let (status, body) = get(
        &client,
        url,
        "/contoso/Shop/Platform/_apis/work/teamsettings/teamfieldvalues",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("team-field-values.json")["Platform"]);

    let (status, body) = get(&client, url, "/contoso/Shop/Platform/_apis/work/backlogs").await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("backlogs.json"));

    let (status, body) = get(
        &client,
        url,
        "/contoso/Shop/Platform/_apis/work/teamsettings/iterations",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("iterations.json"));

    let (status, body) = get(
        &client,
        url,
        "/contoso/_apis/work/processes/adcc42ab-9882-485e-a3ed-7678f126f1c0/workItemTypes",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("process.json")["workItemTypes"]);

    let (status, body) = get(
        &client,
        url,
        "/contoso/Shop/_apis/wit/workitemtypes/Epic/fields",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["value"], fixtures("type-fields.json")["withDates"]);

    let (status, body) = get(
        &client,
        url,
        "/contoso/_apis/work/processes/agile/workItemTypes/User%20Story/layout",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("layout.json"));

    let (status, body) = post(
        &client,
        url,
        "/contoso/Shop/_apis/wit/wiql",
        json!({ "query": "SELECT [System.Id] FROM WorkItems" }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["workItems"],
        json!([
            { "id": 1001 },
            { "id": 1002 },
            { "id": 1003 },
            { "id": 1005 },
            { "id": 1012 },
            { "id": 1013 },
            { "id": 1014 },
            { "id": 1020 }
        ])
    );

    let (status, body) = get(&client, url, "/contoso/_apis/projects/Shop/teams").await;
    assert_eq!(status, 200);
    let names: Vec<_> = body["value"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|team| team.get("name").and_then(Value::as_str))
        .collect();
    assert!(names.contains(&"Platform"));
    assert!(names.contains(&"Growth"));
    assert!(names.contains(&"Empty"));

    let (status, body) = get(
        &client,
        url,
        "/contoso/Shop/Empty/_apis/work/teamsettings/teamfieldvalues",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("team-field-values.json")["Empty"]);

    let (status, body) = post(
        &client,
        url,
        "/contoso/Shop/_apis/wit/wiql",
        json!({
            "query": "SELECT [System.Id] FROM WorkItems WHERE [System.AreaPath] UNDER 'Shop\\Empty'"
        }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["workItems"], json!([]));

    let (status, body) = get(
        &client,
        url,
        "/contoso/_apis/identities?searchFilter=General&filterValue=a",
    )
    .await;
    assert_eq!(status, 200);
    assert!(
        body["value"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item.get("displayName").and_then(Value::as_str))
            .count()
            >= 4
    );

    let (status, body) = post(
        &client,
        url,
        "/contoso/Shop/_apis/wit/workitemsbatch",
        json!({ "ids": [1003] }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["value"][0]["id"], 1003);

    let (status, body) = get(&client, url, "/contoso/Shop/_apis/wit/workitems/1003").await;
    assert_eq!(status, 200);
    assert_eq!(body["fields"]["System.Title"], "Persist cart");

    let (status, body) = get(
        &client,
        url,
        "/contoso/Shop/_apis/wit/workitems?ids=1012&$expand=relations&errorPolicy=Omit",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["value"][0]["id"], 1012);
    assert_eq!(
        body["value"][0]["relations"][0]["rel"],
        "System.LinkTypes.Dependency-Reverse"
    );

    let (status, body) = get(
        &client,
        url,
        "/contoso/_apis/identities?searchFilter=General",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body, fixtures("identities.json"));

    mock.close().await;
}

#[tokio::test]
async fn patch_rev_conflict_and_success() {
    let mock = ado_mock::start().await;
    let client = reqwest::Client::new();
    let url = &mock.url;

    let (status, _) = patch(
        &client,
        url,
        "/contoso/Shop/_apis/wit/workitems/1003",
        json!([
            { "op": "test", "path": "/rev", "value": 1 },
            { "op": "add", "path": "/fields/System.Title", "value": "nope" }
        ]),
    )
    .await;
    assert_eq!(status, 412);

    let (status, body) = patch(
        &client,
        url,
        "/contoso/Shop/_apis/wit/workitems/1003",
        json!([
            { "op": "test", "path": "/rev", "value": 5 },
            { "op": "add", "path": format!("/fields/{START_DATE_FIELD}"), "value": "2026-07-07T00:00:00Z" },
            { "op": "add", "path": format!("/fields/{TARGET_DATE_FIELD}"), "value": "2026-07-21T00:00:00Z" }
        ]),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["rev"], 6);
    assert_eq!(body["fields"][START_DATE_FIELD], "2026-07-07T00:00:00Z");

    let response = client
        .patch(format!("{url}/contoso/Shop/_apis/wit/workitems/1003"))
        .header("content-type", "application/json")
        .body(json!([{ "op": "test", "path": "/rev", "value": 6 }]).to_string())
        .send()
        .await
        .expect("patch json");
    assert_eq!(response.status().as_u16(), 415);

    let (status, body) = patch(
        &client,
        url,
        "/contoso/Shop/_apis/wit/workitems/1012",
        json!([
            { "op": "test", "path": "/rev", "value": 2 },
            { "op": "add", "path": "/relations/-", "value": {
                "rel": "System.LinkTypes.Dependency-Reverse",
                "url": format!("{url}/contoso/Shop/_apis/wit/workitems/1005")
            }}
        ]),
    )
    .await;
    assert_eq!(status, 200);
    assert!(
        body["relations"]
            .as_array()
            .is_some_and(|rels| rels.iter().any(|rel| {
                rel["rel"] == "System.LinkTypes.Dependency-Reverse"
                    && rel["url"]
                        .as_str()
                        .is_some_and(|url| url.ends_with("/1005"))
            })),
        "PATCH /relations/- should append a dependency"
    );

    let rev = body["rev"].as_i64().expect("rev");
    let (status, body) = patch(
        &client,
        url,
        "/contoso/Shop/_apis/wit/workitems/1012",
        json!([
            { "op": "test", "path": "/rev", "value": rev },
            { "op": "remove", "path": "/relations/1" }
        ]),
    )
    .await;
    assert_eq!(status, 200);
    assert!(
        body["relations"]
            .as_array()
            .is_some_and(|rels| !rels.iter().any(|rel| {
                rel["url"]
                    .as_str()
                    .is_some_and(|url| url.ends_with("/1005"))
            })),
        "PATCH remove /relations/n should drop that dependency"
    );
}

#[tokio::test]
async fn create_work_item_assigns_the_next_id() {
    let mock = ado_mock::start().await;
    let client = reqwest::Client::new();
    let (status, body) = post(
        &client,
        &mock.url,
        "/contoso/Shop/_apis/wit/workitems/$User%20Story",
        json!([
            { "op": "add", "path": "/fields/System.Title", "value": "New work item" },
            { "op": "add", "path": "/fields/System.AreaPath", "value": "Shop\\Platform" }
        ]),
    )
    .await;
    assert_eq!(status, 200);
    assert!(
        body["id"].as_i64().is_some_and(|id| id > 1020),
        "create should allocate an id after the fixtures, got {}",
        body["id"]
    );
    assert_eq!(body["fields"]["System.Title"], "New work item");
    assert_eq!(body["fields"]["System.WorkItemType"], "User Story");
}

#[tokio::test]
async fn fail_next_injects_error() {
    let mock = ado_mock::start().await;
    mock.fail_next("/fabrikam/_apis/projects", 503, "Projects unavailable");
    let client = reqwest::Client::new();
    let (status, body) = get(&client, &mock.url, "/fabrikam/_apis/projects").await;
    assert_eq!(status, 503);
    assert_eq!(body["message"], "Projects unavailable");
}
