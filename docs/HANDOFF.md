# Handoff

Read this first. Keep it short; history goes to an archive, not here.
Last updated 2026-11-28 (flake #55 fixed; error-boundary items worked; sandbox terms are per document).

## Objective

Ship v0.9 from `build/v0.9`. The line is nearly all built (see the v0.9 section
of [the roadmap](./06-roadmap.md)); what is left is listed there.

## State

- 2026-11-28, **EB-1 built** (Opus plan `C:/Users/USER/.claude/plans/functional-dancing-turtle-of-01953b0e-8f7f-4511-ab0a-f2645d5d9c94.md`). Every patchable binding and `StructuralParent` records its boundary (`boundary: Option<String>`, `open_boundary()` in `rux-style`); `patch_values` and `reconcile_lists` run each in its own failure frame (`Engine::boundary_open`, `frame_end`, `Document::fail_caught`) and fail that boundary; `apply_change_depth` rebuilds only when `boundary_dirty` (it takes it, so no second build). `rebuild_children` pushes the parent's boundary so a list built alone records the same. `Document::whole_builds()` counts builds for tests. `BindingRegistry::boundaries` is now unread (left, doc says so). Tests (five new in `error_boundaries.rs`, the outside-every-boundary one failed first). **Owner's next, after this: grants narrower than a module** (roadmap v0.10 item 2), its own Opus plan. Not driven in a window. Local, not pushed.
- 2026-11-28, **EB-5 done, typos only (owner chose).** `rux check` now errors on `error.mesage` in a `<fallback>`: `known_error_fields` and `misspelt_error_fields` in `crates/rux-cli/src/permission_check.rs` (a text scan like `stray_retry`), called from `check.rs`; known fields come from `rux_native::registry::all_throws()` (new), `.with("..")` in `native/src`, and `throw { .. }` records; `near_miss` is now public and re-exported by `rux-runtime`. Other unknown fields are deliberately not reported. Tests: five unit tests in `permission_check.rs`, `a_misspelt_error_field_in_a_fallback_is_found` in `tests/check.rs`; `--test check` green (29), shipped examples clean. Not driven with a real native crate (the scan's types path is unit-tested only); not run: the full workspace. Local, not pushed. **Left (design, Opus):** EB-1.
- 2026-11-28, **sandbox terms are per document** (Opus plan `C:/Users/USER/.claude/plans/staged-pondering-kettle-of-1aeb0ac1-2df8-4177-9683-facf0b56361d.md`). A `Document` keeps `terms`; `Document::make_current` sets the thread's `CURRENT_SANDBOX`, row limit and (via `Engine::apply_terms`) `WITHHELD`; the shell calls it at `App::window_event`, `App::user_event`, `Shell::each` and `ask_close`. **Found on the way, an escape:** `load_window` took only the process default, so a window a sandboxed document opened was trusted; `Document::load_window_from(opener, ..)` now gives it the opener's terms (also on reload and in `open_child`). A missed `make_current` degrades to the old last-loaded-wins. Tested through `Document` (`tests/sandbox.rs`), not driven: nothing opens two sandboxed windows yet. **Still open (design):** EB-5, EB-1, fetching documents (held), grants narrower than a module. Local, not pushed.
- 2026-11-28, **#55 fixed and the error-boundary items worked (owner: "do all starting from #55").** #55: `you_step_in_full` (`async_listeners.rs`) asserted it had seen "Checking..." on screen, a timing glimpse a busy machine missed (3 in 25 runs under load, 0 in 40 after); it now asks the verify call itself. **EB-2:** `count_rendered` counts a failed boundary as its fallback (`TemplatePlace`, same key as the build), the build that finds a failure sets `take_counts_stale` and the runtime builds once more, and children of a `<boundary>`/`<slot>` now start from the siblings built before them (they counted from 1; `build_children_from`'s `first` carries them). Driven in a window (`rux-harness/eb2`). **EB-3, EB-8 found already working** (tests added; EB-8 still says "`shop` is not defined" as a second report when the line runs). **EB-4:** a timer or `async fn` started by the run that fails its component away is dropped (`is_gone`). **EB-6:** an `async fn` remembers its handler's boundary (`task_catchers`, `last_catcher`), so a failure after an `await` reaches it, a page's included. **Not done, needs Opus (design):** EB-5 (types for a native error's fields), EB-1 (map from boundary to tree, no rebuild per failing patch), per-document sandbox terms. EB-7 (Android, web, real network) not driven. Local, not pushed; stage by explicit path (never `git add -A`, never the field report).
- 2026-11-27, **sandbox leftovers worked (owner listed three).** (1) Markup that reads an ungranted `window` or `app` now gets the not-granted sentence (thread-local `WITHHELD` in `rux-script`, read by `rux_phrasing`; also an ungranted `pickFile`); `rux check --sandbox` says it too. (2) Fetching documents: **held by the owner** (asked: Rust+CLI first, script call, markup element, or hold; answer hold). (3) **Driven** the real playground page in Brave (`rux-harness/sandbox-web`) and a trusted app on the `rux_test` emulator (`rux-harness/sandbox-and`); rows in `08-user-tests`. Nothing opens a sandboxed document on Android (`rux run --device` takes no `--sandbox`). **Found and fixed while driving:** a failed tap was never on the panel when the tap only patched, and the press's restyle wiped it when it was, so on the web (no stderr) a failing or stopped handler said nothing; now `tap_warnings` (`05-as-built`, "A tap that fails is listed when it fails"), driven on web and Android, watchlist #53 likely fixed. Three tests that read the warning queue after a tap now read `doc.diagnostics()`. **Pre-existing flake** (watchlist #55): `the_you_step_signs_in_…` fails ~5 in 12 on 8191c8d too. Local, not pushed. **Next:** the owner picks (fetch held; per-document terms; EB items; #55).
- 2026-11-25, **capability model sandbox half built** (roadmap v0.10 item 2; plan `C:/Users/USER/.claude/plans/floofy-spinning-rabin-of-5a67fdac-e452-4160-adc4-6fbd1abf2ec6.md`, all six steps). **Owner's 1.0 answer:** a file shipped in the app is trusted; anything opened from elsewhere is sandboxed; a file never marks itself trusted. Commits `3a64a50`, `ebf40c0`, `4e5411b`, `c3c1d06`, `e38cfef`, `74dc96b` and the docs commit after them; pushed to `0ceedd5` only, the rest local. How it works: `05-as-built` ("The sandbox"); the reference: `07-script` ("Security"); the leftovers, in full: `06-roadmap` v0.10 item 2. Tests: `rux-script/tests/sandbox.rs`, `rux-runtime/tests/sandbox.rs` and `default_sandbox.rs`, `rux-cli/tests/check.rs`, a unit test each in `rux-syntax` and `rux-parser`; the touched crates' suites are green (1,377 tests) and `cargo check -p rux-web --target wasm32-unknown-unknown` builds. **Driven** in the desktop window (probe `C:/Users/USER/Mobile/rux-harness/sandbox`, rows in `08-user-tests`): ungranted refused at load with the flag named, granted copies, trusted unchanged, a doubling string and a stuck loop each stopped and caught by a `<boundary>`. Trusted-path benchmark: the same within noise. **Open, named so they are not lost:** per-document terms (the row limit, `images` and grants follow the document loaded last); the row cap counts rows, not every element; markup that reads an ungranted `window` says "not defined"; grants narrower than a module and object-capability grants; no fetching or over-the-air API yet (only the playground, `rux run --sandbox` and Rust open a sandboxed document); A trusted `JSON.parse` now stops at 512 deep (owner chose a fixed cap, 2026-11-25; unit test on a 1 MiB stack). **Not driven:** the playground in a browser, Android. **Next:** the owner picks: open v0.9 items moved to v0.10, an EB item (error boundaries), or the sandbox leftovers above.
- 2026-11-25, **error boundaries built** (roadmap v0.10 item 1; plan `C:/Users/USER/.claude/plans/lucky-hatching-squid-of-fd6f5fcc-7b3c-448a-ab19-67df0d074b49.md`; the owner chose `<boundary>` with a last-child `<fallback>`, render and component code caught, a caught failure still reported). Commits `bf9aa7d`, `3884310`, `8487ced`, `88fa375` and this one, local, not pushed. How it works is in `05-as-built` ("Error boundaries"), the language side in `07-script` ("Error boundaries"). Tests: `crates/rux-script/tests/boundary_capture.rs`, `crates/rux-runtime/tests/error_boundaries.rs`. **Driven** in the desktop window (probe `C:/Users/USER/Mobile/rux-harness/boundary`, rows in `08-user-tests`): fallback, Try again, fallback again. **Fixed after the first report (2026-11-25, same day):** a page's own handler inside a boundary now fails it (`__caught_by`), `rux check` builds each fallback once with a stand-in error, and warns on a `retry()` outside a `<fallback>` (quiet where the file declares its own `retry`). **Future improvements, named so they are not lost (full text in `06-roadmap.md`, "Error boundaries, future improvements"):** EB-1 a rebuild per failing patch with a boundary open; EB-2 `:last-child` counting ignores a failed boundary; EB-3 a failing `mounted` hook is not attributed; EB-4 timers of a discarded instance are not dropped; EB-5 `error` fields are unchecked (a typo is not found until it fails); EB-6 an `async fn` a page handler started is not caught; EB-7 not driven on Android, the web or with a real network call; EB-8 a missing store function reports three errors and a warning for one line. **Next:** the owner picks: the capability model's sandbox half (item 3 below), or the open v0.9 items.
- 2026-11-24, **an app's own `network` (or `location`, `settings`…) is no longer taken for the built-in module inside a component's `fn`** (Urbandrop partner app, after UD-012 was confirmed on the emulator: 5 of 5 sign-ins, no logcat errors). The built-in scan for a component read only its functions' text, so its `let network` or `computed network` was not seen and the module was linked over it: "`network` is what line 0 imports…" at run time. `ComponentSource::script` now carries the whole script (`rux-script/src/lib.rs`). Test `crates/rux-runtime/tests/builtin_shadowing.rs` (failed before with Urbandrop's exact errors). **Left open:** `rux check` reported nothing where the load failed, so its component path does not build functions as the load does; worth a look. Urbandrop's one-off 0-byte `assets/tailwind.css` is written by the Tailwind CLI their `watch` runs, not by Rux; not reproduced. Also fixed: `rux-shell`'s secrets test raced the settings test over the process-wide store folder (failed 1 in a few runs); both hold `settings::STORE_TEST_LOCK`. Verify green, 1,848 tests. Not pushed; the installed CLI is still `7aeadb4`.
- 2026-11-24, **a removed component's field reads as nothing and takes no writes** (`Document::instance_gone` in `rux-runtime`; `value_in`, `typed_value_in`, `apply_value_in`). The shell keeps a field's `(model, row, instance)` past the frame that removed it (Android's autofill list, an input method's last commit, a late picker); it was read and written in the page's scope, the likely source of UD-012's "`email` is not defined" lines, and a late write could overwrite a page signal of the same name. Test `crates/rux-runtime/tests/removed_fields.rs` (both failed before). Urbandrop (`urbandrop-apps-d9`) has been sent the decision on every UD entry. **Left open:** a queued *handler* of a removed component (`dispatch`, the `instance.filter(contains_key)` fallback) still runs at page level; dropping it is a behaviour change for the owner to weigh. **Next:** the owner picks; proposed, an Opus plan for error boundaries (v0.10 item 1).
- 2026-11-24, **UD-012's Android case fixed** (Urbandrop re-reported it: the booking's You step stuck on the sign-in 2 of 3). A run in one component refreshed every instance's computeds and kept only its own moves, so the page's `signedIn` (moved by the native re-read in the resumed `verify()`) was already true when the change reached it and its `r-if` never patched; now `refresh_own_computeds` (`crates/rux-runtime/src/lib.rs`), five call sites. Test `the_you_step_signs_in_when_the_answer_is_settled_before_the_session_is_read` (5 of 5 failing before). Also `rux-cli`'s absolute-path test uses the host's form (failed on Linux CI). CI on `8640d39`: Advisories green, warnings gone. **Not driven on the emulator yet; the `email is not defined` logcat lines are probably `publish_autofill` reading the removed fields (unconfirmed).** Field report status added, file left unstaged. **Pushed; CI green on `ad2185d`** (also: `rux new`'s tests use `CARGO_BIN_EXE_rux`, they had never run on Linux). CLI reinstalled from `7aeadb4`. **Confirmed fixed by Urbandrop on the emulator, 2026-11-24.**
- 2026-11-24, **both CI decisions done (owner: replace chrono, handle the rest); committed locally, not pushed.** `chrono` is replaced by `jiff` 1 in `rux-script` (`LocalTime` and `wall_clock_ms` in `src/lib.rs`): the system zone on native, the browser's `Date` offset on the web so no zone database is bundled; `Date.now()` reads `web_time::SystemTime`. The 14 newer-Rust warnings were fixed without that Rust (the `#[expect]`s in vendored vello are `#[allow]`, marked `Rux:`, noted in `vendor/README.md`; `rux-web`'s `rux-fmt`/`rux-highlight` are wasm-only deps; workspace `homepage`/`documentation` removed), and the web build's 4 dead-code warnings are allowed. Local `./scripts/release.sh verify` passes (1,840 tests). **Not verified:** the newer Rust's verdict (CI's on push) and `rux-shell` for Android (no NDK clang on PATH; `rux-script` checks for Android). **Next:** push when the owner says, watch CI and Advisories; then ask the owner before telling Urbandrop (UD-012 fixed, UD-006 workable, UD-016 a server PDF) and re-drive its sign-in. Mava (MS-001..014) and Tracer (AW-001..053) have nothing open.
- 2026-11-24, **pushed `build/v0.10` to `45bb55b` (owner said push); CI still red, two owner decisions open.** The CLI is installed from `a99eaa8` (all of today's work). To get CI's verify further: site docs regenerated (`./site/sync-docs.sh`), vocabulary regenerated (`./scripts/sync-vocabulary.sh`), the extension's tests updated for `datetime-local`, `time`, `:dragging`, `:drag-over`, Linux-only dead code allowed (`caption`, `settings::file_path`), `TwoFinger::center` removed, and `a_tapped_async_fn_shows_what_it_wrote_after_its_await` waits for its answer (it failed 1 in 3 under the full suite). **Local `./scripts/release.sh verify` passes in full (1,840 tests).** **CI (run 45382154856) fails only on 14 warnings from a newer Rust** (`ubuntu-latest` stable; this machine has 1.94.0 and no `rust-toolchain.toml`): `homepage`/`documentation` unused in `[workspace.package]`; 6 unfulfilled `#[expect]` in `vendor/vello` and `vendor/vello_encoding`; `rux-web`'s `rux-fmt` and `rux-highlight` unused natively (used only in its wasm module: move them to the `cfg(target_arch = "wasm32")` dependencies). **Advisories fails** on RUSTSEC-2026-0543, `chrono` unmaintained, a direct dependency of `rux-script`. **Decisions for the owner, asked, not answered:** (1) fix the 14 warnings with a newer Rust installed beside 1.94 (recommended; 1.94 stays default; disk has about 13 GB free), or fix blind and let CI check, or pin CI to 1.94; (2) replace `chrono` (with `jiff` or `time`) or accept the advisory in `deny.toml`. **Next:** ask the owner those two, then do them; then tell Urbandrop (UD-012 fixed, UD-006 workable, UD-016 a server PDF) and re-drive its sign-in.
- 2026-11-24, **Image memory bounded and a disk cache for network images** (plan `C:\Users\USER\.claude\plans\bounded-image-memory-and-disk-cache.md`, the owner chose the proposals: fixed budgets, `Cache-Control` with 7 days by default, memory first). `rux_paint::ImageCache` keeps 128 MB (64 MB Android/web) of decoded pictures, `trim()` after each scene in `rux-shell` drops the least recently drawn, never one the scene drew; `remote.rs` keeps 32 MB of bytes (LRU `Store`) and runs 6 fetches at once, newest first (`Queue`); remote image sizes join `IMAGE_SIZES`; **the leak that was left was `rux_style::interpreted`'s memo**, cleared only by whole builds: capped at 1,024 (`INTERPRETED_MAX`). New `rux-runtime/src/disk_cache.rs` (100 MB, `Cache-Control`, URL checked on read, touched on read, trimmed to 90%); the shell sets its folder (`settings::image_cache_dir`, Android `cache/rux-images` beside `files`). The panning-map stress test: was endless growth (635 MB at 40 s), now flat at about 350 MB through 3,300 tiles; frames under 16.1 ms. Tests: two `rux-paint` cache tests, three `remote` tests, two `disk_cache` tests, `shared_interpretations_stay_bounded_without_a_build`. **Plan changes, recorded:** queued fetches are not dropped (a still page asks nothing again, so its images would be lost); step 5 (GPU) needed no fix (`gpu` includes waiting for the frame); a shared `ureq::Agent` was tried and reverted (dead pooled connections failed and stayed failed). **Emulator** (`rux-harness/tiles-and`, rows in `08-user-tests`): builds; 4,200 tiles in 240 s, the picture budget full at 30 s, total PSS 191 MB at the end with only the 32 MB bytes budget still filling; 34 MB on disk. **Not driven:** a phone, a real server's headers, the web. **Open:** vello's atlas grows to 256 MB before evicting (a report for Linebender); writing a new cache file costs about 6 ms of CPU on Windows; the emulator draws this probe at about 4 to 8 fps (`patch` 69 ms), not a phone figure. **Next:** tell Urbandrop UD-006's state (a tile map in its own app is workable; memory is bounded; tiles kept on disk), and re-drive its sign-in (UD-012) with the CLI from 431787e.
- 2026-11-24, **CLI reinstalled from 431787e; map stress test; image memory plan waiting for the owner.** A panning map (`rux-harness/tiles-perf/map.rux` with `tile_server.py`, rows in `08-user-tests`) grew 11 MB a second to 635 MB over 1,300 tiles and never gave it back: `remote.rs` and `rux_paint::ImageCache` keep every image forever. Layout and patch are fine (1.3 ms); new pictures cost the GPU 9 to 14 ms a frame against 2.5 ms. Plan `C:\Users\USER\.claude\plans\bounded-image-memory-and-disk-cache.md`: bound decoded pictures, bound remote bytes, bound fetches, then the disk cache, then look at the GPU cost; three decisions for the owner in it (budgets, freshness, order). **Next:** the owner decides the plan; meanwhile the Urbandrop session re-drives sign-in with the new CLI.
- 2026-11-24, **UD-012's desktop case fixed, image sizes cached, a retired name in a timer checked.** (1) **UD-012:** the emulator probe that marked it fixed never removed `<sign-in />`; the client app's shape as a test failed 3 of 3 with "No orders yet.". Cause: a component's `effect` reading a `computed` never re-ran when the computed moved (from a native signal, or from a handler's write), since only written state woke effects. `refresh_instance_computeds` now returns what moved by instance; `Wake::Signals` carries it, and every handler, model, mount and own-effect path adds it to the names that wake (`rux-runtime/src/lib.rs`). Tests: `a_component_removed_by_the_signal_its_await_sets_lets_the_pages_load_finish` (`async_listeners.rs`), `a_components_effect_on_a_computed_runs_when_the_handler_moves_its_source` (`computed_pull.rs`), both failing before. (2) **Map tiles, UD-006's speed test** (`rux-harness/tiles-perf`, rows in `08-user-tests`): 64 tiles in a moving box ran at 10.2 ms a frame, 6.3 ms of it `patch` re-reading every image file; `IMAGE_SIZES` (thread-local, cleared in `load_with` and `from_source_checked`) makes it 4.5 ms (about 2.2x faster), an image as cheap as a box. Test: `an_images_size_is_kept_across_patches_and_read_again_on_load` (`image_base.rs`). (3) **Checker:** `check.rs` now errors on every retired free name (`stdlib::retired_free`), so a `setInterval` block is covered; the `fn` lint skips those names so it is said once. Test: `a_retired_function_in_an_interval_block_is_an_error`. **Not driven:** the client app's sign-in with a CLI built from this tree (desktop and Android); tiles on a phone, from the network, or under real fingers. **Probe trap:** a `<view>` is a block as in CSS; `flex-direction` needs `display: flex` (the first tile timings were of one long column). **Next:** reinstall the CLI from this tree and re-drive the client app's sign-in during a booking (memory `urbandrop-client-app-2026-11-12`), then tell Urbandrop: UD-012 fixed; UD-006 a tile map in its own app is fast enough on the desktop (the disk cache for remote tiles is the remaining Rux piece, unplanned); UD-016 a server PDF with `links.open`. **The field report is another session's uncommitted file: its UD-012 status line needs this; never stage it.**
- 2026-11-24, **escapes, `<` bug, left-out pieces, pinch and rotate built** (plan `C:\Users\USER\.claude\plans\zazzy-sniffing-kazoo-of-93cbf624-a060-494f-923c-7610a3123d40.md`, assessment `docs/field-reports/urbandrop-remaining-assessment.md`). `\u{2190}` escapes (`rux-syntax/src/lexer.rs`); a `<` that starts no tag is text and `{{ }}` is never scanned for tags (`rux-parser`); `location.enabled()`, `permissions.openLocationSettings()` and `openBatterySettings()` (separate calls, not an optional argument: the checker has no optional arguments), `network.online` and `rux::network` (`rux-native/src/network.rs`, `rux-shell/src/network.rs`, Java `ruxWatchNetwork`, `ACCESS_NETWORK_STATE` beside `INTERNET` in `apk.rs`); pinch and rotate as `@gesturestart` / `@gesturechange` / `@gestureend` (`rux-shell/src/two_finger.rs` is the pure geometry, `Pinch` and `pinch_begin/moved/end` in `rux-shell/src/lib.rs`, `Gesture::GestureStart..` and `pinch_at` in `rux-layout`, `GestureEvent` in `rux-script/src/native.rs`). Docs: `07-script` (Location, Permissions, Network, Pinch and rotate). **Driven on the emulator** (probe `rux-harness퀄7-and`, `.rux-build` deleted; rows in `08-user-tests`): pinch, rotate and pan with two injected fingers (`two_touch.py` in the scratchpad: `sendevent` on `/dev/input/event2` after `adb root`), `network.online` with airplane mode, `location.enabled()`, both settings pages, UD-012's Android shape (fixed). Also fixed: a faint line on an inset shadow's edge (`rux-paint`), and the IR printer's built-in type list now holds `GestureEvent`. **Not driven:** a real phone's fingers, the camera after Android kills the app, the web and macOS touch. **The field report's status lines for UD-001 to UD-007, UD-013, UD-014 and UD-015 are another session's uncommitted file: tell the owner, never stage it.** **Next:** the owner picks: maps (UD-006, pinch is now there, a map component is Urbandrop's to write; static map images need no Rux) or print (UD-016, assessed: a server PDF opened with `links.open`).
- 2026-11-24, **UD-004 and UD-007 built** (plan `C:\Users\USER\.claude\plans\zazzy-sniffing-kazoo-of-93cbf624-a060-494f-923c-7610a3123d40.md`; UD-015 was assessed (`docs/field-reports/urbandrop-ud-015-assessment.md`) and only its `#[path]` step is built (the scanner follows `#[path = "..."] mod x;`, so two apps share one export file; untested: `rux run` rebuilding when a shared file outside `native/` changes); the dependency-crate and `.rux` halves stay held on the owner's rule: assess necessity first and find a general building block instead of building the request as written; do not plan or start it without asking). **UD-004:** `rux::File` (`rux-native/src/files.rs`: `FromRux`/`IntoRux`/`FromRuxRef`, `bytes().await`, `set_reader`); the shell installs the reader (`rux-shell/src/files.rs`, `install_reader`): desktop `fs::read(path)`, Android `android_read_uri`, web `array_buffer` through a now generic `Slot<T>`. **UD-007:** `secrets.get/set/remove` (strings) and `rux::secrets` (`rux-native/src/secrets.rs`, `rux-shell/src/secrets.rs`, `secrets_interface` and the alias in `rux-script/src/native.rs`, a `vocab.rs` entry); Windows DPAPI (`Win32_Security_Cryptography` added to `rux-shell/Cargo.toml`; `secrets.json` beside `settings.json`); Android `RuxActivity.ruxSecret` (Keystore AES-GCM, file in `noBackupFilesDir`, `android_secret` in `rux-shell/src/lib.rs`); macOS, Linux and the web throw `unsupported`. Driven on the emulator (probe `rux-harness\ud0047-and`, `.rux-build` deleted) and in tests; rows in `08-user-tests`. **Not driven:** the Windows window for `secrets`, the web `File::bytes`, a camera photo after Android killed the app, big files. **Not built:** image downscaling and cache release (the app's Rust does them), macOS Keychain, structured secret values. **The field report's UD-004 and UD-007 status lines are another session's uncommitted file: tell the owner, never stage it.** **Next:** the owner picks: maps (UD-006, needs pinch) or print (UD-016); UD-015 waits for approval.
- 2026-11-24, **UD-001 built, committed 45162b9, not pushed** (plan `C:\Users\USER\.claude\plans\zazzy-sniffing-kazoo-of-93cbf624-a060-494f-923c-7610a3123d40.md`). `rux::IntoError` (`KINDS`, `into_error`) and `rux::Error::with / with_fields` (`rux-native/src/value.rs`; the pick between it and `Display` is autoref specialisation, `__private::Thrown` + the macro in `rux-native-macros`); an `Err(rux::Error)` is not wrapped again; `error_value` in `rux-script/src/native.rs` builds `{ message, kind, ...fields }`; `throw "text"` is caught as `{ message, kind: "error" }` (`Interp::thrown_pub`); `rux-bindgen` scan reads each `Result`'s error type, its `IntoError` impl (`KINDS`, `.with("n", ..)`, `.with_fields(S { .. })`) into `Scan::throws`, which `native::declare_for` puts in `registry::declare_throws`; `ExportKind::Fn::throws`; the checker records what a `try` body can throw (`Throws`, `caught`, `check_caught_kind` in `rux-script/src/check.rs`) and errors on `e.kind == "typo"` with a "did you mean". Tests: `rux-native/tests/macros.rs`, `rux-runtime/tests/native_errors.rs`, `rux-bindgen` scan test, `check.rs` unit test, `the_native_seam` in `rux-cli/tests/check.rs`; the AOT corpus line for `throw "x"` now reads `e.message`. **Narrower than the plan, deliberately:** `e` stays `any` (typing it as a record changes the typed IR and a field the error lacks would fail at run time), so `e.status` is `any`; only `e.kind == "..."` is checked; a script function, a closure, or a method on an unknown type in the `try` body leaves every kind possible (no per-function summaries); `rux run`'s overlay has no scan, so there it narrows nothing. **Not driven in a window**, tests only. **Not done:** `docs/05-as-built`, `08-user-tests` rows; the field report's UD-001 status line (another session's uncommitted file: tell the owner, never stage it). All 148 workspace suites pass. **Next:** UD-004 (file bytes to Rust), UD-007, UD-015.
- 2026-11-24, **UD-013 and UD-014 built** (plan `C:\Users\USER\.claude\plans\temporal-enchanting-dawn-of-430432c5-d4b4-4704-b4be-40a08e65aaed.md`). `el.scrollLeft / scrollTop / scrollWidth / scrollHeight / clientWidth / clientHeight`, `el.scrollTo(x, y, { smooth })`, `@scroll` and `@scrollend` (`ScrollEvents` in `Rare`, `ScrollRegion::path`, `App::report_scrolls`: `@scrollend` after 100 ms still with nothing coasting); `type="datetime-local"` and `type="time"` (`rux_layout::Moment`, `parse_moment`, `InputKind::picker()`; Android's date then time dialogs through `ruxOpenDateTime`, a time outside `min`/`max` refused when it arrives); select options as `{ value, label, disabled }` (`rux_layout::OptionMeta`, `rux_style::read_options`, `SelectRegion::{label, is_disabled, numeric}`; all-number values write a number and `@change` carries one; Android's dialog greys disabled rows). `:options` is checked as `any[]` and a select's model and shown type as `any`. Rows in `08-user-tests`. **Not driven:** a phone browser's hidden inputs and `<option disabled>`, typing on the web, a touch fling's `@scrollend`, an RTL scroller. **Not built:** `<option>` elements and `r-for` over options, `step` and seconds on date-time inputs, `el.scrollTop = n`. **The field report's UD-013 and UD-014 status lines are another session's uncommitted file: tell the owner, never stage it.** The disk filled once (os error 112): 67 GB of `target/debug/**/*.pdb`, then the `.rux-build` caches of the older probes in `rux-harness`, were deleted with the owner's say-so. **Next:** the owner picks: the Rust boundary (UD-001, UD-004, UD-007, UD-015), maps (UD-006) or print (UD-016).
- 2026-11-23, **UD-005 (links) built** (plan `C:\Users\USER\.claude\plans\temporal-enchanting-dawn-of-430432c5-d4b4-4704-b4be-40a08e65aaed.md`). `links.open / dial / sms / directions / share / canOpen` in script (aliased when a script mentions `links.`) and `rux::links` in app Rust (`rux-native/src/links.rs` holds the scheme allow-list and URL builders; `rux-shell/src/links.rs` the script side and the platforms: `ShellExecuteW` / `open` / `xdg-open`, Java intents, an anchor click). All sync (the plan said async `open`). `rux check` warns on a literal URL or number Rux would refuse (`refused_links`). Driven on Windows, the emulator (dialer, messaging, Google Maps, share sheet) and Edge; rows in `08-user-tests`. **Not driven:** `open` on Android (the probe's first row sat under the status bar), `notfound`, turn-by-turn starting, WhatsApp, `gesture`, macOS and Linux. **The field report's UD-005 status line is another session's uncommitted file: tell the owner, never stage it.** **Next:** the owner decides; the open v0.10 plans are the Rust boundary (UD-001, UD-004, UD-007, UD-015), UD-013, UD-014, then maps (UD-006) and print (UD-016).
- 2026-11-23, **UD-002 (location) built** (plan `C:\Users\USER\.claude\plans\temporal-enchanting-dawn-of-430432c5-d4b4-4704-b4be-40a08e65aaed.md`). `location.current / watch / stop` and the signals `position` and `watching` in script (aliased when a script mentions `location.`, as `background` is) and `rux::location` in app Rust (`rux-native/src/location.rs`, `rux-shell/src/location.rs`, Java `ruxLocationCurrent / Watch / Stop` in `RuxActivity.java`). Android uses `LocationManager` (`getCurrentLocation` on API 30+); the web uses `navigator.geolocation`; the desktop throws `unsupported`. `rux check` warns on a location call with no `[permissions.location]` (`undeclared_location`). Driven on the emulator (`rux-harness\ud002-and`) and in Edge (`rux-harness\ud002-web`); rows in `08-user-tests`. **Not driven:** a real GPS, a `timeout`, an approximate-only grant, Android 10 and below, a watch with the screen off beside a `location` service, `on_fix` from real app Rust, and the field report's piece 5 (open the location settings) and 6 (battery exemption), not built. **The field report's UD-002 status line is another session's uncommitted file: tell the owner, never stage it.** **Next:** UD-005 (open URL, dial, directions), an Opus plan the owner approves.
- 2026-11-23, **UD-003 in progress** (plan `C:\Users\USER\.claude\plans\clever-beaming-pie-of-16e1b4b9-46d9-48a3-acf3-06852ed24a3a.md`, owner approved: same-process service, notifications on Android + web + Windows, channels in `rux.toml`). **Step 1 done**: `app.state` (`foreground`/`background`) in script and `rux::app` in app Rust (`rux-native/src/app.rs`); desktop driven (minimize and restore, probe `C:\Users\USER\Mobile\rux-harness\ud003`); web compiled only; Android Java and JNI written, not compiled (no NDK clang here). **Step 2 done**: `[channels.<id>]` and `[background]` (`types`, `keep-awake`) parse in `manifest.rs` (cross-checked against `[permissions]`), reach the shell through `rux_shell::set_channels` / `set_background` (`rux-shell/src/notifications.rs`, a module-level `allow(dead_code)` to remove in step 3) from `build.rs` and `rux run`, and the Android manifest gets the `<service android:name="...RuxActivity$Background">` and `FOREGROUND_SERVICE*`/`WAKE_LOCK` permissions; the Java class that service names does not exist until step 4, so do not build an APK with `[background]` before then. **Step 3 done**: `notifications.post / cancel / cancelAll` in script and `rux::notifications` in app Rust (`rux-native/src/notifications.rs`, `rux-shell/src/notifications.rs`, Java `ruxNotify` / `ruxCancelNotification` / `ruxEnsureChannels` in `RuxActivity.java`), Android only, driven on the emulator (rows in `08-user-tests`); the web and Windows throw `unsupported` until steps 5 and 6. **Android builds and runs here** (the NDK is installed, the emulator `rux_test` starts, `rux build --target android` takes about a minute once warm): the older notes that say "no NDK clang" are stale. **Watch in step 4:** the activity can be destroyed and recreated while a service keeps the process alive, and a second `run_android` in one process panics (`RecreationAttempt`, see `run_android_with`): handle that before shipping the service. **Step 4a done (a fix found on the way)**: Android destroying the activity while the loop runs used to wedge the process (main thread blocked in `onDestroy` waiting for `android_main`; winit ignores the destroy), the existing "splash forever after a relaunch" bug. `RuxActivity.onDestroy` now calls `nativeDestroyed`, which ends the loop like a close (`RuxEvent::AndroidDestroyed`). A launch that clears the task now ends on the launcher instead of hanging. **Step 4 done (4b to 4d)**: `background.start / stop / active` in script and `rux::background` in app Rust (`rux-native/src/background.rs`, `rux-shell/src/background.rs`, Java `RuxActivity$Background` and `ruxBackgroundStart/Stop/MoveToBack`); Back at the root goes to the back while the service runs; the loop ending keeps the process while it runs; **a second activity in a process that already had one kills the process** (`attachBaseContext`, variant 1 of the spike: Android restarts the app straight away with the intent, route kept; the alarm and second-process variants were not needed). Driven on the emulator (rows in `08-user-tests`; traps in `09-author-notes`). **Not driven:** `start` from the back (`background` error: needs an app thread), a `location` service without location, `keep-awake`, `onTimeout`, the app's Rust calling it, a phone under Doze. **Steps 5 to 8 done 2026-11-23: UD-003 is built.** Notifications on the web (`new Notification` by tag, a tap pushes the route through `RuxEvent::WebLink`; driven in Edge over CDP, probe `rux-harness퀃-web`) and on Windows (WinRT toasts in `rux-shell/src/notifications.rs`, the AppUserModelID registered at the first post from `[app] id` and the new `set_app_name`; a tap restores the window and pushes the route through `RuxEvent::NoticeTapped`; driven, probe `rux-harness퀃-win`). `rux check` warns on an undeclared literal `channel:` and on `background.start` with no `[background]` (`permission_check.rs`, `undeclared_notifications`). Docs: `07-script` (App state, Notifications, Background work), `05-as-built`, `09-author-notes`, `08-user-tests` rows. **Not driven:** the web in a phone browser (reports `unsupported`), a Windows toast after the app quit (does nothing), `min` importance, a packaged Windows build, macOS and Linux (`unsupported`), and what step 4 listed (location service, `keep-awake`, `onTimeout`, a phone under Doze). **The field report's UD-003 status line is another session's uncommitted file: tell the owner, never stage it.** Site docs follow the per-release rule. Build probes with `target/debug/rux`: the `rux` on the PATH is stale. **Next:** UD-002 (location) and UD-005 (open URL, dial, directions), each an Opus plan the owner approves.
- 2026-11-23: **v0.10 work is on `build/v0.10`** (cut from `build/v0.9`, pushed to origin). **Permissions are done on Android, half done elsewhere.** `rux.toml` `[permissions.<name>] why` (location with `background` or `approximate`, notifications, camera, microphone; `network`; raw `android` names) writes the manifest; `permissions.status / request / why / openSettings` in script and `rux::permissions` in app Rust; `rux check` warns on an undeclared literal name (`permission_check.rs`, documents and `native/src`). A file input's `capture` asks for the camera first when declared. Driven on the emulator (rows in `08-user-tests`, "Permissions, 2026-11-12"). **Left undone, in this order:** (1) ~~the web backend~~ **done 2026-11-23**: the web build is fixed (`slides.rs` uses `web_time`, `overlay_active` is `false` on wasm, the render guards drop before `position_web_ime`) and the backend was driven in Edge over CDP: granted, blocked and ask all work (rows in `08-user-tests`; not driven: the `gesture` error, the location and notification prompts themselves, `openSettings` shows nothing when unhandled). The `rux` on the PATH is stale and drops the permission table from a web build: build with `target/debug/rux`. Probe `C:\Users\USER\Mobile\rux-harness\perm-web-app`, driver `perm-web\drive.py`, states by `perm-web\setprefs.py allow|block|ask`; (2) desktop not driven in a window (answers `"granted"`, unit-tested only); (3) `rux::permissions` not driven from real app Rust on a device; (4) site docs (`site/content/tooling/build.md`) not updated, by the per-release rule; (5) microphone has a manifest mapping and no request test on a device. **Next:** the web backend, then UD-003 (notifications, foreground service, lifecycle) and UD-002 (location) on top of this; each an Opus plan the owner approves.
- 2026-11-12: **one script reference.** `docs/07-script.md` is now the whole language (grammar, types, state, functions, modules, native, standard library, runtime calls); `10-types.md` and `11-next.md` are in `docs/archive/` and `docs/archive/script-reference-merge-2026-11.md` maps every section. The pipeline moved to `04-architecture.md`. Still one page on the site; splitting it is a separate step. **Next:** the same docs on `release/v0.8.0` (worktree `../rux-v080-docs`; Learn, anchors, how-it-works, why done in `9a17365`).
- 2026-11-12: the rhai-era names are retired (local commit on `build/v0.9`, not pushed). The list and what to write instead is `RETIRED` in `crates/rux-script/src/interp/stdlib.rs` and "Retired names" in `docs/07-script.md`. `rux check` errors with the replacement, `rux fmt` rewrites the renames that need no types. Math is `Math.*` only; maps lose `len`, `contains` and `is_empty`. All 140 test suites pass (`cargo test --workspace -j 2`). **Next, in its own Opus session:** one script reference (merge 07, 10 and 11). **Open:** the same docs fixes on `release/v0.8.0` (Learn, sync anchors, how-it-works and why are done there in `9a17365`, worktree `../rux-v080-docs`; the reference pages, tooling and home are not). The CLI on the PATH is not reinstalled.
- Branch `build/v0.9`, pushed 2026-11-06 at `d56e2c3` (CI runs on push). `main` and `build/v0.9-icons` are merged in (the icons one with `-s ours`: its content was hand-merged in `20939f2`). CLI reinstalled from it.
- 2026-11-06: `build/v0.9-icons` was found never to have been merged, and is now
  in the tip (merge commit `20939f2`). `<icon>`, `rux-icons` and the build's
  icon tree-shaking are in. Two fixes were needed to merge it: `<icon>` had no
  entry in the parser's attribute tables, and its block inside
  `build_node_inner` overflowed the debug main-thread stack, so it is now
  `build_icon` in `rux-style`. All 138 test suites pass on the merged tree
  (`cargo test --workspace -j 2`; at full parallelism the machine runs out of
  memory compiling test files).
- Deleted locally on 2026-11-06 as superseded: `wip/g4-kept-tree` (landed as
  `812f71e`) and `site/launch` (the site sync has moved far past it). The
  remote `origin/site/launch` was left alone. `spike/c0-frames` is kept on
  purpose: a measuring harness, results recorded in `docs/13`.
- `build/v0.9-icons` and its worktree `../rux-icons` are fully merged and can go.
- Docs corrected to the code on 2026-11-06 (every doc was checked against the
  crates and `rux check` probes, about 350 commits of changes since v0.7.1
  considered). Rewritten: `05-as-built`, `07-script`, `10-types`, `11-next`
  (2,900 to 1,000 lines), `04-architecture`, and `06-roadmap` (3,300 to 211
  lines: open work and the decisions that still bind only). Removed text lives in
  `docs/archive/` (one file per doc; `roadmap-history.md` holds the whole old
  roadmap). Not reviewed in this pass: `02-spec`, `03-guide` (both known to be
  history, still published to the site), `08-user-tests` (a log), `09-author-notes`
  (19 open rows, statuses not re-checked), field reports.
- The full audit record (method, what was left unverified per doc, questions
  for the owner) is `docs/archive/docs-audit-2026-11-06.md`. Read it before
  auditing the docs again.
- The owner will decide the docs and site redesign. Open questions for it:
  (the script references are one document now); much of the CSS section of `05-as-built` is not CSS (r-transition,
  computed/effect, keyed lists, Tailwind, `env()`); `03-guide` is false as
  written; `02-spec` is design history.

## Decisions not to revisit

- `native/` is on by default for `rux new`, beside `rux.toml`; locks are per
  target (`locks/<target>.lock`); the Rux app stays at the project root.
- Superseded branches above stay deleted; do not resurrect them.

## Known issues and open work

- **Urbandrop field report, `docs/field-reports/urbandrop.md`.** 2026-11-12: the bugs UD-008 to UD-011 and UD-012's
  `effect` cases are fixed, each with a test that failed first (`crates/rux-runtime/tests/`: `modules.rs`, `slot_scope.rs`,
  `async_listeners.rs`, `native_props.rs`), plus the small notes `flex-flow` and the image path in `rux check`
  (`image_base.rs`). All 144 suites pass (`cargo test --workspace -j 2`). Not yet driven in the Urbandrop app (the CLI
  on the PATH is not reinstalled); UD-012's Android sign-in case is open until it is tried on the emulator. Owner's calls, built: a bare attribute on a component tag passes `true`; `autocapitalize` as HTML has it
  (Android caps flags, web attribute; the CLI must be rebuilt for the Java shim). **v0.9 is frozen, not released,
  until v0.8 ships (needs a phone); new work goes on the next version.** **Open features, each an Opus plan the owner approves:** UD-001 to UD-007 and UD-013 to
  UD-016 (native errors with a code, location, background work and notifications, file bytes to Rust, opening other
  apps, maps, secret storage, date-time input, scroll position, sharing code between apps, print or PDF). The owner's
  rule: Urbandrop does not work around these; Rux builds them.

- Still open in v0.9: physics (springs), live regions, pinch and rotate, the
  `rux doctor` device survey (those two need a phone), `hyphens: auto`, video on
  macOS and Linux.
- The 0.9 publish must include `rux-native`, `rux-native-macros`, `rux-bindgen`
  (and `rux-icons`, already in `scripts/release.sh`'s first layer); crates.io has
  `ruxlang` only up to 0.7.1, so a scaffolded `native/` crate cannot build for
  anyone outside a checkout until then.
- `main` has diverged from `build/v0.9`: it holds the v0.7.0 and v0.7.1 release
  commits, which `build/v0.9` does not descend from. A release-process question.
- `cargo clippy -D warnings` fails in `rux-syntax` (four needless borrows); not
  touched yet.
- Mava Studio 2 (`docs/field-reports/mava-studio-2.md`): MS-009, MS-011 and
  MS-012 are fixed on this branch (2026-11-06, commits `66b4536` and
  `ee2eeec`). **MS-010 (drag and drop between containers) is the one open
  entry**: it needs an Opus plan first, since its design sets how drag works
  in general. Mava's pin must move to a commit at or after `20939f2` for
  `<icon>`.
- 2026-11-06 additions: `@contextmenu`, `@doubletap` and `event.button`
  (MS-011); `[window] decorations = false`, `app-region`, and the `window`
  module (MS-012). Driven on Windows only: touch long press, the web, macOS,
  Linux, Android and Windows snap are not (rows in `docs/08-user-tests.md`).
  Probes and drivers: `C:\Users\USER\Mobile\rux-harness\ms011` and `ms012`.
- **Title row beside the system's buttons (roadmap item 8): built 2026-11-06.**
  `[window] title-bar = "overlay"` (owner's syntax) keeps the native frame and
  lets the page into the title row; `env(titlebar-area-x|y|width|height)` is the
  free part of the row (undefined without an overlay, so fallbacks apply). On
  Windows a subclass on winit's window procedure (`rux-shell/src/caption.rs`)
  takes the caption's height back in `WM_NCCALCSIZE` and answers `WM_NCHITTEST`
  (top edge, the three buttons; `HTMAXBUTTON` gives the Windows 11 snap flyout),
  and the shell draws the buttons itself at a fixed 32 px, with the ink picked
  from the box behind them. macOS uses winit's full-size content view (written,
  not run). Driven on Windows 11 at 150% only; the rows and the gaps are in
  `docs/08-user-tests.md`. Known gaps: the drawn buttons have no accessibility
  nodes; `min-width`/`min-height` count the caption's height on Windows.
  Probe and driver: `C:\Users\USER\Mobile\rux-harness\titlebar-overlay`.
- **Child windows (roadmap item 8): built 2026-11-06, steps 1 to 6
  (`db718e2` to `cd746f1`, then the docs commit).** Docs: `07-script` "Other windows". Plan: `C:\Users\USER\.claude\plans\optimized-shimmying-aho.md`
  (copied to `docs/archive/child-windows-plan.md`). Owner's choices:
  `window.open(path, options)` opens a `.rux` file as its own document; stores
  are process-wide; the opener passes `props`; `close()` in a child closes that
  child; the handle has `close()`, `focus()`, `isOpen`, and `onClose` in the
  options; owned by default (`owned: false` for a taskbar button); `window.*`
  is the window the code runs in; `window.canOpen` is false on Android and web.
  - **Done**: (1) store hub `crates/rux-script/src/shared.rs` holds `V` values
    (not `Value`: ints and records survive); `Engine::join_stores` /
    `adopt_stores`, pulled by `Document::refresh_native_signals`; a hot reload
    calls `reset_shared_stores()`. (2) `Document::load_window(path, props)`;
    props become `let`s on the script's first line.
  - **Done (3)**: `Shell` in `rux-shell/src/lib.rs` owns `HashMap<WindowId, App>`,
    `main: Option<WindowId>` and `parked: Option<App>` (the main window's state
    while it has no window: before `resumed`, Android suspended, the web before
    `SurfaceReady`; `Shell::seat` re-files it under its window's id). `App` is
    one window's state; `context`, `text` and `images` are `Shared<T>` =
    `Rc<RefCell<T>>` (render holds the guards for the frame). `App::tick`
    replaces `App::about_to_wait` and returns its deadline; the Shell takes the
    earliest. Routing: `window_event` by id; `Answered`, `VideoFrame` and
    `Reload` go to every window; `PickedFiles` goes to the window with a
    `pending_file`, else main; `Access` by its `window_id`; the Android and web
    events go to main. `Shell::after_event` (end of every handler) runs
    `refresh_native_signals` on every window and redraws the ones that moved.
    Only the main window exists yet, so nothing observable changed.
  - **Done (4)**, in four commits. **4a** `window_module.rs` and `caption.rs` are
    per window: the shell names the window a document runs in around every
    entry (`window_module::as_window`, `Shell::each`), `window.minimize()` etc.
    act on it and `window.maximized` is that window's (`MAXIMIZED` is only the
    change trigger); caption state is keyed by `HWND` (`caption::detach` on
    close). **4b** a script `fn` given to a native call becomes an opaque
    `Callback` resource (`rux-script/src/native.rs`: `to_any` parks the closure;
    the engine claims it in `Document::apply_timer_requests` via
    `Engine::claim_callbacks`; `Document::run_callback(&Handle)` runs it as a
    handler; a dropped handle releases it). **4c** `window.open(path, options)`
    returns a `Window` resource (`close()`, `focus()`, `isOpen`; `window.canOpen`).
    `open` queues a `Request` (`window_module::take_requests`); `Shell::serve`
    drains it after every handler: `open_child` loads `Document::load_window`
    from the app's folder (`rux_runtime::project_root`), builds the child `App`
    (`App::with_document`, shares the main's `context`/`text`/`images`) and
    its window with `App::open_window` (the body `resumed` used to have;
    `owner` = main, `owned_by`), files it in `Shell::children`/`windows`.
    `close_child` (title-bar X, `w.close()`, or `close()` in the child) drops
    the `App`, `detach`es it, marks the handle closed (`isOpen` follows through
    `notify()`) and runs `onClose` in the opener. A `Reload` resets the stores
    once and reloads each window (a child with its props). **4d** `rux check`
    reads literal `window.open(...)` calls (`rux-runtime/src/window_open.rs`,
    run from `load_with`, also reported by `rux run`'s overlay): file exists,
    root is `<screen>`, props named and typed against its `prop`s, options
    named; a path not written out is a warning. Options: `title`, `width`,
    `height`, `minWidth`, `minHeight`, `maximized`, `decorations`, `titleBar`,
    `resizable`, `owned`, `props`, `onClose` (a wrong one throws kind `option`).
  - **The owner's example was not valid Rux**: closures are arrows, so it is
    `onClose: () => { inspector = none; }`, not `fn() { ... }`. Tell the owner;
    use the arrow in the docs. The typed form checks clean:
    `use type native::rux::window::Window; let w: Window? = signal(none);`.
  - **Driven on Windows 11** with `C:\Users\USER\Mobile\rux-harness\child-windows`
    (`start.ps1` then `act*.ps1`; coordinates in the scripts): a store counter
    is the same in both windows and either increments it; props arrive (`id:
    7`); the child's `window.toggleMaximize()` and `window.maximized` are the
    child's; `w.close()` from the opener, the child's X and `close()` in the
    child each close only the child and run `onClose` (`closed: 1`); the owned
    child has the main as its owner and is hidden when the main minimizes;
    `owned: false` has no owner. Not driven: the taskbar button itself, macOS
    and Linux (`with_parent_window`, written not compiled), a child's hot
    reload, a load failure through the overlay (tests cover `rux check` only),
    `onClose` when the opener is itself a child, a child with `titleBar:
    "overlay"` or `decorations: false`, Android and web (`canOpen` is `false`,
    `open` throws `unsupported`; not compiled here).
  - **Done (5)**: a child's X asks its own `@close` (`false` keeps it); `close_child` runs
    `shut_down` first. `App::close_now` only sets `App::quit`; `Shell::finish_quit` (end of
    every handler) closes a child that set it, or for the main closes every child (deepest
    opener chain first, no `onClose`), then the main's `shut_down`, then exits. Driven:
    kept by `@close`, closed with `unmounted`, main close runs child then main (log order).
    `settle_tasks` is per engine, read not driven with two pending tasks. A child opens at
    the OS's default place, not over its opener (not changed).
  - **Done (6)**: docs, user-tests and author-notes rows, `sync-docs.sh` run.
  - **Decided (owner, 2026-11-06)**: when the main window closes, every child
    closes with it and its context is destroyed as the main's is: components'
    `unmounted` deepest first, then the document's; children are not asked
    their own `@close`.
  - **Checked in 4**: a script `fn` could not cross into a native call; the
    owner chose the callback handle (4b), which keeps the approved API.
  - **Known**: `what_the_scaffold_writes_checks_clean` overflows the stack on
    unmodified `HEAD` too (watchlist #41): 6 of 8 runs alone on 2026-11-06,
    against 5 of 6 with step 3; not new. A whole-workspace run with
    `--no-fail-fast` passed all 138 suites. The wasm32 build already fails at
    this commit in code step 3 did not touch (`window_settings` in
    `overlay_active`, and `slides` taking `std::time::Instant`); the Shell's
    wasm and Android paths are written but not compiled (no NDK clang here).
    Init side effects in a store file still run once per window.
- `Style` has a size budget (744 bytes, watchlist #41): a new rarely used
  style property goes in `Rare`, as `app_region` does. One byte over it
  overflowed `rux check` on the tasks scaffold in a debug build.
- Android and web builds of a scaffold with `native/` have not been driven.
- Code defects found by the docs pass (none fixed yet):
  - `throw "x"` is caught as the string, not as an `Error`; `do`, `break value`
    and the bitwise operators are silently inert in a `<script>` body; `trunc`
    of NaN does not throw; an `any` is not checked where it enters a typed name.
  - `[1,2].for_each(...)` passes `rux check` but the interpreter has no
    `for_each`. Rhai-era names (`len`, `retain`, `drain`, `dedup`, `parse_int`,
    snake_case string methods) still run and are undocumented: document or remove.
  - Stale text in code: `vocab.rs` says a component declaring `mounted` is
    warned (it runs per instance); the hover text for `border`, `outline` and
    `position` is out of date (that text feeds the generated css-properties page);
    the hover text for `trim` mentions rhai; `rux-ir/src/types.rs` says `Set` and
    non-string `Map` keys "come with Rux's own interpreter"; `lexer.rs` and many
    `crates/*/src/lib.rs` module docs still cite the fork or "milestones M2 to M9".
  - `rux check` warns that `font-size: calc(100% + 2px)` is ignored while a unit
    test says it resolves; `transition: opacity 1s linear(0,1)` is reported
    badly (calls `opacity` a function); `rux new --help` is rejected.
  - Two doc statements rest on one grep only: slider arrow keys, a drawn desktop
    date picker and dropping a file on the input are listed as not built.

## MS-010 in progress (2026-11-06)

Plan approved by the owner: `C:\Users\USER\.claude\plans\twinkly-floating-pebble.md`
(copied to `docs/archive/ms010-drag-drop-plan.md`). **Owner rule: Rux ships primitives, not
effects.** So there is no shell-drawn drag copy and no auto-scroll feature: the
author draws the copy from `@dragmove` (every pointer move) and scrolls with a
`scrollBy` call (not built; script has `scrollIntoView` only).

- **Step 1 done and committed** (`7c3edc3`, docs `22b68fb`): a keyed `r-for` row
  whose `transition` names `transform` slides when it moves
  (`LayoutState::set_nudges`, `crates/rux-shell/src/slides.rs`). Driven on
  Windows (`rux-harness\ms010`).
- **MS-010 is built and committed** (steps 1 and 2, `7c3edc3`, `d5c141e`; then
  `scrollBy`). Rux ships primitives only: `drag-data`, `drag-group`,
  `drag-handle`, `drop-group`, seven events with one `DropEvent`, `:dragging`,
  `:drag-over`, `el.scrollBy(dx, dy)`, and keyed rows that slide
  (`transition: transform`). Geometry is pure and tested in
  `crates/rux-shell/src/dnd.rs`; press and session state is `dnd_session.rs`.
  Driven on Windows only (`rux-harness\ms010`, `ms010-dnd`, `ms010-scroll`);
  not driven: touch (handle pickup, long press), a box over a source, an empty
  target, auto-scroll from `@dragmove`, web, macOS, Linux, Android. Spots are
  read from the Node tree and boxes from the last frame's metrics
  (untransformed). A target is any element with `drop-group` or an
  enter/over/leave/drop handler (the plan said `@drop` required).
- **`rux check` on the tasks scaffold is one frame from a stack overflow in a
  debug build** (watchlist #41, `what_the_scaffold_writes_checks_clean`). It
  tipped twice in MS-010: a field on `Node` (moved into `Style::rare`) and a
  match arm in the interpreter's `method` (made a bare call to an `inline(never)`
  fn). Also the recursive style builders. A debug build gives every match arm
  its own locals. It passes alone and can fail under `--workspace`, so run
  `cargo test --workspace --test new` after touching any of those. Worth
  deciding: run `rux check` on a thread with a larger stack.
- Python heredocs lose backslashes: use Edit for docs with Windows paths.

## Next sessions (owner's decisions, 2026-11-06; each its own Opus plan session, then Sonnet builds)

1. ~~`rux check` checks every `.rux` file~~ **done 2026-11-06** (plan:
   `C:\Users\USER\.claude\plans\valiant-kindling-tulip.md`). A bare `rux check` walks the
   `rux.toml` folder from any subfolder (skips `target`, `node_modules`, `locks`, `dist`,
   `native`). A component is read on its own: a declared prop is owed and keeps its type,
   any other undefined name is an **error** (`level_for` in `rux-script`), an unresolvable
   call stays a warning, and `<slot>` and `<router-view />` are not reported as stray in a
   file with a caller. A routed page is dropped from the list and reached through the
   entry. Findings seen two ways print once, and a caller's wording of a component's own
   mistake is left to the component's own read. In a folder with no `app.rux` a component
   resolves `use` from the folder the walk started in (`rux_runtime::set_loose_root`), so
   `rux check examples/components` alone cannot find `stores/`. The native seam is tested
   end to end (`the_native_seam` in `tests/check.rs`; no gap found). `rux check --native`
   runs `cargo check` on `native/` (`native::cargo_check`; the patch list for a `-dev` CLI
   is `native::checkout_crates`, shared with `build.rs`); its e2e test is `#[ignore]`
   (`cargo test -p ruxlang --test check -- --ignored`) and was driven by hand too.
   A method a number does not have (`n.trim()` on an `int` or `float`) is now an error in
   a template (`method` in `rux-script/src/check.rs`); other receivers' unknown methods are
   still unchecked.
   ~~MS-014~~ **done 2026-11-06**: a dev build's wrapper embeds the full icon set
   (`embed_all_icons` in `build.rs`) and says so in the build output; covered by a wrapper
   test, and driven in a real native-crate window (the heart draws).
2. ~~`rux new` asks whether to include a router~~ **done 2026-11-06** (plan:
   `C:\Users\USER\.claude\plans\deep-popping-raccoon.md`). `--router` / `--no-router`;
   at a terminal `minimal` asks `Include a router? [Y/n]` (Enter = yes); no terminal = on.
   Routed minimal gains `pages/about.rux` and a tab bar; router-off writes `<home />` in
   `app.rux` and no `missing.rux`/`about.rux`. `tasks` is always routed (`--no-router`
   refused). Driven on Windows: the prompt at a real console, both projects in a window.

## Next action

**2026-11-25, newest: the v0.10 to-do list.** Only this PC is used from now on. Built on v0.10 so far: permissions, UD-001 to UD-005, UD-007, UD-012 to UD-014, pinch and rotate, bounded image memory and the disk cache, and the `#[path]` step of UD-015. CI is green on `4f96fee`. In order:

1. ~~**`rux check` reports a component that fails to load**~~ **done 2026-11-25.** The cause was not the component path: walking a project, `rux check` dropped what a caller's load said about a used component and left it to the component's own read, which loads it as a document (not linked as its caller links it, and with prop defaults instead of the caller's values). Now held and kept unless the own read reported that line (`through_callers` in `crates/rux-cli/src/check.rs`). Test `a_component_that_fails_only_inside_its_caller_is_reported` (failed before). Found on the way, not fixed: one missing store function (`shop.nope()` in a component `fn`) gives three errors and a warning on one line.
2. ~~**Error boundaries**~~ **built 2026-11-25** (State, first entry; leftovers listed there).
3. ~~**The capability model's sandbox half**~~ **built 2026-11-25** (State, first entry; leftovers there and in the roadmap).
4. **Owner decision:** whether a removed component's queued handler stops running at page level (State, 2026-11-24).
5. **Open v0.9 items moved to v0.10** (roadmap, v0.9 section): physics, live regions, `rux doctor` device survey (needs a phone), `hyphens: auto` (owner said not now), video on macOS and Linux, child windows on macOS and Linux.
6. **Held:** UD-015's dependency-crate and `.rux` halves (ask before planning); UD-006 and UD-016 need nothing from Rux now (assessed).

The 2026-11-12 block below is history: the branch was made on 2026-11-23 and its order is done or listed above.

**2026-11-12, read this first.** The owner froze v0.9: it is not released before v0.8, which waits for a
phone to test its Android features. New work goes on the next version (v0.10). Two decisions are open with
the owner before any v0.10 work:

1. **The branch:** proposed `build/v0.10` made from the tip of `build/v0.9`. Ask; do not create it unasked.
2. **The order.** Proposed, owner has not answered:
   (a) fix the web build: `cargo check -p rux-web --target wasm32-unknown-unknown` fails in `rux-shell`
   (`self.window_settings` at about line 4192 does not exist on `App`; `web_time::Instant` passed where
   `std::time::Instant` is wanted at about 8034 and 8063). Found 2026-11-12, already in committed code; a
   frozen version should build for every target;
   (b) the web-standard Urbandrop items: UD-013 (`datetime-local`, `time`, labelled and disabled select
   options), UD-014 (`scrollTop`/`scrollLeft`, `@scroll`, `scrollTo`), UD-005 parts 1 to 3 (open a URL, dial,
   directions);
   (c) the Rust boundary: UD-001 (native errors with a kind and fields), UD-004 (file bytes to Rust),
   UD-007 (secret storage), UD-015 (sharing code between apps);
   (d) the capability model (roadmap v0.10 item 2: manifest permissions from what native code uses);
   (e) platform services: UD-002 (location, foreground service), UD-003 (notifications, background work,
   lifecycle). The emulator can fake location and notifications; background survival needs a phone;
   (f) later: UD-006 maps (needs pinch), UD-016 print and PDF.
   The open v0.9 items (physics, live regions, pinch and rotate, `rux doctor` survey, `hyphens: auto`, video
   on macOS and Linux, child windows driven off Windows) move to v0.10. The owner may want the "needed now"
   items (location, notifications) first instead.

Each feature group is its own Opus plan the owner approves before Sonnet builds it.

**2026-11-11, docs and site pass** (owner's decisions: docs are versioned and
public docs show only their release; the playground keeps running the latest
release and moves to `playground.ruxlang.dev`). The audit is
`docs/archive/docs-audit-2026-11-11.md`; its "Fixed" section lists what was
corrected on this branch. Open from it: the restructure (section 4), the
playground move (section 5), the per-command `--help` defect, and the same
stale text on `release/v0.8.0` (Learn, home, how-it-works, why), which needs its
own pass on that branch, with the owner's go-ahead.

Next: the owner decides (see the list below). `main` takes a version only when it is published, and v0.8 is queued ahead of v0.9, so no merge to `main` yet. 2026-11-07: an `<icon>` now draws in its own `color`, and an author `fill`/`stroke` beats it (`icon_box` in `rux-style`; tests in `rux-runtime/tests/icon_color.rs`, driven). Not checked: `transition: color` on an icon. The older list follows:

1. ~~Child windows~~ done (steps 1 to 6).
2. ~~MS-013~~ done 2026-11-06 (`3a2c96c`): percent radii per axis, `a / b`,
   two-value longhands; drawn by an elliptical path in `rux-paint`
   (`rounded_rect` takes `radius_y`). Driven in no window yet; tests only.
   Gaps: outer `box-shadow` on an elliptical corner uses the shorter radius;
   `transition: border-radius` animates horizontal radii only. Boxes that
   relied on the old pill must use `9999px`.

~~`rux new` scaffolds get `pages/` and `stores/`~~ done 2026-11-06: `minimal` gains
`stores/greeter.rux` (the home page's greeting goes through it; with `native/` it
calls `hello.greet`). The native variant checks clean but was not built and run
(the Rust build takes minutes). `rux check` now runs on a 256 MB thread, which
ends the scaffold stack overflow (watchlist #41; 7 of 7 runs pass).

After those, decide with the owner when to push `build/v0.9` and cut v0.9.
MS-012's status line in the Mava report still says the overlay is not built.

## Verified on 2026-11-06

Step 3: `cargo test --workspace -j 2 --no-fail-fast` 138 suites, 0 failed (the
scaffold flake hit once in a run without `--no-fail-fast`). Probes driven on
Windows with the new build, same rects and screenshots as before:
`ms012`, `titlebar-overlay`, `ms010-dnd` (shots in the scratchpad, not the
harness folders).

Trial merge built and tested in a scratch worktree; `rux new` + `rux check` on
the `tasks` template passes after the stack fix; the roadmap was checked against
`git log` since the v0.8 freeze. An `<icon>` has not been driven in a window
since the merge.
