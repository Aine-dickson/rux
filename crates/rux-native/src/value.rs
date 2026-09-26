//! The value that crosses between Rux and Rust, and the conversions each
//! Rust type makes to and from it.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;

/// A Rux value on the Rust side of the boundary, and Rux's `any`: a native
/// function taking or giving one is typed `any`.
///
/// Unlike the runtime's display value it keeps `int` and `float` apart, so a
/// whole number crosses as one.
#[derive(Clone, Debug, Default)]
pub enum Any {
    #[default]
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Any>),
    /// A map or a record, by field name.
    Map(BTreeMap<String, Any>),
    /// A `#[rux::resource]`.
    Resource(Handle),
}

impl Any {
    /// The kind of value, as a message names it.
    pub fn kind(&self) -> &'static str {
        match self {
            Any::None => "none",
            Any::Bool(_) => "bool",
            Any::Int(_) => "int",
            Any::Float(_) => "float",
            Any::Str(_) => "string",
            Any::Array(_) => "array",
            Any::Map(_) => "map",
            Any::Resource(_) => "resource",
        }
    }
}

impl PartialEq for Any {
    fn eq(&self, other: &Any) -> bool {
        match (self, other) {
            (Any::None, Any::None) => true,
            (Any::Bool(a), Any::Bool(b)) => a == b,
            (Any::Int(a), Any::Int(b)) => a == b,
            (Any::Float(a), Any::Float(b)) => a == b,
            (Any::Int(a), Any::Float(b)) | (Any::Float(b), Any::Int(a)) => *a as f64 == *b,
            (Any::Str(a), Any::Str(b)) => a == b,
            (Any::Array(a), Any::Array(b)) => a == b,
            (Any::Map(a), Any::Map(b)) => a == b,
            (Any::Resource(a), Any::Resource(b)) => a.same(b),
            _ => false,
        }
    }
}

/// A `#[rux::resource]` held by Rux: shared, opaque, and never forged, since
/// it is the `Arc` itself and not a number standing for one.
#[derive(Clone)]
pub struct Handle {
    inner: Arc<dyn std::any::Any + Send + Sync>,
    name: &'static str,
}

impl Handle {
    pub fn new<T: Resource>(value: T) -> Handle {
        Handle::from_arc(Arc::new(value))
    }

    pub fn from_arc<T: Resource>(value: Arc<T>) -> Handle {
        Handle { inner: value, name: T::NAME }
    }

    /// A handle of type `name` over something that is not the resource: what
    /// a declared, uncompiled export gives. No Rust method can take it, since
    /// [`Handle::get`] finds no `T` in it.
    pub(crate) fn placeholder(name: &'static str, value: Arc<dyn std::any::Any + Send + Sync>) -> Handle {
        Handle { inner: value, name }
    }

    /// Its Rux type's name.
    pub fn type_name(&self) -> &'static str {
        self.name
    }

    /// The resource, when it is a `T`.
    pub fn get<T: Resource>(&self) -> Option<Arc<T>> {
        Arc::clone(&self.inner).downcast::<T>().ok()
    }

    /// Whether the two are one resource.
    pub fn same(&self, other: &Handle) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}(..)", self.name)
    }
}

/// What `#[rux::resource]` implements: a Rust value Rux holds but cannot see
/// into. Shared between every place Rux keeps it, so it is `Sync`; a method
/// that changes it does so through a lock of its own.
pub trait Resource: Send + Sync + 'static {
    /// Its type's name in Rux.
    const NAME: &'static str;
}

/// What a native call throws: a Rux `Error`, `{ message, kind }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub message: String,
    pub kind: String,
}

impl Error {
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Error {
        Error { message: message.into(), kind: kind.into() }
    }

    /// A Rust error returned as `Err(e)`: its text, and its type's name as
    /// the `kind`.
    pub fn from_rust<E: fmt::Display>(e: E) -> Error {
        Error::new(short_type_name::<E>(), e.to_string())
    }

    /// A value that is not what a parameter takes.
    pub fn wrong(what: &str, got: &Any) -> Error {
        Error::new("type", format!("expected {what}, got {}", got.kind()))
    }

    pub fn overflow(value: impl fmt::Display, into: &str) -> Error {
        Error::new("overflow", format!("{value} does not fit in {into}"))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.kind)
    }
}

impl std::error::Error for Error {}

/// `std::io::error::Error` as `Error`, `my_app::DbError<X>` as `DbError`.
fn short_type_name<T: ?Sized>() -> String {
    let full = std::any::type_name::<T>();
    let base = full.split('<').next().unwrap_or(full);
    base.rsplit("::").next().unwrap_or(base).to_string()
}

/// A Rust value made from a Rux one.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot come from Rux by value",
    note = "a `#[rux::resource]` is taken as `&{Self}`; a struct crosses when it is `#[rux::export]`ed"
)]
pub trait FromRux: Sized {
    fn from_rux(v: Any) -> Result<Self, Error>;
}

/// A Rust value handed to Rux.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be handed to Rux",
    note = "Rux takes the types listed in rux-native's README, `#[rux::export]` structs and `#[rux::resource]`s"
)]
pub trait IntoRux {
    fn into_rux(self) -> Result<Any, Error>;
}

/// What a parameter written `&T` is held in while the call runs: a `String`
/// for `&str`, a `Vec` for `&[T]`, the `Arc` for a resource.
#[diagnostic::on_unimplemented(message = "`&{Self}` cannot come from Rux")]
pub trait FromRuxRef {
    type Holder: std::ops::Deref<Target = Self>;
    fn hold(v: Any) -> Result<Self::Holder, Error>;
}

impl FromRux for Any {
    fn from_rux(v: Any) -> Result<Self, Error> {
        Ok(v)
    }
}

impl IntoRux for Any {
    fn into_rux(self) -> Result<Any, Error> {
        Ok(self)
    }
}

impl FromRux for bool {
    fn from_rux(v: Any) -> Result<Self, Error> {
        match v {
            Any::Bool(b) => Ok(b),
            other => Err(Error::wrong("a bool", &other)),
        }
    }
}

impl IntoRux for bool {
    fn into_rux(self) -> Result<Any, Error> {
        Ok(Any::Bool(self))
    }
}

/// A whole number from Rux. A `float` with nothing after the point is taken
/// too, because a number that went through the runtime's state comes back as
/// one (as an index does in the interpreter).
fn whole(v: Any) -> Result<i64, Error> {
    match v {
        Any::Int(i) => Ok(i),
        Any::Float(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.007_199_254_740_992e15 => Ok(f as i64),
        other => Err(Error::wrong("an int", &other)),
    }
}

macro_rules! integers {
    ($($t:ty),*) => {$(
        impl FromRux for $t {
            fn from_rux(v: Any) -> Result<Self, Error> {
                let i = whole(v)?;
                <$t>::try_from(i).map_err(|_| Error::overflow(i, stringify!($t)))
            }
        }
        impl IntoRux for $t {
            fn into_rux(self) -> Result<Any, Error> {
                i64::try_from(self).map(Any::Int).map_err(|_| Error::overflow(self, "a Rux int"))
            }
        }
    )*};
}
integers!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

impl FromRux for f64 {
    fn from_rux(v: Any) -> Result<Self, Error> {
        match v {
            Any::Float(f) => Ok(f),
            Any::Int(i) => Ok(i as f64),
            other => Err(Error::wrong("a float", &other)),
        }
    }
}

impl IntoRux for f64 {
    fn into_rux(self) -> Result<Any, Error> {
        Ok(Any::Float(self))
    }
}

impl FromRux for f32 {
    fn from_rux(v: Any) -> Result<Self, Error> {
        f64::from_rux(v).map(|f| f as f32)
    }
}

impl IntoRux for f32 {
    fn into_rux(self) -> Result<Any, Error> {
        Ok(Any::Float(self as f64))
    }
}

impl FromRux for String {
    fn from_rux(v: Any) -> Result<Self, Error> {
        match v {
            Any::Str(s) => Ok(s),
            other => Err(Error::wrong("a string", &other)),
        }
    }
}

impl IntoRux for String {
    fn into_rux(self) -> Result<Any, Error> {
        Ok(Any::Str(self))
    }
}

impl IntoRux for &str {
    fn into_rux(self) -> Result<Any, Error> {
        Ok(Any::Str(self.to_string()))
    }
}

impl FromRuxRef for str {
    type Holder = String;
    fn hold(v: Any) -> Result<String, Error> {
        String::from_rux(v)
    }
}

impl IntoRux for () {
    fn into_rux(self) -> Result<Any, Error> {
        Ok(Any::None)
    }
}

impl<T: FromRux> FromRux for Option<T> {
    fn from_rux(v: Any) -> Result<Self, Error> {
        match v {
            Any::None => Ok(None),
            v => T::from_rux(v).map(Some),
        }
    }
}

impl<T: IntoRux> IntoRux for Option<T> {
    fn into_rux(self) -> Result<Any, Error> {
        match self {
            None => Ok(Any::None),
            Some(v) => v.into_rux(),
        }
    }
}

impl<T: FromRux> FromRux for Vec<T> {
    fn from_rux(v: Any) -> Result<Self, Error> {
        match v {
            Any::Array(items) => items.into_iter().map(T::from_rux).collect(),
            other => Err(Error::wrong("an array", &other)),
        }
    }
}

impl<T: IntoRux> IntoRux for Vec<T> {
    fn into_rux(self) -> Result<Any, Error> {
        self.into_iter().map(T::into_rux).collect::<Result<_, _>>().map(Any::Array)
    }
}

impl<T: FromRux> FromRuxRef for [T] {
    type Holder = Vec<T>;
    fn hold(v: Any) -> Result<Vec<T>, Error> {
        Vec::from_rux(v)
    }
}

impl<T: Clone + IntoRux> IntoRux for &[T] {
    fn into_rux(self) -> Result<Any, Error> {
        self.to_vec().into_rux()
    }
}

fn entries(v: Any) -> Result<BTreeMap<String, Any>, Error> {
    match v {
        Any::Map(m) => Ok(m),
        other => Err(Error::wrong("a map", &other)),
    }
}

impl<T: FromRux> FromRux for BTreeMap<String, T> {
    fn from_rux(v: Any) -> Result<Self, Error> {
        entries(v)?.into_iter().map(|(k, v)| Ok((k, T::from_rux(v)?))).collect()
    }
}

impl<T: IntoRux> IntoRux for BTreeMap<String, T> {
    fn into_rux(self) -> Result<Any, Error> {
        self.into_iter().map(|(k, v)| Ok((k, v.into_rux()?))).collect::<Result<_, _>>().map(Any::Map)
    }
}

impl<T: FromRux, S: std::hash::BuildHasher + Default> FromRux for HashMap<String, T, S> {
    fn from_rux(v: Any) -> Result<Self, Error> {
        entries(v)?.into_iter().map(|(k, v)| Ok((k, T::from_rux(v)?))).collect()
    }
}

impl<T: IntoRux, S> IntoRux for HashMap<String, T, S> {
    fn into_rux(self) -> Result<Any, Error> {
        self.into_iter().map(|(k, v)| Ok((k, v.into_rux()?))).collect::<Result<_, _>>().map(Any::Map)
    }
}

impl<T: Resource> IntoRux for Arc<T> {
    fn into_rux(self) -> Result<Any, Error> {
        Ok(Any::Resource(Handle::from_arc(self)))
    }
}

impl<T: Resource> FromRux for Arc<T> {
    fn from_rux(v: Any) -> Result<Self, Error> {
        match v {
            Any::Resource(h) => h.get::<T>().ok_or_else(|| {
                Error::new("type", format!("expected a {}, got a {}", T::NAME, h.type_name()))
            }),
            other => Err(Error::wrong(T::NAME, &other)),
        }
    }
}

/// A record's field, for the code `#[rux::export]` writes for a struct: an
/// absent field reads as `none`, which an `Option` takes and anything else
/// refuses by name.
pub fn field<T: FromRux>(m: &mut BTreeMap<String, Any>, name: &str) -> Result<T, Error> {
    T::from_rux(m.remove(name).unwrap_or(Any::None))
        .map_err(|e| Error::new(e.kind, format!("field `{name}`: {}", e.message)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_are_checked_both_ways() {
        assert_eq!(u8::from_rux(Any::Int(255)), Ok(255));
        assert_eq!(u8::from_rux(Any::Int(256)).unwrap_err().kind, "overflow");
        assert_eq!(u64::MAX.into_rux().unwrap_err().kind, "overflow");
        assert_eq!(i32::from_rux(Any::Float(3.0)), Ok(3));
        assert_eq!(i32::from_rux(Any::Float(3.5)).unwrap_err().kind, "type");
    }

    #[test]
    fn containers_convert_through() {
        let v = vec![Some(1i64), None].into_rux().unwrap();
        assert_eq!(v, Any::Array(vec![Any::Int(1), Any::None]));
        assert_eq!(Vec::<Option<i64>>::from_rux(v), Ok(vec![Some(1), None]));
        let mut m = HashMap::new();
        m.insert("a".to_string(), 1.5f64);
        let back: BTreeMap<String, f64> = FromRux::from_rux(m.into_rux().unwrap()).unwrap();
        assert_eq!(back["a"], 1.5);
    }

    struct Db(#[allow(dead_code)] u8);
    impl Resource for Db {
        const NAME: &'static str = "Db";
    }
    #[derive(Debug)]
    struct Other;
    impl Resource for Other {
        const NAME: &'static str = "Other";
    }

    #[test]
    fn a_resource_is_itself_and_nothing_else() {
        let h = Arc::new(Db(1)).into_rux().unwrap();
        assert_eq!(h, h.clone());
        assert_ne!(h, Arc::new(Db(1)).into_rux().unwrap());
        assert!(Arc::<Db>::from_rux(h.clone()).is_ok());
        let e = Arc::<Other>::from_rux(h).unwrap_err();
        assert_eq!((e.kind.as_str(), e.message.as_str()), ("type", "expected a Other, got a Db"));
    }

    #[test]
    fn a_rust_error_is_named_by_its_type() {
        let e = Error::from_rust("x".parse::<i32>().unwrap_err());
        assert_eq!(e.kind, "ParseIntError");
    }
}
