//! `rux check` for `window.open`: what can be known of a call from how it is
//! written. A path written out as a string is looked up (the file exists, and
//! is a window); the props written out beside it are held to what that file
//! declares; an option the call does not take is named. A path that is not
//! written out cannot be looked up, and says so.
//!
//! The same messages come back at run time when a window fails to load
//! ([`super::window_own_props`]), so an author meets the one wording either way.

use std::path::Path;

use rux_reactive::Value;
use rux_syntax::ast::{Expr, ExprKind, Script};

use super::{extract_imports, extract_props, source, value_fits, value_kind};

/// The options `window.open` takes, in the order the message lists them.
pub const OPTIONS: &[&str] = &[
    "title",
    "width",
    "height",
    "minWidth",
    "minHeight",
    "maximized",
    "decorations",
    "titleBar",
    "resizable",
    "owned",
    "props",
    "onClose",
];

/// The options `window.openRemote` takes: those of `window.open` without
/// `props` (a remote file's props cannot be checked, and what goes in is a
/// trust question of its own), plus the grants and the failure callback.
pub const REMOTE_OPTIONS: &[&str] = &[
    "title",
    "width",
    "height",
    "minWidth",
    "minHeight",
    "maximized",
    "decorations",
    "titleBar",
    "resizable",
    "owned",
    "sandbox",
    "onClose",
    "onError",
];

/// Why `grant` may not be handed to a remote document, when it may not. The
/// stores of `settings` and `secrets` are the app's own, and a remote file
/// reading or writing them is the grant that is dangerous to give whole.
/// Lifted when a document's stores are kept apart.
pub fn refused_for_remote(grant: &str) -> Option<String> {
    let module = grant.split(['.', '@']).next().unwrap_or(grant);
    matches!(module, "settings" | "secrets").then(|| {
        format!(
            "`{grant}` cannot be granted to a remote document: `{module}` is the app's own, and a document from elsewhere would read and write it"
        )
    })
}

/// The terms a remote document opens on, from the grants its opener wrote and
/// the terms the opener itself runs on (`None` for an app's own code). Each
/// grant has to be a known one and not one of the app's own stores
/// ([`refused_for_remote`]); a sandboxed opener may hand out only what it
/// holds, a name exactly or its module granted whole, and its limits go on to
/// the document. `Err` says which grant and why.
pub fn remote_terms(opener: Option<&crate::Sandbox>, grants: &[String]) -> Result<crate::Sandbox, String> {
    let mut terms = crate::Sandbox::new();
    if let Some(opener) = opener {
        terms.limits = opener.limits.clone();
    }
    for grant in grants {
        crate::Sandbox::check(grant).map_err(|why| format!("window.openRemote cannot grant `{grant}`: {why}"))?;
        if let Some(why) = refused_for_remote(grant) {
            return Err(why);
        }
        if let Some(opener) = opener {
            let module = grant.split(['.', '@']).next().unwrap_or(grant);
            if !(opener.grants.contains(grant) || opener.grants(module)) {
                return Err(format!(
                    "window.openRemote cannot grant `{grant}`: the document that opens it was not granted it, and an opener hands out no more than it holds"
                ));
            }
        }
        terms = terms.grant(grant);
    }
    Ok(terms)
}

/// Check every `window.open(...)` written in `script` (the text of a `<script>`
/// whose first line is `script_line` of its file), against the app whose
/// folder is `root`. Findings are raised on the line of the call.
pub(super) fn check(script: &Script, text: &str, script_line: usize, root: &Path) {
    let lines = rux_syntax::LineIndex::new(text);
    rux_syntax::visit::exprs(script, &mut |e| {
        if let ExprKind::Method { recv, name, args, .. } = &e.kind {
            let remote = name.name == "openRemote";
            if (name.name == "open" || remote) && matches!(&recv.kind, ExprKind::Var(v) if v == "window") {
                let line = lines.line_col(text, e.span.start as usize).0 + script_line - 1;
                let found = if remote { remote_findings(args) } else { findings(args, root) };
                for (message, is_error) in found {
                    rux_script::located(Some(line), || {
                        if is_error {
                            rux_script::error_script(message);
                        } else {
                            rux_script::warn_script(message);
                        }
                    });
                }
            }
        }
        true
    });
}

/// What is wrong with one call's arguments, and whether it is an error.
fn findings(args: &[Expr], root: &Path) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let options = args.get(1).and_then(|a| match &a.kind {
        ExprKind::Map { entries, .. } => Some(entries),
        _ => None,
    });
    if let Some(entries) = options {
        for entry in entries {
            if !OPTIONS.contains(&entry.key.name.as_str()) {
                out.push((
                    format!("window.open has no option `{}`: it takes {}", entry.key.name, OPTIONS.join(", ")),
                    true,
                ));
            }
        }
    }
    let Some(first) = args.first() else {
        out.push(("window.open takes the path of a .rux file first".to_string(), true));
        return out;
    };
    let ExprKind::Str(path) = &first.kind else {
        out.push((
            "window.open is given a path that is not written out, so the file and its props are not checked: \
             write the path as a string to have them checked"
                .to_string(),
            false,
        ));
        return out;
    };
    if path.contains("://") {
        out.push((
            format!(
                "window.open(\"{path}\") is given an address: `window.open` opens the app's own files, and a document from an address is opened with `window.openRemote(\"{path}\", {{ ... }})`"
            ),
            true,
        ));
        return out;
    }
    let file = root.join(path);
    if !source::exists(&file) {
        out.push((
            format!(
                "window.open(\"{path}\") names a file that is not there: write the path from the app's folder, \
                 as in `windows/inspector.rux`"
            ),
            true,
        ));
        return out;
    }
    let Ok(text) = source::read_text(&file) else { return out };
    let sfc = match rux_parser::parse_sfc(&text) {
        Ok(sfc) => sfc,
        Err(e) => {
            out.push((format!("window.open(\"{path}\"): {path} does not parse: {}", e.message), true));
            return out;
        }
    };
    if sfc.template.tag != "screen" {
        out.push((
            format!(
                "window.open(\"{path}\"): a window's file has a `<screen>` as its root, and {path} has `<{}>`",
                sfc.template.tag
            ),
            true,
        ));
        return out;
    }
    // The props written out, when `props` is a map written out.
    let given: Option<Vec<(String, Option<Value>)>> = match options.and_then(|e| e.iter().find(|e| e.key.name == "props")) {
        None => Some(Vec::new()),
        Some(entry) => match &entry.value.kind {
            ExprKind::Map { entries, .. } => {
                Some(entries.iter().map(|e| (e.key.name.clone(), literal(&e.value))).collect())
            }
            _ => None,
        },
    };
    let Some(given) = given else { return out };
    let (script, _) = extract_imports(&sfc.script);
    let (_, declared, _) = extract_props(&script, false);
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let declares = || {
        if declared.is_empty() {
            "it declares no props".to_string()
        } else {
            format!("it declares {}", declared.iter().map(|p| format!("`{}`", p.name)).collect::<Vec<_>>().join(", "))
        }
    };
    for (key, _) in &given {
        if !declared.iter().any(|p| p.name == *key) {
            out.push((format!("window.open passed the prop `{key}`, and {name} does not declare it: {}", declares()), true));
        }
    }
    for prop in &declared {
        match given.iter().find(|(k, _)| *k == prop.name) {
            Some((_, Some(value))) => {
                if let Some(Ok(ty)) = prop.ty.as_deref().map(rux_script::types::parse_type) {
                    if !value_fits(&ty, value) {
                        out.push((
                            format!(
                                "window.open passed the prop `{}` {}, and {name} declares it `{ty}`",
                                prop.name,
                                value_kind(value)
                            ),
                            true,
                        ));
                    }
                }
            }
            // Passed, but not written out: only the run can say what it is.
            Some((_, None)) => {}
            None if prop.default.is_none() => out.push((
                format!(
                    "{name} needs the prop `{0}`: pass it in `props`, as in `window.open(\"{path}\", {{ props: {{ {0}: … }} }})`",
                    prop.name
                ),
                true,
            )),
            None => {}
        }
    }
    out
}

/// What is wrong with one `window.openRemote` call, and whether it is an
/// error: the address, the options, and the grants, as far as they are
/// written out.
fn remote_findings(args: &[Expr]) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let options = args.get(1).and_then(|a| match &a.kind {
        ExprKind::Map { entries, .. } => Some(entries),
        _ => None,
    });
    if let Some(entries) = options {
        for entry in entries {
            if !REMOTE_OPTIONS.contains(&entry.key.name.as_str()) {
                out.push((
                    format!(
                        "window.openRemote has no option `{}`: it takes {}",
                        entry.key.name,
                        REMOTE_OPTIONS.join(", ")
                    ),
                    true,
                ));
            }
            if entry.key.name == "sandbox" {
                if let ExprKind::Array(items) = &entry.value.kind {
                    for item in items {
                        let ExprKind::Str(grant) = &item.kind else { continue };
                        if let Err(why) = crate::Sandbox::check(grant) {
                            out.push((format!("window.openRemote cannot grant `{grant}`: {why}"), true));
                        } else if let Some(why) = refused_for_remote(grant) {
                            out.push((why, true));
                        }
                    }
                }
            }
        }
    }
    let Some(first) = args.first() else {
        out.push(("window.openRemote takes the address of a .rux file first".to_string(), true));
        return out;
    };
    match &first.kind {
        ExprKind::Str(url) => {
            if let Err(why) = crate::fetch::check_url(url) {
                out.push((format!("window.openRemote(\"{url}\"): {why}"), true));
            }
        }
        _ => out.push((
            "window.openRemote is given an address that is not written out, so it is not checked: write the address as a string to have it checked"
                .to_string(),
            false,
        )),
    }
    out
}

/// An expression written as a plain value, or `None` when it is anything that
/// has to be run to know.
fn literal(e: &Expr) -> Option<Value> {
    Some(match &e.kind {
        ExprKind::Null => Value::Null,
        ExprKind::Bool(b) => Value::Bool(*b),
        ExprKind::Int(i) => Value::Number(*i as f64),
        ExprKind::Float(f) => Value::Number(*f),
        ExprKind::Str(s) => Value::Text(s.clone()),
        ExprKind::Unary { op: "-", expr } => match literal(expr)? {
            Value::Number(n) => Value::Number(-n),
            _ => return None,
        },
        ExprKind::Array(items) => Value::list(items.iter().map(literal).collect::<Option<Vec<_>>>()?),
        ExprKind::Map { entries, .. } => {
            Value::Map(entries.iter().map(|e| Some((e.key.name.clone(), literal(&e.value)?))).collect::<Option<Vec<_>>>()?)
        }
        _ => return None,
    })
}
