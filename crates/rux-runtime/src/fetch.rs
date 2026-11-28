//! A document fetched from the network, opened as one that is not the app's
//! own (roadmap v0.10 item 2).
//!
//! The opener hands it a [`Sandbox`]: a fetched file never marks itself
//! trusted. What is kept before any of it is parsed: `https` (plain `http`
//! only for this machine's own, so a dev server works), a body no longer than
//! the sandbox lets a file be, a time limit, and a redirect that stays on the
//! host it started on. One file: what it imports from other files has nowhere
//! to come from, and is said at the line.
//!
//! Native only: a page has no sockets (the browser's `fetch` is the web's
//! business, and the playground is handed its source). [`check_url`] is
//! text alone and is there everywhere, for `rux check`.

#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use crate::{Document, LoadError, Sandbox};

/// How long a fetch may take, all of it.
#[cfg(not(target_arch = "wasm32"))]
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// How many redirects are followed.
#[cfg(not(target_arch = "wasm32"))]
const MAX_REDIRECTS: usize = 3;

/// The host of `url`, when it is an address a document may be fetched from:
/// `https://host/...`, or `http://` for this machine (`localhost`,
/// `127.0.0.1`, `[::1]`). `Err` says what is wrong, before anything connects.
pub fn check_url(url: &str) -> Result<String, String> {
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(format!("`{}` has a space or a control character in it", url.trim()));
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return Err(format!("`{url}` is not an address: write it out, such as \"https://example.com/app.rux\""));
    };
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "https" && scheme != "http" {
        return Err(format!("`{scheme}://` is not an address a document is fetched from; it is https"));
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err(format!("`{url}` has a user name in it, which is not read: write the address without one"));
    }
    let (host, port) = match authority.strip_prefix('[') {
        Some(v6) => match v6.split_once(']') {
            Some((inside, after)) => (format!("[{}]", inside.to_ascii_lowercase()), after.strip_prefix(':').unwrap_or("")),
            None => return Err(format!("`{url}` has an unclosed `[`")),
        },
        None => match authority.split_once(':') {
            Some((h, p)) => (h.to_ascii_lowercase(), p),
            None => (authority.to_ascii_lowercase(), ""),
        },
    };
    if host.is_empty() {
        return Err(format!("`{url}` has no host"));
    }
    if !port.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("`{url}` has a port that is not a number"));
    }
    let loopback = matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]");
    if scheme == "http" && !loopback {
        return Err(format!(
            "`{url}` is not https: a document that arrives unencrypted can be replaced on the way. \
             Only this machine's own (`http://localhost`, `http://127.0.0.1`) may be plain"
        ));
    }
    let valid = host.starts_with('[') || host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if !valid {
        return Err(format!("`{host}` is not a host name"));
    }
    Ok(host)
}

/// The text at `url`, read as a document is: at most `max_bytes` of UTF-8,
/// within `timeout`, following up to three redirects that stay on the host.
#[cfg(not(target_arch = "wasm32"))]
pub fn fetch_text(url: &str, max_bytes: usize, timeout: Duration) -> Result<String, String> {
    let first = check_url(url)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .max_redirects_will_error(false)
        .http_status_as_error(false)
        .build()
        .into();
    let mut at = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let mut response = agent.get(&at).call().map_err(|e| format!("could not fetch {at}: {e}"))?;
        let code = response.status().as_u16();
        if (300..400).contains(&code) {
            let Some(location) = response.headers().get("location").and_then(|v| v.to_str().ok()).map(str::to_string)
            else {
                return Err(format!("{at} answered {code} with nowhere to go"));
            };
            let next = if location.starts_with('/') {
                let (scheme, rest) = at.split_once("://").unwrap_or(("https", &at));
                let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
                format!("{scheme}://{authority}{location}")
            } else {
                location
            };
            let host = check_url(&next).map_err(|why| format!("{at} redirects somewhere not allowed: {why}"))?;
            if host != first {
                return Err(format!("{url} redirects from {first} to {host}; a redirect has to stay on the host"));
            }
            at = next;
            continue;
        }
        if !(200..300).contains(&code) {
            return Err(format!("{at} answered {code}"));
        }
        let bytes = response.body_mut().with_config().limit(max_bytes as u64).read_to_vec().map_err(|e| {
            if matches!(e, ureq::Error::BodyExceedsLimit(_)) {
                format!("{at} is longer than a sandboxed document may be (at most {max_bytes} bytes)")
            } else {
                format!("could not read {at}: {e}")
            }
        })?;
        return String::from_utf8(bytes).map_err(|_| format!("{at} is not text: a document is UTF-8"));
    }
    Err(format!("{url} redirects more than {MAX_REDIRECTS} times"))
}

/// Fetch the document at `url` and open it under `sandbox`. It is one file:
/// a `use` of another `.rux` file is refused at its line, since nothing beside
/// a fetched document can be read.
#[cfg(not(target_arch = "wasm32"))]
pub fn fetch_document(url: &str, sandbox: Sandbox) -> Result<Document, LoadError> {
    let text = fetch_text(url, sandbox.limits.max_source_bytes, TIMEOUT).map_err(LoadError::plain)?;
    document_from_fetched(&text, sandbox)
}

/// Open text that was fetched as a document under `sandbox`: the second half
/// of [`fetch_document`], for a caller that fetched the text on another
/// thread (a `Document` is not `Send`). One file, as there.
#[cfg(not(target_arch = "wasm32"))]
pub fn document_from_fetched(text: &str, sandbox: Sandbox) -> Result<Document, LoadError> {
    if let Ok(sfc) = rux_parser::parse_sfc(text) {
        let (_, imports) = crate::extract_imports(&sfc.script);
        for import in &imports {
            if crate::native_module_of(&import.file).is_none() {
                let written = import.file.trim_end_matches(".rux").replace('/', "::");
                return Err(LoadError::at_line(
                    format!("a fetched document is one file: `use {written}` cannot load beside it"),
                    sfc.script_line + import.line - 1,
                    std::path::Path::new(""),
                ));
            }
        }
    }
    Document::from_source_sandboxed(text, sandbox)
}
