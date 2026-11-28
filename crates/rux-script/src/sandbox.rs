//! What a document opened from somewhere that is not the app's own gets.
//!
//! Compiled app code and the files shipped in the app have full trust. A
//! document that arrives from elsewhere (the playground, a fetched or
//! over-the-air one) is run under a [`Sandbox`]: limits on what it may spend,
//! and no native module unless the opener grants it by name. A file can never
//! mark itself trusted; only whoever opens it hands over a `Sandbox`, and with
//! none the document is trusted, as it has always been.

use std::collections::BTreeSet;

/// What a sandboxed document may spend. Every limit is checked before the work
/// it limits, so a document is stopped, not slowed.
#[derive(Clone, Debug)]
pub struct Limits {
    /// The most steps one run may take. See [`crate::MAX_OPERATIONS`].
    pub steps_per_run: u64,
    /// The longest one piece of text may be, in bytes.
    pub max_text_bytes: usize,
    /// The most entries in one array, map or record.
    pub max_items: usize,
    /// What one run may grow memory by, in bytes.
    pub run_alloc_bytes: usize,
    /// What the document's state may come to after a run, in bytes.
    pub state_bytes: usize,
    /// The most elements one render may build.
    pub max_nodes: usize,
    /// The longest document text, in bytes, one file may be.
    pub max_source_bytes: usize,
    /// The deepest brackets, blocks, templates and prefix operators may nest.
    pub max_nesting: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            steps_per_run: crate::MAX_OPERATIONS,
            max_text_bytes: 16 << 20,
            max_items: 1_000_000,
            run_alloc_bytes: 64 << 20,
            state_bytes: 64 << 20,
            max_nodes: 50_000,
            max_source_bytes: 1 << 20,
            max_nesting: 64,
        }
    }
}

/// The terms a document is run on when it is not the app's own: [`Limits`], and
/// the native modules it was granted, by the name script uses (`clipboard`,
/// `settings`, `files`, `native::shop`, `images`). Nothing is granted by
/// [`Sandbox::new`].
#[derive(Clone, Debug, Default)]
pub struct Sandbox {
    pub grants: BTreeSet<String>,
    pub limits: Limits,
}

/// Every grant that names something Rux gives every script: the built-in modules,
/// the file dialogs and sounds, and `images`, which lets an `<image>` come from the network.
pub const BUILTIN_GRANTS: &[&str] = &[
    "clipboard", "settings", "window", "permissions", "app", "notifications", "background",
    "location", "links", "secrets", "network", "files", "images",
];

impl Sandbox {
    /// The default limits and no grants.
    pub fn new() -> Self {
        Self::default()
    }

    /// What the playground runs its source on: the default limits, and every
    /// built-in granted, since the browser tab is already the outer sandbox and
    /// asks the person itself. It has no app modules to grant.
    pub fn playground() -> Self {
        BUILTIN_GRANTS.iter().fold(Self::new(), |s, g| s.grant(g))
    }

    /// Whether `name` is something a grant can name. See [`check_grant`].
    pub fn is_known_grant(name: &str) -> bool {
        check_grant(name).is_ok()
    }

    /// [`check_grant`], for a caller that has only a `Sandbox` in reach.
    pub fn check(name: &str) -> Result<(), String> {
        check_grant(name)
    }

    /// Allow the document `module`.
    pub fn grant(mut self, module: &str) -> Self {
        self.grants.insert(module.to_string());
        self
    }

    /// Whether `module` was granted whole, by that exact name.
    pub fn grants(&self, module: &str) -> bool {
        self.grants.contains(module)
    }

    /// Whether anything of `module` was granted, whole or in part: what lets
    /// the document import it. Each call is asked again by
    /// [`allows_call`](Self::allows_call).
    pub fn names(&self, module: &str) -> bool {
        self.grants.iter().any(|g| Grant::parse(g).is_some_and(|g| g.module == module))
    }

    /// Whether the call `member` of `module` may run with `first`, its first
    /// argument when that is text. `Err` is what to tell the author.
    pub fn allows_call(&self, module: &str, member: &str, first: Option<&str>) -> Result<(), String> {
        let mine: Vec<Grant> =
            self.grants.iter().filter_map(|g| Grant::parse(g)).filter(|g| g.module == module).collect();
        if mine.is_empty() {
            return Err(not_granted(module));
        }
        let call = format!("{module}.{member}");
        let mut hosts: Vec<&str> = Vec::new();
        for g in &mine {
            match (&g.member, &g.host) {
                (None, _) => return Ok(()),
                (Some(m), None) if m == member => return Ok(()),
                (Some(m), Some(h)) if m == member => {
                    hosts.push(h);
                    if first.is_some_and(|url| url_on(url, h)) {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
        if !hosts.is_empty() {
            return Err(format!(
                "`{call}` may only reach {} in this document, and that is not where this goes",
                hosts.join(" or ")
            ));
        }
        let given: Vec<String> = mine.iter().map(|g| format!("`{}`", g.text())).collect();
        Err(format!(
            "`{call}` is not available in this document: it was granted {}, not `{call}` \
             (`--sandbox={call}`, `.grant(\"{call}\")`)",
            given.join(", ")
        ))
    }

    /// Whether an `<image>` may be fetched from `url`.
    pub fn allows_image(&self, url: &str) -> bool {
        self.grants
            .iter()
            .filter_map(|g| Grant::parse(g))
            .filter(|g| g.module == "images")
            .any(|g| g.host.as_deref().is_none_or(|h| url_on(url, h)))
    }

    /// The first reason `text`, one file of a document, may not be loaded under
    /// these limits: the offset it goes wrong at, and what to tell the author.
    /// Checked before the text is lexed, so nothing hostile reaches a parser.
    pub fn refuse(&self, text: &str) -> Option<(usize, String)> {
        let l = &self.limits;
        if text.len() > l.max_source_bytes {
            return Some((
                0,
                format!(
                    "this file is {} bytes; a sandboxed document may be at most {}",
                    text.len(),
                    l.max_source_bytes
                ),
            ));
        }
        rux_syntax::nests_deeper_than(text, l.max_nesting).map(|at| {
            (at, format!("this is nested more than {} deep, the most a sandboxed document may be", l.max_nesting))
        })
    }
}

/// One grant, taken apart: `links`, `links.open`, `links.open@example.com`,
/// `images@cdn.example.com`, `native::shop.search`.
#[derive(Clone, Debug, PartialEq)]
struct Grant {
    module: String,
    member: Option<String>,
    host: Option<String>,
}

impl Grant {
    fn parse(text: &str) -> Option<Grant> {
        let (head, host) = match text.split_once('@') {
            Some((h, host)) => (h, Some(host.to_string())),
            None => (text, None),
        };
        let (module, member) = match head.split_once('.') {
            Some((m, rest)) => (m, Some(rest.to_string())),
            None => (head, None),
        };
        (!module.is_empty()).then(|| Grant { module: module.to_string(), member, host })
    }

    fn text(&self) -> String {
        let mut out = self.module.clone();
        if let Some(m) = &self.member {
            out += &format!(".{m}");
        }
        if let Some(h) = &self.host {
            out += &format!("@{h}");
        }
        out
    }
}

/// Whether `text` is a grant: a built-in module or `native::name`, optionally
/// narrowed to one function (`links.open`), and for `links.open` and `images`
/// to a host (`links.open@example.com`). `Err` says what the forms are.
pub fn check_grant(text: &str) -> Result<(), String> {
    let forms = "a module (`links`), one of its functions (`links.open`), and for `links.open` and \
                 `images` a host (`links.open@example.com`)";
    let Some(g) = Grant::parse(text) else { return Err(format!("`{text}` names nothing; a grant is {forms}")) };
    if !(BUILTIN_GRANTS.contains(&g.module.as_str()) || g.module.starts_with("native::")) {
        return Err(format!(
            "`{}` is not something to grant; the names are {}, and `native::name` for an app's own module",
            g.module,
            BUILTIN_GRANTS.join(", ")
        ));
    }
    if g.member.as_deref() == Some("") {
        return Err(format!("`{text}` has no function after the `.`; a grant is {forms}"));
    }
    if let Some(host) = &g.host {
        let target = (g.module.as_str(), g.member.as_deref());
        if !matches!(target, ("links", Some("open")) | ("images", None)) {
            return Err(format!("`{text}`: only `links.open` and `images` take a host; a grant is {forms}"));
        }
        let valid = !host.is_empty()
            && !host.starts_with(['.', '-'])
            && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        if !valid {
            return Err(format!("`{text}`: `{host}` is not a host name (letters, digits, `.` and `-`)"));
        }
    }
    Ok(())
}

/// The host of an `http` or `https` address, lowercased: what a browser would
/// connect to. `None` for anything else, and for an address a browser could
/// read differently from this (a backslash, a space, a control character, an
/// IP literal).
pub fn host_of(url: &str) -> Option<String> {
    if url.chars().any(|c| c.is_control() || c.is_whitespace() || c == '\\') {
        return None;
    }
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if host.starts_with('[') {
        return None;
    }
    let host = host.split(':').next()?.trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Whether `url` is on `host` or a subdomain of it.
fn url_on(url: &str, host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host_of(url).is_some_and(|h| h == host || h.ends_with(&format!(".{host}")))
}

/// The name a grant uses for native module `module` (as linked): `clipboard`
/// for `native/rux/clipboard`, `files` for the dialogs and sounds Rux gives
/// every script by bare name (`native/rux`), and the path an app's own module
/// is imported by (`native::shop`) for the rest.
pub fn grant_name(module: &str) -> String {
    if module == crate::native::BUILTIN_MODULE {
        return "files".to_string();
    }
    match module.strip_prefix("native/rux/") {
        Some(name) => name.to_string(),
        None => module.replace('/', "::"),
    }
}

/// What a document is told when it uses what it was not granted: what to
/// call it, and who can change it.
pub fn not_granted(name: &str) -> String {
    format!(
        "`{name}` is not available in this document: whoever opens it has to grant it \
         (`--sandbox={name}` on the command line, `.grant(\"{name}\")` in Rust)"
    )
}

impl Sandbox {
    /// Whether anything of the native module `module` (as linked) was granted.
    pub fn allows(&self, module: &str) -> bool {
        self.names(&grant_name(module))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_playground_gets_every_built_in_and_no_app_module() {
        let p = Sandbox::playground();
        for module in [
            crate::native::BUILTIN_MODULE,
            crate::native::CLIPBOARD_MODULE,
            crate::native::SETTINGS_MODULE,
            crate::native::WINDOW_MODULE,
            crate::native::LOCATION_MODULE,
            crate::native::NETWORK_MODULE,
        ] {
            assert!(p.allows(module), "{module}");
        }
        assert!(p.grants("images"));
        assert!(!p.allows("native/shop"));
        assert!(!Sandbox::new().grants("images"));
    }

    #[test]
    fn a_grant_is_a_module_a_function_or_a_function_on_a_host() {
        for ok in [
            "links", "links.open", "links.open@example.com", "images", "images@cdn.example.com", "native::shop",
            "native::shop.search", "native::shop.Cart.add", "settings.get",
        ] {
            assert!(check_grant(ok).is_ok(), "{ok}: {:?}", check_grant(ok));
        }
        for bad in [
            "links@example.com", "settings.get@example.com", "links.", "@example.com", "images@a/b", "images@",
            "links.dial@example.com", "nothing", "links.open@.example.com", "links.open@-x",
        ] {
            assert!(check_grant(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_host_is_read_as_a_browser_reads_it() {
        let on = url_on;
        assert!(on("https://example.com/a?b=c", "example.com"));
        assert!(on("https://EXAMPLE.com:8443/x", "example.com"));
        assert!(on("https://sub.example.com", "example.com"));
        assert!(on("https://example.com./", "example.com"));
        assert!(!on("https://example.com.evil.com/", "example.com"));
        assert!(!on("https://notexample.com/", "example.com"));
        assert!(!on("https://example.com@evil.com/", "example.com"));
        assert!(on("https://user:pw@example.com/", "example.com"));
        assert!(!on("https://evil.com/?u=example.com", "example.com"));
        assert!(!on("https://evil.com#example.com", "example.com"));
        assert!(!on("https://evil.com\\@example.com/", "example.com"));
        assert!(!on("http:example.com", "example.com"));
        assert!(!on("javascript://example.com/%0aalert(1)", "example.com"));
        assert!(!on("mailto:a@example.com", "example.com"));
        assert!(!on("https://[::1]/", "example.com"));
        assert!(!on("https://exa mple.com/", "example.com"));
    }

    #[test]
    fn a_call_is_allowed_by_the_narrowest_grant_that_covers_it() {
        let whole = Sandbox::new().grant("links");
        assert!(whole.allows_call("links", "dial", Some("123")).is_ok());
        let one = Sandbox::new().grant("links.open");
        assert!(one.allows_call("links", "open", Some("https://any.org")).is_ok());
        let e = one.allows_call("links", "dial", Some("123")).unwrap_err();
        assert!(e.contains("links.dial") && e.contains("links.open"), "{e}");
        let host = Sandbox::new().grant("links.open@example.com");
        assert!(host.allows_call("links", "open", Some("https://a.example.com/x")).is_ok());
        let e = host.allows_call("links", "open", Some("https://evil.com")).unwrap_err();
        assert!(e.contains("example.com"), "{e}");
        assert!(host.allows_call("links", "open", Some("tel:123")).is_err());
        assert!(host.allows_call("links", "open", None).is_err());
        assert!(host.allows_call("links", "dial", Some("123")).is_err());
        let two = host.clone().grant("links.open@other.org");
        assert!(two.allows_call("links", "open", Some("https://other.org")).is_ok());
        assert!(Sandbox::new().allows_call("links", "open", Some("https://a.b")).is_err());
        let app = Sandbox::new().grant("native::shop.search");
        assert!(app.allows_call("native::shop", "search", None).is_ok());
        assert!(app.allows_call("native::shop", "checkout", None).is_err());
        assert!(app.names("native::shop") && !app.names("links"));
    }

    #[test]
    fn an_image_is_allowed_by_a_whole_grant_or_its_host() {
        assert!(Sandbox::new().grant("images").allows_image("https://anywhere.net/a.png"));
        let host = Sandbox::new().grant("images@cdn.example.com");
        assert!(host.allows_image("https://cdn.example.com/a.png"));
        assert!(!host.allows_image("https://evil.com/a.png"));
        assert!(!Sandbox::new().allows_image("https://cdn.example.com/a.png"));
    }
}
