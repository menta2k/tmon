//! OpenWrt rpcd/ubus JSON-RPC client: session login, DHCP leases, re-login.

use serde_json::{Value, json};
use tmon_enrich::openwrt::OpenWrtClient;
use tmon_enrich::router_source::RouterSource;
use tmon_enrich::routeros::RouterError;
use tmon_store::duck::repo::settings::{Router, RouterKind};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn cfg(url: &str) -> Router {
    Router {
        kind: RouterKind::OpenWrt,
        enabled: true,
        url: url.into(),
        username: "tmon".into(),
        password: "secret".into(),
        verify_tls: true,
        lease_poll_secs: 60,
        dns_poll_secs: 30,
    }
}

fn params(r: &Request) -> Vec<Value> {
    serde_json::from_slice::<Value>(&r.body).unwrap()["params"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn is_login(r: &Request) -> bool {
    let p = params(r);
    p.get(1) == Some(&json!("session")) && p.get(2) == Some(&json!("login"))
}

fn is_leases(token: &'static str) -> impl Fn(&Request) -> bool {
    move |r: &Request| {
        let p = params(r);
        p.first() == Some(&json!(token))
            && p.get(1) == Some(&json!("luci-rpc"))
            && p.get(2) == Some(&json!("getDHCPLeases"))
    }
}

fn rpc(result: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"jsonrpc": "2.0", "id": 1, "result": result}))
}

fn login_ok(token: &str) -> ResponseTemplate {
    rpc(json!([0, {"ubus_rpc_session": token, "timeout": 300}]))
}

fn leases_body() -> ResponseTemplate {
    rpc(json!([0, {
        "dhcp_leases": [
            {"expires": 3600, "hostname": "Living-Room-TV", "ipaddr": "192.168.1.20", "macaddr": "AA:BB:CC:DD:EE:01"},
            {"expires": 3600, "hostname": "*", "ipaddr": "192.168.1.21", "macaddr": "aa:bb:cc:dd:ee:02"},
            {"expires": 3600, "ipaddr": "not-an-ip"}
        ],
        "dhcp6_leases": [
            {"expires": 3600, "hostname": "laptop", "ip6addr": "fd00::20/128", "duid": "0001"}
        ]
    }]))
}

#[tokio::test]
async fn logs_in_once_and_parses_v4_and_v6_leases() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/ubus"))
        .and(is_login)
        .respond_with(login_ok("tok1"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/ubus"))
        .and(is_leases("tok1"))
        .respond_with(leases_body())
        .expect(2)
        .mount(&server)
        .await;
    let c = OpenWrtClient::new(&cfg(&server.uri())).unwrap();
    let leases = c.leases().await.unwrap();
    let got: Vec<_> = leases
        .iter()
        .map(|l| (l.ip.to_string(), l.mac.clone(), l.hostname.clone()))
        .collect();
    assert_eq!(
        got,
        [
            (
                "192.168.1.20".to_string(),
                Some("aa:bb:cc:dd:ee:01".to_string()),
                Some("living-room-tv".to_string())
            ),
            (
                "192.168.1.21".to_string(),
                Some("aa:bb:cc:dd:ee:02".to_string()),
                None
            ),
            ("fd00::20".to_string(), None, Some("laptop".to_string())),
        ]
    );
    // The session is reused: `expect(1)` on login is verified on drop.
    assert_eq!(c.leases().await.unwrap().len(), 3);
}

#[tokio::test]
async fn expired_session_logs_in_again_once() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(is_login)
        .respond_with(login_ok("tok1"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(is_login)
        .respond_with(login_ok("tok2"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(is_leases("tok1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32002, "message": "Access denied"}}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(is_leases("tok2"))
        .respond_with(leases_body())
        .mount(&server)
        .await;
    let c = OpenWrtClient::new(&cfg(&server.uri())).unwrap();
    assert_eq!(c.leases().await.unwrap().len(), 3);
}

#[tokio::test]
async fn bad_credentials_and_missing_acl_are_distinct_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(is_login)
        .respond_with(rpc(json!([6])))
        .mount(&server)
        .await;
    let c = OpenWrtClient::new(&cfg(&server.uri())).unwrap();
    assert!(matches!(c.leases().await, Err(RouterError::Unauthorized)));

    let acl = MockServer::start().await;
    Mock::given(method("POST"))
        .and(is_login)
        .respond_with(login_ok("tok1"))
        .mount(&acl)
        .await;
    Mock::given(method("POST"))
        .and(is_leases("tok1"))
        .respond_with(rpc(json!([6])))
        .mount(&acl)
        .await;
    let c = OpenWrtClient::new(&cfg(&acl.uri())).unwrap();
    match c.leases().await {
        Err(RouterError::Denied(m)) => assert!(m.contains("getDHCPLeases"), "{m}"),
        other => panic!("expected Denied, got {other:?}"),
    }
}

#[tokio::test]
async fn garbage_and_unreachable_are_errors_not_panics() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
        .mount(&server)
        .await;
    let c = OpenWrtClient::new(&cfg(&server.uri())).unwrap();
    assert!(matches!(c.leases().await, Err(RouterError::Decode(_))));
    let dead = OpenWrtClient::new(&cfg("http://127.0.0.1:9")).unwrap();
    assert!(matches!(
        dead.leases().await,
        Err(RouterError::Transport(_))
    ));
    assert!(OpenWrtClient::new(&cfg("ftp://router")).is_err());
}

#[tokio::test]
async fn router_source_dispatches_by_kind() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(is_login)
        .respond_with(login_ok("tok1"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(is_leases("tok1"))
        .respond_with(leases_body())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/ip/dhcp-server/lease"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"[{"address":"192.168.88.10","host-name":"pc"}]"#),
        )
        .mount(&server)
        .await;
    let owrt = RouterSource::new(&cfg(&server.uri())).unwrap();
    assert_eq!(owrt.leases().await.unwrap().len(), 3);
    assert!(
        owrt.dns_cache().await.unwrap().is_none(),
        "no DNS cache on OpenWrt"
    );
    let ros = RouterSource::new(&Router {
        kind: RouterKind::RouterOs,
        ..cfg(&server.uri())
    })
    .unwrap();
    assert_eq!(ros.leases().await.unwrap().len(), 1);
}

#[tokio::test]
async fn redirects_are_not_followed_so_the_password_stays_put() {
    let thief = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(login_ok("stolen"))
        .expect(0)
        .mount(&thief)
        .await;
    let router = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(307).insert_header("location", format!("{}/ubus", thief.uri())),
        )
        .mount(&router)
        .await;
    let c = OpenWrtClient::new(&cfg(&router.uri())).unwrap();
    assert!(matches!(c.leases().await, Err(RouterError::Status(307))));
}

#[tokio::test]
async fn oversized_and_null_payloads_are_handled() {
    let big = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(5 * 1024 * 1024)))
        .mount(&big)
        .await;
    let c = OpenWrtClient::new(&cfg(&big.uri())).unwrap();
    match c.leases().await {
        Err(RouterError::Decode(m)) => assert!(m.contains("too large"), "{m}"),
        other => panic!("expected Decode, got {other:?}"),
    }
    let nulls =
        tmon_enrich::openwrt::parse_leases(json!({"dhcp_leases": null, "dhcp6_leases": null}));
    assert_eq!(nulls.unwrap().len(), 0);
    let long_err = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"jsonrpc": "2.0", "id": 1, "error": {"code": 1, "message": "e".repeat(10_000)}}),
        ))
        .mount(&long_err)
        .await;
    let c = OpenWrtClient::new(&cfg(&long_err.uri())).unwrap();
    let msg = c.leases().await.unwrap_err().to_string();
    assert!(
        msg.len() < 400,
        "router error text is truncated: {}",
        msg.len()
    );
}
