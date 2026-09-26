//! What a type name stands for, and which types fit where: the two questions
//! every reader of the IR asks, answered one way.
//!
//! The same rules as the checker's (`rux-script`, `check.rs`), which reads
//! them from its own copy while it works; this one is for everything after
//! it, from the verifier on.

use std::collections::HashMap;

use crate::types::Type;

/// How deep a type is unfolded before giving up, so a recursive type such as
/// `type Tree = { kids: Tree[] }` cannot send a question round forever.
const MAX_DEPTH: usize = 24;

/// The types a unit can name.
#[derive(Clone, Debug, Default)]
pub struct Table {
    decls: HashMap<String, (Vec<String>, Type)>,
}

impl Table {
    pub fn new<'a>(decls: impl IntoIterator<Item = &'a (String, Vec<String>, Type)>) -> Self {
        Table { decls: decls.into_iter().map(|(n, p, t)| (n.clone(), (p.clone(), t.clone()))).collect() }
    }

    /// `ty` with its declared names unfolded until it is not one. A name that
    /// is not declared, or a generic one named without its arguments, is
    /// `any`.
    pub fn resolve(&self, ty: &Type) -> Type {
        let mut ty = ty.clone();
        for _ in 0..MAX_DEPTH {
            match ty {
                Type::Named(ref name) => match self.decls.get(name) {
                    Some((params, t)) if params.is_empty() => ty = t.clone(),
                    _ => return Type::Any,
                },
                Type::Generic(ref name, ref args) => match self.decls.get(name) {
                    Some((params, body)) if params.len() == args.len() => {
                        let given = params.iter().cloned().zip(args.iter().cloned()).collect();
                        ty = body.substitute(&given);
                    }
                    _ => return Type::Any,
                },
                _ => return ty,
            }
        }
        Type::Any
    }

    /// Whether a value of type `from` may go where a `to` is wanted.
    pub fn assignable(&self, from: &Type, to: &Type) -> bool {
        self.assignable_at(from, to, 0)
    }

    fn assignable_at(&self, from: &Type, to: &Type, depth: usize) -> bool {
        if depth > MAX_DEPTH || from == to {
            return true;
        }
        let from = self.resolve(from);
        let to = self.resolve(to);
        let d = depth + 1;
        match (&from, &to) {
            (Type::Any, _) | (_, Type::Any) => true,
            // Inside a generic function nothing is known of a parameter, and
            // every use of one was checked where it was written.
            (Type::Param(_), _) | (_, Type::Param(_)) => true,
            (Type::Union(members), _) => members.iter().all(|m| self.assignable_at(m, &to, d)),
            (_, Type::Union(members)) => members.iter().any(|m| self.assignable_at(&from, m, d)),
            (Type::Int, Type::Float) => true,
            (Type::BoolLit(_), Type::Bool) => true,
            (Type::Null, Type::Void) => true,
            (Type::Literal(_), Type::String) => true,
            (Type::Literal(a), Type::Literal(b)) => a == b,
            (Type::Array(a), Type::Array(b)) => self.assignable_at(a, b, d),
            (Type::Record(have), Type::Record(want)) => want.iter().all(|w| match have.iter().find(|h| h.name == w.name) {
                Some(h) => (w.optional || !h.optional) && self.assignable_at(&h.ty, &w.ty, d),
                None => w.optional,
            }),
            (Type::Record(have), Type::Dict(value)) => have.iter().all(|h| self.assignable_at(&h.ty, value, d)),
            (Type::Dict(a), Type::Dict(b)) => self.assignable_at(a, b, d),
            (Type::Function(fp, fr), Type::Function(tp, tr)) => {
                fp.len() <= tp.len()
                    && fp.iter().zip(tp).all(|(f, t)| self.assignable_at(t, f, d))
                    && (matches!(**tr, Type::Null | Type::Void) || self.assignable_at(fr, tr, d))
            }
            _ => false,
        }
    }

    /// Whether `ty` is, once unfolded, exactly `want`'s kind: `int` for
    /// [`Type::Int`], any text for [`Type::String`].
    pub fn is(&self, ty: &Type, want: &Type) -> bool {
        match (self.resolve(ty), want) {
            (Type::Literal(_), Type::String) => true,
            (Type::BoolLit(_), Type::Bool) => true,
            (t, w) => t == *w,
        }
    }
}
