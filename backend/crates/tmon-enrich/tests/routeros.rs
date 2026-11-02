use tmon_enrich::routeros::{RouterClient, RouterError};
use tmon_store::duck::repo::settings::{Router, RouterKind};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cfg(url: &str) -> Router {
    Router {
        kind: RouterKind::RouterOs,
        enabled: true,
        url: url.into(),
        username: "tmon-ro".into(),
        password: "secret".into(),
        verify_tls: true,
        lease_poll_secs: 60,
        dns_poll_secs: 30,
    }
}

const AUTH: &str = "Basic dG1vbi1ybzpzZWNyZXQ="; // tmon-ro:secret

#[tokio::test]
async fn leases_and_dns_are_parsed_with_basic_auth() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/ip/dhcp-server/lease"))
        .and(header("authorization", AUTH))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"[{"address":"192.168.88.10","mac-address":"AA:BB:CC:DD:EE:FF","host-name":"laptop"}]"#))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/ip/dns/cache"))
        .and(header("authorization", AUTH))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"[{"name":"cdn.example.com","data":"203.0.113.7","type":"A"},{"name":"x","data":"cdn.example.com","type":"CNAME"},{"name":"v6.example.com","data":"2001:db8::1","type":"AAAA"}]"#,
        ))
        .mount(&server)
        .await;
    let c = RouterClient::new(&cfg(&server.uri())).unwrap();
    let leases = c.leases().await.unwrap();
    assert_eq!(leases.len(), 1);
    assert_eq!(leases[0].mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
    assert_eq!(leases[0].hostname.as_deref(), Some("laptop"));
    let dns = c.dns_cache().await.unwrap();
    assert_eq!(
        dns.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
        ["cdn.example.com", "v6.example.com"]
    );
}

#[tokio::test]
async fn unauthorized_and_unreachable_are_errors_not_panics() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let c = RouterClient::new(&cfg(&server.uri())).unwrap();
    assert!(matches!(c.leases().await, Err(RouterError::Unauthorized)));

    let slow = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(15))
                .set_body_string("[]"),
        )
        .mount(&slow)
        .await;
    let c = RouterClient::new(&cfg(&slow.uri())).unwrap();
    assert!(
        matches!(c.dns_cache().await, Err(RouterError::Transport(_))),
        "timeout is a transport error"
    );

    let dead = RouterClient::new(&cfg("http://127.0.0.1:9")).unwrap();
    assert!(matches!(
        dead.leases().await,
        Err(RouterError::Transport(_))
    ));
    assert!(RouterClient::new(&cfg("ftp://router")).is_err());
}
