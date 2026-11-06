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

/// Check every `window.open(...)` written in `script` (the text of a `<script>`
/// whose first line is `script_line` of its file), against the app whose
/// folder is `root`. Findings are raised on the line of the call.
pub(super) fn check(script: &Script, text: &str, script_line: usize, root: &Path) {
    let lines = rux_syntax::LineIndex::new(text);
    rux_syntax::visit::exprs(script, &mut |e| {
        if let ExprKind::Method { recv, name, args, .. } = &e.kind {
            if name.name == "open" && matches!(&recv.kind, ExprKind::Var(v) if v == "window") {
                let line = lines.line_col(text, e.span.start as usize).0 + script_line - 1;
                for (message, is_error) in findings(args, root) {
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
