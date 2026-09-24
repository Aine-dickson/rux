//! RUX DIVERGENCE: type annotations, kept beside the AST rather than in it.
//!
//! Rux script takes TypeScript's annotations (`let n: int = 0;`,
//! `fn f(x: Task): string`, `type Filter = "a" | "b";`). The parser recognises
//! them and records each one here; nothing in the evaluator reads this, so an
//! annotated script runs exactly as the same script with its annotations
//! deleted. See `DIVERGENCE.md`, item 8, and `docs/10-types.md`.

use crate::{ImmutableString, Position};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// One annotation, as written.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Annotation {
    /// What was annotated.
    pub kind: AnnotationKind,
    /// The annotated name: the variable, the parameter, the function whose
    /// result it is, or the type being declared.
    pub name: ImmutableString,
    /// Where that name is.
    pub pos: Position,
    /// The type's text, with its tokens rejoined. The parser only recognises a
    /// type far enough to know where it ends; understanding it is for whoever
    /// reads this table.
    pub ty: String,
    /// Where the type starts.
    pub ty_pos: Position,
}

/// What an [`Annotation`] is attached to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AnnotationKind {
    /// A `let` or `const`. The statement's [`Ident`][crate::ast::Ident] has the
    /// same position as the annotation.
    Var,
    /// A parameter of `function`, which takes `arity` parameters in all.
    ///
    /// For an arrow, `function` is the generated `anon$…` name and `arity`
    /// counts the captured variables the parser puts ahead of the declared
    /// parameters, so it is the arity of the function definition in the AST.
    Param {
        /// The function the parameter belongs to.
        function: ImmutableString,
        /// That function's number of parameters.
        arity: usize,
    },
    /// The result of the function named by [`Annotation::name`].
    Result {
        /// That function's number of parameters.
        arity: usize,
    },
    /// `type Name = …;`
    Type,
}
