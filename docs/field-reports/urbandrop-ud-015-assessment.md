# UD-015 assessment: sharing code between two Rux apps

**Update 2026-11-24:** the owner approved the `#[path]` step (building block 1 below) and it is built: the scanner follows `#[path = "..."] mod x;`, with tests in `rux-bindgen` and `the_native_seam`, documented in `07-script.md` (Native code). The dependency-crate and `.rux` halves stay parked.

Written 2026-11-24 under the owner's rule: a field report is not a spec. First ask how necessary the request is,
what it costs and risks, and whether a smaller general building block serves it. Nothing here is built.

## What was asked

Urbandrop's partner and client apps are two Rux apps. The report asks for (1) `#[rux::export]` items from a
dependency crate, and (2) `.rux` modules and components from a path outside the project, named in `rux.toml`.

## What is actually duplicated (measured in the Urbandrop repo, 2026-11-24)

| Piece | Partner | Client | Same? |
|---|---|---|---|
| `urbandrop-core` (API client, formatting, places, tests; about 1,160 lines) | uses it | uses it | **Already shared**, plain Rust |
| `stores/session.rux` | 34 lines | 30 lines | Same logic; the partner adds `isPartner` |
| `native/src/api.rs`: client bootstrap, runtime, `#[rux::init]`, `ugx` / `when` / `clock` / `km`, the `Session` record and auth exports | 358 lines | 706 lines | A repeated block of about 150 lines of thin wrappers, the rest app-specific |
| Sign-in form | `pages/sign-in.rux`, 98 lines | `components/sign-in.rux`, 77 lines | **Not the same already** (a page with its own fields against a component) |

So the real duplication is one 30-line store and about 150 lines of wrappers. Nothing is blocked. The cost of
leaving it is drift between the copies (the client's own comment says "same as the partner app's") and a third
copy if a third app (admin, say) is built.

## Two things that change the picture

- **UD-007 and UD-001 are now built.** `rux::secrets` lets the refresh token be kept from Rust, and
  `rux::IntoError` lets errors carry codes. The restore-and-keep logic that `session.rux` does in script can move
  into `urbandrop-core`: `restore()` reads the secret, `on_session` writes or removes it. The store then shrinks
  to a few lines (`ready`, `start`, `isPartner`), and most of what UD-015 part 2 wanted to share is gone, as Rust
  that is shared the way `urbandrop-core` already is.
- **No second consumer.** Mava Studio, Tracer and the other Rux apps have asked for nothing like this. The field
  reports and the roadmap name one requester.

## The two halves, assessed

### 1. Exports from a dependency crate

- **Necessity:** low today. The wrappers are thin and the logic is already shared. A shared export crate would
  save about 150 lines and add a moving part.
- **Cost if built as asked:** the scanner (`rux-bindgen`) reads only the app's own `native/` crate. Following a
  dependency means reading `cargo metadata`, accepting registry crates as well as path ones, requiring the
  dependency to name Rux's crate `rux` (the macros write `::rux`) at the one version that builds the app, and
  registering exports under another crate's paths. `rux check` and the editor then depend on a resolved
  dependency graph, which is the opposite of today's "read the source, no build".
- **Risk:** two copies of `rux-native` in one build make two registries, and the exports land in the one nobody
  reads (the scaffold's own comment warns of exactly this). A published crate pins a Rux version, so every Rux
  release would break shared crates until they are bumped.

### 2. `.rux` modules from outside the project

- **Necessity:** very low. One 30-line store, and part of it moves to Rust (above). The sign-in forms already
  differ.
- **Cost if built as asked:** a new `[dependencies]` surface in `rux.toml`, which the manifest's own header
  says cannot be taken back once shipped ("a key earns its place by being needed by something that already
  works"). `use` resolution, hot reload (watching a folder outside the project), `rux build` packaging, `rux
  check`, assets and locks all change. Stores are process-wide, so two packages that each `export let session`
  need namespacing rules.
- **Security:** a path dependency is as trusted as the repo. The risk grows only if it later takes a git URL or a
  registry name, where a package's script and assets run inside the app. That needs a trust model written
  before it is allowed, not after.

## The smaller general building blocks

1. **Honour `#[path = "..."]` in the scanner.** Rust already lets two crates compile the same file:
   `#[path = "../../shared/session.rs"] pub mod session;`. The scanner today refuses it ("a `#[path]` module is
   not read for exports", `rux-bindgen/src/scan.rs`). Reading the file it names (relative to the declaring file)
   lets any app share exported Rust by source with no new manifest key, no dependency graph and no second
   registry: the exports compile into each app's own crate. About 20 lines and a test. This is a standard Rust
   feature, so it adds no Rux syntax.
2. **For `.rux`, use what exists.** `[build] before` and `[dev] watch` already run a command Rux starts and
   stops, so a shared folder can be copied or linked into the project by a script, and the copy is generated
   rather than hand-kept. Costs nothing in Rux. It is a workaround, and the report's rule says workarounds
   are Urbandrop's to avoid, so it is offered only as the interim, not the answer.
3. **The real package mechanism, later.** If a second app needs shared `.rux` (a maps component would: the tile
   maths and gestures were already pictured as a shared component), design it once as "a Rux project is a
   package": `[dependencies] shared = { path = "../shared" }`, `use shared::stores::session`, read-only,
   namespaced, with the trust rules written first. Do it when there are two real consumers and a v0.9 publish
   to anchor versions, not before.

## Recommendation

- **Do not build UD-015 as written.**
- **Do (1)** if the owner agrees: it is small, general, removes the export duplication, and needs no decision
  about packages. Optional; Urbandrop can also live with the current copy.
- **Move the session logic to `urbandrop-core`** (Urbandrop's own change, using UD-007 and UD-001): the largest
  cut in duplication with no Rux change at all.
- **Park (2) and the dependency-crate half** until a second app or the maps component needs them; record the
  design sketch above in the roadmap rather than the manifest.
- **Update the field report's UD-015 entry** to "assessed: not built as asked; `#[path]` offered; package
  mechanism parked" (another session's uncommitted file, so left for the owner).

## Not verified

The line counts are from the Urbandrop repo on this machine, which may be ahead of or behind the field report's
date. The `#[path]` change was read in the scanner but not tried, and `cargo`'s rule for a `#[path]` inside a
nested non-`mod.rs` file differs from the crate root's, so the first version would cover the root and `mod.rs`
cases and report the rest.
