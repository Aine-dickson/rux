//! Rust code a Rux app calls from script: step 8 of `docs/11-next.md`.
//!
//! An app's `native/` crate depends on this one as `rux` and marks what Rux
//! may call with [`export`]; a type Rux holds without seeing into is a
//! [`resource`]. The CLI reads the crate's source for the signatures, so
//! `rux check` and the editor know them without a build, and generates the
//! code that installs each export into the [`registry`] before the first
//! document loads.
//!
//! Everything crosses as an [`Any`] and is converted at the boundary by
//! [`FromRux`] and [`IntoRux`], which is where an integer that does not fit
//! throws `kind` `"overflow"` instead of wrapping.
//!
//! A Rust program that runs the shell itself can register a module by hand
//! with [`Module`], which is what the attributes generate.

mod exec;
pub mod registry;
mod value;

pub use exec::{block_on, panic_text, set_spawner, spawn, CatchUnwind, Task};
pub use registry::{install, BoxFuture, Call, Export, FieldSig, Interface, Item, ItemKind, Module, Sig};
pub use value::{field, Any, Error, FromRux, FromRuxRef, Handle, IntoRux, Resource};

#[cfg(feature = "macros")]
pub use rux_native_macros::{export, init, resource};

/// What the attributes' generated code names. Not an API.
#[doc(hidden)]
pub mod __private {
    pub use crate::exec::CatchUnwind;
    pub use std::collections::BTreeMap;
    pub use std::sync::Arc;

    use crate::{Any, Error, IntoRux};

    /// A struct's or a resource's name in Rux, for the methods of an
    /// exported `impl`.
    pub trait Named {
        const NAME: &'static str;
    }

    /// The `i`th argument, taken out of the list.
    pub fn arg(args: &mut [crate::Any], i: usize) -> crate::Any {
        std::mem::take(&mut args[i])
    }

    /// A call's arguments counted before anything is converted.
    pub fn arity(name: &str, args: &[Any], n: usize) -> Result<(), Error> {
        if args.len() == n {
            Ok(())
        } else {
            Err(Error::new("type", format!("{name} takes {n} arguments, was given {}", args.len())))
        }
    }

    /// A value returned by a native function.
    pub fn ok<T: IntoRux>(v: T) -> Result<Any, Error> {
        v.into_rux()
    }

    /// A `Result` returned by a native function: its `Err` thrown.
    pub fn result<T: IntoRux, E: std::fmt::Display>(r: Result<T, E>) -> Result<Any, Error> {
        match r {
            Ok(v) => v.into_rux(),
            Err(e) => Err(Error::from_rust(e)),
        }
    }

    /// Run a synchronous export with a panic caught and thrown as `kind`
    /// `"panic"`.
    pub fn guarded(f: impl FnOnce() -> Result<Any, Error>) -> Result<Any, Error> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(r) => r,
            Err(p) => Err(Error::new("panic", crate::panic_text(&*p))),
        }
    }

    /// An async export's future with its panic caught the same way.
    pub async fn guarded_async(f: impl std::future::Future<Output = Result<Any, Error>>) -> Result<Any, Error> {
        match CatchUnwind::new(f).await {
            Ok(r) => r,
            Err(p) => Err(Error::new("panic", p)),
        }
    }
}
