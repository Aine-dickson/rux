//! Rust code a Rux app calls from script: step 8 of `docs/archive/language-build-log.md`.
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

pub mod app;
pub mod background;
pub mod files;
mod exec;
pub mod links;
pub mod location;
pub mod notifications;
pub mod permissions;
pub mod registry;
pub mod secrets;
mod signal;
mod value;

pub use exec::{block_on, panic_text, set_spawner, spawn, CatchUnwind, Task};
pub use signal::{notify, set_signal_waker, signal_epoch, Channel, Signal, SignalValue};
pub use registry::{declare_throws, install, is_channel, throws, BoxFuture, Call, Export, FieldSig, Interface, Item, ItemKind, Module, Sig};
pub use files::File;
pub use value::{field, Any, Error, FromRux, FromRuxRef, Handle, IntoError, IntoRux, Resource};

#[cfg(feature = "macros")]
pub use rux_native_macros::{export, init, resource};

/// What the attributes' generated code names. Not an API.
#[doc(hidden)]
pub mod __private {
    pub use crate::exec::CatchUnwind;
    pub use crate::signal::read_exported;
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

    /// An export's `Err(e)` held while its error is chosen: through
    /// [`crate::IntoError`] when `E` implements it, else through `Display`
    /// with the type's name as the `kind`. The pick is autoref specialisation:
    /// `(&Thrown::new(e)).rux_error()` finds [`ViaIntoError`] first (its
    /// `Self` is `Thrown<E>`, so `&self` matches the receiver as it is) and
    /// [`ViaDisplay`] (its `Self` is `&Thrown<E>`) only when that does not
    /// apply.
    pub struct Thrown<E>(std::cell::Cell<Option<E>>);

    impl<E> Thrown<E> {
        pub fn new(e: E) -> Thrown<E> {
            Thrown(std::cell::Cell::new(Some(e)))
        }
    }

    pub trait ViaIntoError {
        fn rux_error(&self) -> Error;
    }

    impl<E: crate::IntoError> ViaIntoError for Thrown<E> {
        fn rux_error(&self) -> Error {
            let e = self.0.take().expect("an error is chosen once").into_error();
            if cfg!(debug_assertions) && !E::KINDS.is_empty() && !E::KINDS.contains(&e.kind.as_str()) {
                eprintln!(
                    "rux: `{}` threw kind `{}`, which its `KINDS` does not list; `rux check` trusts that list",
                    std::any::type_name::<E>(),
                    e.kind
                );
            }
            e
        }
    }

    pub trait ViaDisplay {
        fn rux_error(&self) -> Error;
    }

    impl<E: std::fmt::Display> ViaDisplay for &Thrown<E> {
        fn rux_error(&self) -> Error {
            Error::from_rust(self.0.take().expect("an error is chosen once"))
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
