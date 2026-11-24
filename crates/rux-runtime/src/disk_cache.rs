//! Images from the network kept on disk, so a map's tiles or a feed's photos
//! are read from the device the next time, not fetched again (Urbandrop's
//! UD-006).
//!
//! **Where:** a folder the shell names at start ([`set_dir`]): the system's
//! cache folder for the app on the desktop, the app's cache directory on
//! Android (which the system may empty, as a cache should be). The web has
//! none: the browser keeps its own.
//!
//! **What is kept:** an answer the server did not mark `no-store` or
//! `no-cache`, until `max-age` runs out, or 7 days when it says nothing. Each
//! file holds the URL and when it runs out, then the bytes; a file whose URL is
//! not the one asked for (two URLs with the same hash) is a miss.
//!
//! **How much:** [`CAP`] in all. A read touches the file, and past the cap the
//! files touched longest ago go, down to nine tenths of it.
//!
//! Read and written by the fetch threads only, never by the window's.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The most kept on disk: 100 MB.
pub const CAP: u64 = 100 * 1024 * 1024;

/// How long an answer that says nothing about it is kept.
const DEFAULT_LIFE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

const MAGIC: &str = "rux-image 1";

struct Disk {
    dir: PathBuf,
    cap: u64,
    /// The bytes in the folder, once counted.
    held: Option<u64>,
}

static DISK: Mutex<Option<Disk>> = Mutex::new(None);

/// Keep images from the network in `dir`, up to [`CAP`].
pub fn set_dir(dir: PathBuf) {
    set_dir_with_cap(dir, CAP);
}

fn set_dir_with_cap(dir: PathBuf, cap: u64) {
    *DISK.lock().unwrap_or_else(|e| e.into_inner()) = Some(Disk { dir, cap, held: None });
}

/// The file `url` is kept in.
fn file_for(dir: &Path, url: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut a = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut a);
    let mut b = std::collections::hash_map::DefaultHasher::new();
    (url, 0x5eed_u32).hash(&mut b);
    dir.join(format!("{:016x}{:016x}.img", a.finish(), b.finish()))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The bytes kept for `url`, if they are there and have not run out.
pub fn read(url: &str) -> Option<Vec<u8>> {
    let dir = DISK.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.dir.clone();
    let path = file_for(&dir, url);
    let data = std::fs::read(&path).ok()?;
    let (kept_url, expires, body) = parse(&data)?;
    if kept_url != url {
        return None;
    }
    if expires <= now() {
        let _ = std::fs::remove_file(&path);
        forget_size(data.len() as u64);
        return None;
    }
    // Touched, so it is the last to go.
    if let Ok(file) = std::fs::File::options().write(true).open(&path) {
        let _ = file.set_modified(SystemTime::now());
    }
    Some(body.to_vec())
}

fn parse(data: &[u8]) -> Option<(&str, u64, &[u8])> {
    let mut rest = data;
    let mut line = || {
        let end = rest.iter().position(|&b| b == b'\n')?;
        let (head, tail) = rest.split_at(end);
        rest = &tail[1..];
        std::str::from_utf8(head).ok()
    };
    if line()? != MAGIC {
        return None;
    }
    let expires = line()?.parse().ok()?;
    let url = line()?;
    Some((url, expires, rest))
}

/// How long an answer with this `Cache-Control` may be kept: `None` for not
/// at all.
pub fn life(cache_control: Option<&str>) -> Option<Duration> {
    let Some(header) = cache_control else { return Some(DEFAULT_LIFE) };
    let mut life = DEFAULT_LIFE;
    for part in header.split(',').map(|p| p.trim().to_ascii_lowercase()) {
        if part == "no-store" || part == "no-cache" {
            return None;
        }
        if let Some(secs) = part.strip_prefix("max-age=") {
            let secs: u64 = secs.trim_matches('"').parse().ok()?;
            if secs == 0 {
                return None;
            }
            life = Duration::from_secs(secs);
        }
    }
    Some(life)
}

/// Keep `bytes` for `url` for `life`.
pub fn write(url: &str, bytes: &[u8], life: Duration) {
    let (dir, cap) = {
        let guard = DISK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(disk) = guard.as_ref() else { return };
        (disk.dir.clone(), disk.cap)
    };
    // One answer that would take a quarter of the whole is not kept.
    if bytes.len() as u64 > cap / 4 || url.contains('\n') {
        return;
    }
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = file_for(&dir, url);
    let expires = now().saturating_add(life.as_secs());
    let mut data = format!("{MAGIC}\n{expires}\n{url}\n").into_bytes();
    data.extend_from_slice(bytes);
    // Beside it, then over it: a reader never sees half a file.
    let part = path.with_extension("part");
    let replaced = std::fs::metadata(&path).map_or(0, |m| m.len());
    if std::fs::write(&part, &data).is_err() || std::fs::rename(&part, &path).is_err() {
        let _ = std::fs::remove_file(&part);
        return;
    }
    let over = {
        let mut guard = DISK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(disk) = guard.as_mut() else { return };
        let held = match disk.held {
            Some(h) => h.saturating_sub(replaced) + data.len() as u64,
            None => folder_size(&dir),
        };
        disk.held = Some(held);
        held > cap
    };
    if over {
        trim(&dir, cap);
    }
}

fn forget_size(bytes: u64) {
    if let Some(disk) = DISK.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        disk.held = disk.held.map(|h| h.saturating_sub(bytes));
    }
}

fn folder_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|it| it.flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum())
        .unwrap_or(0)
}

/// Remove the files touched longest ago until nine tenths of `cap` is left.
fn trim(dir: &Path, cap: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "img"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().unwrap_or(UNIX_EPOCH), m.len(), e.path()))
        })
        .collect();
    let mut held: u64 = files.iter().map(|f| f.1).sum();
    files.sort();
    let goal = cap / 10 * 9;
    for (_, len, path) in files {
        if held <= goal {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            held -= len;
        }
    }
    if let Some(disk) = DISK.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        disk.held = Some(held);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test, since the folder is the process's: kept and read back, a
    /// lie about the URL refused, run out, `no-store`, and the cap.
    #[test]
    fn images_are_kept_read_back_run_out_and_capped() {
        let dir = std::env::temp_dir().join(format!("rux_disk_cache_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        set_dir_with_cap(dir.clone(), 1000);

        write("https://t.test/a.png", &[1, 2, 3], Duration::from_secs(60));
        assert_eq!(read("https://t.test/a.png"), Some(vec![1, 2, 3]));
        assert_eq!(read("https://t.test/b.png"), None);

        // A file that holds another URL is not this one's.
        let path = file_for(&dir, "https://t.test/c.png");
        std::fs::write(&path, format!("{MAGIC}\n{}\nhttps://t.test/other\nxyz", now() + 60)).unwrap();
        assert_eq!(read("https://t.test/c.png"), None);

        // Run out: a miss, and the file goes.
        let path = file_for(&dir, "https://t.test/old.png");
        std::fs::write(&path, format!("{MAGIC}\n{}\nhttps://t.test/old.png\nxyz", now() - 1)).unwrap();
        assert_eq!(read("https://t.test/old.png"), None);
        assert!(!path.exists());

        // Over the cap, the ones touched longest ago go.
        let _ = std::fs::remove_dir_all(&dir);
        set_dir_with_cap(dir.clone(), 1000);
        for i in 0..6 {
            write(&format!("https://t.test/{i}"), &[7; 200], Duration::from_secs(60));
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(folder_size(&dir) <= 1000, "{}", folder_size(&dir));
        assert_eq!(read("https://t.test/0"), None, "the oldest went");
        assert_eq!(read("https://t.test/5"), Some(vec![7; 200]), "the newest stayed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_control_says_how_long() {
        assert_eq!(life(None), Some(DEFAULT_LIFE));
        assert_eq!(life(Some("public, max-age=3600")), Some(Duration::from_secs(3600)));
        assert_eq!(life(Some("no-store")), None);
        assert_eq!(life(Some("private, no-cache")), None);
        assert_eq!(life(Some("max-age=0")), None);
        assert_eq!(life(Some("public")), Some(DEFAULT_LIFE));
    }
}
