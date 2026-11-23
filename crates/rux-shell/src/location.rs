//! Where the device is: `location.current`, `location.watch`, `location.stop`
//! and the signals `location.position` and `location.watching`, and the same
//! calls for the app's own Rust (`rux::location`).
//!
//! The checks every platform shares come first: `[permissions.location]` is
//! declared (else `undeclared`, naming the table to add). Whether the person
//! allowed it is the platform's to check, since only it can say.
//!
//! The platforms: Android (the platform's `LocationManager`) and the web
//! (`navigator.geolocation`). The desktop throws `unsupported`.

use std::collections::{BTreeMap, HashMap};

use rux_runtime::native::location::{
    set_backend, set_watching, Accuracy, Answer, Backend, Fix, Options, POSITION_SIGNAL, WATCHING_SIGNAL,
};
use rux_runtime::native::{install, Any, Call, Error, IntoRux, Module};

fn option_error(message: String) -> Error {
    Error::new("option", message)
}

/// A call's options map. `call` names it in messages; `allowed` are its keys.
fn options_from(call: &str, args: Vec<Any>, allowed: &[&str]) -> Result<Options, Error> {
    let map: BTreeMap<String, Any> = match args.into_iter().next() {
        Some(Any::Map(m)) => m,
        Some(Any::None) | None => BTreeMap::new(),
        Some(other) => {
            return Err(option_error(format!("location.{call} takes its options as a map, not a {}", other.kind())))
        }
    };
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(option_error(format!(
                "location.{call} has no option `{key}`. Its options are {}",
                allowed.join(", ")
            )));
        }
    }
    let number = |key: &str| -> Result<Option<f64>, Error> {
        match map.get(key) {
            Some(Any::Int(n)) if *n >= 0 => Ok(Some(*n as f64)),
            Some(Any::Float(f)) if *f >= 0.0 && f.is_finite() => Ok(Some(*f)),
            Some(Any::None) | None => Ok(None),
            Some(Any::Int(_) | Any::Float(_)) => Err(option_error(format!("`{key}` cannot be negative"))),
            Some(other) => Err(option_error(format!("`{key}` takes a number, not a {}", other.kind()))),
        }
    };
    let mut options = Options::new();
    match map.get("accuracy") {
        Some(Any::Str(s)) if s == "high" => options.accuracy = Accuracy::High,
        Some(Any::Str(s)) if s == "balanced" => options.accuracy = Accuracy::Balanced,
        Some(Any::None) | None => {}
        Some(other) => {
            let said = match other {
                Any::Str(s) => format!("\"{s}\""),
                other => format!("a {}", other.kind()),
            };
            return Err(option_error(format!("`accuracy` is \"high\" or \"balanced\", not {said}")));
        }
    }
    if let Some(ms) = number("timeoutMs")? {
        options.timeout_ms = ms as u64;
    }
    if let Some(ms) = number("intervalMs")? {
        options.interval_ms = ms as u64;
    }
    if let Some(m) = number("distanceM")? {
        options.distance_m = m;
    }
    Ok(options)
}

/// The declaration check every platform shares.
fn checked() -> Result<(), Error> {
    crate::permissions::ensure_declared("location")
}

async fn current(options: Options) -> Result<Fix, Error> {
    checked()?;
    platform::current(options).await
}

fn watch(options: Options) -> Result<(), Error> {
    checked()?;
    platform::watch(options)
}

fn stop() -> Result<(), Error> {
    platform::stop()
}

/// Install the calls for every document this process loads, and the same
/// three for the app's own Rust (`rux::location`).
pub(crate) fn install_location() {
    set_backend(Backend { current: |options| -> Answer { Box::pin(current(options)) }, watch, stop });
    let mut calls = HashMap::new();
    calls.insert(
        "current".to_string(),
        Call::future(|args: Vec<Any>| async move {
            let options = options_from("current", args, &["accuracy", "timeoutMs"])?;
            current(options).await?.into_rux()
        }),
    );
    calls.insert(
        "watch".to_string(),
        Call::sync(|args: Vec<Any>| {
            watch(options_from("watch", args, &["accuracy", "intervalMs", "distanceM"])?)?;
            Ok(Any::None)
        }),
    );
    calls.insert("stop".to_string(), Call::sync(|_args: Vec<Any>| stop().map(|()| Any::None)));
    calls.insert("position".to_string(), Call::sync(|_args: Vec<Any>| POSITION_SIGNAL.__read()));
    calls.insert("watching".to_string(), Call::sync(|_args: Vec<Any>| WATCHING_SIGNAL.__read()));
    let _ = install(Module { interface: rux_runtime::builtins::location_interface(), calls });
    // Not watching at start-up. Only the first time: a second start of the
    // shell must not report a running watch as stopped.
    if WATCHING_SIGNAL.get().is_none() {
        set_watching(false);
    }
}

#[cfg(target_os = "android")]
mod platform {
    use super::*;
    use crate::files::Slot;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Mutex;

    const FINE: &str = "android.permission.ACCESS_FINE_LOCATION";
    const COARSE: &str = "android.permission.ACCESS_COARSE_LOCATION";

    /// The one-fix requests waiting on Android, by id.
    static PENDING: Mutex<Option<HashMap<i64, Slot>>> = Mutex::new(None);
    static NEXT: AtomicI64 = AtomicI64::new(1);

    /// `Ok(high)` when location is allowed: whether GPS may be asked, which an
    /// approximate-only grant forbids.
    fn allowed(options: &Options) -> Result<bool, Error> {
        let granted = |name: &str| crate::android_permission_flag("granted", name);
        if !(granted(FINE) || granted(COARSE)) {
            return Err(Error::new(
                "permission",
                "location is not allowed: ask with `permissions.request(\"location\")` from a tap first",
            ));
        }
        Ok(options.accuracy == Accuracy::High && granted(FINE))
    }

    /// Java says `kind: why`.
    fn from_java(why: String) -> Error {
        match why.split_once(": ") {
            Some((kind @ ("disabled" | "permission" | "timeout" | "location"), message)) => Error::new(kind, message),
            _ => Error::new("location", why),
        }
    }

    pub(super) async fn current(options: Options) -> Result<Fix, Error> {
        let high = allowed(&options)?;
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let slot = Slot::default();
        PENDING.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(id, slot.clone());
        if let Err(why) = crate::android_location_current(id, high, options.timeout_ms) {
            PENDING.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).remove(&id);
            return Err(from_java(why));
        }
        match slot.await? {
            Any::Map(m) => Ok(fix_of(&m)),
            _ => Err(Error::new("location", "Android gave no position")),
        }
    }

    fn fix_of(m: &BTreeMap<String, Any>) -> Fix {
        let f = |k: &str| match m.get(k) {
            Some(Any::Float(v)) => *v,
            _ => 0.0,
        };
        Fix {
            lat: f("lat"),
            lng: f("lng"),
            accuracy_m: f("accuracyM"),
            at: match m.get("at") {
                Some(Any::Int(v)) => *v,
                _ => 0,
            },
            is_mock: matches!(m.get("isMock"), Some(Any::Bool(true))),
        }
    }

    pub(super) fn watch(options: Options) -> Result<(), Error> {
        let high = allowed(&options)?;
        crate::android_location_watch(high, options.interval_ms, options.distance_m as f32).map_err(from_java)?;
        set_watching(true);
        Ok(())
    }

    pub(super) fn stop() -> Result<(), Error> {
        crate::android_location_stop();
        set_watching(false);
        Ok(())
    }

    /// Java delivered a position: to the request waiting on `id`, or, for the watch (0), to everyone.
    pub(crate) fn fix_arrived(id: i64, fix: Fix) {
        if id == 0 {
            rux_runtime::native::location::report(fix);
            return;
        }
        let slot = PENDING.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|p| p.remove(&id));
        if let Some(slot) = slot {
            let mut m = BTreeMap::new();
            m.insert("lat".to_string(), Any::Float(fix.lat));
            m.insert("lng".to_string(), Any::Float(fix.lng));
            m.insert("accuracyM".to_string(), Any::Float(fix.accuracy_m));
            m.insert("at".to_string(), Any::Int(fix.at));
            m.insert("isMock".to_string(), Any::Bool(fix.is_mock));
            slot.set(Ok(Any::Map(m)));
        }
    }

    /// Java could not deliver one.
    pub(crate) fn error_arrived(id: i64, kind: &str, why: &str) {
        if id == 0 {
            crate::android_log(&format!("location watch: {kind}: {why}"));
            return;
        }
        let slot = PENDING.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|p| p.remove(&id));
        if let Some(slot) = slot {
            slot.set(Err(from_java(format!("{kind}: {why}"))));
        }
    }
}

#[cfg(target_os = "android")]
pub(crate) use platform::{error_arrived, fix_arrived};

#[cfg(target_arch = "wasm32")]
mod platform {
    use super::*;
    use crate::files::Slot;

    /// One fix as the browser's `getCurrentPosition` gives it, answered as
    /// `"lat,lng,accuracy,timestamp"`; a refusal rejects with the kind.
    const CURRENT: &str = "return new Promise((ok, no) => {
        if (!navigator.geolocation) { no('unsupported'); return; }
        navigator.geolocation.getCurrentPosition(
            (p) => ok([p.coords.latitude, p.coords.longitude, p.coords.accuracy, p.timestamp].join(',')),
            (e) => no(['permission', 'location', 'timeout'][e.code - 1] || 'location'),
            { enableHighAccuracy: high, timeout: timeoutMs, maximumAge: 0 });
    });";

    /// `watchPosition`, replacing a running watch. A browser takes no interval
    /// or distance: it delivers as the device does. A refusal is not reported
    /// here (the watch just never fires), as the first `current` would have been.
    const WATCH: &str = "if (!navigator.geolocation) { return 'unsupported'; }
        if (window.__ruxGeo != null) { navigator.geolocation.clearWatch(window.__ruxGeo); }
        window.__ruxGeo = navigator.geolocation.watchPosition(
            (p) => fix(p.coords.latitude, p.coords.longitude, p.coords.accuracy, p.timestamp),
            (e) => { console.warn('location watch: ' + e.message); },
            { enableHighAccuracy: high, maximumAge: 0 });
        return 'ok';";

    const STOP: &str = "if (navigator.geolocation && window.__ruxGeo != null) { navigator.geolocation.clearWatch(window.__ruxGeo); }
        window.__ruxGeo = null; return 'ok';";

    thread_local! {
        /// The one callback every watch fix calls, made once.
        static FIX: wasm_bindgen::JsValue = wasm_bindgen::closure::Closure::<dyn FnMut(f64, f64, f64, f64)>::new(
            |lat: f64, lng: f64, accuracy: f64, at: f64| {
                rux_runtime::native::location::report(Fix { lat, lng, accuracy_m: accuracy, at: at as i64, is_mock: false });
            },
        )
        .into_js_value();
    }

    fn unsupported() -> Error {
        Error::new("unsupported", "this browser has no geolocation")
    }

    pub(super) async fn current(options: Options) -> Result<Fix, Error> {
        use wasm_bindgen::JsValue as J;
        let slot = Slot::default();
        let answer = slot.clone();
        let f = js_sys::Function::new_with_args("high,timeoutMs", CURRENT);
        let promise = f.call2(&J::NULL, &J::from_bool(options.accuracy == Accuracy::High), &J::from_f64(options.timeout_ms as f64));
        wasm_bindgen_futures::spawn_local(async move {
            let result = match promise {
                Ok(p) => wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(p)).await,
                Err(e) => Err(e),
            };
            answer.set(match result {
                Ok(v) => Ok(Any::Str(v.as_string().unwrap_or_default())),
                Err(e) => Err(match e.as_string().as_deref() {
                    Some("permission") => Error::new(
                        "permission",
                        "the browser did not allow location: the person refused, or the page is not on https",
                    ),
                    Some("timeout") => Error::new("timeout", "no position arrived in time"),
                    Some("unsupported") => unsupported(),
                    _ => Error::new("location", "the browser could not find the device's position"),
                }),
            });
        });
        let Any::Str(text) = slot.await? else { return Err(Error::new("location", "the browser gave no position")) };
        let n: Vec<f64> = text.split(',').filter_map(|v| v.parse().ok()).collect();
        match n.as_slice() {
            [lat, lng, accuracy, at] => Ok(Fix { lat: *lat, lng: *lng, accuracy_m: *accuracy, at: *at as i64, is_mock: false }),
            _ => Err(Error::new("location", "the browser gave an unreadable position")),
        }
    }

    pub(super) fn watch(options: Options) -> Result<(), Error> {
        use wasm_bindgen::JsValue as J;
        let f = js_sys::Function::new_with_args("high,fix", WATCH);
        let fix = FIX.with(|c| c.clone());
        let answer = f
            .call2(&J::NULL, &J::from_bool(options.accuracy == Accuracy::High), &fix)
            .map_err(|_| unsupported())?;
        if answer.as_string().as_deref() != Some("ok") {
            return Err(unsupported());
        }
        set_watching(true);
        Ok(())
    }

    pub(super) fn stop() -> Result<(), Error> {
        let _ = js_sys::Function::new_no_args(STOP).call0(&wasm_bindgen::JsValue::NULL);
        set_watching(false);
        Ok(())
    }
}

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod platform {
    use super::*;

    fn unsupported() -> Error {
        Error::new("unsupported", "this platform has no location yet: Android and the web do")
    }

    pub(super) async fn current(_options: Options) -> Result<Fix, Error> {
        Err(unsupported())
    }

    pub(super) fn watch(_options: Options) -> Result<(), Error> {
        Err(unsupported())
    }

    pub(super) fn stop() -> Result<(), Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, Any)]) -> Vec<Any> {
        vec![Any::Map(pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect())]
    }

    #[test]
    fn options_are_read_with_their_defaults_or_refused_in_their_own_words() {
        let watch_keys = ["accuracy", "intervalMs", "distanceM"];
        let o = options_from("watch", map(&[("intervalMs", Any::Int(15_000)), ("distanceM", Any::Float(5.5))]), &watch_keys)
            .unwrap();
        assert_eq!((o.interval_ms, o.distance_m, o.accuracy), (15_000, 5.5, Accuracy::High));
        let current_keys = ["accuracy", "timeoutMs"];
        let o = options_from("current", vec![], &current_keys).unwrap();
        assert_eq!(o, Options::new(), "no options is the defaults");
        let o = options_from(
            "current",
            map(&[("accuracy", Any::Str("balanced".into())), ("timeoutMs", Any::Int(2_000))]),
            &current_keys,
        )
        .unwrap();
        assert_eq!((o.accuracy, o.timeout_ms), (Accuracy::Balanced, 2_000));

        let bad = |pairs: &[(&str, Any)]| options_from("current", map(pairs), &current_keys).unwrap_err();
        assert_eq!(bad(&[("accuracy", Any::Str("exact".into()))]).kind, "option");
        assert!(bad(&[("accuracy", Any::Str("exact".into()))]).message.contains("\"high\" or \"balanced\""));
        assert!(bad(&[("timeoutMs", Any::Int(-1))]).message.contains("cannot be negative"));
        assert!(bad(&[("timeoutMs", Any::Str("x".into()))]).message.contains("takes a number"));
        assert!(bad(&[("intervalMs", Any::Int(1))]).message.contains("no option `intervalMs`"));
        assert!(options_from("current", vec![Any::Int(3)], &[]).unwrap_err().message.contains("as a map"));
    }
}
