use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use super::Client;

/// Sends one GET through `client` to a local listener and returns the raw request head the
/// listener received, so the test sees exactly what goes over the wire (reqwest adds a client's
/// default headers, like `User-Agent`, only when the request is executed).
fn captured_request_head(client: &Client) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("should bind a local port");
    let addr = listener
        .local_addr()
        .expect("listener should have an address");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("should accept a connection");
        let mut head = Vec::new();
        let mut buf = [0u8; 1024];
        while !head.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut buf).expect("should read the request");
            if read == 0 {
                break;
            }
            head.extend_from_slice(&buf[..read]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .expect("should write the response");
        String::from_utf8_lossy(&head).into_owned()
    });

    let response = futures::executor::block_on(client.get(format!("http://{addr}/")).send())
        .expect("request should succeed");
    assert!(response.status().is_success());
    server.join().expect("server thread should not panic")
}

fn header_names(head: &str) -> Vec<String> {
    head.lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(name, _)| name.trim().to_ascii_lowercase())
        .collect()
}

#[test]
fn requests_carry_no_warp_client_headers() {
    let client = Client::from_client_builder(reqwest::ClientBuilder::new().no_proxy())
        .expect("should build a client");
    let names = header_names(&captured_request_head(&client));

    for removed in [
        "x-warp-client-id",
        "x-warp-client-version",
        "x-warp-os-category",
        "x-warp-os-name",
        "x-warp-os-version",
        "x-warp-os-linux-kernel-version",
    ] {
        assert!(
            !names.iter().any(|name| name == removed),
            "{removed} must not be sent; got {names:?}"
        );
    }
    assert!(
        !names.iter().any(|name| name.starts_with("x-warp")),
        "no Warp header may be sent; got {names:?}"
    );
}

#[test]
fn with_user_agent_sends_only_the_given_user_agent() {
    let client = Client::with_user_agent("Uncaged/1.2.3");
    let head = captured_request_head(&client);
    let names = header_names(&head);

    assert!(
        head.lines()
            .any(|line| line.eq_ignore_ascii_case("user-agent: Uncaged/1.2.3")),
        "expected the given User-Agent; got {head}"
    );
    assert!(
        !names.iter().any(|name| name.starts_with("x-warp")),
        "no Warp header may be sent; got {names:?}"
    );
}
