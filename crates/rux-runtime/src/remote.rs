//! Images from the network: `<image src="https://…">` and
//! `background-image: url(https://…)`.
//!
//! An image on a disk is read when the document loads, all of it at once.
//! One on the network cannot be: it takes as long as it takes, and the window
//! must not wait. So the first ask starts a fetch and answers "not yet", the
//! box lays out as an image with no size (as a browser's does before the
//! bytes come), and when they come the window is woken, the size is read and
//! the picture painted.
//!
//! **Where the fetch happens** is the platform's: a thread with `ureq` on the
//! desktop and Android, the browser's own `fetch` on the web (the shell
//! installs it with [`set_fetcher`]), since a page cannot open a socket and
//! the browser already has the cache and the cookies.
//!
//! **What is kept** is every answer for the life of the process, by URL: a
//! rebuild, a second `<image>` of the same URL and the painter all read the
//! one copy. A failure is kept too, so a missing image is asked for once and
//! not on every frame.
//!
//! **What is refused:** a body over [`MAX_BYTES`], and a picture over the
//! size the painter draws (`rux_layout::image_too_large`), which matters more
//! now that the bytes are a stranger's.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The largest body read for one image: 32 MB, a large photo with room.
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;

enum State {
    Loading,
    Ready(Arc<[u8]>),
    Failed(String),
}

static STORE: Mutex<Option<HashMap<String, State>>> = Mutex::new(None);

/// Whether anything arrived since [`take_arrived`] last asked.
static ARRIVED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

type Fetcher = Arc<dyn Fn(String) + Send + Sync>;
static FETCHER: Mutex<Option<Fetcher>> = Mutex::new(None);

pub use rux_layout::is_remote;

/// What the network gave for `url`: `None` while it is coming, which starts
/// it coming if nobody has asked yet.
pub fn bytes(url: &str) -> Option<Result<Arc<[u8]>, String>> {
    let started = {
        let mut store = STORE.lock().unwrap_or_else(|e| e.into_inner());
        let store = store.get_or_insert_with(HashMap::new);
        if store.contains_key(url) {
            false
        } else {
            store.insert(url.to_string(), State::Loading);
            true
        }
    };
    // Outside the lock: a fetcher may answer before it returns.
    if started {
        fetch(url.to_string());
    }
    let store = STORE.lock().unwrap_or_else(|e| e.into_inner());
    match store.as_ref().and_then(|s| s.get(url)) {
        Some(State::Ready(bytes)) => Some(Ok(bytes.clone())),
        Some(State::Failed(why)) => Some(Err(why.clone())),
        _ => None,
    }
}

/// What a fetch came back with. Called by whoever fetched, from any thread.
pub fn deliver(url: &str, result: Result<Vec<u8>, String>) {
    let state = match result {
        Ok(bytes) => State::Ready(bytes.into()),
        Err(why) => State::Failed(why),
    };
    STORE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(url.to_string(), state);
    ARRIVED.store(true, std::sync::atomic::Ordering::Release);
    // The painter forgets what it could not draw, and the window wakes.
    rux_layout::image_arrived();
    rux_script::host::wake();
}

/// Whether an image arrived since this was last asked, so the document can
/// read the sizes it now has.
pub fn take_arrived() -> bool {
    ARRIVED.swap(false, std::sync::atomic::Ordering::AcqRel)
}

/// Fetch with `f` instead of the built-in way: the browser's `fetch`, which
/// the web shell installs. `f` is handed the URL and must end in [`deliver`].
pub fn set_fetcher(f: impl Fn(String) + Send + Sync + 'static) {
    *FETCHER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(f));
}

fn fetch(url: String) {
    let installed = FETCHER.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match installed {
        Some(f) => f(url),
        None => fetch_on_a_thread(url),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_on_a_thread(url: String) {
    std::thread::spawn(move || {
        let result = get(&url);
        if let Err(why) = &result {
            eprintln!("rux: cannot load image {url}: {why}");
        }
        deliver(&url, result);
    });
}

#[cfg(target_arch = "wasm32")]
fn fetch_on_a_thread(url: String) {
    // A page has no threads and no sockets; the web shell installs a fetcher
    // before any document loads, so this is a document loaded without one.
    deliver(&url, Err("nothing here can fetch from the network".to_string()));
}

#[cfg(not(target_arch = "wasm32"))]
fn get(url: &str) -> Result<Vec<u8>, String> {
    let mut response = ureq::get(url).call().map_err(|e| e.to_string())?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fetch answers once and is then read from the store, a failure too;
    /// the second ask does not start a second fetch.
    #[test]
    fn an_answer_is_kept_and_a_second_ask_does_not_fetch_again() {
        let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = asked.clone();
        set_fetcher(move |url| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            deliver(&url, if url.ends_with("bad") { Err("404".into()) } else { Ok(vec![1, 2, 3]) });
        });
        assert!(matches!(bytes("https://t.test/ok"), Some(Ok(b)) if b[..] == [1, 2, 3]));
        assert!(matches!(bytes("https://t.test/ok"), Some(Ok(_))));
        assert!(matches!(bytes("https://t.test/bad"), Some(Err(why)) if why == "404"));
        assert!(matches!(bytes("https://t.test/bad"), Some(Err(_))));
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(take_arrived());
        *FETCHER.lock().unwrap() = None;
    }
}
