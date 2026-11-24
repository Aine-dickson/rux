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
//! **What is kept** is the answers, by URL, up to [`BUDGET`] bytes: a rebuild,
//! a second `<image>` of the same URL and the painter all read the one copy.
//! Past the budget the one asked for longest ago goes, and is fetched again
//! if it is asked for again (every answer was kept for the life of the
//! process, so a panned map grew without end). A failure is kept too, so a
//! missing image is asked for once and not on every frame.
//!
//! **What is refused:** a body over [`MAX_BYTES`], and a picture over the
//! size the painter draws (`rux_layout::image_too_large`), which matters more
//! now that the bytes are a stranger's.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The largest body read for one image: 32 MB, a large photo with room.
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// How many bytes of answers are kept: 32 MB. The painter keeps the decoded
/// pictures, which are what is drawn; these are only read again to decode one
/// the painter let go.
pub const BUDGET: usize = 32 * 1024 * 1024;

enum State {
    Loading,
    Ready(Arc<[u8]>),
    Failed(String),
}

/// Each answer with when it was last asked for.
struct Store {
    entries: HashMap<String, (State, u64)>,
    clock: u64,
    held: usize,
    budget: usize,
}

impl Store {
    fn new() -> Self {
        Self { entries: HashMap::new(), clock: 0, held: 0, budget: BUDGET }
    }

    fn get(&mut self, url: &str) -> Option<&State> {
        self.clock += 1;
        let clock = self.clock;
        let entry = self.entries.get_mut(url)?;
        entry.1 = clock;
        Some(&entry.0)
    }

    fn put(&mut self, url: &str, state: State) {
        self.clock += 1;
        if let Some((State::Ready(old), _)) = self.entries.get(url) {
            self.held -= old.len();
        }
        if let State::Ready(bytes) = &state {
            self.held += bytes.len();
        }
        self.entries.insert(url.to_string(), (state, self.clock));
        self.trim(url);
    }

    /// Drop the answers asked for longest ago, never `keep`, until the rest
    /// fit. One still coming is never dropped: its fetch would answer into
    /// nothing and start again.
    fn trim(&mut self, keep: &str) {
        if self.held <= self.budget {
            return;
        }
        let mut old: Vec<(u64, String)> = self
            .entries
            .iter()
            .filter(|(url, (state, _))| *url != keep && matches!(state, State::Ready(_)))
            .map(|(url, (_, used))| (*used, url.clone()))
            .collect();
        old.sort_unstable();
        for (_, url) in old {
            if self.held <= self.budget {
                break;
            }
            if let Some((State::Ready(bytes), _)) = self.entries.remove(&url) {
                self.held -= bytes.len();
            }
        }
    }
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

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
        let store = store.get_or_insert_with(Store::new);
        if store.entries.contains_key(url) {
            false
        } else {
            store.put(url, State::Loading);
            true
        }
    };
    // Outside the lock: a fetcher may answer before it returns.
    if started {
        fetch(url.to_string());
    }
    let mut store = STORE.lock().unwrap_or_else(|e| e.into_inner());
    match store.as_mut().and_then(|s| s.get(url)) {
        Some(State::Ready(bytes)) => Some(Ok(bytes.clone())),
        Some(State::Failed(why)) => Some(Err(why.clone())),
        _ => None,
    }
}

/// What the network gave for `url`, without asking for it: `None` when
/// nobody has, `Some(None)` while it is coming.
pub fn peek(url: &str) -> Option<Option<Result<Arc<[u8]>, String>>> {
    let mut store = STORE.lock().unwrap_or_else(|e| e.into_inner());
    Some(match store.as_mut()?.get(url)? {
        State::Ready(bytes) => Some(Ok(bytes.clone())),
        State::Failed(why) => Some(Err(why.clone())),
        State::Loading => None,
    })
}

/// What a fetch came back with. Called by whoever fetched, from any thread.
pub fn deliver(url: &str, result: Result<Vec<u8>, String>) {
    let state = match result {
        Ok(bytes) => State::Ready(bytes.into()),
        Err(why) => State::Failed(why),
    };
    STORE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Store::new).put(url, state);
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

/// How many fetches run at once. Each URL had a thread of its own, started at
/// once, so a zoom that showed 100 new tiles opened 100 connections.
pub const FETCHES_AT_ONCE: usize = 6;

/// The fetches waiting and running. Pure, so its order can be tested. The
/// web's fetcher is the browser's, which has its own limits.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct Queue {
    waiting: Vec<String>,
    running: usize,
    limit: usize,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
impl Queue {
    const fn new(limit: usize) -> Self {
        Self { waiting: Vec::new(), running: 0, limit }
    }

    /// `url` is wanted: what to start now.
    fn add(&mut self, url: String) -> Vec<String> {
        self.waiting.push(url);
        self.ready()
    }

    /// A fetch ended: what to start now.
    fn done(&mut self) -> Vec<String> {
        self.running = self.running.saturating_sub(1);
        self.ready()
    }

    /// The newest first: what is on the screen now was asked for last, and
    /// what was panned away from waits (it is still fetched, since a page that
    /// stands still asks nothing again and its images are still wanted).
    fn ready(&mut self) -> Vec<String> {
        let mut start = Vec::new();
        while self.running < self.limit {
            let Some(url) = self.waiting.pop() else { break };
            self.running += 1;
            start.push(url);
        }
        start
    }
}

#[cfg(not(target_arch = "wasm32"))]
static QUEUE: Mutex<Queue> = Mutex::new(Queue::new(FETCHES_AT_ONCE));

#[cfg(not(target_arch = "wasm32"))]
fn fetch_on_a_thread(url: String) {
    let start = QUEUE.lock().unwrap_or_else(|e| e.into_inner()).add(url);
    start.into_iter().for_each(run_fetch);
}

#[cfg(not(target_arch = "wasm32"))]
fn run_fetch(url: String) {
    std::thread::spawn(move || {
        // Kept on disk from before, or fetched and kept.
        let result = match crate::disk_cache::read(&url) {
            Some(bytes) => Ok(bytes),
            None => get(&url).map(|(bytes, cache_control)| {
                if let Some(life) = crate::disk_cache::life(cache_control.as_deref()) {
                    crate::disk_cache::write(&url, &bytes, life);
                }
                bytes
            }),
        };
        if let Err(why) = &result {
            eprintln!("rux: cannot load image {url}: {why}");
        }
        deliver(&url, result);
        let next = QUEUE.lock().unwrap_or_else(|e| e.into_inner()).done();
        next.into_iter().for_each(run_fetch);
    });
}

#[cfg(target_arch = "wasm32")]
fn fetch_on_a_thread(url: String) {
    // A page has no threads and no sockets; the web shell installs a fetcher
    // before any document loads, so this is a document loaded without one.
    deliver(&url, Err("nothing here can fetch from the network".to_string()));
}

#[cfg(not(target_arch = "wasm32"))]
/// The body, and its `Cache-Control`, which says how long the disk may keep it.
fn get(url: &str) -> Result<(Vec<u8>, Option<String>), String> {
    let mut response = ureq::get(url).call().map_err(|e| e.to_string())?;
    let cache_control =
        response.headers().get("cache-control").and_then(|v| v.to_str().ok()).map(str::to_string);
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    Ok((body, cache_control))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// So many at once, the newest first, and the next starts as one ends.
    #[test]
    fn fetches_run_a_few_at_once_newest_first() {
        let mut q = Queue::new(2);
        assert_eq!(q.add("a".into()), ["a"]);
        assert_eq!(q.add("b".into()), ["b"]);
        assert!(q.add("c".into()).is_empty());
        assert!(q.add("d".into()).is_empty());
        assert_eq!(q.done(), ["d"]);
        assert_eq!(q.done(), ["c"]);
        assert!(q.done().is_empty());
        assert_eq!(q.running, 1);
    }

    /// Over budget, the answer asked for longest ago goes; one still coming
    /// and the one just delivered stay.
    #[test]
    fn the_store_drops_the_answer_asked_for_longest_ago_once_over_budget() {
        let mut store = Store { budget: 25, ..Store::new() };
        store.put("a", State::Ready(vec![0; 10].into()));
        store.put("b", State::Ready(vec![0; 10].into()));
        store.put("wait", State::Loading);
        assert!(store.get("a").is_some()); // `a` is now newer than `b`
        store.put("c", State::Ready(vec![0; 10].into()));
        assert!(store.entries.contains_key("a") && store.entries.contains_key("c"));
        assert!(!store.entries.contains_key("b"));
        assert!(store.entries.contains_key("wait"));
        assert_eq!(store.held, 20);
        // One answer larger than the budget is still kept, alone.
        store.put("big", State::Ready(vec![0; 40].into()));
        assert!(store.entries.contains_key("big") && !store.entries.contains_key("a"));
        assert_eq!(store.held, 40);
    }

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
