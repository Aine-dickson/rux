//! CSS's maths functions: `calc()`, `min()`, `max()`, `clamp()`, and the
//! stepped and sign-related ones (`round()`, `mod()`, `rem()`, `abs()`,
//! `sign()`), as an expression over lengths, percentages and numbers.
//!
//! Units other than `px`, `rem`, `%` and the viewport ones have been put in
//! pixels by the cascade before an expression is read, so they are not
//! known here.

/// A parsed maths expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Calc {
    /// A plain number: `2` in `calc(2 * 8px)`.
    Num(f32),
    Px(f32),
    /// A percentage, as a fraction: `50%` is `0.5`.
    Pct(f32),
    Vw(f32),
    Vh(f32),
    Vmin(f32),
    Vmax(f32),
    Add(Box<Calc>, Box<Calc>),
    Sub(Box<Calc>, Box<Calc>),
    Mul(Box<Calc>, Box<Calc>),
    Div(Box<Calc>, Box<Calc>),
    Min(Vec<Calc>),
    Max(Vec<Calc>),
    Clamp(Box<Calc>, Box<Calc>, Box<Calc>),
    Round(Rounding, Box<Calc>, Box<Calc>),
    Mod(Box<Calc>, Box<Calc>),
    Rem(Box<Calc>, Box<Calc>),
    Abs(Box<Calc>),
    Sign(Box<Calc>),
}

/// `round()`'s strategy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    Nearest,
    Up,
    Down,
    ToZero,
}

/// What an expression needs to be worked out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Needs {
    /// A percentage, so a basis.
    pub basis: bool,
    /// A viewport unit, so the window's size.
    pub viewport: bool,
    /// Any length at all: without one the result is a plain number.
    pub length: bool,
}

impl Calc {
    /// Works the expression out, with `basis` what a percentage is of and
    /// `vp` the window. `None` if it needs one it was not given.
    pub fn eval(&self, basis: Option<f32>, vp: Option<(f32, f32)>) -> Option<f32> {
        let two = |a: &Calc, b: &Calc| Some((a.eval(basis, vp)?, b.eval(basis, vp)?));
        Some(match self {
            Calc::Num(n) | Calc::Px(n) => *n,
            Calc::Pct(p) => p * basis?,
            Calc::Vw(v) => vp?.0 * v / 100.0,
            Calc::Vh(v) => vp?.1 * v / 100.0,
            Calc::Vmin(v) => vp?.0.min(vp?.1) * v / 100.0,
            Calc::Vmax(v) => vp?.0.max(vp?.1) * v / 100.0,
            Calc::Add(a, b) => two(a, b).map(|(a, b)| a + b)?,
            Calc::Sub(a, b) => two(a, b).map(|(a, b)| a - b)?,
            Calc::Mul(a, b) => two(a, b).map(|(a, b)| a * b)?,
            Calc::Div(a, b) => two(a, b).map(|(a, b)| a / b)?,
            Calc::Min(xs) => xs.iter().map(|x| x.eval(basis, vp)).try_fold(f32::INFINITY, |m, x| Some(m.min(x?)))?,
            Calc::Max(xs) => xs.iter().map(|x| x.eval(basis, vp)).try_fold(f32::NEG_INFINITY, |m, x| Some(m.max(x?)))?,
            // CSS's clamp: the minimum wins over the maximum.
            Calc::Clamp(lo, v, hi) => {
                let (lo, v, hi) = (lo.eval(basis, vp)?, v.eval(basis, vp)?, hi.eval(basis, vp)?);
                v.min(hi).max(lo)
            }
            Calc::Round(how, a, b) => {
                let (a, b) = two(a, b)?;
                if b == 0.0 {
                    f32::NAN
                } else {
                    let q = a / b;
                    let r = match how {
                        Rounding::Nearest => (q + 0.5).floor(),
                        Rounding::Up => q.ceil(),
                        Rounding::Down => q.floor(),
                        Rounding::ToZero => q.trunc(),
                    };
                    r * b
                }
            }
            // `mod()` takes the divisor's sign and `rem()` the dividend's.
            Calc::Mod(a, b) => two(a, b).map(|(a, b)| a - b * (a / b).floor())?,
            Calc::Rem(a, b) => two(a, b).map(|(a, b)| a % b)?,
            Calc::Abs(a) => a.eval(basis, vp)?.abs(),
            Calc::Sign(a) => {
                let v = a.eval(basis, vp)?;
                if v == 0.0 { 0.0 } else { v.signum() }
            }
        })
    }

    pub fn needs(&self) -> Needs {
        let mut n = Needs::default();
        self.walk(&mut |c| match c {
            Calc::Pct(_) => {
                n.basis = true;
                n.length = true;
            }
            Calc::Vw(_) | Calc::Vh(_) | Calc::Vmin(_) | Calc::Vmax(_) => {
                n.viewport = true;
                n.length = true;
            }
            Calc::Px(_) => n.length = true,
            _ => {}
        });
        n
    }

    fn walk(&self, f: &mut impl FnMut(&Calc)) {
        f(self);
        match self {
            Calc::Add(a, b)
            | Calc::Sub(a, b)
            | Calc::Mul(a, b)
            | Calc::Div(a, b)
            | Calc::Round(_, a, b)
            | Calc::Mod(a, b)
            | Calc::Rem(a, b) => {
                a.walk(f);
                b.walk(f);
            }
            Calc::Min(xs) | Calc::Max(xs) => xs.iter().for_each(|x| x.walk(f)),
            Calc::Clamp(a, b, c) => {
                a.walk(f);
                b.walk(f);
                c.walk(f);
            }
            Calc::Abs(a) | Calc::Sign(a) => a.walk(f),
            _ => {}
        }
    }
}

/// An expression kept by [`intern`]: an index rather than the expression's
/// address, so a [`Len`](crate::Len) holding one stays eight bytes. `Style`
/// holds a dozen lengths, and every byte of it is on the stack once per
/// element in a debug build (watchlist #41).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CalcRef(u32);

impl CalcRef {
    /// The expression.
    pub fn get(self) -> &'static Calc {
        table().read().unwrap_or_else(|e| e.into_inner()).list[self.0 as usize]
    }
}

impl std::fmt::Debug for CalcRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.get())
    }
}

#[derive(Default)]
struct Table {
    list: Vec<&'static Calc>,
    by_text: std::collections::HashMap<String, u32>,
}

fn table() -> &'static std::sync::RwLock<Table> {
    static TABLE: std::sync::OnceLock<std::sync::RwLock<Table>> = std::sync::OnceLock::new();
    TABLE.get_or_init(Default::default)
}

/// The expression `text` reads as, kept for the life of the process, so a
/// [`Len::Calc`](crate::Len::Calc) can name it and Taffy can be handed its
/// address: Taffy works a `calc()` out itself, when it knows the basis, by
/// asking [`resolve`] about it.
///
/// Each distinct expression is kept once, by its text. That is what makes
/// two frames' `calc(100% - 20px)` the same value to the layout, so a box
/// that did not change keeps its cached layout, and it is also the cost:
/// an expression written by a script with a new number every frame (a
/// `:style` with `calc(${p}% - 8px)`) is a new one each time and is never
/// let go.
pub fn intern(text: &str) -> Option<CalcRef> {
    if let Some(&i) = table().read().unwrap_or_else(|e| e.into_inner()).by_text.get(text) {
        return Some(CalcRef(i));
    }
    let calc = parse(text)?;
    let mut t = table().write().unwrap_or_else(|e| e.into_inner());
    if let Some(&i) = t.by_text.get(text) {
        return Some(CalcRef(i));
    }
    let i = t.list.len() as u32;
    t.list.push(Box::leak(Box::new(calc)));
    t.by_text.insert(text.to_string(), i);
    Some(CalcRef(i))
}

/// Taffy's question: what the `calc()` at `ptr`, from [`handle`], comes to
/// against `basis`. The cascade has already put every viewport unit in
/// pixels, so the basis is all an expression can still need.
pub fn resolve(ptr: *const (), basis: f32) -> f32 {
    // SAFETY: Taffy is only ever handed addresses from `handle`, of
    // expressions `intern` leaked, which live for the rest of the process.
    let calc = unsafe { &*(ptr as *const Calc) };
    calc.eval(Some(basis), None).unwrap_or(0.0)
}

/// The address Taffy is given for an interned expression.
pub fn handle(c: CalcRef) -> *const () {
    c.get() as *const Calc as *const ()
}

/// The maths functions, so a caller can find one in a value.
pub const FUNCTIONS: &[&str] = &["calc", "min", "max", "clamp", "round", "mod", "rem", "abs", "sign"];

/// One maths function call, `calc(…)`, `min(…)` and the rest, the whole of
/// `s`. `None` if it is not one or does not parse.
pub fn parse(s: &str) -> Option<Calc> {
    let mut p = Parser { tokens: tokenize(s)?, at: 0 };
    if !matches!(p.peek(), Some(Token::Func(f)) if FUNCTIONS.contains(&f.as_str())) {
        return None;
    }
    let c = p.unit()?;
    (p.at == p.tokens.len()).then_some(c)
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Value(Calc),
    Op(char),
    Open,
    Close,
    Comma,
    Func(String),
    Word(String),
}

fn tokenize(s: &str) -> Option<Vec<Token>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // A sign belongs to the number when it cannot be an operator: at the
        // start, or after another operator, a `(` or a `,`. CSS also asks for
        // spaces around `+` and `-` as operators, which this does not check.
        let signed = (c == b'-' || c == b'+')
            && b.get(i + 1).is_some_and(|n| n.is_ascii_digit() || *n == b'.')
            && matches!(out.last(), None | Some(Token::Op(_) | Token::Open | Token::Comma | Token::Func(_)));
        if c.is_ascii_digit() || c == b'.' || signed {
            let start = i;
            i += 1;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.' || ((b[i] == b'e' || b[i] == b'E') && b.get(i + 1).is_some_and(u8::is_ascii_digit))) {
                i += 1;
            }
            let n: f32 = s[start..i].parse().ok()?;
            let ustart = i;
            while i < b.len() && (b[i].is_ascii_alphabetic() || b[i] == b'%') {
                i += 1;
            }
            out.push(Token::Value(match s[ustart..i].to_ascii_lowercase().as_str() {
                "" => Calc::Num(n),
                "px" => Calc::Px(n),
                "rem" => Calc::Px(n * 16.0),
                "%" => Calc::Pct(n / 100.0),
                "vw" => Calc::Vw(n),
                "vh" => Calc::Vh(n),
                "vmin" => Calc::Vmin(n),
                "vmax" => Calc::Vmax(n),
                _ => return None,
            }));
            continue;
        }
        match c {
            b'+' | b'-' | b'*' | b'/' => out.push(Token::Op(c as char)),
            b'(' => out.push(Token::Open),
            b')' => out.push(Token::Close),
            b',' => out.push(Token::Comma),
            _ if c.is_ascii_alphabetic() => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-') {
                    i += 1;
                }
                let word = s[start..i].to_ascii_lowercase();
                if b.get(i) == Some(&b'(') {
                    out.push(Token::Func(word));
                    i += 1;
                } else {
                    out.push(Token::Word(word));
                }
                continue;
            }
            _ => return None,
        }
        i += 1;
    }
    Some(out)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.at).cloned();
        self.at += 1;
        t
    }

    fn expect(&mut self, t: Token) -> Option<()> {
        (self.next()? == t).then_some(())
    }

    fn sum(&mut self) -> Option<Calc> {
        let mut left = self.product()?;
        while let Some(Token::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.at += 1;
            let right = self.product()?;
            left = if op == '+' { Calc::Add(left.into(), right.into()) } else { Calc::Sub(left.into(), right.into()) };
        }
        Some(left)
    }

    fn product(&mut self) -> Option<Calc> {
        let mut left = self.unit()?;
        while let Some(Token::Op(op @ ('*' | '/'))) = self.peek().cloned() {
            self.at += 1;
            let right = self.unit()?;
            left = if op == '*' { Calc::Mul(left.into(), right.into()) } else { Calc::Div(left.into(), right.into()) };
        }
        Some(left)
    }

    fn unit(&mut self) -> Option<Calc> {
        match self.next()? {
            Token::Value(v) => Some(v),
            Token::Open => {
                let v = self.sum()?;
                self.expect(Token::Close)?;
                Some(v)
            }
            Token::Func(name) => {
                let v = self.call(&name)?;
                self.expect(Token::Close)?;
                Some(v)
            }
            _ => None,
        }
    }

    /// A function's arguments, after its `(` and up to its `)`.
    fn call(&mut self, name: &str) -> Option<Calc> {
        if name == "round" {
            let how = match self.peek() {
                Some(Token::Word(w)) => {
                    let how = match w.as_str() {
                        "nearest" => Rounding::Nearest,
                        "up" => Rounding::Up,
                        "down" => Rounding::Down,
                        "to-zero" => Rounding::ToZero,
                        _ => return None,
                    };
                    self.at += 1;
                    self.expect(Token::Comma)?;
                    how
                }
                _ => Rounding::Nearest,
            };
            let a = self.sum()?;
            // The interval defaults to 1 when the value is a number.
            let b = if self.peek() == Some(&Token::Comma) {
                self.at += 1;
                self.sum()?
            } else {
                Calc::Num(1.0)
            };
            return Some(Calc::Round(how, a.into(), b.into()));
        }
        let mut args = vec![self.sum()?];
        while self.peek() == Some(&Token::Comma) {
            self.at += 1;
            args.push(self.sum()?);
        }
        match name {
            "min" => return Some(Calc::Min(args)),
            "max" => return Some(Calc::Max(args)),
            _ => {}
        }
        let mut it = args.into_iter().map(Box::new);
        Some(match (name, it.next(), it.next(), it.next(), it.next()) {
            ("calc", Some(a), None, ..) => *a,
            ("clamp", Some(a), Some(b), Some(c), None) => Calc::Clamp(a, b, c),
            ("mod", Some(a), Some(b), None, _) => Calc::Mod(a, b),
            ("rem", Some(a), Some(b), None, _) => Calc::Rem(a, b),
            ("abs", Some(a), None, ..) => Calc::Abs(a),
            ("sign", Some(a), None, ..) => Calc::Sign(a),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(s: &str, basis: Option<f32>) -> Option<f32> {
        parse(s).unwrap_or_else(|| panic!("{s} did not parse")).eval(basis, Some((1000.0, 500.0)))
    }

    #[test]
    fn the_functions_work_out_as_css_says() {
        assert_eq!(eval("calc(100% - 20px)", Some(300.0)), Some(280.0));
        assert_eq!(eval("calc(100% - 20px)", None), None);
        assert_eq!(eval("calc(2 * (10px + 1rem))", None), Some(52.0));
        assert_eq!(eval("calc(-1 * 8px)", None), Some(-8.0));
        assert_eq!(eval("calc(10px - -2px)", None), Some(12.0));
        assert_eq!(eval("min(100%, 500px)", Some(800.0)), Some(500.0));
        assert_eq!(eval("max(50%, 200px, 3rem)", Some(300.0)), Some(200.0));
        assert_eq!(eval("clamp(1rem, 2.5vw, 2rem)", None), Some(25.0));
        // The minimum wins over a maximum below it.
        assert_eq!(eval("clamp(30px, 10px, 20px)", None), Some(30.0));
        assert_eq!(eval("calc(50vw + 10vmin)", None), Some(550.0));
        assert_eq!(eval("calc(min(10px, 5px) + max(1px, 2px))", None), Some(7.0));
        assert_eq!(eval("round(117px, 25px)", None), Some(125.0));
        assert_eq!(eval("round(down, 117px, 25px)", None), Some(100.0));
        assert_eq!(eval("round(2.5)", None), Some(3.0));
        assert_eq!(eval("mod(-18px, 5px)", None), Some(2.0));
        assert_eq!(eval("rem(-18px, 5px)", None), Some(-3.0));
        assert_eq!(eval("calc(abs(-4px) * sign(-3))", None), Some(-4.0));
        assert_eq!(eval("calc(1e1px)", None), Some(10.0));
    }

    #[test]
    fn what_is_not_maths_is_refused() {
        for bad in ["calc(10px +)", "calc(1em + 1px)", "translate(10px)", "calc(10px", "min()", "clamp(1px, 2px)", "calc(1px) 2px"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn needs_says_what_is_missing() {
        assert_eq!(parse("calc(1 + 2)").unwrap().needs(), Needs::default());
        let n = parse("min(100%, 50vw)").unwrap().needs();
        assert!(n.basis && n.viewport && n.length);
        assert!(parse("calc(2px * 3)").unwrap().needs().length);
    }
}

#[cfg(test)]
mod size_tests {
    /// Watchlist #41: a `Len` that carries maths is no bigger than one
    /// that does not.
    #[test]
    fn a_length_stays_eight_bytes() {
        assert_eq!(std::mem::size_of::<crate::Len>(), 8);
    }
}
