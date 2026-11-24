//! `network.online` and `rux::network` (UD-003 piece 5): whether the device
//! has a connection, as the platform reports it.
//!
//! Android: `ConnectivityManager`'s default-network callback, registered by
//! the activity, which reports through `nativeNetwork`. The web: the page's
//! `online` and `offline` events. The desktop cannot say, and reports online.

use std::collections::HashMap;

use rux_runtime::native::network::{set_online, ONLINE_SIGNAL};
use rux_runtime::native::{install, Any, Call, Module};

/// Install `network.online` for every document this process loads.
pub(crate) fn install_network() {
    let mut calls = HashMap::new();
    calls.insert("online".to_string(), Call::sync(|_args: Vec<Any>| ONLINE_SIGNAL.__read()));
    let _ = install(Module { interface: rux_runtime::builtins::network_interface(), calls });
    // The first report fills the signal; the platform's own follow.
    if ONLINE_SIGNAL.get().is_none() {
        set_online(true);
    }
    #[cfg(target_arch = "wasm32")]
    watch_connection();
}

/// A page is offline when the browser says so (`navigator.onLine`, and the
/// `online` and `offline` events).
#[cfg(target_arch = "wasm32")]
fn watch_connection() {
    let report = wasm_bindgen::closure::Closure::<dyn FnMut(bool)>::new(|online: bool| set_online(online));
    let hook = js_sys::Function::new_with_args(
        "report",
        "const now = () => report(navigator.onLine !== false);
         window.addEventListener('online', now);
         window.addEventListener('offline', now);
         now();",
    );
    let _ = hook.call1(&wasm_bindgen::JsValue::NULL, report.as_ref());
    // Lives as long as the page does.
    report.forget();
}
