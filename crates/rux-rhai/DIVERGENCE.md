# Divergence

**Forked from [rhai](https://github.com/rhaiscript/rhai) 1.25.1.**

This file is the whole answer to "what did Rux change, and why". Every
divergence is listed here, with the upstream file it touches, so that rebasing
onto a new rhai release is a matter of working through one list rather than
reading a diff of forty thousand lines.

The rule this fork is kept under: **nothing is changed here that can be changed
from outside the engine.** A registration in `rux-script` costs nothing forever;
a change in here is a merge conflict at every upstream release, for as long as
Rux exists. Several things that looked like they needed a fork turned out not
to, and they are recorded at the bottom so nobody pays for them twice.

## Why this fork exists

Rux is a UI language whose users are expected to arrive from JavaScript. rhai is
an excellent embedded scripting language that is not trying to be JavaScript,
and a handful of the places the two disagree are places where Rux cannot follow
rhai without either lying to its users or losing errors that matter. See
`docs/06-roadmap.md`, under v0.7, for the full reasoning.

## Changes

### 1. `?.` guards a missing property, not only an absent base

**Files:** `src/eval/chaining.rs`

Upstream `?.` short-circuits when the *base* is `()`, so `missing?.anything` is
fine, but on a map that does exist it raises for a missing *property* exactly as
`.` does.

That is a defensible design on its own. It stops working the moment
`fail_on_invalid_map_property` is on, which Rux turns on for every document: a
missing property becomes an error with **no way to say "absent is fine"**. Rux
needs both halves. Silence on a typo is the failure the dev overlay exists to
kill, and there is no useful UI language in which a field that has not loaded yet
is a hard error.

Upstream's own escape hatch is `"key" in map`, which works and reads well, but it
is not the thing a JavaScript developer reaches for, and it cannot be written
inline in the middle of a chain.

So under this fork, `?.` means "absent is an acceptable answer here", whether
what is absent is the base or the property.

### 2. `x++` and `x--`

**Files:** `src/parser.rs` (`parse_expr_stmt`, and the binary-operator loop in
`parse_binary_op`)

Reserved but unimplemented upstream, and unreachable from outside the engine:
rhai's custom operators are binary, and these are postfix.

Desugared to `x += 1` and `x -= 1` at parse time, so they inherit lvalue
checking, operator overloads, write tracking and everything else `+=` already
does. Nothing new reaches evaluation; this is spelling, not semantics.

Two touch points rather than one, which is worth knowing before a rebase: the
statement parser recognises the postfix form, and the binary-operator loop had to
be taught to *end the expression* when it sees these tokens instead of rejecting
them as unknown operators, since otherwise the statement parser never gets to
see them.

**Statement position only.** In JS `x++` is also an expression evaluating to the
value *before* the increment, which is the rule behind the classic `i = i++`
puzzle. rhai's assignment is a statement rather than an expression, so supporting
the expression form would mean inventing the confusing half of JS's behaviour
rather than matching something that already exists.

### 3. Arrow functions, and multi-token lookahead

**Files:** `src/tokenizer.rs` (`TokenStream`), `src/parser.rs`
(`parse_primary`, `parse_anon_fn`, and two new helpers), plus the six
construction sites in `src/api/`

`x => …`, `() => …`, `(x) => …` and `(a, b) => …`, each meaning exactly what the
matching `|…|` form means. Nothing new reaches evaluation: the parameters are
handed to the existing anonymous-function parser and the same AST comes out.

The parsing is the interesting part. `TokenStream` was
`Peekable<TokenIterator>`, giving one token of lookahead, and one is not enough:
`(a, b) => …` and `(a + b)` are identical until several tokens in, and a
`Peekable` cannot put back what it has read. So `TokenStream` is now a small
struct with a `VecDeque` buffer. `next` and `peek` keep their exact signatures,
which is why the ninety-odd call sites in the parser did not change at all;
`peek_nth` is the only addition and only the arrow lookahead uses it.

Tokens are pulled **on demand and never eagerly**. The tokenizer is stateful:
`TokenizerControl` is toggled by the parser partway through a script, string
interpolation being the main case, so reading further ahead than asked could
tokenize under settings that have not been applied yet. Buffering only what is
requested keeps that bounded to the lookahead itself, which walks names and
commas and nothing interpolation can reach into.

Two things worth knowing before touching this again. The lookahead is pure: it
consumes nothing, so a `None` answer leaves the stream exactly as it was, which
is what lets `(a + b)` fall through untouched. And the arrow arm sits **above**
the ordinary handling of `(`, `()` and identifiers in `parse_primary`, because
all three are matched further down and the first matching arm wins; placed
lower, it is silently unreachable.

Only names are accepted as parameters. JS also allows defaults and destructuring
there; neither exists anywhere else in this language, and accepting them would be
inventing rather than matching.

### 4. Every call runs in the scope it was written in

**Files:** `src/parser.rs` (`parse_postfix`, two places)

The big one, and the reason the fork was worth carrying.

Upstream, rhai functions are **scopeless**: a `fn` cannot see a `let` declared
outside it. That is a reasonable choice for an embedded language where functions
are meant to be pure. It is the wrong one for Rux, where the top-level `let`s are
the application's state and a handler exists to change them. It meant every
handler in every example had to be written inline, because the moment you gave
one a name it could no longer reach anything.

rhai already had the mechanism: `f!(…)` runs a call in the caller's scope. It was
opt-in per call site. Here it is simply what a call means, so `fn bump() { n++ }`
reads and writes `n`, and a function calling a function still reaches it.

**Only two lines needed changing**, which is worth stating plainly because it is
disproportionate to how large this is as a language change: the parser sets
`capture_parent_scope` on every plain call rather than only on the `!` form.

**Method calls do not capture**, and the flag is now *cleared* for them rather
than raising. Upstream this was an error, because asking for capture in method
position was a mistake worth reporting; here the flag no longer reflects anything
the author asked for, so erroring would make `colors.len()` illegal. The
underlying limitation is upstream's and stands: method dispatch passes its
receiver by reference and the scope cannot also be borrowed. Nothing is lost in
practice, since a method's receiver is the thing being worked on, and a function
needing the surrounding state is written as a plain call.

### 5. JavaScript truthiness

**Files:** `src/types/dynamic.rs` (`Dynamic::is_truthy`), `src/eval/stmt.rs`,
`src/eval/expr.rs`, `src/parser.rs` (`ensure_bool_expr`),
`src/packages/logic.rs`

Upstream, a condition must already be a `bool` and anything else is a type
error, checked both at parse time for known types and at evaluation. Rux's users
write `if user { … }` and `r-if="items.length"`, and in binding position they are
not thinking about type rules at all.

`Dynamic::is_truthy` follows JavaScript exactly: `false`, zero of any numeric
type, `NaN`, `""` and `()` are falsy; **everything else is truthy, including an
empty array and an empty object map**. That last part reads worst in isolation
and is kept anyway, because a rule that is *almost* JavaScript is worse than one
that either is or visibly is not: a private exception is only ever discovered by
being bitten by it.

Seven condition sites now call it instead of `as_bool()`. `!` gained a `Dynamic`
overload so it works on anything, left as a second overload so `!` on a real
`bool` still resolves to the cheaper one. `ensure_bool_expr` became a function
that always succeeds, rather than being deleted, so its eight call sites stay
byte-identical to upstream and a rebase has one hunk to reconcile instead of
eight.

### 6. A whole `f64` can index

**Files:** `src/types/dynamic.rs` (`Dynamic::as_index`), `src/eval/chaining.rs`

`signal()` coerces to `f64`, so every number a document handles is a float, and
`rows[i]` for a computed `i` failed with "Data type incorrect: f64 (expecting
i64)". **This was a live bug, not a new requirement**: indexing by a computed
number is most of the indexing anyone does, and nothing in the suite covered it.

A fractional index is still an error rather than being truncated. JavaScript
answers `undefined` for `arr[1.5]`; that is the one place its behaviour is not
worth copying, since silently reading a different element than the one asked for
is exactly the class of failure this milestone exists to remove.

## Deliberately *not* forked

Recorded because each of these was believed to need a fork at some point, and
finding out otherwise cost time that nobody should spend twice.

- **Strict map properties.** `Engine::set_fail_on_invalid_map_property(true)` is
  a stock option. Two milestones of Rux notes said this required forking rhai.
- **`===` and `!==`.** Reserved tokens upstream, and
  `Engine::register_custom_operator` accepts a reserved token, so they are
  registrations.
- **`null`.** A reserved keyword, so it cannot be a scope binding, but custom
  syntax sees it and that is enough.
- **`print` output.** `on_print` and `on_debug` already route it wherever the
  host wants.
- **`rhai_codegen`.** A proc-macro for registering Rust functions, with nothing
  to do with the language changes here.
