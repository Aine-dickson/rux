# Field report: Urbandrop mobile apps

Urbandrop (`github.com/bitcraft-dev/urbandrop`, private) is a delivery marketplace in Kampala. Its two Android apps are written in Rux: the partner app for riders and fleet owners (`mobile/partner`, started 2026-11-11) and the client app for customers (next). Both talk to an Axum API through an app-side Rust client (`mobile/core`, reqwest with rustls) behind each app's `native/` crate, which is ordinary app code and not a Rux limit.

The owner's rule for this report: Urbandrop does not work around a Rux limit in the app; it reports it here and builds what does not depend on it. **Each entry is a whole feature**, listing every piece the apps will need from it, so it can be designed and built once rather than one call at a time. Pieces are marked **needed now** (blocks a screen being built) or **soon** (same feature, needed before launch).

Built against `rux 0.9.0-dev` from `build/v0.9`.

## UD-001 Errors from native code that carry data

- **Found:** 2026-11-11, partner app sign-in.
- **Limit:** a native `Err` reaches script as `{ message, kind }`, with `kind` always the Rust type's short name (`Error::from_rust`, `rux-native/src/value.rs`). An app whose errors are one Rust type with a code inside (an HTTP API answering `{ code, message }`) cannot tell its errors apart in script: `phone_required`, `not_eligible`, `quote_expired`, `payment_failed` and a lost connection all arrive as `kind: "ApiError"`. Returning `rux::Error` itself does not help, since it is wrapped again with kind `"Error"`.
- **What Urbandrop needs (as one feature):**
  1. **Needed now.** A way for an exported function's error to set `kind` itself, e.g. a trait the error type implements (`fn kind(&self) -> String`), with the type name as the default; and `Result<T, rux::Error>` passed through unwrapped.
  2. **Needed now.** Extra fields on the thrown error, e.g. `status: int` and `details: any`, readable as `e.status` and typed by the checker when the Rust type declares them (an `#[rux::export]` error struct whose fields cross like a record's).
  3. **Soon.** `catch e` narrowing on `e.kind` in the checker, so `if e.kind == "phone_required"` is checked against the kinds a native function can throw, where Rust declares them (an enum error type listing its variants as kinds).
  4. **Soon.** `throw` of a string arriving as an `Error` (listed as not built in `11-next`), so script and native errors are handled the same way.
- **Reproduction:** `#[rux::export] pub async fn f() -> Result<(), MyError>` where `MyError { code: String, message: String }` implements `Display`; in script, `catch e` sees only `e.message` and `e.kind == "MyError"`.
- **Client app (2026-11-12) needs the same feature for:** `quote_expired` (the web re-quotes and places the order again
  when the fare is unchanged, else says the price changed), `phone_required` (the phone field only after it, as in the
  partner app), and a receipt that answers 401 ("Sign in to see your receipt" rather than a general failure). An app's
  Rust can rewrite a message (the client turns tracking's 404 into the web's sentence), but script cannot branch on it.
- **Urbandrop meanwhile:** shows `e.message` (the server's sentence). The sign-in page always shows the phone field ("needed the first time you sign in") instead of asking for it only on `phone_required`.
- **Status:** open.

## UD-002 Location

- **Found:** 2026-11-11, partner app current-job screen.
- **Limit:** Rux has no location API. Riders share their position every 15 s while they hold a job (the API's `POST /partner/position`), the admin live map shows it, and the customer's progress bar is computed from it. The web version stops when the phone locks, which is the main reason for a native rider app.
- **What Urbandrop needs (as one feature):**
  1. **Needed now.** Permission: ask for precise location, read its status (granted, denied, denied for good, approximate only), and open the app's system settings page when it was denied for good.
  2. **Needed now.** One fix: `await location.current({ accuracy, timeoutMs })` giving `{ lat, lng, accuracyM, at, isMock }`.
  3. **Needed now.** A watch: start and stop continuous updates with a minimum interval and distance, delivered as a `Channel` or an event, and **reachable from the app's Rust** as well as from script, so the native crate can post positions itself without a page being on screen.
  4. **Needed now.** Keeping on while the screen is off and the app is in the background: on Android a foreground service of type `location` with its required persistent notification (title, text, icon, a tap opening a route), started and stopped by the app; plus the Android 10+ background location permission flow ("Allow all the time").
  5. **Soon.** Location services switched off: detect it and offer the system's location settings.
  6. **Soon.** Battery optimisation: report whether the app is exempt and open the system prompt, since Android vendors kill background work otherwise.
  7. **Soon.** `isMock` (Android's mock-location flag), for fraud checks.
  8. **Soon.** The manifest permissions (`ACCESS_FINE_LOCATION`, `ACCESS_BACKGROUND_LOCATION`, `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_LOCATION`) generated from what the app uses, which ties to the roadmap's capability model item.
  9. **Soon.** The web target mapping 1 to 3 onto `navigator.geolocation`, so a page works on both.
- **Client app (2026-11-12):** "Use my current location" as the pickup on the booking's Route step needs piece 2 (one
  fix, with its permission); turning the point into an address is plain HTTP from Rust. Left out of the app until then.
- **Urbandrop meanwhile:** the current-job screen does not share a position; riders keep using the web partner app for live tracking.
- **Status:** open.

## UD-003 Work in the background, and local notifications

- **Found:** 2026-11-11, partner app job board.
- **Limit:** nothing in Rux runs while the app is not on screen, and nothing can post a notification. The owner chose, for launch, that a rider who is **on shift** gets new-job alerts by the app polling the job board every 20 s in the background and posting a local notification; push through Firebase comes later.
- **What Urbandrop needs (as one feature):**
  1. **Needed now.** Local notifications: post, update and cancel by id, with title, text, small icon, sound, vibration and a channel (Android 8+ channels: "New jobs" loud, "On shift" silent); a tap opens the app on a route with data.
  2. **Needed now.** The notification permission (Android 13+ `POST_NOTIFICATIONS`): ask and read its status.
  3. **Needed now.** Background work while on shift: a foreground service (Android type `dataSync` or shared with UD-002's `location` service) that keeps the app's Rust running, so a Tokio task can poll and post notifications with no page showing. Started and stopped by the app, with its persistent "On shift" notification.
  4. **Needed now.** App lifecycle in script and in Rust: came to the front, went to the back, about to be stopped, so polling can speed up, slow down or stop.
  5. **Soon.** Connectivity: online or offline, and a change event, so the app shows "offline" and pauses calls rather than failing each one.
  6. **Soon.** Push through FCM (token, token refresh, a message waking the app's Rust and showing a notification), designed with 1 to 4 so local and push notifications look and behave the same.
  7. **Soon.** Launcher badge count.
- **Urbandrop meanwhile:** the job board refreshes every 20 s while it is on screen; nothing alerts a rider who has left the app.
- **Status:** open.

## UD-004 Picked and captured files, read from the app's Rust

- **Found:** 2026-11-11, partner app hand-over step. Corrected the same day: the first version of this entry asked for camera capture, which is already built (`<input type="file" accept="image/*" capture="environment">`, `05-as-built`, file inputs). What is left is getting the file to native code.
- **Limit:** a delivery is handed over with the recipient's code and a **photo**, and riders and owners upload KYC papers (ID, permit, logbook, insurance, as images or PDFs). The API takes them as multipart, which the app's Rust client sends. A `File` from a file input or `pickFile` carries `name`, `size`, `type`, `url` and, on the desktop only, `path`; on Android `url` is a `content://` URI and there is no path, and file-input plan decision 2 ("no bytes for now") means script cannot hand the bytes over either. So the photo can be taken and shown, but not uploaded.
- **What Urbandrop needs (as one feature):**
  1. **Needed now.** A `File` passed to an exported Rust function readable there on every target: its bytes (or a reader), its name and media type. Either a `rux::File` parameter type with `async fn bytes()` that goes through the `ContentResolver` on Android and the `blob:` on the web, or a readable path (a copy in the app's cache, as the camera already writes there).
  2. **Needed now.** The same for a captured photo after Android killed and restored the app while the camera was open (the input's value is restored; it must still be readable).
  3. **Soon.** Downscaling and recompressing an image to a size limit, with EXIF orientation applied, either built in or left to Rust once 1 gives bytes (a 12 MP phone photo is well over the API's 5 MB).
  4. **Soon.** Releasing a captured photo from the app's cache once it has been sent.
- **Already built, used as is:** the camera through `capture`, the picker with `accept` (images and PDFs), and the preview with `<image :src="file.url">`.
- **Urbandrop meanwhile:** the hand-over step tells the rider to finish it in the web partner app; papers upload is not built in the mobile app.
- **Status:** open.

## UD-005 Handing over to other apps

- **Found:** 2026-11-11, partner app current-job screen.
- **Limit:** a rider must call the sender and the recipient and open turn-by-turn directions to the pickup and drop-off. Rux cannot open a URL or another app.
- **What Urbandrop needs (as one feature):**
  1. **Needed now.** Open a URL in the system (`https:` in the browser).
  2. **Needed now.** Dial a number (`tel:`), without the call permission (the dialer opens with it filled in).
  3. **Needed now.** Directions to a point in the person's maps app (`geo:` / Google Maps navigation intent), with a label.
  4. **Soon.** SMS and WhatsApp to a number with text (`sms:`, `https://wa.me/`), and the share sheet for text (a tracking link).
  5. **Soon.** Open this app's system settings page (shared with UD-002 and UD-003).
  6. **Soon.** Knowing whether something can handle a link before showing its button.
- **Client app (2026-11-12):** the Confirm step's "Preview route in Google Maps" is piece 1 (an `https:` link); left out.
- **Urbandrop meanwhile:** phone numbers and addresses are shown as text.
- **Status:** open.

## UD-006 Maps

- **Found:** 2026-11-11, planning the client app's booking and tracking screens and the partner app's job screen.
- **Limit:** no map element. The web apps use Leaflet with OpenStreetMap-style tiles: pick pickup and drop-off points, show a straight two-colour line with the covered part green, and a marker moved along it.
- **What Urbandrop needs (as one feature):**
  1. **Soon.** A `<map>` element showing raster tiles from a configurable URL template (`{z}/{x}/{y}`, an API key in the URL, attribution text shown as the provider requires), with pan and zoom by drag, double tap and pinch (pinch is already an open item).
  2. **Soon.** Markers at points (custom content or an icon), polylines with colour and width, fitting the view to a set of points.
  3. **Soon.** Tap on the map giving the point, and the centre and zoom as bindable values, for picking a pickup point.
  4. **Soon.** Tiles cached on disk with a size cap.
  5. **Soon.** The user's own position dot (with UD-002).
- **Client app (2026-11-12):** the Route step is built on address search alone (Google Places or Nominatim from Rust).
  Missing until a map exists: the map under the fields with both pins and the line between them, "Choose on map" (pan
  under a fixed crosshair, confirm, the centre becomes the point: piece 3), and the tracking page's map with the
  covered part of the line in green (piece 2). The tracking page's progress line is drawn with `<path>` meanwhile, as
  the web's RouteProgress does it.
- **Urbandrop meanwhile:** nothing yet; the partner app does not need a map for its first screens.
- **Status:** open.

## UD-007 Secret storage

- **Found:** 2026-11-11, partner app session.
- **Limit:** the long-lived refresh token is kept with `settings`, a plain JSON file in the app's private files. Fine for a first build, but a token for an account that moves money should be in the platform's protected store.
- **What Urbandrop needs (as one feature):**
  1. **Soon.** `secrets.get`, `set`, `remove`, kept with Android Keystore encryption (and DPAPI on Windows, the Keychain on macOS; the web may refuse), readable from the app's Rust too so the token need not pass through script.
  2. **Soon.** Survives app updates, is removed with the app, and is excluded from backups.
- **Urbandrop meanwhile:** `settings.set("refreshToken", ...)` in `stores/session.rux`.
- **Status:** open.

## UD-008 Runtime panic awaiting a store's `async fn` (checker passes)

- **Found:** 2026-11-11, partner app, desktop `rux build` (dev), `rux 0.9.0-dev`.
- **Limit (a bug, not a missing feature):** the app panicked at start-up, before the first frame:
  `thread 'main' panicked at crates\rux-runtime\src\lib.rs:1082:9: the typed IR broke its own rules: fn leave at 512..535: an 'await' of something that is not a call to an 'async fn' or a host function`. `rux check` reported no problems for the same files.
- **Reproduction:** `stores/session.rux` exported `export async fn signOut() { await api.signOut(); }` where `api` is the app's native module (`use native::api;`, `api.signOut` an `#[rux::export] pub async fn sign_out()`). `app.rux` had `async fn leave() { work.clear(); await session.signOut(); replace("/"); }`, started from a `@tap`. Same panic shape expected for `await work.refresh(true)` in a page, where `work.refresh` is a store `async fn` that awaits a native call.
- **Does not reproduce:** a minimal `rux run app.rux` with a store `export async fn later() { text = "done"; }` (no native call inside) awaited from the document's `mounted` runs fine. So the trigger seems to be the store's `async fn` awaiting a native function, or the built binary rather than `rux run`; not narrowed further.
- **Expected:** either it runs (the docs say an exported `async fn` may be awaited from another file's `async fn`), or `rux check` refuses it with the same message, never a panic at load.
- **Urbandrop meanwhile:** awaits the native function directly in the page (`await api.signOut()`) and starts store refreshes without awaiting them.
- **Status:** fixed 2026-11-12 on `build/v0.9`, Not native code: any `await` of another file's `async fn` broke a debug-build IR rule (the panic is debug-only, so the release `rux run` never showed it). Test: `a_modules_async_fn_awaited_from_the_documents_async_fn_loads` in `crates/rux-runtime/tests/modules.rs`.

## UD-009 Slot content is resolved in the component, not the caller

- **Found:** 2026-11-11, partner app Fleet tab, desktop `rux build` (dev). `rux check` reported no problems.
- **Limit (a bug):** the docs say slot content is the caller's code. In the app, markup the Fleet page put inside `<record-card>`'s `<slot />` behaved as if it belonged to `record-card`:
  1. Components used in the slot (`<owner-fields>`, `<vehicle-fields>`, imported by the page, not by `record-card`) failed at run time: `record-card.rux line 21: there is no element or component called <owner-fields>`.
  2. With those imports added to `record-card` as a test, the slot's `r-if='editKind == "owner"'` (`editKind` a page signal, `""` at the time) showed the editor as if true. (The `:vehicles` type error seen at the same time is UD-011 and happens without a slot too.)
- **Reproduction:** a page with `let open = signal(false);` and `<card-like><view r-if="open"><other-component /></view></card-like>` where `card-like` has a `<slot />` and does not import `other-component`.
- **Urbandrop meanwhile:** `record-card` has a `flat` mode and the page puts the card and its editor side by side inside one `card` box, so the screen looks the same without a slot.
- **Status:** fixed 2026-11-12 on `build/v0.9`, Two causes: slot content took its tags from the component's imports, and its `r-if`/`r-elif`/`r-else`/`r-for` were not read at all. It now builds in the caller's file, imports and instance, and a tap in it moves the caller's state. Tests: `crates/rux-runtime/tests/slot_scope.rs`.

## UD-010 An `async fn` started by a component's event loses the caller's scope after a native `await`

- **Found:** 2026-11-11, partner app Fleet tab, desktop `rux build` (dev). `rux check` reported no problems.
- **Limit (a bug):** `<owner-fields ... @submit="saveOwner(event)" />` in `pages/fleet.rux`. `saveOwner` is the page's `async fn`: `busy = true; error = ""; try { await fleetApi.saveOwner(form); saved("..."); await load(); } catch e { error = e.message; } busy = false;` where `fleetApi.saveOwner` is a native `#[rux::export] pub async fn`. The native call ran (the change reached the server), then the overlay said `fleet.rux line 145: async fn saveOwner failed: error is not defined`: after the `await`, `saved` (a page `fn`) threw, and the `catch` could not see the page's `error` either. The button stayed on "Saving...". So after the `await` the run seems to continue in the emitting component's scope (`owner-fields`, which has a prop `busy` and no `error`).
- **Does not reproduce** under `rux run` with a store's (non-native) `async fn`: a child emitting from `mounted`, the listener `@go="run(event)"` on a page inside a router and an `r-if`, `await greeter.later(); done(n);` in a `try`, works. So the trigger looks like the native await (as in UD-008) or the built binary.
- **Same file, works:** a page's `async fn` started from a plain `<button @tap>` (Become fleet: `await fleetApi.saveOwner(...)`, then page functions) completes correctly.
- **Impact:** every save from the shared form components on the Fleet tab (owner payout details, add or edit vehicle, add or edit rider) reaches the server but leaves the screen stuck until the tab is reopened. The apply wizard's last step reaches the server and the app moves on because the session change replaces the page.
- **Urbandrop meanwhile:** nothing; waiting for the fix.
- **Status:** fixed 2026-11-12 on `build/v0.9`, A task a listener started was filed under the emitting component, so it went on in that component's scope once a native call made it wait. It is now the caller's. Test: `crates/rux-runtime/tests/async_listeners.rs`.

## UD-011 A list of native records is refused by a prop typed with that record

- **Found:** 2026-11-11, partner app Fleet tab, desktop `rux build` (dev). `rux check` reported no problems.
- **Limit (a bug):** `pages/fleet.rux` has `computed vehicles = fleet.vehicles;` where `fleet` is a native `#[rux::export]` struct whose `vehicles: Vec<VehicleView>` (`VehicleView` another exported struct). Passing it to a component, `<driver-fields :vehicles="vehicles" ... />` with `prop vehicles: VehicleView[] = [];` (`use type native::fleet::VehicleView;`), fails at run time: `fleet.rux line 66: <driver-fields> was given a list of 1 for :vehicles, which is not the VehicleView[] prop vehicles takes, so it is built without it, and it takes its default`. The same list renders fine with `r-for` in the page.
- **Expected:** a `Vec<VehicleView>` from Rust is a `VehicleView[]` (as `rux check` already agrees).
- **Impact:** the rider form's "Vehicle used" choice is missing when adding or editing a rider.
- **Urbandrop meanwhile:** nothing; waiting for the fix.
- **Status:** fixed 2026-11-12 on `build/v0.9`, A component's `use type native::…` brought in only the named type, so a struct with fields of the module's other types (`Record`, `VehicleForm`) never fitted at run time. The module's other types now come too. Test: `crates/rux-runtime/tests/native_props.rs`.

## UD-012 An `async fn` started by an `effect` while the page is built stops at its native `await`

- **Found:** 2026-11-12, client app My orders, desktop `rux run` (dev), `rux 0.9.0-dev`. `rux check` reported no problems.
- **Limit (a bug):** `pages/account.rux` had `effect { if signedIn { load(); } }`, `load` a page `async fn` that sets
  `loading = true`, awaits `api.myOrders()` (a native `#[rux::export] pub async fn`), then sets the list and
  `loading = false`. When the page is opened with the session already signed in, so the effect's first run (as the
  page is built) starts `load`, the page stays on "Loading..." for good: nothing after the `await` runs, and no error
  or warning appears. The server answered the same call in 25 ms.
- **Reproduces every time:** open My orders a second time in a run (the first visit of a run also hangs whenever the
  session is already known when the page is built).
- **Does not reproduce:** the same `load()` started from `mounted` (visited twice, both load); the same effect when
  the session became signed in after the page was built (the effect's later run loads); the booking page's
  `mounted { loadClasses(); }` on a second visit; the booking page's `effect` that re-quotes (it always runs again
  after the page is built, with the price inputs).
- **Possibly the same bug (seen once, not narrowed):** signing in from the `sign-in` component on My orders. The
  component's own `verify()` (started by its own button, no emit) awaits `api.verifyCode`; Rust sets `api.session`
  during that await, which reruns the page's effect and starts `load()`. Then `verify()` stayed on "Checking..."
  and `load()` reported `async fn load failed: error is not defined` from its `catch` after the `await`, as in UD-010.
  On the booking page, the same component's errors after the `await` were not shown either.
- **Also on Android (2026-11-12, `rux_test` emulator, debug APK), and with no effect starting anything:** on the
  booking's You step the page's only effects are synchronous (the name prefill, and `session.keep` in `app.rux`). The
  `sign-in` component's `verify()` awaited `api.verifyCode`, the server signed the account in (account and phone
  saved, code consumed), and the component stayed on "Checking..." while the page never switched to the signed-in
  form. After a relaunch the kept token signs in and everything works. So the trigger looks like Rust setting a
  signal (`api.session`, from the Tokio thread) that the page reads while a component's `async fn` is awaiting the
  native call that sets it.
- **Impact:** after signing in on My orders, the list appears only on the next visit (the app loads from `mounted`).
  Signing in during a booking leaves the step stuck until the app is restarted.
- **Status:** partly fixed 2026-11-12 on `build/v0.9`. The `effect` cases: an `effect`'s tasks were never given an owner. Run as the page was made, with no `mounted` to claim them, the task was owned by nobody and its answer was dropped; run again by another instance's code (the sign-in case), it was filed under that instance and went on in its scope. Each effect run now owns what it starts. Test: `an_effects_async_fn_started_as_the_page_is_made_goes_on_after_its_await` in `crates/rux-runtime/tests/async_listeners.rs`. **Still open: the Android case.** Its shape (a component's `async fn` awaiting a native call that sets a native signal the page reads) passes on the desktop after the fix (`a_native_signal_set_during_a_components_await_reaches_the_page_and_the_task_goes_on`), so it has to be tried again on the emulator with a CLI built from this tree.

## UD-013 Date and time inputs

- **Found:** 2026-11-12, client app booking, Service step.
- **Limit:** `<input>` has `type="date"` only. The web's Service step asks for the earliest pickup with
  `type="datetime-local"` (with `min` now and `max` 14 days ahead) and a window length from a select whose options
  are values with labels (`60` shown as "1 hour").
- **What Urbandrop needs (as one feature):**
  1. **Needed now.** `type="datetime-local"` (and `type="time"`), holding HTML's `YYYY-MM-DDTHH:mm` in local time,
     with `min`/`max`, and Android's date then time pickers, as `type="date"` already opens its picker.
  2. **Needed now.** Select options with a value and a label (`[{ value: 60, label: "1 hour" }]`), the value typed as
     a number when the options are numbers. The partner app hit the same (vehicle classes with "coming soon").
  3. **Soon.** Disabled options in a select.
- **Urbandrop meanwhile:** Normal bookings are flexible only; the step says choosing a time is on the website.
- **Status:** open.

## UD-014 Reading a scroller's position

- **Found:** 2026-11-12, client app booking, vehicle carousel.
- **Limit:** script can `scrollBy` and read an element's `width`, but cannot read how far a box is scrolled or hear
  that it scrolled. The web's vehicle row shows a dot per vehicle following the swipe, and greys the arrows at either
  end.
- **What Urbandrop needs (as one feature):**
  1. **Needed now.** `el.scrollLeft` / `el.scrollTop` (and the scrollable size), and a `@scroll` event, or a
     `@scrollend` once snapping settles.
  2. **Soon.** `el.scrollTo(x, y)` with a smooth option, so an arrow can go to a card by index.
- **Urbandrop meanwhile:** the arrows move one card (`scrollBy` by a card's width, the snap settles it); no dots.
- **Status:** open.

## UD-015 Sharing code between two Rux apps

- **Found:** 2026-11-12, starting the client app beside the partner app.
- **Limit:** both apps sign in the same way, but nothing Rux reads can be shared. `#[rux::export]` items are read only
  from the app's own `native/` crate (`rux-bindgen` follows `mod` files of that crate), so the `Session` record, its
  signal and the export wrappers are written in each app; and a `use` finds `.rux` modules only inside the project, so
  `stores/session.rux` (and a shared component such as the sign-in form) is copied.
- **What Urbandrop needs (as one feature):**
  1. **Soon.** Exports from a dependency crate: a crate the app's `native/` depends on can carry `#[rux::export]`
     items and `#[rux::init]`, reached as `native::<its module>` (or re-exported with `pub use`).
  2. **Soon.** `.rux` modules and components from a path outside the project, named in `rux.toml`
     (`[dependencies] shared = { path = "../shared" }`, then `use shared::stores::session;`).
- **Urbandrop meanwhile:** the logic is plain Rust in the shared `urbandrop-core` (formatting, address search, the
  API client, with its tests); only the thin exports and the session store are repeated.
- **Status:** open.

## UD-016 Printing and saving a page as PDF

- **Found:** 2026-11-12, client app receipt.
- **Limit:** the web's receipt page has "Print or save as PDF" (`window.print()` with print styles). Rux has neither.
- **What Urbandrop needs (as one feature):**
  1. **Soon.** Save a page or a box as a PDF, then hand it to the share sheet (with UD-005) or save it to a chosen file.
  2. **Later.** The system print dialog.
- **Urbandrop meanwhile:** the receipt is shown without the button.
- **Status:** open.

## Small notes (no entry needed unless they matter)

- (2026-11-12) A `<` inside a string in a template binding, `<text>{{ "<45 min" }}</text>`, is read as the start of a
  tag: `malformed attribute in <45>`. `&lt;45 min` works.
- (2026-11-12) `"\u{2190}"` in a template binding passes `rux check` but fails at run time ("`\u` needs 4 hex digits").
  `rux check` caught the same mistake in another file of the same app, so the check is not consistent.
- (2026-11-12) `rux check` accepts the retired `path_for`; only the run warns ("write `pathFor`").
- (2026-11-12) A page reading `query.code ?? ""` when the query has no `code` warns that there is no `code` on that
  value, so `??` does not cover a missing map key; `"code" in query` does.
- (2026-11-12) A text's trailing space is dropped: `<text>Pickup time </text><text>(optional)</text>` in a row
  reads "Pickup time(optional)". HTML keeps one space there.
- (2026-11-12, Android emulator) Hiding the keyboard with its own hide key after typing in a field turned the whole
  window white for a few seconds before the page came back as it was. The partner app saw the same once.
- (2026-11-12) On the desktop, the mouse wheel over a row that scrolls only sideways (`overflow-x: auto`) does not
  scroll the page around it; a browser passes the vertical wheel on.

- `rux check` resolves `<image src>` in a page differently depending on how it reaches the file: from `app.rux`'s routes it reads `assets/x.png` against the app folder, and for the page "read on its own" against `pages/` (`cannot read image .\pages\assets/urban_logo.png`). The runtime uses the app folder. One of the two messages is always printed. **Fixed 2026-11-12**: a file something uses reads its images from the app's folder in `rux check` too.
- A tab with `box-shadow: inset 0 3px 0 <colour>` seems to show a faint vertical line at its left edge on Windows (seen in the partner app's tab bar, not yet checked closely).
- `<input autocapitalize>` is not an attribute Rux knows. The web form uses `autocapitalize="characters"` for number plates so the phone keyboard opens in capitals; the app uppercases in Rust meanwhile. Worth adding with the other keyboard hints (`inputmode`, `enterkeyhint`).
- `flex-flow` (the shorthand for `flex-direction` and `flex-wrap`) is not a property Rux knows. Tailwind's `--minify` (Lightning CSS) writes it on its own whenever a rule sets both longhands, so a Tailwind app that wraps a row gets the warning and loses the wrap. Supporting the shorthand fixes every minified stylesheet at once. **Fixed 2026-11-12**.
- A bare boolean attribute on a component tag (`<driver-fields strict with_name />`) does not pass `true` to `prop strict = false;`; the prop keeps its default, silently, and `rux check` says nothing. On elements a bare `disabled` means on, as in HTML, so the same reading on component tags (or an error) would be expected. The app writes `:strict="true"` meanwhile.
