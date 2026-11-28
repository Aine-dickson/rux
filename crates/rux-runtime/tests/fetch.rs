//! Fetching a document: the rules a download keeps before any of it is parsed
//! (roadmap v0.10 item 2). The server is a thread on a loopback port, so what
//! the client does with a status, a redirect or a stall is what is tested.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use rux_runtime::fetch::{check_url, fetch_document, fetch_text};
use rux_runtime::Sandbox;

const DOC: &str = "<template><screen><text>hello</text></screen></template>\n<script>\nlet a = 1;\n</script>";

/// Serve `answer(path)` to every request until the test ends. Returns the port.
fn serve(answer: impl Fn(&str) -> Vec<u8> + Send + 'static) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
            let _ = stream.write_all(&answer(&path));
        }
    });
    port
}

fn ok(body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out
}

fn status(code: u16, extra: &str) -> Vec<u8> {
    format!("HTTP/1.1 {code} X\r\n{extra}Content-Length: 0\r\nConnection: close\r\n\r\n").into_bytes()
}

fn url(port: u16, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}")
}

const LONG: Duration = Duration::from_secs(5);

#[test]
fn a_plain_document_is_fetched_and_runs_sandboxed() {
    let port = serve(|_| ok(DOC.as_bytes()));
    assert_eq!(fetch_text(&url(port, "/app.rux"), 1 << 20, LONG).unwrap(), DOC);
    assert!(fetch_document(&url(port, "/app.rux"), Sandbox::new()).is_ok());
}

#[test]
fn a_fetched_document_is_held_to_its_sandbox() {
    let src = "<template><screen><text>x</text></screen></template>\n<script>\nfn go() { clipboard.writeText(\"x\"); }\n</script>";
    let port = serve(move |_| ok(src.as_bytes()));
    let err = fetch_document(&url(port, "/a.rux"), Sandbox::new()).err().expect("refused");
    assert!(err.to_string().contains("`clipboard`"), "{err}");
    assert!(fetch_document(&url(port, "/a.rux"), Sandbox::new().grant("clipboard")).is_ok());
}

#[test]
fn a_body_past_the_limit_is_refused() {
    let port = serve(|_| ok(&vec![b'x'; 5000]));
    let why = fetch_text(&url(port, "/big"), 1000, LONG).unwrap_err();
    assert!(why.contains("at most 1000"), "{why}");
    let mut sandbox = Sandbox::new();
    sandbox.limits.max_source_bytes = 1000;
    assert!(fetch_document(&url(port, "/big"), sandbox).is_err());
}

#[test]
fn an_error_status_is_an_error_with_the_status() {
    let port = serve(|path| if path == "/gone" { status(404, "") } else { status(500, "") });
    assert!(fetch_text(&url(port, "/gone"), 1 << 20, LONG).unwrap_err().contains("404"));
    assert!(fetch_text(&url(port, "/x"), 1 << 20, LONG).unwrap_err().contains("500"));
}

#[test]
fn a_redirect_on_the_same_host_is_followed() {
    let port = serve(|path| match path {
        "/old" => status(302, "Location: /new\r\n"),
        _ => ok(DOC.as_bytes()),
    });
    assert_eq!(fetch_text(&url(port, "/old"), 1 << 20, LONG).unwrap(), DOC);
}

#[test]
fn a_redirect_to_another_host_is_refused() {
    let other = serve(|_| ok(DOC.as_bytes()));
    let port = serve(move |_| status(302, &format!("Location: http://localhost:{other}/a.rux\r\n")));
    let why = fetch_text(&url(port, "/a.rux"), 1 << 20, LONG).unwrap_err();
    assert!(why.contains("localhost") && why.contains("127.0.0.1"), "{why}");
}

#[test]
fn redirects_stop_after_a_few() {
    let port = serve(|_| status(302, "Location: /again\r\n"));
    let why = fetch_text(&url(port, "/a"), 1 << 20, LONG).unwrap_err();
    assert!(why.contains("redirect"), "{why}");
}

#[test]
fn a_server_that_never_answers_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let held: Vec<TcpStream> = listener.incoming().flatten().collect();
        std::thread::sleep(Duration::from_secs(20));
        drop(held);
    });
    let started = std::time::Instant::now();
    assert!(fetch_text(&url(port, "/a"), 1 << 20, Duration::from_millis(400)).is_err());
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_body_that_is_not_text_is_refused() {
    let port = serve(|_| ok(&[0xff, 0xfe, 0x00, 0x80]));
    assert!(fetch_text(&url(port, "/a"), 1 << 20, LONG).unwrap_err().contains("UTF-8"));
}

#[test]
fn a_fetched_document_is_one_file() {
    let src = "<template><screen><card /></screen></template>\n<script>\nuse components::card;\n</script>";
    let port = serve(move |_| ok(src.as_bytes()));
    let err = fetch_document(&url(port, "/a.rux"), Sandbox::new()).err().expect("refused");
    let said = err.to_string();
    assert!(said.contains("one file") && said.contains("components::card"), "{said}");
}

#[test]
fn only_https_or_a_loopback_dev_server_is_fetched() {
    assert_eq!(check_url("https://example.com/a.rux").unwrap(), "example.com");
    assert_eq!(check_url("http://127.0.0.1:8000/a.rux").unwrap(), "127.0.0.1");
    assert_eq!(check_url("http://localhost:8000/a.rux").unwrap(), "localhost");
    assert_eq!(check_url("http://[::1]:8000/a.rux").unwrap(), "[::1]");
    for bad in [
        "http://example.com/a.rux",
        "ftp://example.com/a.rux",
        "file:///etc/passwd",
        "https://user@example.com/a.rux",
        "https://user:pw@example.com/",
        "https://exa mple.com/",
        "https://example.com/\n",
        "https://",
        "example.com/a.rux",
        "http://127.0.0.1.evil.com/a.rux",
    ] {
        assert!(check_url(bad).is_err(), "{bad}");
    }
    assert!(check_url("http://example.com/a.rux").unwrap_err().contains("https"));
}
