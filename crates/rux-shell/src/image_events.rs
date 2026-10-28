//! `<image>`'s `@load` and `@error`.
//!
//! Found the way `<audio>`'s events are (`crate::media`): after every wake
//! the shell looks at each image that listens, and fires once for each `src`
//! when its picture is there or cannot be. A file on the disk is there at the
//! first look; one from the network when it comes; a new `src` fires again,
//! as a browser's `load` does.

use std::collections::HashMap;
use std::sync::Mutex;

use rux_layout::{ImageEvents, Node as LayoutNode};
use rux_reactive::Value;
use rux_runtime::ImageState;

/// Per element key, the `src` its event was given for.
static FIRED: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn walk<'a>(node: &'a LayoutNode, out: &mut Vec<(&'a str, &'a ImageEvents, Option<&'a str>)>) {
    if let Some(image) = &node.image {
        if let Some(events) = image.events.as_deref() {
            out.push((&image.src, events, node.instance.as_deref()));
        }
    }
    for c in &node.children {
        walk(c, out);
    }
}

/// The handlers to run for the images in `root` whose picture came or
/// failed since the last look, with their instance and `event`.
pub(crate) fn sync(root: &LayoutNode) -> Vec<(String, Option<String>, Value)> {
    let mut found = Vec::new();
    walk(root, &mut found);
    let mut out = Vec::new();
    let mut fired = FIRED.lock().unwrap_or_else(|e| e.into_inner());
    let fired = fired.get_or_insert_with(HashMap::new);
    if found.is_empty() {
        fired.clear();
        return out;
    }
    let mut seen = Vec::with_capacity(found.len());
    for (src, events, instance) in found {
        seen.push(events.key.as_str());
        if fired.get(&events.key).is_some_and(|s| s == src) {
            continue;
        }
        let (handler, event) = match rux_runtime::image_state(src) {
            ImageState::Loading => continue,
            ImageState::Ready(w, h) => (
                &events.on[0],
                Value::Map(vec![
                    ("width".to_string(), Value::Number(f64::from(w))),
                    ("height".to_string(), Value::Number(f64::from(h))),
                ]),
            ),
            ImageState::Failed(why) => (&events.on[1], Value::Map(vec![("message".to_string(), Value::Text(why))])),
        };
        fired.insert(events.key.clone(), src.to_string());
        if let Some(body) = handler {
            out.push((body.clone(), instance.map(str::to_string), event));
        }
    }
    // An image that left the tree fires again if it comes back.
    fired.retain(|key, _| seen.contains(&key.as_str()));
    out
}
