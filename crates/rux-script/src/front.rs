//! The front door: Rux's own parser decides what a script is, and the fork
//! compiles what it lets through.
//!
//! Step 2 of `docs/11-next.md`. Every binding, handler and script reaches the
//! fork through [`compile`], which parses it with `rux-syntax` first. A syntax
//! error is that parser's, in Rux's words, at the line and column it names.
//! What passes is handed to the fork as text, rewritten only where Rux has
//! syntax the fork does not (`setInterval(ms) { … }`), until Rux's own
//! interpreter replaces the fork in step 5.
//!
//! **Debug builds check the two parsers agree on every compile.** The Rux AST
//! is printed back with every group made explicit, the fork compiles that too,
//! and the two fork ASTs are compared with positions removed. If Rux read
//! `a ?? 0 == 1` one way and the fork another, the trees differ and the build
//! panics naming the source. A script Rux refuses is checked the other way:
//! the fork must refuse it as well. So every script the test suite compiles,
//! which is every example, recipe and `/learn` chapter, is a case of the
//! differential test, without a corpus file to keep current.

use rhai::{Engine as RhaiEngine, Position, AST};
use rux_syntax::ast::{Expr, ExprKind, Script};
use rux_syntax::{Options, Span};

use crate::profile;

/// A script that did not compile, from either parser.
#[derive(Clone, Debug)]
pub(crate) struct CompileError {
    pub message: String,
    pub position: Position,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Parse `src` as Rux, then compile what it says with the fork.
pub(crate) fn compile(engine: &RhaiEngine, src: &str) -> Result<AST, CompileError> {
    compile_with(engine, src, Options::default()).map(|(ast, _)| ast)
}

/// [`compile`], for a whole document or component script, where `prop`,
/// `computed` and the lifecycle blocks parse. The runtime has taken each one
/// out by the time a script that parses gets here; this is so a script that
/// does not is reported where it really goes wrong, not at its first `prop`.
/// Rux's own reading of the script comes back too, for the type checker.
pub(crate) fn compile_script_keeping(engine: &RhaiEngine, src: &str) -> Result<(AST, Script), CompileError> {
    compile_with(engine, src, Options { declarations: true })
}

fn compile_with(engine: &RhaiEngine, src: &str, opts: Options) -> Result<(AST, Script), CompileError> {
    let script = match profile::time(profile::Phase::Parse, || rux_syntax::parse(src, opts)) {
        Ok(script) => script,
        Err(e) => {
            #[cfg(debug_assertions)]
            the_fork_refuses_too(engine, src, &e.message);
            let (line, col) = e.line_col(src);
            return Err(CompileError { message: e.message, position: position(line, col) });
        }
    };
    let text = lower(src, &script);
    let ast = profile::time(profile::Phase::Compile, || engine.compile(&text)).map_err(|e| CompileError {
        message: e.to_string(),
        position: e.1,
    })?;
    #[cfg(debug_assertions)]
    agree(src, &text, &script);
    Ok((ast, script))
}

fn position(line: usize, col: usize) -> Position {
    Position::new(line.clamp(1, u16::MAX as usize) as u16, col.min(u16::MAX as usize) as u16)
}

/// The text the fork compiles: `src` with each `setInterval(args) { body }`
/// rewritten to `__interval(args, "body")`, the call it has always been for
/// the fork (see [`crate::TimerRequest`]). A body keeps its newlines as
/// escapes inside the string, and the lines it took are added back after
/// the call, so everything after it is still on the line it was written on.
pub(crate) fn lower(src: &str, script: &Script) -> String {
    let mut edits: Vec<(Span, String)> = Vec::new();
    rux_syntax::visit::exprs(script, &mut |e: &Expr| {
        let ExprKind::Interval { args, body } = &e.kind else { return true };
        let mut out = String::from("__interval(");
        if let (Some(first), Some(last)) = (args.first(), args.last()) {
            out.push_str(src[first.span.start as usize..last.span.end as usize].trim());
            out.push_str(", ");
        }
        out.push('"');
        let inner = &src[body.span.start as usize + 1..body.span.end as usize - 1];
        for c in inner.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                '\n' => out.push_str("\\n"),
                '\r' => {}
                c => out.push(c),
            }
        }
        out.push_str("\")");
        let lines = e.span.text(src).matches('\n').count();
        out.push_str(&"\n".repeat(lines));
        edits.push((e.span, out));
        // A `setInterval` inside the body stays text inside the string, and
        // is rewritten when the body itself is compiled to run.
        false
    });
    if edits.is_empty() {
        return src.to_string();
    }
    let mut out = String::with_capacity(src.len() + 32);
    let mut at = 0;
    for (span, text) in edits {
        out.push_str(&src[at..span.start as usize]);
        out.push_str(&text);
        at = span.end as usize;
    }
    out.push_str(&src[at..]);
    out
}

/// Debug builds: a script Rux refused must be one the fork refuses too.
#[cfg(debug_assertions)]
fn the_fork_refuses_too(engine: &RhaiEngine, src: &str, why: &str) {
    // `setInterval` blocks were never the fork's syntax; the old text rewrite
    // made them so, and there is nothing to compare against.
    if src.contains("setInterval") {
        return;
    }
    if engine.compile(src).is_ok() {
        panic!(
            "rux-syntax refused a script the fork accepts ({why}). Step 2 of docs/11-next.md \
             accepts exactly the fork's language, so this is a parser bug:\n{src}"
        );
    }
}

/// The engine the debug check compiles with: the fork with the syntax Rux
/// registers on it (`null`, `===`, `!==`), nothing optimized away, and no limit
/// on nesting, since the text Rux's reading is printed as puts every operator
/// in parentheses and nests deeper than what an author writes.
#[cfg(debug_assertions)]
fn with_check_engine<T>(f: impl FnOnce(&RhaiEngine) -> T) -> T {
    thread_local! {
        static ENGINE: RhaiEngine = {
            let mut e = RhaiEngine::new();
            e.set_optimization_level(rhai::OptimizationLevel::None);
            e.set_max_expr_depths(0, 0);
            let _ = e.register_custom_operator("===", 90);
            let _ = e.register_custom_operator("!==", 90);
            let _ = e.register_custom_syntax(["null"], false, |_, _| Ok(rhai::Dynamic::UNIT));
            e
        };
    }
    ENGINE.with(f)
}

/// Debug builds: the fork builds the same tree from Rux's reading of `src` as
/// from `text`, what it compiled `src` as.
#[cfg(debug_assertions)]
fn agree(src: &str, text: &str, script: &Script) {
    let canonical = rux_syntax::print::script(script, src);
    let Ok(ast) = with_check_engine(|e| e.compile(text)) else { return };
    let theirs = match with_check_engine(|e| e.compile(&canonical)) {
        Ok(ast) => ast,
        Err(e) => panic!(
            "the fork refuses rux-syntax's reading of a script it accepts ({e}); a parser bug.\n\
             source:\n{src}\nread as:\n{canonical}"
        ),
    };
    let (a, b) = (normal::tree(&ast), normal::tree(&theirs));
    if a != b {
        let at = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
        let from = at.saturating_sub(300);
        panic!(
            "rux-syntax and the fork read a script differently; a parser bug.\n\
             source:\n{src}\nread as:\n{canonical}\n\
             the fork's tree from the source, where they part:\n…{}\n\
             and from rux-syntax's reading:\n…{}",
            a.get(from..(at + 300).min(a.len())).unwrap_or(&a),
            b.get(from..(at + 300).min(b.len())).unwrap_or(&b),
        );
    }
    let (a, b) = (normal::annotations(&ast), normal::annotations(&theirs));
    if a != b {
        panic!(
            "rux-syntax and the fork read a script's annotations differently; a parser bug.\n\
             source:\n{src}\nread as:\n{canonical}\nfrom the source: {a:?}\nfrom the reading: {b:?}"
        );
    }
}

/// A fork AST as text with its positions taken out, for comparing two
/// parses of one script written two ways.
#[cfg(debug_assertions)]
mod normal {
    use std::collections::HashMap;

    use rhai::AST;

    /// The statements and the named functions, positions removed. A closure
    /// is named by a hash of its body, positions included, so its name
    /// differs between two writings of the same script; each reference to
    /// one is replaced by its body instead.
    pub fn tree(ast: &AST) -> String {
        let mut anon: HashMap<String, String> = HashMap::new();
        let mut named: Vec<String> = Vec::new();
        for def in ast.iter_fn_def() {
            let body = strip(&format!("{:?}", def.body));
            let params: Vec<&str> = def.params.iter().map(|p| p.as_str()).collect();
            if def.name.starts_with("anon$") {
                anon.insert(def.name.to_string(), format!("|{}| {body}", params.join(",")));
            } else {
                named.push(format!("fn {}({}) {body}", def.name, params.join(",")));
            }
        }
        named.sort();
        let body = strip(&format!("{:?}", ast.statements()));
        let mut out = inline(&body, &anon, 0);
        for f in named {
            out.push('\n');
            out.push_str(&inline(&f, &anon, 0));
        }
        out
    }

    /// Every annotation as kind, name and type, the type read into
    /// `rux-script`'s own `Type` so spelling (`| A | B` against `A | B`)
    /// does not count.
    pub fn annotations(ast: &AST) -> Vec<String> {
        let mut out: Vec<String> = ast
            .annotations()
            .iter()
            .map(|a| {
                let kind = match &a.kind {
                    rhai::AnnotationKind::Param { function, arity } if function.starts_with("anon$") => {
                        format!("param of a closure/{arity}")
                    }
                    other => format!("{other:?}"),
                };
                let ty = crate::types::parse_type(&a.ty).map(|t| t.to_string()).unwrap_or_else(|_| a.ty.clone());
                format!("{kind} {} {ty}", a.name)
            })
            .collect();
        out.sort();
        out
    }

    fn inline(text: &str, anon: &HashMap<String, String>, depth: usize) -> String {
        if depth > 16 || !text.contains("anon$") {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(i) = rest.find("anon$") {
            out.push_str(&rest[..i]);
            let tail = &rest[i..];
            let end = 5 + tail[5..].find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(tail.len() - 5);
            let name = &tail[..end];
            match anon.get(name) {
                Some(body) => {
                    out.push('{');
                    out.push_str(&inline(body, anon, depth + 1));
                    out.push('}');
                }
                None => out.push_str("anon"),
            }
            rest = &tail[end..];
        }
        out.push_str(rest);
        out
    }

    /// Remove every `@ line:col`, bare `line:col` and `none` position, and
    /// the whitespace `{:#?}`-style output would leave behind.
    fn strip(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let bytes = text.as_bytes();
        let mut i = 0;
        let mut in_string = false;
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
                in_string = !in_string;
            }
            if !in_string {
                // `12:34`, not inside a name.
                if c.is_ascii_digit() && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_' || bytes[i - 1] == b'$')) {
                    let mut j = i;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b':' && j + 1 < bytes.len() && bytes[j + 1].is_ascii_digit() {
                        let mut k = j + 1;
                        while k < bytes.len() && bytes[k].is_ascii_digit() {
                            k += 1;
                        }
                        // A span goes on to its end: `-40` on the same line,
                        // `-3:1` on another.
                        if k + 1 < bytes.len() && bytes[k] == b'-' && bytes[k + 1].is_ascii_digit() {
                            k += 1;
                            while k < bytes.len() && (bytes[k].is_ascii_digit() || bytes[k] == b':') {
                                k += 1;
                            }
                        }
                        out.push_str("·");
                        i = k;
                        continue;
                    }
                }
                if text[i..].starts_with("none") && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
                    let after = bytes.get(i + 4).copied().unwrap_or(b' ');
                    if !after.is_ascii_alphanumeric() && after != b'_' {
                        out.push_str("·");
                        i += 4;
                        continue;
                    }
                }
            }
            let ch = text[i..].chars().next().expect("on a character boundary");
            out.push(ch);
            i += ch.len_utf8();
        }
        // `@ ·` is a position that was there; so is a lone `·`. Neither is
        // part of the tree.
        out.replace(" @ ·", "").replace("·", "")
    }
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;

    fn tree(src: &str) -> String {
        with_check_engine(|e| normal::tree(&e.compile(src).unwrap()))
    }

    /// The check is only worth having if it can fail: the trees it compares
    /// must differ when grouping differs, and match when only layout does.
    #[test]
    fn the_comparison_sees_grouping_and_not_layout() {
        assert_eq!(tree("a + b * c"), tree("a +\n   (b * c)"));
        assert_ne!(tree("a + b * c"), tree("(a + b) * c"));
        assert_eq!(tree("let f = |x| x + 1;"), tree("let f =\n  |x|   x + 1;"));
        assert_ne!(tree("let f = |x| x + 1;"), tree("let f = |x| x - 1;"));
        assert_eq!(tree("fn f() { if a { 1 } }"), tree("fn f() {\n if a {\n 1\n }\n}"));
    }

    #[test]
    fn a_body_after_set_interval_becomes_a_string_and_keeps_its_lines() {
        let src = "let t = setInterval(1000) {\n  n++;\n};\nlet after = 1;";
        let script = rux_syntax::parse(src, Options::default()).unwrap();
        let text = lower(src, &script);
        assert!(text.starts_with(r#"let t = __interval(1000, "\n  n++;\n")"#), "{text}");
        assert_eq!(text.lines().position(|l| l.contains("after")), Some(3), "{text}");
    }
}
