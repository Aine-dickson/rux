# Urbandrop: assessment of what is still open

Written 2026-11-24 under the owner's rule (a field report is not a spec: first necessity, feasibility,
security, and a general building block). Companion to `urbandrop-ud-015-assessment.md`. Nothing here is built.
"Verified" below means run against the current tree (`target/debug/rux check`) on 2026-11-24; everything else
is read from code or the reports and is marked so.

## 1. Entries still open

### UD-006 Maps

- **Need:** real but narrower than the entry. The client's "Choose on map" needs a draggable, zoomable map.
  The tracking page and the partner's job screen only need to *show* a route, a pickup and a drop-off.
- **Alternatives that need no new Rux:** (a) hand the person to their maps app (`links.directions`, built);
  (b) a **static map image**: `<image src="https://...staticmap?...">` with the markers and path in the URL,
  which `<image>` already loads. That covers the tracking and job screens. Only "Choose on map" needs gestures.
- **General building blocks** (and who else gains): pinch and rotate gestures (any image viewer, canvas or
  Mava), a bounded on-disk cache for many small images (any remote-image app), clipping and transforms (exist).
  The owner's rule stands: no `<map>` tag. A map is tiles as `<image>` in a transformed box, markers as
  children, polylines as `<path>`. It would live as an app component or a recipe, not a Rux element.
- **Feasibility:** pinch and rotate are an open v0.9 item ("touch reporting carries every finger, nothing turns two
  fingers into a pinch"). The roadmap says it needs a phone, and the emulator is the only test device here (and
  injected Windows touch does not reach winit), so it cannot be driven properly now. Performance of dozens of
  tile images is unmeasured.
- **Security and policy:** an API key in a tile URL ships inside the app and can be lifted, so tiles should come
  through Urbandrop's own server or a keyed proxy. OpenStreetMap's public tile servers forbid heavy app use.
  Attribution text is a licence term, not a nicety.
- **Recommendation:** do not build a map. Step 1, Urbandrop: static-map images for display (no Rux work).
  Step 2, Rux: pinch and rotate, when a phone exists, since other apps want them. Step 3, only then: the disk
  image cache, and a tile component in Urbandrop's own app. Park until step 2.

### UD-016 Printing and saving a page as PDF

- **Need:** a receipt a person can keep. That is a document, not a screen.
- **Feasibility:** Rux paints with vello on the GPU and the lockfile has no PDF library (only `image` and `png`).
  A PDF would be a second renderer for every element, text and font: large, and it would never match the screen.
  Whether a box can be read back to an image offscreen was not checked.
- **Alternative that needs no new Rux:** the server renders the receipt as a PDF and the app opens it
  (`links.open("https://.../receipt.pdf")`, built). The browser or PDF viewer then saves, shares and prints it.
  The server already holds the data, a server PDF can be the official record, and one route serves the web and
  both apps.
- **General block, if ever:** "save a box as an image" (a screenshot of an element, shareable). Not PDF, and it
  needs file sharing through Android's `FileProvider`, which `links.share` (text and URLs) does not do.
- **Recommendation:** do not build. Urbandrop adds the server route. Drop the print dialog ("Later").

### UD-012, the Android case

- The desktop case is fixed and tested; the Android one ("a component's `async fn` awaiting a native call that sets
  a native signal") has never been re-driven on the emulator.
- **Recommendation:** verify, do not build. Rebuild the client app with the current CLI and sign in once on the
  emulator. About 15 minutes plus the build. Not done here (needs Urbandrop's API server running).

## 2. The small notes, checked today

| Note | State now | Assessment |
|---|---|---|
| `<` inside a string in a binding (`{{ "<45 min" }}`) | **Reproduces** (verified): `malformed attribute in <45>` | A real bug. HTML and Vue read `<` followed by a digit as text, and `{{ }}` contents should not be scanned for tags at all. Fix in the template parser; accepts more, changes no syntax. **Build.** |
| `"\u{2190}"` passes `rux check`, fails at run | **Check now reports it in both places** (verified, template and script): the inconsistency is gone | Left: `\u{...}` is unsupported (4 hex digits only). JavaScript and Rust both accept `\u{2190}`, so by "keep syntax familiar" supporting it is right, but it is a language change: **owner's call**. Small (lexer). |
| `rux check` accepts retired `path_for` | **Fixed** (verified: it errors with "write `pathFor`") | Close the note. |
| `query.code ?? ""` warns | Not verified (needs a routed page) | Likely by design: strict map reads raise on an absent key, and `?.` or `"code" in query` is how a program says it knows. If so only the message needs to name `in`. **Check the message, no behaviour change.** |
| A text's trailing space is dropped in a row | Not run | Probably correct CSS: `<text>` children of a flex row are blockified, and a block's trailing space collapses in a browser too. Rux follows CSS unless a divergence is written down. Ask which markup the web page used (spans?). **No change unless it is a real divergence.** |
| White window for seconds after hiding the keyboard (Android) | Not reproduced | Plausibly a surface recreate on the IME hide. It could hit every Android app, so worth a drive on the emulator before anything is decided. **Investigate.** |
| Desktop wheel over an x-only scroller does not scroll the page | Not run | The code hands leftover scroll outward (`scroll_chain_from`), but the wheel latch keeps a run on the first scroller. Browsers pass it on. Cheap desktop drive. **Verify, then fix if real.** |
| Faint line at a tab's `inset` box-shadow edge on Windows | Not run | Unchecked by the reporter too. **Look once**, low priority. |

## 3. Pieces of built entries that were left out

| Piece | Need | Recommendation |
|---|---|---|
| UD-002.5 location services switched off | Real: a rider with location off tracks nothing, silently | General and small. A `location` read of whether it is enabled, and a way to open the system location settings. New API surface, so the owner approves the spelling first. **Build, small.** |
| UD-002.6 battery-optimisation exemption | Real for background work on some vendors | Play policy limits who may *request* the exemption, and it cannot be validated without a phone. Offer only "open the battery settings" through the existing settings opener. **Park the request; document.** |
| UD-003.5 connectivity (online/offline, change event) | Real, and every networked app wants it | General: a `network.online` signal beside `app.state`. Android `ConnectivityManager`, web `navigator.onLine`. Honest limit: "online" means a link, not that the server answers. **Build, small.** |
| UD-003.6 push (FCM) | Eventually | Needs Google services, a server and a trust model. Local notifications are shaped to take it. **Park until the owner asks.** |
| UD-003.7 launcher badge | Doubtful | Android has no standard badge API (launchers differ). **Drop.** |
| UD-004.3 / .4 downscale, release the cache copy | Real | The app's Rust does both now it has the bytes (an `image` crate). **Nothing to build; a recipe.** |
| UD-004.2 camera photo after Android killed the app | Unknown | Drive once on the emulator (kill the process while the camera is open). **Verify.** |
| UD-007 on macOS, Linux, the web | None from Urbandrop | Throws `unsupported`. **Park.** |
| UD-013 `<option>` elements and `r-for` over options, `step` and seconds | Low | **Park.** |
| UD-014 `el.scrollTop = n` | Low: `scrollTo` covers it | Familiar DOM spelling, so add only if asked. **Park.** |

## 3b. Done and checked since (2026-11-24, same day)

- **Built:** `\u{...}` escapes; the `<` in a binding fix; `location.enabled()`;
  `permissions.openLocationSettings()` and `openBatterySettings()` (two calls, not one with an optional page:
  the checker has no optional arguments); `network.online` and `rux::network`; **pinch and rotate** as
  `@gesturestart`, `@gesturechange` and `@gestureend`. All in `docs/07-script.md`, driven on the emulator
  (rows in `08-user-tests`).
- **Checked and fixed:** the faint line down a tab's edge with `box-shadow: inset 0 3px 0` on Windows was real.
  A blurred cut with a blur of 0 put half its strength on its edge, so where the cut met the box's edge, half
  the shadow stayed. A hard-edged cut that reaches past the box fixes it (`rux-paint`, `InsetShadow`);
  seen before and after in a window at 150%.
- **Checked, not a defect:** the desktop wheel over a sideways scroller does pass to the page, with and without
  `scroll-snap` (driven in a window). `query.code ?? ""` fails when the key is absent by design (strict map
  reads), and the message already says to write `"code" in query`. The keyboard's white flash did not reproduce
  (the Back key and the keyboard's own hide key, five frames each, on the emulator).
- **Still open:** UD-012's Android shape and a camera photo after Android kills the app. The photo's `content://`
  URI is kept in the saved state and is read by the path driven for UD-004, but the camera itself was not driven.
  The trailing space in a row of `<text>` stays "probably correct CSS" until the web page's markup is known.

## 4. Suggested order

1. **Fix** the `<` in bindings bug (small, no decision needed).
2. **Verify** on the emulator: the keyboard white flash, the camera photo after a kill, UD-012's Android case.
3. **Verify on the desktop:** the wheel over an x-only scroller, the `query` message, the inset shadow line.
4. **Decide** (owner): `\u{...}` escapes; the shape of `location` enabled and `network.online`.
5. **Build** the two small additions once the spellings are agreed.
6. **Urbandrop-side:** static-map images, a server receipt PDF, session logic into `urbandrop-core`.
7. **Park:** maps component (waits on pinch and a phone), FCM, badge, battery request, UD-015's other halves.

The status lines in `urbandrop.md` still say "open" for entries that are built; that file holds another
session's uncommitted edits, so it is for the owner to update (this assessment lists what each should say).
