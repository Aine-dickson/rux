//! Rux's typed IR, and the types it is written in.
//!
//! Step 4 of `docs/11-next.md`: between the checker and every backend sits
//! one representation of what a Rux file means. [`ir`] is that
//! representation, [`verify`] checks the rules it promises, [`print`] shows
//! it as text, and [`table`] answers what a type name stands for and what
//! fits where. [`types`] is the type language itself, shared with the checker
//! in `rux-script`.
//!
//! The IR is built from a checked script by `rux-script` (its `lower`
//! module), since that is where the checker is. Nothing here depends on the
//! rhai fork, so the interpreter and the code generator that read it will
//! not either.

pub mod ir;
pub mod print;
pub mod table;
pub mod types;
pub mod verify;
