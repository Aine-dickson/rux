//! The types an annotation can name, and the parser that reads one from text.
//!
//! Rux's parser reads an annotation in a script into its AST, and the checker
//! turns it into a [`Type`] by printing it back and parsing that here. A
//! `prop`'s type and an imported type arrive here as text. The grammar is the
//! one in `docs/10-types.md`, and `rux-syntax` accepts the same shapes: a test
//! below runs one list of types through both.

use std::fmt;

/// A type, as written.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    /// Any number.
    Number,
    /// A whole number, a subtype of [`Type::Number`].
    Int,
    /// Text.
    String,
    /// `true` or `false`.
    Bool,
    /// The empty value.
    Null,
    /// Anything, unchecked.
    Any,
    /// Exactly this string, as in `"all"`.
    Literal(String),
    /// `T[]`.
    Array(Box<Type>),
    /// `{ id: int, note?: string }`, with its fields in the order written.
    Record(Vec<Field>),
    /// `{ [string]: T }`.
    Dict(Box<Type>),
    /// `A | B`, and `T?` as `T | none`. Never nested and never of one member:
    /// [`Type::union`] flattens.
    Union(Vec<Type>),
    /// `(A, B) => R`.
    Function(Vec<Type>, Box<Type>),
    /// A declared type, by name. What it stands for is the checker's business.
    Named(String),
}

/// One field of a record type.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Field {
    /// The field's name.
    pub name: String,
    /// Written `name?:`, so it may be absent.
    pub optional: bool,
    /// Its type.
    pub ty: Type,
}

/// A type's text that is not a type. `at` is a byte offset into that text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeSyntaxError {
    /// What is wrong, as a sentence fragment that reads after the text.
    pub message: String,
    /// Where in the text it went wrong.
    pub at: usize,
}

impl fmt::Display for TypeSyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// The built-in type names, which are all lower case. A declared type starts
/// with a capital letter, so the two cannot collide.
pub const BUILT_IN: &[&str] = &["number", "int", "string", "bool", "none", "any"];

/// Names from other languages that mean a built-in here.
const SPELLED_ELSEWHERE: &[(&str, &str)] = &[
    ("boolean", "bool"),
    ("float", "number"),
    ("f64", "number"),
    ("i64", "int"),
    ("integer", "int"),
    ("str", "string"),
    ("String", "string"),
    ("undefined", "none"),
    ("unknown", "any"),
    ("void", "none"),
    ("object", "a record type such as `{ id: int }`"),
];

impl Type {
    /// A union of `members`, flattened, with duplicates dropped, and a single
    /// member returned as itself.
    pub fn union(members: impl IntoIterator<Item = Type>) -> Type {
        let mut out: Vec<Type> = Vec::new();
        for member in members {
            let parts = match member {
                Type::Union(inner) => inner,
                other => vec![other],
            };
            for part in parts {
                if !out.contains(&part) {
                    out.push(part);
                }
            }
        }
        // A member a wider one already covers says nothing: `int | number` is
        // `number`, and `"a" | string` is `string`.
        if out.contains(&Type::Number) {
            out.retain(|t| *t != Type::Int);
        }
        if out.contains(&Type::String) {
            out.retain(|t| !matches!(t, Type::Literal(_)));
        }
        if out.len() == 1 {
            out.pop().unwrap()
        } else {
            Type::Union(out)
        }
    }

    /// `T?`: this type or `none`.
    pub fn optional(self) -> Type {
        Type::union([self, Type::Null])
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Number => f.write_str("number"),
            Type::Int => f.write_str("int"),
            Type::String => f.write_str("string"),
            Type::Bool => f.write_str("bool"),
            Type::Null => f.write_str("none"),
            Type::Any => f.write_str("any"),
            Type::Literal(s) => write!(f, "{s:?}"),
            Type::Array(inner) => match **inner {
                Type::Union(_) | Type::Function(..) => write!(f, "({inner})[]"),
                _ => write!(f, "{inner}[]"),
            },
            Type::Record(fields) if fields.is_empty() => f.write_str("{}"),
            Type::Record(fields) => {
                f.write_str("{ ")?;
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    let mark = if field.optional { "?" } else { "" };
                    write!(f, "{}{mark}: {}", field.name, field.ty)?;
                }
                f.write_str(" }")
            }
            Type::Dict(value) => write!(f, "{{ [string]: {value} }}"),
            Type::Union(members) => {
                // `T | none` of one other member reads better as `T?`.
                if let [one, Type::Null] | [Type::Null, one] = members.as_slice() {
                    return match one {
                        Type::Union(_) | Type::Function(..) => write!(f, "({one})?"),
                        _ => write!(f, "{one}?"),
                    };
                }
                for (i, member) in members.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" | ")?;
                    }
                    match member {
                        Type::Function(..) => write!(f, "({member})")?,
                        _ => write!(f, "{member}")?,
                    }
                }
                Ok(())
            }
            Type::Function(params, result) => {
                f.write_str("(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, ") => {result}")
            }
            Type::Named(name) => f.write_str(name),
        }
    }
}

/// Read a type from its text.
pub fn parse_type(text: &str) -> Result<Type, TypeSyntaxError> {
    let tokens = tokenize(text)?;
    let mut p = Parser { tokens, at: 0, len: text.len() };
    let ty = p.union()?;
    match p.peek() {
        None => Ok(ty),
        Some((tok, at)) => Err(TypeSyntaxError {
            message: format!("`{}` does not continue a type here", tok.text()),
            at,
        }),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Name(String),
    Str(String),
    Punct(&'static str),
}

impl Tok {
    fn text(&self) -> String {
        match self {
            Tok::Name(s) => s.clone(),
            Tok::Str(s) => format!("{s:?}"),
            Tok::Punct(p) => (*p).to_string(),
        }
    }
}

fn tokenize(text: &str) -> Result<Vec<(Tok, usize)>, TypeSyntaxError> {
    const PUNCT: &[&str] = &["=>", "{", "}", "[", "]", "(", ")", "|", "?", ":", ",", ";"];
    let mut out = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some(&(at, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c.is_alphabetic() || c == '_' {
            let mut name = String::new();
            while let Some(&(_, c)) = chars.peek() {
                if c.is_alphanumeric() || c == '_' {
                    name.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push((Tok::Name(name), at));
        } else if c == '"' {
            chars.next();
            let mut s = String::new();
            loop {
                match chars.next() {
                    Some((_, '"')) => break,
                    Some((_, '\\')) => match chars.next() {
                        Some((_, 'n')) => s.push('\n'),
                        Some((_, 't')) => s.push('\t'),
                        Some((_, other)) => s.push(other),
                        None => break,
                    },
                    Some((_, other)) => s.push(other),
                    None => {
                        return Err(TypeSyntaxError { message: "a string is never closed".into(), at })
                    }
                }
            }
            out.push((Tok::Str(s), at));
        } else if let Some(p) = PUNCT.iter().find(|p| text[at..].starts_with(**p)) {
            for _ in 0..p.len() {
                chars.next();
            }
            out.push((Tok::Punct(p), at));
        } else {
            let message = if c == '\'' {
                "a literal type is a string in double quotes, `\"all\"`".to_string()
            } else if c == '<' {
                "there are no generics: an array of `T` is written `T[]`".to_string()
            } else if c == '&' {
                "there are no intersection types".to_string()
            } else {
                format!("`{c}` cannot appear in a type")
            };
            return Err(TypeSyntaxError { message, at });
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<(Tok, usize)>,
    at: usize,
    len: usize,
}

impl Parser {
    fn peek(&self) -> Option<(Tok, usize)> {
        self.tokens.get(self.at).cloned()
    }

    fn peek_nth(&self, n: usize) -> Option<Tok> {
        self.tokens.get(self.at + n).map(|(t, _)| t.clone())
    }

    fn is(&self, punct: &str) -> bool {
        matches!(self.peek(), Some((Tok::Punct(p), _)) if p == punct)
    }

    fn here(&self) -> usize {
        self.peek().map_or(self.len, |(_, at)| at)
    }

    fn expect(&mut self, punct: &str, what: &str) -> Result<(), TypeSyntaxError> {
        if self.is(punct) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.error(format!("expecting `{punct}` {what}")))
        }
    }

    fn error(&self, message: String) -> TypeSyntaxError {
        TypeSyntaxError { message, at: self.here() }
    }

    fn union(&mut self) -> Result<Type, TypeSyntaxError> {
        if self.is("|") {
            self.at += 1;
        }
        let mut members = vec![self.postfix()?];
        while self.is("|") {
            self.at += 1;
            members.push(self.postfix()?);
        }
        Ok(Type::union(members))
    }

    fn postfix(&mut self) -> Result<Type, TypeSyntaxError> {
        let mut ty = self.primary()?;
        loop {
            if self.is("[") && self.peek_nth(1) == Some(Tok::Punct("]")) {
                self.at += 2;
                ty = Type::Array(Box::new(ty));
            } else if self.is("?") {
                self.at += 1;
                ty = ty.optional();
            } else {
                return Ok(ty);
            }
        }
    }

    fn primary(&mut self) -> Result<Type, TypeSyntaxError> {
        let Some((tok, at)) = self.peek() else {
            return Err(self.error("expecting a type".into()));
        };
        match tok {
            Tok::Name(name) => {
                self.at += 1;
                named(&name).map_err(|message| TypeSyntaxError { message, at })
            }
            Tok::Str(s) => {
                self.at += 1;
                Ok(Type::Literal(s))
            }
            Tok::Punct("{") => {
                self.at += 1;
                self.record()
            }
            Tok::Punct("(") => {
                self.at += 1;
                let mut items = Vec::new();
                if !self.is(")") {
                    loop {
                        items.push(self.union()?);
                        if self.is(",") {
                            self.at += 1;
                        } else {
                            break;
                        }
                    }
                }
                self.expect(")", "to close the parentheses")?;
                if self.is("=>") {
                    self.at += 1;
                    let result = self.union()?;
                    Ok(Type::Function(items, Box::new(result)))
                } else if items.len() == 1 {
                    Ok(items.pop().unwrap())
                } else {
                    Err(self.error(
                        "expecting `=>`: a list of types in parentheses is a function's parameters"
                            .into(),
                    ))
                }
            }
            other => Err(TypeSyntaxError { message: format!("expecting a type, not `{}`", other.text()), at }),
        }
    }

    /// After the `{`.
    fn record(&mut self) -> Result<Type, TypeSyntaxError> {
        if self.is("}") {
            self.at += 1;
            return Ok(Type::Record(Vec::new()));
        }
        if self.is("[") {
            self.at += 1;
            let key_at = self.here();
            match self.peek() {
                Some((Tok::Name(n), _)) if n == "string" => self.at += 1,
                _ => {
                    return Err(TypeSyntaxError {
                        message: "a dictionary's keys are strings: write `{ [string]: T }`".into(),
                        at: key_at,
                    })
                }
            }
            self.expect("]", "after the key type")?;
            self.expect(":", "before the value type")?;
            let value = self.union()?;
            if self.is(",") || self.is(";") {
                self.at += 1;
            }
            self.expect("}", "to close the dictionary type")?;
            return Ok(Type::Dict(Box::new(value)));
        }
        let mut fields: Vec<Field> = Vec::new();
        loop {
            let (name, at) = match self.peek() {
                Some((Tok::Name(n) | Tok::Str(n), at)) => (n, at),
                _ => return Err(self.error("expecting a field name".into())),
            };
            self.at += 1;
            if fields.iter().any(|f| f.name == name) {
                return Err(TypeSyntaxError { message: format!("the field `{name}` is written twice"), at });
            }
            let optional = self.is("?");
            if optional {
                self.at += 1;
            }
            self.expect(":", &format!("after the field `{name}`"))?;
            let ty = self.union()?;
            fields.push(Field { name, optional, ty });
            if self.is(",") || self.is(";") {
                self.at += 1;
                if self.is("}") {
                    self.at += 1;
                    return Ok(Type::Record(fields));
                }
            } else if self.is("}") {
                self.at += 1;
                return Ok(Type::Record(fields));
            } else {
                return Err(self.error("expecting `,` or `}` after a field".into()));
            }
        }
    }
}

/// The type a name stands for: a built-in, or a declared type left for the
/// checker to resolve.
fn named(name: &str) -> Result<Type, String> {
    Ok(match name {
        "number" => Type::Number,
        "int" => Type::Int,
        "string" => Type::String,
        "bool" => Type::Bool,
        // `null` was its name until step 3 of `docs/11-next.md`.
        "none" | "null" => Type::Null,
        "any" => Type::Any,
        "Array" => return Err("there are no generics: an array of `T` is written `T[]`".into()),
        _ => {
            if let Some((_, here)) = SPELLED_ELSEWHERE.iter().find(|(n, _)| *n == name) {
                let here = if here.contains(' ') { here.to_string() } else { format!("`{here}`") };
                return Err(format!("there is no type `{name}`; Rux calls it {here}"));
            }
            if !name.starts_with(|c: char| c.is_uppercase()) {
                return Err(format!(
                    "there is no type `{name}`. The built-in types are {}, and a declared \
                     type starts with a capital letter",
                    BUILT_IN.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
                ));
            }
            Type::Named(name.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(fields: &[(&str, bool, Type)]) -> Type {
        Type::Record(
            fields
                .iter()
                .map(|(n, o, t)| Field { name: n.to_string(), optional: *o, ty: t.clone() })
                .collect(),
        )
    }

    #[test]
    fn every_shape_parses() {
        let t = |s: &str| parse_type(s).unwrap_or_else(|e| panic!("{s}: {e}"));
        assert_eq!(t("number"), Type::Number);
        assert_eq!(t("int[]"), Type::Array(Box::new(Type::Int)));
        assert_eq!(t("string?"), Type::Union(vec![Type::String, Type::Null]));
        assert_eq!(t("string?[]"), Type::Array(Box::new(Type::String.optional())));
        assert_eq!(t("\"all\" | \"open\""), Type::Union(vec![Type::Literal("all".into()), Type::Literal("open".into())]));
        assert_eq!(t("| \"a\" | \"b\""), t("\"a\" | \"b\""));
        assert_eq!(
            t("{ id: int, note?: string }"),
            rec(&[("id", false, Type::Int), ("note", true, Type::String)])
        );
        assert_eq!(t("{ id: int; }"), rec(&[("id", false, Type::Int)]));
        assert_eq!(t("{ \"x-y\": bool }"), rec(&[("x-y", false, Type::Bool)]));
        assert_eq!(t("{}"), Type::Record(Vec::new()));
        assert_eq!(t("{ [string]: bool }"), Type::Dict(Box::new(Type::Bool)));
        assert_eq!(
            t("(Task, int) => bool"),
            Type::Function(vec![Type::Named("Task".into()), Type::Int], Box::new(Type::Bool))
        );
        assert_eq!(t("() => null"), Type::Function(vec![], Box::new(Type::Null)));
        assert_eq!(
            t("(Task | string)[]"),
            Type::Array(Box::new(Type::Union(vec![Type::Named("Task".into()), Type::String])))
        );
        assert_eq!(t("string | string"), Type::String, "a union flattens");
    }

    #[test]
    fn display_reads_back_as_the_same_type() {
        for s in [
            "number",
            "int[]",
            "string?",
            "\"all\" | \"open\"",
            "{ id: int, note?: string }",
            "{ [string]: bool }",
            "(Task, int) => bool",
            "(Task | string)[]",
            "((int) => bool)?",
            "{ load: { state: \"done\", rows: Task[] } | { state: \"idle\" } }",
        ] {
            let ty = parse_type(s).unwrap();
            assert_eq!(parse_type(&ty.to_string()).unwrap(), ty, "{s} displayed as {ty}");
        }
    }

    #[test]
    fn a_name_from_another_language_is_named() {
        let e = |s: &str| parse_type(s).unwrap_err().message;
        assert!(e("boolean").contains("Rux calls it `bool`"), "{}", e("boolean"));
        assert!(e("Array<int>").contains("`T[]`"));
        assert!(e("task").contains("capital letter"));
        assert!(e("{ [int]: bool }").contains("keys are strings"));
        assert!(e("{ a: int, a: int }").contains("twice"));
        assert!(e("'a'").contains("double quotes"));
        assert!(e("(int, string)").contains("expecting `=>`"));
        assert!(e("int string").contains("does not continue"));
        assert!(e("").contains("expecting a type"));
    }

    /// Rux's parser and this one must read a type the same way. Every text
    /// here is parsed as an annotation by `rux-syntax`, and what it read,
    /// printed back, is parsed here to the same type.
    #[test]
    fn rux_syntax_and_this_parser_agree() {
        for s in [
            "number",
            "Task[]",
            "string?",
            "string?[]",
            "\"all\" | \"open\" | \"done\"",
            "| { state: \"idle\" } | { state: \"done\", rows: Task[] }",
            "{ id: int, title: string, note?: string, }",
            "{ [string]: bool }",
            "(Task, int) => bool",
            "() => null",
            "(Task | string)[]",
            "{ on: (string) => null }",
        ] {
            let script = rux_syntax::parse(&format!("let x: {s} = 1;"), Default::default())
                .unwrap_or_else(|e| panic!("rux-syntax refused `{s}`: {e}"));
            let rux_syntax::ast::StmtKind::Let { ty: Some(ty), .. } = &script.stmts[0].kind else { panic!("{s}") };
            let recorded = &rux_syntax::print::ty(ty);
            assert_eq!(
                parse_type(recorded).unwrap_or_else(|e| panic!("`{recorded}` from `{s}`: {e}")),
                parse_type(s).unwrap(),
                "{s}"
            );
        }
    }
}
