//! A record's layout: step 10, track (c) of `docs/11-next.md`.
//!
//! A record holds its fields' values in slots, in the order its type
//! declares them, and a [`Shape`] says which name is in which slot. A shape
//! is made once per record type and shared by every record of it
//! ([`Shape::closed`] interns them), so compiled code can tell a record's
//! layout by comparing one pointer, and the interpreter finds a field by
//! name through the shape, not in a map of its own per record.
//!
//! A closed shape is a declared record type's: writing a field it does not
//! declare is an error. An open one is a record nothing declared (`{ a: 1 }`
//! where no type says what it is, or a record back from the runtime, which
//! keeps its fields' order but not their types): writing a new field there
//! makes it a map, as it always was.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::types::Field;

/// Past this many fields a shape finds a name through a hash map rather than
/// by looking along its names.
const LINEAR: usize = 12;

#[derive(Debug)]
pub struct Shape {
    names: Vec<Rc<str>>,
    optional: Vec<bool>,
    closed: bool,
    index: Option<HashMap<Rc<str>, usize>>,
}

thread_local! {
    static TYPED: RefCell<HashMap<(Vec<Rc<str>>, Vec<bool>, bool), Rc<Shape>>> = RefCell::new(HashMap::new());
    /// The open shapes made last, so the records of one list (all with the
    /// same fields) share a shape as they come back from the runtime.
    static OPEN: RefCell<Vec<Rc<Shape>>> = const { RefCell::new(Vec::new()) };
}

/// How many recent open shapes are kept to be shared.
const OPEN_KEPT: usize = 8;

impl Shape {
    fn new(names: Vec<Rc<str>>, optional: Vec<bool>, closed: bool) -> Shape {
        let index = (names.len() > LINEAR).then(|| names.iter().enumerate().map(|(i, n)| (Rc::clone(n), i)).collect());
        Shape { names, optional, closed, index }
    }

    /// The shape of a declared record type with these fields, in their order:
    /// the same `Rc` for the same fields, on this thread.
    pub fn closed(fields: &[Field]) -> Rc<Shape> {
        Shape::typed(fields, true)
    }

    /// The shape of a record type no declaration names (one written inline,
    /// or a literal's own): its fields' order and which are optional, and
    /// open, as a record nothing declared is.
    pub fn open_typed(fields: &[Field]) -> Rc<Shape> {
        Shape::typed(fields, false)
    }

    fn typed(fields: &[Field], closed: bool) -> Rc<Shape> {
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        let optional: Vec<bool> = fields.iter().map(|f| f.optional).collect();
        Shape::interned(&names, &optional, closed)
    }

    /// The shape of `names`, which of them are `optional`, and whether it is
    /// `closed`: the same `Rc` for the same three, on this thread. What a
    /// literal's shape is made by, in the interpreter and in compiled code.
    pub fn interned(names: &[&str], optional: &[bool], closed: bool) -> Rc<Shape> {
        let names: Vec<Rc<str>> = names.iter().map(|n| Rc::from(*n)).collect();
        let key = (names, optional.to_vec(), closed);
        TYPED.with(|c| {
            let mut c = c.borrow_mut();
            if let Some(s) = c.get(&key) {
                return Rc::clone(s);
            }
            let s = Rc::new(Shape::new(key.0.clone(), key.1.clone(), closed));
            c.insert(key, Rc::clone(&s));
            s
        })
    }

    /// An open shape with these names, in this order: shared with a recent
    /// one of the same names when there is one.
    pub fn open<'a>(names: impl Iterator<Item = &'a str> + Clone) -> Rc<Shape> {
        OPEN.with(|o| {
            let mut o = o.borrow_mut();
            if let Some(s) = o.iter().find(|s| s.names.len() == names.clone().count() && s.names.iter().map(|n| &**n).eq(names.clone())) {
                return Rc::clone(s);
            }
            let names: Vec<Rc<str>> = names.map(Rc::from).collect();
            let n = names.len();
            let s = Rc::new(Shape::new(names, vec![false; n], false));
            if o.len() >= OPEN_KEPT {
                o.remove(0);
            }
            o.push(Rc::clone(&s));
            s
        })
    }

    /// The slot `name` is in.
    #[inline]
    pub fn slot(&self, name: &str) -> Option<usize> {
        match &self.index {
            Some(index) => index.get(name).copied(),
            None => self.names.iter().position(|n| &**n == name),
        }
    }

    /// The fields' names, in slot order.
    pub fn names(&self) -> &[Rc<str>] {
        &self.names
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Whether the field in slot `i` is optional (`pet?: Pet`): holding
    /// `none`, it is left out where the record is shown or walked.
    #[inline]
    pub fn optional(&self, i: usize) -> bool {
        self.optional[i]
    }

    /// A declared record type's: a field it does not declare cannot be
    /// written.
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Type;

    fn field(name: &str, optional: bool) -> Field {
        Field { name: name.to_string(), optional, ty: Type::Int }
    }

    #[test]
    fn one_shape_per_record_type() {
        let a = Shape::closed(&[field("id", false), field("pet", true)]);
        let b = Shape::closed(&[field("id", false), field("pet", true)]);
        let c = Shape::closed(&[field("pet", true), field("id", false)]);
        assert!(Rc::ptr_eq(&a, &b));
        assert!(!Rc::ptr_eq(&a, &c));
        assert_eq!(a.slot("pet"), Some(1));
        assert_eq!(c.slot("pet"), Some(0));
        assert!(a.optional(1) && !a.optional(0) && a.is_closed());
    }

    #[test]
    fn open_shapes_are_shared_while_recent() {
        let a = Shape::open(["b", "a"].into_iter());
        let b = Shape::open(["b", "a"].into_iter());
        assert!(Rc::ptr_eq(&a, &b));
        assert!(!a.is_closed());
        assert_eq!(a.slot("a"), Some(1));
        assert_eq!(a.slot("c"), None);
    }

    #[test]
    fn a_wide_shape_finds_names_by_hash() {
        let names: Vec<String> = (0..40).map(|i| format!("k{i}")).collect();
        let s = Shape::open(names.iter().map(String::as_str));
        assert_eq!(s.slot("k33"), Some(33));
        assert_eq!(s.slot("k40"), None);
    }
}
