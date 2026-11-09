//! Serves the fixtures on a loopback port until killed, so the app can run
//! against the mock by hand. See `docs/agents/dev-session.md`.

#[tokio::main]
async fn main() {
    let mock = ado_mock::start().await;
    println!("ADO_PLANNER_ADO_BASE_URL={}", mock.url);
    println!("ADO_PLANNER_VSSPS_URL={}", mock.url);
    std::future::pending::<()>().await;
}
