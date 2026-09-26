//! Rux's own parser for its script language.
//!
//! `docs/11-next.md` is the plan this belongs to: Rux's parser, then its type
//! checker, a typed IR and an interpreter, replacing the rhai fork piece by
//! piece. This crate is step 2, the parser. It accepts exactly the language
//! the fork accepts today and builds the same structure, which is proved in
//! `rux-script` by a test that parses every script in the repository both
//! ways and compares the results. What it adds is Rux's own AST, with a span
//! on every node, and errors that talk about Rux.
//!
//! ```
//! let script = rux_syntax::parse("let n = signal(0); fn bump() { n++ }", Default::default()).unwrap();
//! assert_eq!(script.stmts.len(), 2);
//! ```

pub mod ast;
pub mod lexer;
mod parser;
pub mod print;
pub mod span;
pub mod visit;

pub use parser::Options;
pub use span::{LineIndex, Span};

use std::fmt;

/// Text that is not Rux, and where it goes wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    /// What is wrong, as a sentence fragment an author reads.
    pub message: String,
    pub span: Span,
}

impl SyntaxError {
    /// The 1-based line and column where it goes wrong in `src`.
    pub fn line_col(&self, src: &str) -> (usize, usize) {
        LineIndex::new(src).line_col(src, self.span.start as usize)
    }
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SyntaxError {}

/// Parse a script, a handler or a binding: statements until the end.
pub fn parse(src: &str, opts: Options) -> Result<ast::Script, SyntaxError> {
    let tokens = lexer::lex(src)?;
    parser::Parser::new(&tokens, src, opts).script()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::*;

    fn ok(src: &str) -> Script {
        parse(src, Options::default()).unwrap_or_else(|e| panic!("{src:?}: {e} at {:?}", e.line_col(src)))
    }

    fn fails(src: &str) -> String {
        match parse(src, Options::default()) {
            Ok(s) => panic!("{src:?} parsed as {s:?}"),
            Err(e) => e.message,
        }
    }

    fn one_expr(src: &str) -> Expr {
        match ok(src).stmts.into_iter().next().map(|s| s.kind) {
            Some(StmtKind::Expr(e)) => e,
            other => panic!("{other:?}"),
        }
    }

    /// Operators as the fork binds them, written back with every group made
    /// explicit.
    fn grouped(src: &str) -> String {
        print::expr(&one_expr(src), src)
    }

    #[test]
    fn precedence_is_the_forks() {
        assert_eq!(grouped("a + b * c"), "(a + (b * c))");
        assert_eq!(grouped("a - b - c"), "((a - b) - c)");
        assert_eq!(grouped("a ** b ** c"), "(a ** (b ** c))");
        // `??` binds tighter than a comparison, and `in` looser.
        assert_eq!(grouped("a ?? 0 == 1"), "((a ?? 0) == 1)");
        assert_eq!(grouped("a < b in c"), "((a < b) in c)");
        assert_eq!(grouped("a == b is int"), "((a == b) is int)");
        assert_eq!(grouped("a + b is int"), "((a + b) is int)");
        assert_eq!(grouped("x is T && y"), "((x is T) && y)");
        assert_eq!(grouped("!a == b"), "((!(a)) == b)");
        assert_eq!(grouped("-a.b"), "(-(a.b))");
    }

    #[test]
    fn a_brace_is_a_map_when_a_key_and_colon_follow() {
        assert!(matches!(one_expr("{ a: 1 }").kind, ExprKind::Map { .. }));
        let StmtKind::Assign { value, .. } = &ok("x = {}").stmts[0].kind else { panic!() };
        assert!(matches!(value.kind, ExprKind::Map { .. }), "an empty `{{}}` as a value is a map");
        assert!(matches!(ok("{}").stmts[0].kind, StmtKind::Block(_)));
        let arrow = one_expr("rows.map(r => { id: r.id })");
        let ExprKind::Method { args, .. } = arrow.kind else { panic!() };
        let ExprKind::Closure { body, .. } = &args[0].kind else { panic!() };
        assert!(matches!(&body.kind, StmtKind::Expr(Expr { kind: ExprKind::Map { .. }, .. })));
    }

    #[test]
    fn arrows_and_their_annotations() {
        ok("let add = (a: number, b: number) => a + b;");
        ok("items.filter(t => !t.done)");
        ok("let noop = () => {};");
        ok("|x| x + 1");
    }

    #[test]
    fn annotations_parse() {
        let s = ok("type Filter = \"all\" | \"open\";\n\
                    type Task = { id: int, title: string, note?: string };\n\
                    let tasks: Task[] = signal([]);\n\
                    fn label(t: Task, n: int): string { t.title }\n\
                    fn shape(): { a: number } { { a: 1 } }\n\
                    let m: { [string]: bool } = {};\n\
                    let f: (Task) => bool = t => t.done;\n\
                    let o: Task?[] = [];");
        assert_eq!(s.stmts.len(), 8);
    }

    #[test]
    fn statements() {
        ok("x++; y--; a.b += 1; a[0] = 2;");
        ok("if a { b } else if c { d } else { e }");
        ok("for i in 0..n { if i > 2 { break; } }");
        ok("for (x, i) in items { continue; }");
        ok("while true { break }");
        ok("loop { break 3; }");
        ok("do { x++ } while x < 3;");
        ok("try { throw \"no\"; } catch (e) { print(e) }");
        ok("switch f { \"all\" => 1, \"open\" | \"done\" => 2, _ => { 3 } }");
        ok("fn f() { return; }");
        ok("let s = `a ${n + 1} b ${ {a: 1}.a }`;");
        ok("host::save(level); print(1); debug(2);");
        ok("let v = user?.name ?? \"none\"; let w = m?[k];");
        ok("f!(1)");
        ok("setInterval(1000) { seconds++; if seconds >= 5 { clearInterval(timer); } }");
    }

    #[test]
    fn what_the_fork_refuses_is_refused() {
        fails("let x = ;");
        fails("x = ");
        fails("f() = 1");
        fails("a.b() = 1");
        fails("const x = 1; x = 2;");
        fails("if { }");
        fails("if a = b { }");
        fails("fn f(a, a) {}");
        fails("fn f() {} fn f() {}");
        fails("{ fn f() {} }");
        fails("break;");
        fails("let print = 1;");
        fails("switch x { _ => 1, 2 => 3 }");
        fails("switch x { y => 1 }");
        fails("a b");
        fails("x = this;");
        assert!(fails("'ab'").contains("double quotes"));
    }

    #[test]
    fn declarations_only_where_asked_for() {
        let opts = Options { declarations: true };
        let s = parse(
            "use types::Task;\n\
             prop label: string;\n\
             prop size = 16\n\
             computed total: number = items.length * price\n\
             effect { print(total) }\n\
             mounted { level = 1; }\n\
             let n = 1;",
            opts,
        )
        .unwrap();
        assert_eq!(s.stmts.len(), 7);
        assert!(matches!(s.stmts[0].kind, StmtKind::Use(_)));
        assert!(matches!(s.stmts[3].kind, StmtKind::Computed { .. }));
        assert!(parse("computed total = 1", Options::default()).is_err());
        // Still ordinary names elsewhere.
        ok("let prop = 3; prop = 4; effect(1);");
    }
}
