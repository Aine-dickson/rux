//! Tokens to the Rux AST.
//!
//! Step 2 of `docs/11-next.md`: this accepts exactly what the rhai fork
//! accepts, and gives the same structure (the differential test in
//! `rux-script` compares the two over every script in the repository). The
//! functions below follow the fork's `parser.rs` one for one, and are named
//! after them where that helps a reader hold the two side by side. The
//! messages are Rux's own.

use crate::ast::*;
use crate::lexer::{Tok, Token, TplPart, CALLABLE_RESERVED, METHOD_RESERVED, RESERVED_SYMBOLS};
use crate::span::Span;
use crate::SyntaxError;

/// What a parse accepts beyond the script language itself.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// The top-level declarations only a document or component `<script>`
    /// holds: `computed`, `effect`, `mounted`, `unmounted`, `prop` and `use`.
    pub declarations: bool,
}

type PResult<T> = Result<T, SyntaxError>;

pub(crate) struct Parser<'t> {
    toks: &'t [Token<'t>],
    /// The source the tokens came from, for the few decisions that turn on
    /// where a line ends.
    src: &'t str,
    pos: usize,
    opts: Options,
    /// Inside a `fn` or closure body: `this` is allowed.
    in_fn: bool,
    /// Inside a loop: `break` and `continue` are allowed.
    breakable: bool,
    /// At the top level: `fn`, `type` and `export` are allowed.
    global: bool,
    /// Inside a `switch` case's values, where `|` separates them.
    no_pipe: bool,
    /// Every `let` and `const` in scope, innermost last, with whether it is a
    /// constant. The fork refuses an assignment to a constant while parsing.
    vars: Vec<(&'t str, bool)>,
    /// Functions declared so far, by name, arity and `this` type, since the
    /// fork refuses a second definition of the same one.
    fns: Vec<(String, usize, Option<String>)>,
    /// While a type is read: the `>>` here has had its first `>` taken, by
    /// the inner of two type argument lists it closes (`Map<string, Page<T>>`).
    half_gt: bool,
    /// The id the next expression gets. See [`ExprId`].
    next: std::cell::Cell<u32>,
}

impl<'t> Parser<'t> {
    pub(crate) fn new(toks: &'t [Token<'t>], src: &'t str, opts: Options) -> Self {
        Self {
            toks,
            src,
            pos: 0,
            opts,
            in_fn: false,
            breakable: false,
            global: true,
            no_pipe: false,
            vars: Vec::new(),
            fns: Vec::new(),
            half_gt: false,
            next: std::cell::Cell::new(0),
        }
    }

    fn next_id(&self) -> ExprId {
        let id = self.next.get();
        self.next.set(id + 1);
        ExprId(id)
    }

    // ----- the token stream ---------------------------------------------

    // Tokens are borrowed for the parser's whole life, not from `self`, so a
    // `match self.peek()` can go on to move the parser.
    fn peek(&self) -> &'t Tok<'t> {
        let toks = self.toks;
        &toks[self.pos.min(toks.len() - 1)].tok
    }

    fn peek_nth(&self, n: usize) -> &'t Tok<'t> {
        let toks = self.toks;
        &toks[(self.pos + n).min(toks.len() - 1)].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos.min(self.toks.len() - 1)].span
    }

    fn prev_span(&self) -> Span {
        if self.pos == 0 {
            return Span::at(0);
        }
        self.toks[(self.pos - 1).min(self.toks.len() - 1)].span
    }

    fn bump(&mut self) -> &'t Token<'t> {
        let toks = self.toks;
        let t = &toks[self.pos.min(toks.len() - 1)];
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn at_punct(&self, p: &str) -> bool {
        self.peek().is_punct(p)
    }

    fn at_kw(&self, k: &str) -> bool {
        self.peek().is_kw(k)
    }

    fn eat_punct(&mut self, p: &str) -> bool {
        if self.at_punct(p) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), Tok::Eof)
    }

    fn error<T>(&self, message: impl Into<String>) -> PResult<T> {
        Err(SyntaxError { message: message.into(), span: self.span() })
    }

    fn expect_punct(&mut self, p: &str, what: &str) -> PResult<Span> {
        if self.at_punct(p) {
            Ok(self.bump().span)
        } else {
            self.error(format!("expecting `{p}` {what}, found {}", self.peek().describe()))
        }
    }

    fn with_flags<T>(
        &mut self,
        in_fn: bool,
        breakable: bool,
        global: bool,
        f: impl FnOnce(&mut Self) -> PResult<T>,
    ) -> PResult<T> {
        let saved = (self.in_fn, self.breakable, self.global, self.no_pipe);
        self.in_fn = in_fn;
        self.breakable = breakable;
        self.global = global;
        self.no_pipe = false;
        let out = f(self);
        (self.in_fn, self.breakable, self.global, self.no_pipe) = saved;
        out
    }

    /// A new function scope: its own variables, as the fork gives a `fn` or a
    /// closure a fresh parse state.
    fn in_new_scope<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        let saved = std::mem::take(&mut self.vars);
        let out = f(self);
        self.vars = saved;
        out
    }

    // ----- statements -----------------------------------------------------

    /// The top level: statements until the end.
    pub(crate) fn script(&mut self) -> PResult<Script> {
        let mut stmts = Vec::new();
        while !self.at_eof() {
            let stmt = self.stmt()?;
            if matches!(stmt.kind, StmtKind::Empty) {
                continue;
            }
            let noop = matches!(stmt.kind, StmtKind::Fn(_) | StmtKind::Type { .. });
            let need_semicolon = !stmt.is_self_terminated();
            stmts.push(stmt);
            if noop {
                continue;
            }
            match self.peek() {
                Tok::Eof => break,
                Tok::Punct(";") if need_semicolon => {
                    self.bump();
                }
                _ if !need_semicolon => {}
                other => {
                    return self.error(format!(
                        "expecting `;` to end this statement, found {}",
                        other.describe()
                    ))
                }
            }
        }
        Ok(Script { stmts })
    }

    fn block(&mut self) -> PResult<Block> {
        if !self.at_punct("{") {
            return self.error(format!("expecting `{{` to start a block, found {}", self.peek().describe()));
        }
        let open = self.bump().span;
        let frame = self.vars.len();
        let saved_global = self.global;
        self.global = false;
        let mut stmts = Vec::new();
        let result = loop {
            match self.peek() {
                Tok::Punct("}") => break Ok(self.bump().span),
                Tok::Eof => break self.error("this block is never closed with `}`"),
                _ => {}
            }
            let stmt = match self.stmt() {
                Ok(s) => s,
                Err(e) => break Err(e),
            };
            if matches!(stmt.kind, StmtKind::Empty) {
                continue;
            }
            let noop = matches!(stmt.kind, StmtKind::Fn(_) | StmtKind::Type { .. });
            let need_semicolon = !stmt.is_self_terminated();
            stmts.push(stmt);
            if noop {
                continue;
            }
            match self.peek() {
                Tok::Punct("}") => break Ok(self.bump().span),
                Tok::Punct(";") => {
                    self.bump();
                }
                _ if !need_semicolon => {}
                other => {
                    break self.error(format!(
                        "expecting `;` to end this statement, found {}",
                        other.describe()
                    ))
                }
            }
        };
        self.global = saved_global;
        self.vars.truncate(frame);
        let close = result?;
        Ok(Block { stmts, span: open.to(close) })
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        let start = self.span();
        let is_brace_map = self.brace_opens_map(false);

        // `type Name = T;` or `type Name<T> = …;`, where `type` could never have
        // meant anything else.
        if self.peek().is_ident("type") && matches!(self.peek_nth(1), Tok::Ident(_)) && self.type_params_then_eq(2) {
            if !self.global {
                return self.error("a `type` is declared at the top level of the script, not inside a block");
            }
            self.bump();
            let name = self.ident()?;
            let (params, _) = self.type_params()?;
            self.bump(); // `=`
            let ty = self.take_type("`=`")?;
            return Ok(Stmt { span: start.to(ty.span), kind: StmtKind::Type { name, params, ty } });
        }

        if self.opts.declarations && self.global {
            if let Some(stmt) = self.declaration()? {
                return Ok(stmt);
            }
        }

        let kind = match self.peek() {
            Tok::Eof => return Ok(Stmt { kind: StmtKind::Empty, span: start }),
            Tok::Punct(";") => {
                self.bump();
                StmtKind::Empty
            }
            Tok::Punct("{") if !is_brace_map => StmtKind::Block(self.block()?),
            Tok::Kw("fn") | Tok::Kw("private") | Tok::Kw("async") => {
                let private = self.at_kw("private");
                if private {
                    self.bump();
                }
                let is_async = self.at_kw("async");
                if is_async {
                    self.bump();
                }
                if !self.at_kw("fn") {
                    return self.error(match (private, is_async) {
                        (true, false) => "expecting `fn` after `private`",
                        _ => "expecting `fn` after `async`: only a function can be `async`",
                    });
                }
                if !self.global {
                    return self.error("a `fn` is declared at the top level of the script, not inside a block");
                }
                let mut decl = self.fn_decl(private)?;
                decl.is_async = is_async;
                StmtKind::Fn(decl)
            }
            Tok::Kw("if") => StmtKind::If(self.if_stmt()?),
            Tok::Kw("switch") => StmtKind::Switch(self.switch()?),
            Tok::Kw("while") | Tok::Kw("loop") => self.while_loop()?,
            Tok::Kw("do") => self.do_loop()?,
            Tok::Kw("for") => self.for_loop()?,
            Tok::Kw("continue") | Tok::Kw("break") if !self.breakable => {
                return self.error(format!("`{}` belongs inside a loop", self.peek().describe().trim_matches('`')));
            }
            Tok::Kw("continue") => {
                self.bump();
                StmtKind::Continue
            }
            Tok::Kw("break") => {
                self.bump();
                StmtKind::Break(self.optional_expr()?)
            }
            Tok::Kw("return") => {
                self.bump();
                StmtKind::Return(self.optional_expr()?)
            }
            Tok::Kw("throw") => {
                self.bump();
                StmtKind::Throw(self.optional_expr()?)
            }
            Tok::Kw("try") => self.try_catch()?,
            Tok::Kw("let") => self.let_stmt(false)?,
            Tok::Kw("const") => self.let_stmt(true)?,
            Tok::Kw("import") => {
                self.bump();
                let path = self.expr()?;
                let alias = if self.at_kw("as") {
                    self.bump();
                    Some(self.var_name()?)
                } else {
                    None
                };
                StmtKind::Import { path, alias }
            }
            Tok::Kw("export") if !self.global => {
                return self.error("`export` belongs at the top level of the script");
            }
            Tok::Kw("export") => {
                self.bump();
                if self.at_kw("let") || self.at_kw("const") {
                    let constant = self.at_kw("const");
                    let s = self.span();
                    let kind = self.let_stmt(constant)?;
                    StmtKind::Export(Export::Let(Box::new(Stmt { kind, span: s.to(self.prev_span()) })))
                } else {
                    let name = self.var_name()?;
                    let alias = if self.at_kw("as") {
                        self.bump();
                        Some(self.var_name()?)
                    } else {
                        None
                    };
                    StmtKind::Export(Export::Name { name, alias })
                }
            }
            _ => return self.expr_stmt(),
        };
        Ok(Stmt { kind, span: start.to(self.prev_span()) })
    }

    /// `break`, `return` and `throw` take an expression when one is there.
    /// As in the fork, one that fails before reading anything is absent.
    fn optional_expr(&mut self) -> PResult<Option<Expr>> {
        let before = self.pos;
        match self.expr() {
            Ok(e) => Ok(Some(e)),
            Err(_) if self.pos == before => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn expr_stmt(&mut self) -> PResult<Stmt> {
        let start = self.span();
        let target = self.expr()?;
        let kind = match self.peek() {
            Tok::Punct(p @ ("++" | "--")) => {
                self.bump();
                self.check_assignable(&target, "=")?;
                StmtKind::Step { target, up: *p == "++" }
            }
            Tok::Punct(
                op @ ("=" | "+=" | "-=" | "*=" | "/=" | "%=" | "**=" | "<<=" | ">>=" | "&=" | "|=" | "^="),
            ) => {
                let op_span = self.bump().span;
                let value = self.expr()?;
                self.check_assignable(&target, op).map_err(|mut e| {
                    if e.span == Span::default() {
                        e.span = op_span;
                    }
                    e
                })?;
                StmtKind::Assign { target, op, value }
            }
            _ => StmtKind::Expr(target),
        };
        Ok(Stmt { kind, span: start.to(self.prev_span()) })
    }

    /// The fork's `make_assignment_stmt`: what can be assigned to.
    fn check_assignable(&self, target: &Expr, op: &str) -> PResult<()> {
        let bad = |span: Span, message: &str| Err(SyntaxError { message: message.into(), span });
        match &target.kind {
            ExprKind::Var(name) => {
                if self.vars.iter().rev().find(|(n, _)| n == name).is_some_and(|(_, c)| *c) {
                    return bad(target.span, &format!("`{name}` is a constant and cannot be assigned to"));
                }
                Ok(())
            }
            ExprKind::This => Ok(()),
            ExprKind::Field { .. } | ExprKind::Index { .. } => {
                // Every step after the root is a field or an index, and the
                // root is a name: `a.b[0].c`, not `a.b().c` or `f().c`.
                let mut e = target;
                loop {
                    match &e.kind {
                        ExprKind::Field { base, .. } | ExprKind::Index { base, .. } => e = base,
                        ExprKind::Var(_) | ExprKind::This => return Ok(()),
                        _ => return bad(e.span, "only a name, or a field or index of one, can be assigned to"),
                    }
                }
            }
            ExprKind::Binary { op: "&&" | "||" | "??", .. } if op == "=" => {
                bad(target.span, "this `=` assigns; to compare, write `==`")
            }
            ExprKind::Unit
            | ExprKind::Null
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::Char(_)
            | ExprKind::Bool(_) => bad(target.span, "a constant cannot be assigned to"),
            _ => bad(target.span, "only a name, or a field or index of one, can be assigned to"),
        }
    }

    /// `if {` has no condition, and `if a = b` is a typo of `==`.
    fn condition(&mut self) -> PResult<Expr> {
        if self.at_punct("{") {
            return self.error("expecting a condition here, before the `{`");
        }
        let e = self.expr()?;
        if self.at_punct("=") {
            return self.error("this `=` assigns; to compare, write `==`");
        }
        Ok(e)
    }

    fn if_stmt(&mut self) -> PResult<If> {
        self.bump(); // `if`
        let cond = self.condition()?;
        let then = self.block()?;
        let otherwise = if self.at_kw("else") {
            self.bump();
            let start = self.span();
            if self.at_kw("if") {
                let inner = self.if_stmt()?;
                Some(Box::new(Stmt { kind: StmtKind::If(inner), span: start.to(self.prev_span()) }))
            } else {
                let b = self.block()?;
                Some(Box::new(Stmt { span: b.span, kind: StmtKind::Block(b) }))
            }
        } else {
            None
        };
        Ok(If { cond, then, otherwise })
    }

    fn while_loop(&mut self) -> PResult<StmtKind> {
        let cond = if self.bump().tok.is_kw("while") { Some(self.condition()?) } else { None };
        let (in_fn, global) = (self.in_fn, self.global);
        let body = self.with_flags(in_fn, true, global, |p| p.block())?;
        Ok(StmtKind::While { cond, body })
    }

    fn do_loop(&mut self) -> PResult<StmtKind> {
        self.bump(); // `do`
        let (in_fn, global) = (self.in_fn, self.global);
        let body = self.with_flags(in_fn, true, global, |p| p.block())?;
        let until = match self.peek() {
            Tok::Kw("while") => false,
            Tok::Kw("until") => true,
            _ => return self.error("expecting `while` or `until` after the body of `do`"),
        };
        self.bump();
        let cond = self.condition()?;
        Ok(StmtKind::Do { body, cond, until })
    }

    fn for_loop(&mut self) -> PResult<StmtKind> {
        self.bump(); // `for`
        let (var, counter) = if self.eat_punct("(") {
            let var = self.var_name()?;
            self.expect_punct(",", "after the loop variable")?;
            let counter = self.var_name()?;
            if counter.name == var.name {
                return Err(SyntaxError {
                    message: format!("`{}` is named twice", counter.name),
                    span: counter.span,
                });
            }
            self.expect_punct(")", "to close the loop variables")?;
            (var, Some(counter))
        } else {
            (self.var_name()?, None)
        };
        if !self.at_kw("in") {
            return self.error(format!("expecting `in` after the loop variable, found {}", self.peek().describe()));
        }
        self.bump();
        if self.at_punct("{") {
            return self.error("expecting what to loop over, before the `{`");
        }
        let iter = self.expr()?;
        let frame = self.vars.len();
        if let Some(c) = &counter {
            self.vars.push((c.span.text(self.src), false));
        }
        self.vars.push((var.span.text(self.src), false));
        let (in_fn, global) = (self.in_fn, self.global);
        let body = self.with_flags(in_fn, true, global, |p| p.block());
        self.vars.truncate(frame);
        Ok(StmtKind::For { var, counter, iter, body: body? })
    }

    fn let_stmt(&mut self, constant: bool) -> PResult<StmtKind> {
        self.bump(); // `let` or `const`
        let name = self.var_name()?;
        let ty = if self.eat_punct(":") { Some(self.take_type("`:`")?) } else { None };
        let value = if self.eat_punct("=") { Some(self.expr()?) } else { None };
        // A `let` of a name already in this block replaces it there.
        self.vars.push((name.span.text(self.src), constant));
        Ok(StmtKind::Let { name, ty, value, constant })
    }

    fn try_catch(&mut self) -> PResult<StmtKind> {
        self.bump(); // `try`
        let body = self.block()?;
        if !self.at_kw("catch") {
            return self.error("expecting `catch` after the body of `try`");
        }
        self.bump();
        let var = if self.eat_punct("(") {
            let v = self.var_name()?;
            self.expect_punct(")", "to close the name `catch` binds")?;
            Some(v)
        } else if matches!(self.peek(), Tok::Ident(_)) {
            // `catch e { … }`, the reference's spelling; `catch (e)` is the
            // fork's, still read.
            Some(self.var_name()?)
        } else {
            None
        };
        let frame = self.vars.len();
        if let Some(v) = &var {
            self.vars.push((v.span.text(self.src), false));
        }
        let catch = self.block();
        self.vars.truncate(frame);
        Ok(StmtKind::Try { body, var, catch: catch? })
    }

    fn fn_decl(&mut self, private: bool) -> PResult<FnDecl> {
        let fn_span = self.bump().span; // `fn`
        // `fn Type.name()`, rhai's method on a type.
        let this_type = match (self.peek(), self.peek_nth(1).is_punct(".")) {
            (Tok::Ident(t), true) => {
                self.bump();
                self.bump();
                Some(t.to_string())
            }
            (Tok::Str(t), true) => {
                self.bump();
                self.bump();
                Some(t.to_string())
            }
            (Tok::Str(_), false) => return self.error("expecting `.` after the type name of `this`"),
            _ => None,
        };
        let name = match self.peek() {
            Tok::Ident(n) => Ident { name: n.to_string(), span: self.bump().span },
            Tok::Reserved(r) => return self.error(format!("`{r}` is a reserved word, so a function cannot take it as its name")),
            Tok::Kw(k) => return self.error(format!("`{k}` is a keyword, so a function cannot take it as its name")),
            _ => return self.error("expecting the function's name after `fn`"),
        };
        let (type_params, type_params_span) = self.type_params()?;
        let params = self.in_new_scope(|p| {
            let params = p.fn_params(&name.name)?;
            let result = if p.eat_punct(":") { Some(p.take_type("the parameter list")?) } else { None };
            if !p.at_punct("{") {
                return p.error(format!("expecting the body of `{}`, a block in `{{ }}`", name.name));
            }
            let body = p.with_flags(true, false, false, |p| p.block())?;
            Ok((params, result, body))
        })?;
        let (params, result, body) = params;
        let key = (name.name.clone(), params.len(), this_type.clone());
        if self.fns.contains(&key) {
            return Err(SyntaxError {
                message: format!("`{}` with {} parameters is defined twice", name.name, params.len()),
                span: name.span,
            });
        }
        self.fns.push(key);
        let _ = fn_span;
        Ok(FnDecl { name, private, is_async: false, this_type, type_params, type_params_span, params, result, body })
    }

    fn fn_params(&mut self, fn_name: &str) -> PResult<Vec<Param>> {
        let mut params: Vec<Param> = Vec::new();
        if self.eat_punct("()") {
            return Ok(params);
        }
        if !self.eat_punct("(") {
            return self.error(format!("expecting `(` and the parameters of `{fn_name}`"));
        }
        if self.eat_punct(")") {
            return Ok(params);
        }
        loop {
            match self.peek() {
                Tok::Punct(")") => {
                    self.bump();
                    break;
                }
                Tok::Ident(n) => {
                    let span = self.bump().span;
                    if params.iter().any(|p| p.name.name == *n) {
                        return Err(SyntaxError { message: format!("`{fn_name}` has two parameters named `{n}`"), span });
                    }
                    self.vars.push((*n, false));
                    let ty = if self.eat_punct(":") { Some(self.take_type("`:`")?) } else { None };
                    params.push(Param { name: Ident { name: n.to_string(), span }, ty });
                }
                Tok::Reserved(r) => return self.error(format!("`{r}` is a reserved word, so a parameter cannot take it as its name")),
                Tok::Kw(k) => return self.error(format!("`{k}` is a keyword, so a parameter cannot take it as its name")),
                _ => return self.error(format!("expecting `)` to close the parameters of `{fn_name}`")),
            }
            match self.peek() {
                Tok::Punct(")") => {
                    self.bump();
                    break;
                }
                Tok::Punct(",") => {
                    self.bump();
                }
                _ => return self.error(format!("expecting `,` between the parameters of `{fn_name}`")),
            }
        }
        Ok(params)
    }

    fn switch(&mut self) -> PResult<Switch> {
        self.bump(); // `switch`
        let value = self.expr()?;
        self.expect_punct("{", "to start the cases of this `switch`")?;
        let mut arms = Vec::new();
        let mut seen_default = false;
        let mut seen_range = false;
        loop {
            let arm_start = self.span();
            let (patterns, guard) = match self.peek() {
                Tok::Punct("}") => {
                    self.bump();
                    break;
                }
                Tok::Eof => return self.error("this `switch` is never closed with `}`"),
                Tok::Kw("_") if !seen_default => {
                    self.bump();
                    if self.at_kw("if") {
                        return self.error("the `_` case takes no condition");
                    }
                    seen_default = true;
                    (Vec::new(), None)
                }
                _ if seen_default => {
                    return Err(SyntaxError {
                        message: "the `_` case must be the last one".into(),
                        span: arm_start,
                    })
                }
                _ => {
                    let mut patterns = Vec::new();
                    loop {
                        let saved = self.no_pipe;
                        self.no_pipe = true;
                        let e = self.expr();
                        self.no_pipe = saved;
                        let e = e.map_err(|e| SyntaxError { message: "expecting a literal value for this case".into(), span: e.span })?;
                        match literal_kind(&e) {
                            None => {
                                return Err(SyntaxError {
                                    message: "a case is a literal value, such as `\"open\"` or `3`".into(),
                                    span: e.span,
                                })
                            }
                            Some(Literal::Range) => seen_range = true,
                            Some(Literal::Number) if seen_range => {
                                return Err(SyntaxError {
                                    message: "a number case cannot come after a range case".into(),
                                    span: e.span,
                                })
                            }
                            Some(_) => {}
                        }
                        patterns.push(e);
                        if !self.eat_punct("|") {
                            break;
                        }
                    }
                    let guard = if self.at_kw("if") {
                        self.bump();
                        Some(self.condition()?)
                    } else {
                        None
                    };
                    (patterns, guard)
                }
            };
            self.expect_punct("=>", "in this `switch` case")?;
            let body = self.stmt()?;
            let need_comma = !body.is_self_terminated();
            arms.push(Arm { patterns, guard, body: Box::new(body), span: arm_start.to(self.prev_span()) });
            match self.peek() {
                Tok::Punct(",") => {
                    self.bump();
                }
                Tok::Punct("}") => {}
                Tok::Eof => return self.error("this `switch` is never closed with `}`"),
                _ if need_comma => return self.error("expecting `,` between the cases of this `switch`"),
                _ => {}
            }
        }
        Ok(Switch { value, arms })
    }

    /// The top-level declarations of a document or component script.
    fn declaration(&mut self) -> PResult<Option<Stmt>> {
        let start = self.span();
        match (self.peek(), self.peek_nth(1)) {
            (Tok::Ident(w), Tok::Ident(_)) if *w == "computed" => {
                self.bump();
                let name = self.ident()?;
                let ty = if self.eat_punct(":") { Some(self.take_type("`:`")?) } else { None };
                self.expect_punct("=", "and the expression a `computed` is worked out from")?;
                let value = self.expr()?;
                self.vars.push((name.span.text(self.src), false));
                Ok(Some(Stmt { kind: StmtKind::Computed { name, ty, value }, span: start.to(self.prev_span()) }))
            }
            (Tok::Ident(w), Tok::Punct("{")) if matches!(*w, "effect" | "mounted" | "unmounted") => {
                self.bump();
                let kind = match *w {
                    "effect" => Lifecycle::Effect,
                    "mounted" => Lifecycle::Mounted,
                    _ => Lifecycle::Unmounted,
                };
                let body = self.with_flags(false, false, false, |p| p.block())?;
                Ok(Some(Stmt { kind: StmtKind::Lifecycle { kind, body }, span: start.to(self.prev_span()) }))
            }
            // `prop` then a name, or something meant as one (`prop new-x`), on
            // the same line; `prop = 3` and `prop(x)` stay ordinary code.
            (Tok::Ident(w), Tok::Ident(_) | Tok::Reserved(_) | Tok::Kw(_))
                if *w == "prop" && !self.newline_after_this() =>
            {
                let (pos, vars) = (self.pos, self.vars.len());
                self.bump();
                if let Some(decls) = self.prop_decls() {
                    for d in &decls {
                        self.vars.push((d.name.span.text(self.src), false));
                    }
                    return Ok(Some(Stmt { kind: StmtKind::Prop(decls), span: start.to(self.prev_span()) }));
                }
                // Not a clean declaration: a hyphenated name, a `:` with no
                // type, two names sharing one default. The runtime says which,
                // from the statement's text, and the file still loads, as it
                // did when props were read line by line.
                self.pos = pos;
                self.vars.truncate(vars);
                Ok(Some(self.raw_statement(start, StmtKind::Prop(Vec::new()))))
            }
            (Tok::Reserved(w), _) if *w == "use" && !self.newline_after_this() => {
                let pos = self.pos;
                self.bump();
                if let Some(path) = self.use_path() {
                    return Ok(Some(Stmt { kind: StmtKind::Use(path), span: start.to(self.prev_span()) }));
                }
                self.pos = pos;
                Ok(Some(self.raw_statement(start, StmtKind::Use(Vec::new()))))
            }
            _ => Ok(None),
        }
    }

    /// `a, b: T = default`, up to the end of the statement, or `None` if it
    /// is not that shape.
    fn prop_decls(&mut self) -> Option<Vec<PropDecl>> {
        let mut decls = Vec::new();
        loop {
            let name = self.ident().ok()?;
            let ty = if self.eat_punct(":") { Some(self.take_type("`:`").ok()?) } else { None };
            decls.push(PropDecl { name, ty, default: None });
            if !self.eat_punct(",") {
                break;
            }
        }
        if self.eat_punct("=") {
            if decls.len() > 1 {
                return None;
            }
            decls[0].default = Some(self.expr().ok()?);
        }
        self.statement_end().then_some(decls)
    }

    /// `a::b::c`, each segment a name, or several joined by `-` with nothing
    /// between them (`task-row`, which the runtime then reports).
    fn use_path(&mut self) -> Option<Vec<Ident>> {
        let mut path = Vec::new();
        loop {
            let mut seg = self.ident().ok()?;
            while self.at_punct("-")
                && self.span().start == seg.span.end
                && matches!(self.peek_nth(1), Tok::Ident(_))
                && self.toks[self.pos + 1].span.start == self.span().end
            {
                self.bump();
                let next = self.ident().ok()?;
                seg.name = format!("{}-{}", seg.name, next.name);
                seg.span = seg.span.to(next.span);
            }
            path.push(seg);
            if !self.eat_punct("::") {
                break;
            }
        }
        self.statement_end().then_some(path)
    }

    /// Whether a declaration ends here: a `;`, which is taken, the end, or a
    /// new line.
    fn statement_end(&mut self) -> bool {
        if self.eat_punct(";") {
            return true;
        }
        self.at_eof() || self.newline_before()
    }

    /// Whether the token after this one starts a new line.
    fn newline_after_this(&self) -> bool {
        let (here, next) = (self.span(), self.toks[(self.pos + 1).min(self.toks.len() - 1)].span);
        self.src.get(here.end as usize..next.start as usize).is_some_and(|gap| gap.contains('\n'))
    }

    fn newline_before(&self) -> bool {
        let (prev, here) = (self.prev_span(), self.span());
        self.src.get(prev.end as usize..here.start as usize).is_some_and(|gap| gap.contains('\n'))
    }

    /// The rest of a statement as it stands, to its `;` or the end of its
    /// line, for a declaration the runtime reads from its text.
    fn raw_statement(&mut self, start: Span, kind: StmtKind) -> Stmt {
        self.bump();
        while !self.at_eof() && !self.newline_before() {
            if self.bump().tok.is_punct(";") {
                break;
            }
        }
        Stmt { kind, span: start.to(self.prev_span()) }
    }

    // ----- expressions ----------------------------------------------------

    pub(crate) fn expr(&mut self) -> PResult<Expr> {
        let lhs = self.unary()?;
        self.binary(1, lhs)
    }

    fn unary(&mut self) -> PResult<Expr> {
        let start = self.span();
        match self.peek() {
            Tok::Punct("|") if self.no_pipe => self.error("`|` separates the values of a case here"),
            Tok::Punct(op @ ("-" | "+")) => {
                self.bump();
                let operand = self.unary()?;
                let span = start.to(operand.span);
                let kind = match (*op, operand.kind) {
                    ("-", ExprKind::Int(n)) => match n.checked_neg() {
                        Some(n) => ExprKind::Int(n),
                        None => ExprKind::Float(-(n as f64)),
                    },
                    ("-", ExprKind::Float(f)) => ExprKind::Float(-f),
                    ("+", k @ (ExprKind::Int(_) | ExprKind::Float(_))) => k,
                    (op, kind) => ExprKind::Unary { op, expr: Box::new(Expr { id: self.next_id(), kind, span: operand.span }) },
                };
                Ok(Expr { id: self.next_id(), kind, span })
            }
            Tok::Kw("await") => {
                self.bump();
                let operand = self.unary()?;
                Ok(Expr { id: self.next_id(), span: start.to(operand.span), kind: ExprKind::Await(Box::new(operand)) })
            }
            Tok::Punct("!") => {
                self.bump();
                let operand = self.unary()?;
                Ok(Expr { id: self.next_id(), span: start.to(operand.span), kind: ExprKind::Unary { op: "!", expr: Box::new(operand) } })
            }
            Tok::Eof => self.error("the expression ends too soon"),
            _ => self.primary(),
        }
    }

    /// An operator's precedence, the fork's table; `None` for a token that
    /// ends the expression. `is` binds as `<` does on both sides, so
    /// `a == b is int` is `a == (b is int)`; the fork gave it no precedence
    /// after a right-hand side, and read that as `(a == b) is int` (step 3.5
    /// of `docs/11-next.md`). The fork never parses `is` now: it is handed
    /// `__is(x, "T")`.
    fn precedence(&self, tok: &Tok) -> PResult<Option<u8>> {
        Ok(Some(match tok {
            Tok::Punct("|") if self.no_pipe => return Ok(None),
            Tok::Punct("||" | "^" | "|") => 30,
            Tok::Punct("&&" | "&") => 60,
            Tok::Punct("==" | "!=" | "===" | "!==") => 90,
            Tok::Kw("in") | Tok::Punct("!in") => 110,
            Tok::Punct("<" | "<=" | ">" | ">=") => 130,
            Tok::Reserved(r) if *r == "is" => 130,
            Tok::Punct("??") => 135,
            Tok::Punct(".." | "..=") => 140,
            Tok::Punct("+" | "-") => 150,
            Tok::Punct("*" | "/" | "%") => 180,
            Tok::Punct("**") => 190,
            Tok::Punct("<<" | ">>") => 210,
            Tok::Punct("++" | "--") => return Ok(None),
            Tok::Punct(p) if RESERVED_SYMBOLS.contains(p) || *p == "?" => {
                return self.error(format!("`{p}` is not an operator"));
            }
            _ => return Ok(None),
        }))
    }

    fn binary(&mut self, parent: u8, lhs: Expr) -> PResult<Expr> {
        let mut root = lhs;
        loop {
            let op_tok = self.peek();
            let Some(prec) = self.precedence(&op_tok)? else { return Ok(root) };
            let right = op_tok.is_punct("**");
            if prec < parent || (prec == parent && !right) {
                return Ok(root);
            }
            let op_span = self.bump().span;

            if op_tok.is_reserved("is") {
                let ty = self.take_type("`is`")?;
                root = Expr { id: self.next_id(), span: root.span.to(ty.span), kind: ExprKind::Is { expr: Box::new(root), ty } };
                continue;
            }

            let rhs = match (&op_tok, self.peek()) {
                (Tok::Punct("??"), Tok::Kw("break" | "continue" | "return" | "throw")) => {
                    let s = self.stmt()?;
                    Expr { id: self.next_id(), span: s.span, kind: ExprKind::Stmt(Box::new(s)) }
                }
                (Tok::Punct(".." | "..="), Tok::Punct("]" | ")" | "}" | "," | ";" | "=>")) => {
                    Expr { id: self.next_id(), kind: ExprKind::Unit, span: Span::at(self.span().start as usize) }
                }
                _ => self.unary()?,
            };
            let next = self.precedence(&self.peek())?;
            let rhs = match next {
                Some(n) if n > prec || (n == prec && right) => self.binary(prec, rhs)?,
                _ => rhs,
            };
            let op: &'static str = match op_tok {
                Tok::Punct(p) => p,
                Tok::Kw(k) => k,
                _ => unreachable!("a binary operator is punctuation or `in`"),
            };
            let _ = op_span;
            root = Expr { id: self.next_id(), span: root.span.to(rhs.span), kind: ExprKind::Binary { op, lhs: Box::new(root), rhs: Box::new(rhs) } };
        }
    }

    fn primary(&mut self) -> PResult<Expr> {
        let start = self.span();
        if self.no_pipe && (self.at_punct("|") || self.at_punct("||")) {
            return self.error("`|` separates the values of a case here");
        }
        let arrow = self.arrow_params_len().is_some();
        let brace_map = self.brace_opens_map(true);
        let tok = self.peek();
        let root = match tok {
            Tok::Eof => return self.error("the expression ends too soon"),
            Tok::Ident(_) | Tok::Punct("()") | Tok::Punct("(") if arrow => return self.arrow(),
            Tok::Punct("()") => {
                self.bump();
                Expr { id: self.next_id(), kind: ExprKind::Unit, span: start }
            }
            Tok::Int(n) => {
                self.bump();
                Expr { id: self.next_id(), kind: ExprKind::Int(*n), span: start }
            }
            Tok::Float(f) => {
                self.bump();
                Expr { id: self.next_id(), kind: ExprKind::Float(*f), span: start }
            }
            Tok::Char(c) => {
                self.bump();
                Expr { id: self.next_id(), kind: ExprKind::Char(*c), span: start }
            }
            Tok::Str(s) => {
                self.bump();
                Expr { id: self.next_id(), kind: ExprKind::Str(s.to_string()), span: start }
            }
            Tok::Kw(b @ ("true" | "false")) => {
                self.bump();
                Expr { id: self.next_id(), kind: ExprKind::Bool(*b == "true"), span: start }
            }
            // `..5` reads as `0..5`, as it does in the fork.
            Tok::Punct(".." | "..=") => Expr { id: self.next_id(), kind: ExprKind::Int(0), span: Span::at(start.start as usize) },
            Tok::Punct("{") if brace_map => self.map_literal(false)?,
            Tok::Punct("{") => {
                let b = self.block()?;
                Expr { id: self.next_id(), span: b.span, kind: ExprKind::Stmt(Box::new(Stmt { span: b.span, kind: StmtKind::Block(b) })) }
            }
            Tok::Punct("(") => {
                self.bump();
                let saved = self.no_pipe;
                self.no_pipe = false;
                let inner = self.expr();
                self.no_pipe = saved;
                let inner = inner?;
                if !self.eat_punct(")") {
                    return self.error("expecting `)` to match the `(` of this expression");
                }
                // The parentheses are part of what was written, so a span that
                // ends on one ends after it.
                Expr { span: start.to(self.prev_span()), ..inner }
            }
            Tok::Kw("if") => {
                let s = self.if_stmt()?;
                let span = start.to(self.prev_span());
                Expr { id: self.next_id(), span, kind: ExprKind::Stmt(Box::new(Stmt { kind: StmtKind::If(s), span })) }
            }
            Tok::Kw("while" | "loop") => {
                let s = self.while_loop()?;
                let span = start.to(self.prev_span());
                Expr { id: self.next_id(), span, kind: ExprKind::Stmt(Box::new(Stmt { kind: s, span })) }
            }
            Tok::Kw("do") => {
                let s = self.do_loop()?;
                let span = start.to(self.prev_span());
                Expr { id: self.next_id(), span, kind: ExprKind::Stmt(Box::new(Stmt { kind: s, span })) }
            }
            Tok::Kw("for") => {
                let s = self.for_loop()?;
                let span = start.to(self.prev_span());
                Expr { id: self.next_id(), span, kind: ExprKind::Stmt(Box::new(Stmt { kind: s, span })) }
            }
            Tok::Kw("switch") => {
                let s = self.switch()?;
                let span = start.to(self.prev_span());
                Expr { id: self.next_id(), span, kind: ExprKind::Stmt(Box::new(Stmt { kind: StmtKind::Switch(s), span })) }
            }
            Tok::Punct("|" | "||") => self.pipe_closure()?,
            Tok::Template(parts) => {
                self.bump();
                let mut out = Vec::new();
                for part in parts {
                    match part {
                        TplPart::Text(t, _) => {
                            if !t.is_empty() {
                                out.push(TemplatePart::Text(t.to_string()));
                            }
                        }
                        TplPart::Code(tokens, span) => out.push(TemplatePart::Code(self.template_code(tokens, *span)?)),
                    }
                }
                Expr { id: self.next_id(), kind: ExprKind::Template(out), span: start }
            }
            Tok::Punct("[") => self.array_literal()?,
            Tok::Punct("#{") => self.map_literal(true)?,
            // `none`, and `null`, its spelling until step 3 of
            // `docs/11-next.md`, which still reads the same.
            Tok::Kw("none") => {
                self.bump();
                Expr { id: self.next_id(), kind: ExprKind::Null, span: start }
            }
            Tok::Reserved(r) if *r == "null" => {
                self.bump();
                Expr { id: self.next_id(), kind: ExprKind::Null, span: start }
            }
            Tok::Ident(name) => {
                self.bump();
                Expr { id: self.next_id(), kind: ExprKind::Var(name.to_string()), span: start }
            }
            Tok::Reserved(r) => {
                let callable = matches!(self.peek_nth(1), Tok::Punct("(" | "()" | "!"));
                if callable && CALLABLE_RESERVED.contains(r) {
                    self.bump();
                    Expr { id: self.next_id(), kind: ExprKind::Var(r.to_string()), span: start }
                } else if *r == "this" {
                    if !self.in_fn {
                        return self.error("`this` can only be used inside a function");
                    }
                    self.bump();
                    Expr { id: self.next_id(), kind: ExprKind::This, span: start }
                } else {
                    return self.error(format!("`{r}` is a reserved word, so it cannot be used as a name here"));
                }
            }
            other => return self.error(format!("{} cannot start an expression", other.describe())),
        };
        self.postfix(root)
    }

    /// The statements inside `${ … }`, from the tokens the lexer kept.
    fn template_code(&mut self, tokens: &'t [Token<'t>], span: Span) -> PResult<Block> {
        let mut inner = Parser {
            toks: tokens,
            src: self.src,
            pos: 0,
            opts: self.opts,
            in_fn: self.in_fn,
            breakable: self.breakable,
            global: false,
            no_pipe: false,
            vars: std::mem::take(&mut self.vars),
            fns: std::mem::take(&mut self.fns),
            half_gt: false,
            // The ids go on from the outer parse's, so none repeats.
            next: std::cell::Cell::new(self.next.get()),
        };
        let result = (|| {
            let mut stmts = Vec::new();
            loop {
                if inner.at_eof() {
                    break;
                }
                let stmt = inner.stmt()?;
                if matches!(stmt.kind, StmtKind::Empty) {
                    continue;
                }
                let need_semicolon = !stmt.is_self_terminated();
                stmts.push(stmt);
                match inner.peek() {
                    Tok::Eof => break,
                    Tok::Punct(";") => {
                        inner.bump();
                    }
                    _ if !need_semicolon => {}
                    other => {
                        return inner.error(format!("expecting `;` to end this statement, found {}", other.describe()))
                    }
                }
            }
            Ok(stmts)
        })();
        self.vars = std::mem::take(&mut inner.vars);
        self.fns = std::mem::take(&mut inner.fns);
        self.next.set(inner.next.get());
        Ok(Block { stmts: result?, span })
    }

    fn postfix(&mut self, mut lhs: Expr) -> PResult<Expr> {
        loop {
            let is_var = matches!(lhs.kind, ExprKind::Var(_) | ExprKind::Path(_));
            match self.peek() {
                Tok::Punct("(" | "()") if is_var => {
                    let callee = callee_of(&lhs);
                    let args = self.call_args()?;
                    let span = lhs.span.to(self.prev_span());
                    // `setInterval(ms) { … }`: a block after the call is its body.
                    if matches!(&callee[..], [one] if one.name == "setInterval") && self.at_punct("{") {
                        let body = self.with_flags(false, false, false, |p| p.block())?;
                        lhs = Expr { id: self.next_id(), span: span.to(body.span), kind: ExprKind::Interval { args, body } };
                        continue;
                    }
                    lhs = Expr { id: self.next_id(), span, kind: ExprKind::Call { callee, args, bang: false } };
                }
                Tok::Punct("!") if is_var => {
                    if matches!(lhs.kind, ExprKind::Path(_)) {
                        return self.error("`!` cannot call a function in a module");
                    }
                    self.bump();
                    if !matches!(self.peek(), Tok::Punct("(" | "()")) {
                        return self.error("expecting `(` and the arguments after `!`");
                    }
                    let callee = callee_of(&lhs);
                    let args = self.call_args()?;
                    lhs = Expr { id: self.next_id(), span: lhs.span.to(self.prev_span()), kind: ExprKind::Call { callee, args, bang: true } };
                }
                Tok::Punct("::") if is_var => {
                    self.bump();
                    let next = self.var_name()?;
                    let mut path = callee_of(&lhs);
                    path.push(next);
                    lhs = Expr { id: self.next_id(), span: lhs.span.to(self.prev_span()), kind: ExprKind::Path(path) };
                }
                Tok::Punct(b @ ("[" | "?[")) => {
                    self.bump();
                    let saved = self.no_pipe;
                    self.no_pipe = false;
                    let index = self.expr();
                    self.no_pipe = saved;
                    let index = index?;
                    if !self.eat_punct("]") {
                        return self.error("expecting `]` to match the `[` of this index");
                    }
                    lhs = Expr {
                        id: self.next_id(),
                        span: lhs.span.to(self.prev_span()),
                        kind: ExprKind::Index { base: Box::new(lhs), index: Box::new(index), optional: *b == "?[" },
                    };
                }
                Tok::Punct(d @ ("." | "?.")) => {
                    self.bump();
                    let optional = *d == "?.";
                    let name = match self.peek() {
                        Tok::Ident(n) => Ident { name: n.to_string(), span: self.bump().span },
                        Tok::Reserved(r) if METHOD_RESERVED.contains(r) && matches!(self.peek_nth(1), Tok::Punct("(" | "()")) => {
                            Ident { name: r.to_string(), span: self.bump().span }
                        }
                        Tok::Reserved(r) => {
                            return self.error(format!("`{r}` is a reserved word, so it cannot be used as a name here"))
                        }
                        other => return self.error(format!("expecting a field name after `{d}`, found {}", other.describe())),
                    };
                    if matches!(self.peek(), Tok::Punct("(" | "()")) {
                        let args = self.call_args()?;
                        lhs = Expr {
                        id: self.next_id(),
                            span: lhs.span.to(self.prev_span()),
                            kind: ExprKind::Method { recv: Box::new(lhs), name, args, optional },
                        };
                    } else if self.at_punct("::") {
                        return self.error("a module path cannot follow a `.`");
                    } else if self.at_punct("!") && matches!(self.peek_nth(1), Tok::Punct("(" | "()")) {
                        // `a.f!()`: the fork parses it and drops the `!`.
                        self.bump();
                        let args = self.call_args()?;
                        lhs = Expr {
                        id: self.next_id(),
                            span: lhs.span.to(self.prev_span()),
                            kind: ExprKind::Method { recv: Box::new(lhs), name, args, optional },
                        };
                    } else {
                        lhs = Expr { id: self.next_id(), span: lhs.span.to(name.span), kind: ExprKind::Field { base: Box::new(lhs), name, optional } };
                    }
                }
                _ => return Ok(lhs),
            }
        }
    }

    fn call_args(&mut self) -> PResult<Vec<Expr>> {
        let mut args = Vec::new();
        if self.eat_punct("()") {
            return Ok(args);
        }
        self.bump(); // `(`
        let saved = self.no_pipe;
        self.no_pipe = false;
        let result = (|| loop {
            if !self.at_punct(")") {
                args.push(self.expr()?);
            }
            match self.peek() {
                Tok::Punct(")") => {
                    self.bump();
                    return Ok(());
                }
                Tok::Punct(",") => {
                    self.bump();
                }
                Tok::Eof => return self.error("this call's arguments are never closed with `)`"),
                other => return self.error(format!("expecting `,` between arguments, found {}", other.describe())),
            }
        })();
        self.no_pipe = saved;
        result?;
        Ok(args)
    }

    fn array_literal(&mut self) -> PResult<Expr> {
        let start = self.bump().span; // `[`
        let saved = self.no_pipe;
        self.no_pipe = false;
        let mut items = Vec::new();
        let result = (|| loop {
            match self.peek() {
                Tok::Punct("]") => {
                    self.bump();
                    return Ok(());
                }
                Tok::Eof => return self.error("this array is never closed with `]`"),
                _ => items.push(self.expr()?),
            }
            match self.peek() {
                Tok::Punct(",") => {
                    self.bump();
                }
                Tok::Punct("]") => {}
                Tok::Eof => return self.error("this array is never closed with `]`"),
                other => return self.error(format!("expecting `,` between the items of this array, found {}", other.describe())),
            }
        })();
        self.no_pipe = saved;
        result?;
        Ok(Expr { id: self.next_id(), kind: ExprKind::Array(items), span: start.to(self.prev_span()) })
    }

    fn map_literal(&mut self, hash: bool) -> PResult<Expr> {
        let start = self.bump().span; // `{` or `#{`
        let saved = self.no_pipe;
        self.no_pipe = false;
        let mut entries: Vec<MapEntry> = Vec::new();
        let result = (|| loop {
            match self.peek() {
                Tok::Punct("}") => {
                    self.bump();
                    return Ok(());
                }
                Tok::Eof => return self.error("this map is never closed with `}`"),
                _ => {}
            }
            let (key, quoted) = match self.peek() {
                Tok::Ident(k) => (Ident { name: k.to_string(), span: self.bump().span }, false),
                Tok::Str(k) => (Ident { name: k.to_string(), span: self.bump().span }, true),
                Tok::Reserved(r) => return self.error(format!("`{r}` is a reserved word; write it as a string key, \"{r}\"")),
                _ if entries.is_empty() => return self.error("expecting `}` to close this map"),
                _ => return self.error("expecting a key: a name or a string"),
            };
            if entries.iter().any(|e| e.key.name == key.name) {
                return Err(SyntaxError { message: format!("`{}` is in this map twice", key.name), span: key.span });
            }
            if !self.eat_punct(":") {
                return self.error(format!("expecting `:` after `{}` in this map", key.name));
            }
            let value = self.expr()?;
            entries.push(MapEntry { key, quoted, value });
            match self.peek() {
                Tok::Punct(",") => {
                    self.bump();
                }
                Tok::Punct("}") => {}
                Tok::Ident(_) => return self.error("expecting `,` between the entries of this map"),
                _ => return self.error("expecting `}` to close this map"),
            }
        })();
        self.no_pipe = saved;
        result?;
        Ok(Expr { id: self.next_id(), kind: ExprKind::Map { entries, hash }, span: start.to(self.prev_span()) })
    }

    /// The fork's `arrow_params_len`: whether an arrow function starts here.
    /// Pure lookahead.
    fn arrow_params_len(&self) -> Option<usize> {
        let arrow_at = |n: usize| self.peek_nth(n).is_punct("=>");
        match self.peek() {
            Tok::Ident(_) | Tok::Punct("()") => return arrow_at(1).then_some(2),
            Tok::Punct("(") => {}
            _ => return None,
        }
        let mut n = 1;
        loop {
            if !matches!(self.peek_nth(n), Tok::Ident(_)) {
                return None;
            }
            n += 1;
            if self.peek_nth(n).is_punct(":") {
                n = self.type_end(n + 1)?;
            }
            match self.peek_nth(n) {
                Tok::Punct(",") => n += 1,
                Tok::Punct(")") => return arrow_at(n + 1).then_some(n + 2),
                _ => return None,
            }
        }
    }

    fn arrow(&mut self) -> PResult<Expr> {
        let start = self.span();
        let mut params: Vec<Param> = Vec::new();
        match &self.bump().tok {
            Tok::Ident(n) => params.push(Param { name: Ident { name: n.to_string(), span: start }, ty: None }),
            Tok::Punct("()") => {}
            _ => loop {
                let t = self.bump();
                let Tok::Ident(n) = t.tok else { unreachable!("checked by the lookahead") };
                if params.iter().any(|p| p.name.name == *n) {
                    return Err(SyntaxError { message: format!("two parameters are named `{n}`"), span: t.span });
                }
                let ty = if self.eat_punct(":") { Some(self.take_type("`:`")?) } else { None };
                params.push(Param { name: Ident { name: n.to_string(), span: t.span }, ty });
                if self.bump().tok.is_punct(")") {
                    break;
                }
            },
        }
        self.bump(); // `=>`
        self.closure_body(start, params, true)
    }

    fn pipe_closure(&mut self) -> PResult<Expr> {
        let start = self.span();
        let mut params: Vec<Param> = Vec::new();
        if !self.bump().tok.is_punct("||") {
            loop {
                let t = self.bump();
                match t.tok {
                    Tok::Punct("|") => break,
                    Tok::Ident(n) => {
                        if params.iter().any(|p| p.name.name == *n) {
                            return Err(SyntaxError { message: format!("two parameters are named `{n}`"), span: t.span });
                        }
                        params.push(Param { name: Ident { name: n.to_string(), span: t.span }, ty: None });
                    }
                    Tok::Reserved(r) => {
                        return Err(SyntaxError { message: format!("`{r}` is a reserved word, so a parameter cannot take it as its name"), span: t.span })
                    }
                    Tok::Kw(k) => {
                        return Err(SyntaxError { message: format!("`{k}` is a keyword, so a parameter cannot take it as its name"), span: t.span })
                    }
                    _ => return Err(SyntaxError { message: "expecting `|` to close the parameters of this closure".into(), span: t.span }),
                }
                let t = self.bump();
                match t.tok {
                    Tok::Punct("|") => break,
                    Tok::Punct(",") => {}
                    _ => return Err(SyntaxError { message: "expecting `,` between the parameters of this closure".into(), span: t.span }),
                }
            }
        }
        self.closure_body(start, params, false)
    }

    fn closure_body(&mut self, start: Span, params: Vec<Param>, arrow: bool) -> PResult<Expr> {
        let body = self.in_new_scope(|p| {
            for param in &params {
                p.vars.push((param.name.span.text(p.src), false));
            }
            p.with_flags(true, false, false, |p| p.stmt())
        })?;
        Ok(Expr { id: self.next_id(), span: start.to(body.span), kind: ExprKind::Closure { params, body: Box::new(body), arrow } })
    }

    /// The fork's `brace_opens_map`: `{` then a name or string then `:`, or
    /// `{}` where `empty_is_map`.
    fn brace_opens_map(&self, empty_is_map: bool) -> bool {
        if !self.at_punct("{") {
            return false;
        }
        match self.peek_nth(1) {
            Tok::Punct("}") => empty_is_map,
            Tok::Ident(_) | Tok::Str(_) => self.peek_nth(2).is_punct(":"),
            _ => false,
        }
    }

    fn ident(&mut self) -> PResult<Ident> {
        match self.peek() {
            Tok::Ident(n) => Ok(Ident { name: n.to_string(), span: self.bump().span }),
            other => self.error(format!("expecting a name, found {}", other.describe())),
        }
    }

    /// The fork's `parse_var_name`.
    fn var_name(&mut self) -> PResult<Ident> {
        match self.peek() {
            Tok::Ident(n) => Ok(Ident { name: n.to_string(), span: self.bump().span }),
            Tok::Reserved(r) => self.error(format!("`{r}` is a reserved word, so it cannot be used as a name here")),
            other => self.error(format!("expecting a name, found {}", other.describe())),
        }
    }

    // ----- types ------------------------------------------------------------
    //
    // `type_end` and its helpers are the fork's recognizer, lookahead only; the
    // `take_*` functions read what it accepted into a `TypeExpr`.

    fn type_end(&self, at: usize) -> Option<usize> {
        let mut half = false;
        let end = self.type_end_half(at, &mut half)?;
        (!half).then_some(end)
    }

    /// [`Self::type_end`], inside a type argument list: `half` is set when the
    /// type ends at a `>>` whose first `>` closed it, and the second is left for
    /// the list around it.
    fn type_end_half(&self, at: usize, half: &mut bool) -> Option<usize> {
        let mut i = at;
        if self.peek_nth(i).is_punct("|") {
            i += 1;
        }
        i = self.type_postfix_end(i, half)?;
        while !*half && self.peek_nth(i).is_punct("|") {
            i = self.type_postfix_end(i + 1, half)?;
        }
        Some(i)
    }

    fn type_postfix_end(&self, at: usize, half: &mut bool) -> Option<usize> {
        let mut i = self.type_primary_end(at, half)?;
        if *half {
            return Some(i);
        }
        loop {
            match self.peek_nth(i) {
                Tok::Punct("[" | "?[") if self.peek_nth(i + 1).is_punct("]") => i += 2,
                Tok::Punct("?") => i += 1,
                _ => return Some(i),
            }
        }
    }

    fn type_primary_end(&self, at: usize, half: &mut bool) -> Option<usize> {
        match self.peek_nth(at) {
            Tok::Ident(_) if self.peek_nth(at + 1).is_punct("<") => {
                let mut i = at + 2;
                let mut inner = false;
                loop {
                    i = self.type_end_half(i, &mut inner)?;
                    if inner || !self.peek_nth(i).is_punct(",") {
                        break;
                    }
                    i += 1;
                }
                match self.peek_nth(i) {
                    // The inner list took the first `>`; this one takes both.
                    Tok::Punct(">>") if inner => Some(i + 1),
                    Tok::Punct(">") if !inner => Some(i + 1),
                    Tok::Punct(">>") => {
                        *half = true;
                        Some(i)
                    }
                    _ => None,
                }
            }
            Tok::Ident(_) | Tok::Str(_) => Some(at + 1),
            Tok::Reserved(r) if *r == "null" || *r == "void" => Some(at + 1),
            Tok::Kw("none") => Some(at + 1),
            Tok::Punct("{") => self.type_record_end(at + 1),
            Tok::Punct("()") if self.peek_nth(at + 1).is_punct("=>") => self.type_end(at + 2),
            Tok::Punct("(") => {
                let mut i = at + 1;
                let mut count = 0;
                loop {
                    i = self.type_end(i)?;
                    count += 1;
                    match self.peek_nth(i) {
                        Tok::Punct(",") => i += 1,
                        Tok::Punct(")") => break,
                        _ => return None,
                    }
                }
                if self.peek_nth(i + 1).is_punct("=>") {
                    self.type_end(i + 2)
                } else if count == 1 {
                    Some(i + 1)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn type_record_end(&self, at: usize) -> Option<usize> {
        let mut i = at;
        match self.peek_nth(i) {
            Tok::Punct("}") => return Some(i + 1),
            Tok::Punct("[") => {
                if !matches!(self.peek_nth(i + 1), Tok::Ident(_))
                    || !self.peek_nth(i + 2).is_punct("]")
                    || !self.peek_nth(i + 3).is_punct(":")
                {
                    return None;
                }
                i = self.type_end(i + 4)?;
                if matches!(self.peek_nth(i), Tok::Punct("," | ";")) {
                    i += 1;
                }
                return self.peek_nth(i).is_punct("}").then_some(i + 1);
            }
            _ => {}
        }
        loop {
            match self.peek_nth(i) {
                Tok::Ident(_) | Tok::Str(_) => i += 1,
                _ => return None,
            }
            if self.peek_nth(i).is_punct("?") {
                i += 1;
            }
            if !self.peek_nth(i).is_punct(":") {
                return None;
            }
            i = self.type_end(i + 1)?;
            match self.peek_nth(i) {
                Tok::Punct("," | ";") => {
                    i += 1;
                    if self.peek_nth(i).is_punct("}") {
                        return Some(i + 1);
                    }
                }
                Tok::Punct("}") => return Some(i + 1),
                _ => return None,
            }
        }
    }

    /// Whether a `type` declaration's name, at `at`, is followed by type
    /// parameters, if any, and then `=`.
    fn type_params_then_eq(&self, at: usize) -> bool {
        let mut i = at;
        if self.peek_nth(i).is_punct("<") {
            i += 1;
            loop {
                if !matches!(self.peek_nth(i), Tok::Ident(_)) {
                    return false;
                }
                i += 1;
                match self.peek_nth(i) {
                    Tok::Punct(",") => i += 1,
                    Tok::Punct(">") => break,
                    _ => return false,
                }
            }
            i += 1;
        }
        self.peek_nth(i).is_punct("=")
    }

    /// `<T, U>` after a function's or a type's name, and the span it covers.
    /// Nothing, when there is no `<`.
    fn type_params(&mut self) -> PResult<(Vec<Ident>, Option<Span>)> {
        if !self.at_punct("<") {
            return Ok((Vec::new(), None));
        }
        let open = self.bump().span;
        let mut params: Vec<Ident> = Vec::new();
        loop {
            let name = match self.peek() {
                Tok::Ident(_) => self.ident()?,
                other => return self.error(format!("expecting a type parameter's name, found {}", other.describe())),
            };
            if params.iter().any(|p| p.name == name.name) {
                return Err(SyntaxError { message: format!("two type parameters are named `{}`", name.name), span: name.span });
            }
            params.push(name);
            if self.eat_punct(",") {
                continue;
            }
            let close = self.expect_punct(">", "to end the type parameters")?;
            return Ok((params, Some(open.to(close))));
        }
    }

    /// Read a type, once the recognizer has said one is there.
    fn take_type(&mut self, after: &str) -> PResult<TypeExpr> {
        if self.type_end(0).is_none() {
            return self.error(format!("expecting a type after {after}"));
        }
        Ok(self.type_union())
    }

    fn type_union(&mut self) -> TypeExpr {
        let start = self.span();
        self.eat_punct("|");
        let mut members = vec![self.type_postfix()];
        while !self.half_gt && self.eat_punct("|") {
            members.push(self.type_postfix());
        }
        if members.len() == 1 {
            return members.pop().unwrap();
        }
        TypeExpr { span: start.to(self.prev_span()), kind: TypeKind::Union(members) }
    }

    fn type_postfix(&mut self) -> TypeExpr {
        let mut ty = self.type_primary();
        loop {
            let span = ty.span;
            if self.half_gt {
                return ty;
            }
            if self.at_punct("[") && self.peek_nth(1).is_punct("]") {
                self.bump();
                self.bump();
                ty = TypeExpr { span: span.to(self.prev_span()), kind: TypeKind::Array(Box::new(ty)) };
            } else if self.at_punct("?[") && self.peek_nth(1).is_punct("]") {
                // `T?[]`, whose `?[` is one token.
                self.bump();
                let optional = TypeExpr { span, kind: TypeKind::Optional(Box::new(ty)) };
                self.bump();
                ty = TypeExpr { span: span.to(self.prev_span()), kind: TypeKind::Array(Box::new(optional)) };
            } else if self.at_punct("?") {
                self.bump();
                ty = TypeExpr { span: span.to(self.prev_span()), kind: TypeKind::Optional(Box::new(ty)) };
            } else {
                return ty;
            }
        }
    }

    fn type_primary(&mut self) -> TypeExpr {
        let start = self.span();
        let t = self.bump();
        let kind = match &t.tok {
            Tok::Ident(n) if self.at_punct("<") => {
                self.bump();
                let mut args = vec![self.type_union()];
                while !self.half_gt && self.eat_punct(",") {
                    args.push(self.type_union());
                }
                if self.half_gt {
                    // The inner list took the first `>` of this `>>`.
                    self.half_gt = false;
                    self.bump();
                } else if self.at_punct(">>") {
                    self.half_gt = true;
                } else {
                    self.bump(); // `>`
                }
                TypeKind::Generic { name: n.to_string(), args }
            }
            Tok::Ident(n) => TypeKind::Name(n.to_string()),
            Tok::Str(s) => TypeKind::Literal(s.to_string()),
            // `void` is reserved as a name, and is a type.
            Tok::Reserved(r) if *r == "void" => TypeKind::Name("void".to_string()),
            Tok::Reserved(_) | Tok::Kw("none") => TypeKind::Null,
            Tok::Punct("()") => {
                self.bump(); // `=>`
                TypeKind::Function(Vec::new(), Box::new(self.type_union()))
            }
            Tok::Punct("(") => {
                let mut items = vec![self.type_union()];
                while self.eat_punct(",") {
                    items.push(self.type_union());
                }
                self.bump(); // `)`
                if self.eat_punct("=>") {
                    TypeKind::Function(items, Box::new(self.type_union()))
                } else {
                    TypeKind::Paren(Box::new(items.pop().unwrap()))
                }
            }
            Tok::Punct("{") => {
                if self.eat_punct("[") {
                    let key = match &self.bump().tok {
                        Tok::Ident(k) => k,
                        _ => unreachable!("checked by the recognizer"),
                    };
                    self.bump(); // `]`
                    self.bump(); // `:`
                    let value = self.type_union();
                    if matches!(self.peek(), Tok::Punct("," | ";")) {
                        self.bump();
                    }
                    self.bump(); // `}`
                    TypeKind::Dict { key: key.to_string(), value: Box::new(value) }
                } else {
                    let mut fields = Vec::new();
                    while !self.eat_punct("}") {
                        let t = self.bump();
                        let name = match &t.tok {
                            Tok::Ident(n) => Ident { name: n.to_string(), span: t.span },
                            Tok::Str(n) => Ident { name: n.to_string(), span: t.span },
                            _ => unreachable!("checked by the recognizer"),
                        };
                        let optional = self.eat_punct("?");
                        self.bump(); // `:`
                        let ty = self.type_union();
                        fields.push(TypeField { name, optional, ty });
                        if matches!(self.peek(), Tok::Punct("," | ";")) {
                            self.bump();
                        }
                    }
                    TypeKind::Record(fields)
                }
            }
            _ => unreachable!("checked by the recognizer"),
        };
        TypeExpr { span: start.to(self.prev_span()), kind }
    }
}

fn callee_of(e: &Expr) -> Vec<Ident> {
    match &e.kind {
        ExprKind::Var(n) => vec![Ident { name: n.clone(), span: e.span }],
        ExprKind::Path(p) => p.clone(),
        _ => unreachable!("only a name is called"),
    }
}

enum Literal {
    Number,
    Range,
    Other,
}

/// Whether a `switch` case is a literal, as the fork's `get_literal_value`
/// decides.
fn literal_kind(e: &Expr) -> Option<Literal> {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Float(_) => Some(Literal::Number),
        ExprKind::Str(_) | ExprKind::Char(_) | ExprKind::Bool(_) | ExprKind::Unit => Some(Literal::Other),
        ExprKind::Array(items) => items.iter().all(|i| literal_kind(i).is_some()).then_some(Literal::Other),
        ExprKind::Map { entries, .. } => entries.iter().all(|e| literal_kind(&e.value).is_some()).then_some(Literal::Other),
        ExprKind::Binary { op: ".." | "..=", lhs, rhs }
            if matches!(lhs.kind, ExprKind::Int(_)) && matches!(rhs.kind, ExprKind::Int(_)) =>
        {
            Some(Literal::Range)
        }
        _ => None,
    }
}
