//! Turning one drawing into the resource tree Android wants: the launcher icon,
//! and the splash screen, which on Android is that same icon on a plate.
//!
//! **This is the whole of Rux's resource story, and it is deliberately small.**
//! A `res/` directory is where Gradle normally enters the picture: resource
//! merging, generated `R` classes, library manifests. None of that is here.
//! `aapt2 compile --dir` takes a tree in one invocation and `aapt2 link` takes
//! what it produced, and both tools are already in the pipeline for the
//! manifest, so all of this costs the build one extra call and no new
//! dependency.
//!
//! What the author supplies is one PNG and one colour ([`crate::manifest::Icon`]
//! says why two rather than one). What Android wants is five copies of that PNG
//! at five densities, an XML file naming the two layers, and a colour resource.
//! Generating them is not a convenience: hand-maintaining five rescales of the
//! same art is how an icon ends up subtly different on one class of device.
//!
//! **The splash screen needs no artwork of its own, and asking for some would
//! be fighting the platform.** Since Android 12 the system draws the splash
//! itself, from the launcher icon and a background colour, and an app does not
//! get to put a picture there. So the only thing to supply is the colour, and
//! even that defaults to the icon's own plate.

use std::path::{Path, PathBuf};

use image::imageops::FilterType;

/// The densities an adaptive icon's foreground is rasterized for, with the
/// pixel size of its 108dp canvas at each.
///
/// **108dp is the canvas and 72dp is the safe zone**, which is the platform's
/// rule: a launcher masks the outside and may parallax what survives. The sizes
/// below are 108dp at each bucket's scale (1x, 1.5x, 2x, 3x, 4x), so they are
/// that rule expressed in pixels and not numbers anyone should round.
const DENSITIES: [(&str, u32); 5] =
    [("mdpi", 108), ("hdpi", 162), ("xhdpi", 216), ("xxhdpi", 324), ("xxxhdpi", 432)];

/// The resource name every generated file hangs off.
///
/// Referred to from the manifest as `@mipmap/ic_launcher`. `ic_launcher` is the
/// platform's own convention rather than ours, and an icon under a different
/// name works exactly as well, so the convention is kept: someone reading an
/// unpacked Rux APK beside any other app should find what they expect.
const NAME: &str = "ic_launcher";

/// The generated theme, which exists only to carry the splash screen.
const THEME: &str = "RuxSplash";

/// The API the system splash screen arrived in.
///
/// Below it there is no splash to configure and the window background is all an
/// app gets, which is why the theme is written twice: see [`write_theme`].
const SPLASH_API: u32 = 31;

/// What `<application android:icon>` is set to when an app has one.
pub fn manifest_reference() -> String {
    format!("@mipmap/{NAME}")
}

/// What `<activity android:theme>` is set to when an app has one.
pub fn theme_reference() -> String {
    format!("@style/{THEME}")
}

/// Write the `res/` tree for `foreground` on `background` under `staging`.
///
/// `splash` is the colour behind the splash screen, which the caller has
/// already defaulted to the icon's own background.
///
/// Returns the directory to hand to `aapt2 compile --dir`.
pub fn generate(
    foreground: &Path,
    background: &str,
    splash: &str,
    staging: &Path,
) -> Result<PathBuf, String> {
    let res = staging.join("res");
    // Removed rather than written over, for the reason the library staging in
    // `apk::pack` is: an icon renamed or dropped from the manifest would
    // otherwise stay in the tree and keep being packed, and an app showing the
    // icon it had two builds ago is a bug nobody thinks to look for in a
    // packager.
    let _ = std::fs::remove_dir_all(&res);

    let source = image::open(foreground).map_err(|e| {
        format!(
            "reading the icon {}: {e}\n\nIt needs to be an image file; PNG with transparency is \
             what an icon usually is.",
            foreground.display()
        )
    })?;

    // Square, because a 108dp canvas is square and a launcher will not letterbox
    // it for us: a non-square image would be stretched into the canvas and come
    // out distorted. Refused rather than padded, because padding silently
    // decides where the art sits, and only the author knows that.
    let (width, height) = (source.width(), source.height());
    if width != height {
        return Err(format!(
            "the icon {} is {width} by {height}, and an Android icon is square.\n\nIt is drawn on \
             a 108dp canvas of which only the middle 72dp is certain to be visible, so the art \
             wants room around it rather than a crop.",
            foreground.display()
        ));
    }
    // The largest bucket, so anything smaller is only ever downscaled. Upscaling
    // is not refused: a small icon is a bad icon and not a broken build, and
    // saying so once beats failing a build someone is in the middle of.
    let largest = DENSITIES.iter().map(|(_, size)| *size).max().unwrap_or_default();
    if width < largest {
        println!(
            "rux: the icon is {width}px and the largest density wants {largest}px, so it will be \
             enlarged and look soft on a high-density screen"
        );
    }

    for (density, size) in DENSITIES {
        let dir = res.join(format!("mipmap-{density}"));
        std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        // Lanczos3 rather than the default: this is one image, resized five
        // times, at build time. The quality is free here and it is the
        // difference between a crisp glyph and a blurred one at mdpi.
        let scaled = source.resize_exact(size, size, FilterType::Lanczos3);
        let file = dir.join(format!("{NAME}_foreground.png"));
        scaled.save(&file).map_err(|e| format!("writing {}: {e}", file.display()))?;
    }

    // `anydpi-v26` and not `anydpi`: the XML is an `<adaptive-icon>`, which is
    // an API 26 element. The qualifier is what stops an older platform reading
    // a file it cannot parse, and while `MIN_API` is already 26 the qualifier
    // is the platform's spelling for this and costs nothing to keep.
    let anydpi = res.join("mipmap-anydpi-v26");
    std::fs::create_dir_all(&anydpi).map_err(|e| format!("creating {}: {e}", anydpi.display()))?;
    write(
        &anydpi.join(format!("{NAME}.xml")),
        &format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<!-- Generated by `rux build`. Edits are overwritten on the next build. -->
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@color/{NAME}_background" />
    <foreground android:drawable="@mipmap/{NAME}_foreground" />
</adaptive-icon>
"#
        ),
    )?;

    let values = res.join("values");
    std::fs::create_dir_all(&values).map_err(|e| format!("creating {}: {e}", values.display()))?;
    write(
        &values.join(format!("{NAME}.xml")),
        &format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<!-- Generated by `rux build`. Edits are overwritten on the next build. -->
<resources>
    <color name="{NAME}_background">{background}</color>
    <color name="{NAME}_splash">{splash}</color>
</resources>
"#
        ),
    )?;

    write_theme(&res)?;

    Ok(res)
}

/// The theme, written twice: once for every Android, once for API 31 and up.
///
/// **A splash screen is a theme, which is why this exists at all.** There is no
/// manifest attribute for one and no API to call: an app says what its splash
/// looks like by styling the window the system shows before the app has drawn
/// anything.
///
/// Two files because the attributes that configure it did not exist before API
/// 31, and Rux's floor is 26. Below 31 there is no system splash, and the only
/// thing on screen while an app starts is its window background, so the base
/// theme sets that to the same colour: the result is a plain coloured screen
/// rather than a branded one, which is the most those releases can do. A
/// qualified `values-v31` directory is how a resource says "only on this
/// release and up", and the platform picks between them.
///
/// **The style is repeated rather than extended in the v31 copy.** A resource
/// in a qualified directory replaces the unqualified one outright rather than
/// merging with it, so a v31 style that listed only the splash attributes would
/// silently drop the window background it was meant to add to.
fn write_theme(res: &Path) -> Result<(), String> {
    // `NoActionBar`, and this is not cosmetic: the plain `DeviceDefault` theme
    // has an action bar, and an app that has never asked for one would grow a
    // grey title strip above its own first row the moment a theme was declared.
    // The activity was themeless until now, so nothing was inherited before.
    let parent = "@android:style/Theme.DeviceDefault.NoActionBar";
    let header = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
                  <!-- Generated by `rux build`. Edits are overwritten on the next build. -->";

    let base = res.join("values");
    write(
        &base.join("themes.xml"),
        &format!(
            r#"{header}
<resources>
    <style name="{THEME}" parent="{parent}">
        <item name="android:windowBackground">@color/{NAME}_splash</item>
    </style>
</resources>
"#
        ),
    )?;

    let modern = res.join(format!("values-v{SPLASH_API}"));
    std::fs::create_dir_all(&modern).map_err(|e| format!("creating {}: {e}", modern.display()))?;
    write(
        &modern.join("themes.xml"),
        &format!(
            r#"{header}
<resources>
    <style name="{THEME}" parent="{parent}">
        <item name="android:windowBackground">@color/{NAME}_splash</item>
        <item name="android:windowSplashScreenBackground">@color/{NAME}_splash</item>
        <item name="android:windowSplashScreenAnimatedIcon">@mipmap/{NAME}</item>
    </style>
</resources>
"#
        ),
    )
}

fn write(path: &Path, contents: &str) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|e| format!("writing {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square PNG of `size`, which is all any of these tests needs to be.
    fn source(dir: &Path, size: u32) -> PathBuf {
        let path = dir.join("icon.png");
        image::RgbaImage::from_pixel(size, size, image::Rgba([124, 58, 237, 255]))
            .save(&path)
            .expect("writing the test icon");
        path
    }

    /// A staging directory of this test's own.
    ///
    /// **Named per test, not per process.** Sharing one directory across the
    /// module failed exactly as the product is supposed to: `generate` clears
    /// the tree it is about to write, so two tests running in parallel deleted
    /// each other's output and reported a missing density.
    fn staging(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rux-icon-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("creating the staging directory");
        dir
    }

    #[test]
    fn every_density_is_written_at_the_size_that_density_means() {
        let dir = staging("every_density_is_written");
        let res = generate(&source(&dir, 512), "#7c3aed", "#7c3aed", &dir).expect("generating");
        for (density, size) in DENSITIES {
            let file = res.join(format!("mipmap-{density}")).join("ic_launcher_foreground.png");
            let written = image::open(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            assert_eq!(
                (written.width(), written.height()),
                (size, size),
                "{density} is not 108dp at its own scale"
            );
        }
    }

    #[test]
    fn the_two_layers_name_the_resources_that_were_written() {
        let dir = staging("the_two_layers_name_the_");
        let res = generate(&source(&dir, 512), "#7c3aed", "#7c3aed", &dir).expect("generating");
        let xml = std::fs::read_to_string(res.join("mipmap-anydpi-v26").join("ic_launcher.xml"))
            .expect("reading the adaptive icon");
        // The names in the XML and the files on disk are written in different
        // places and drifted once already in the Java shim, which is why this
        // checks them against each other rather than against a literal.
        assert!(xml.contains(&format!("@mipmap/{NAME}_foreground")), "{xml}");
        assert!(xml.contains(&format!("@color/{NAME}_background")), "{xml}");
        assert!(res.join("mipmap-mdpi").join(format!("{NAME}_foreground.png")).is_file());
        let colors = std::fs::read_to_string(res.join("values").join("ic_launcher.xml"))
            .expect("reading the colour");
        assert!(colors.contains(&format!("name=\"{NAME}_background\"")), "{colors}");
        assert!(colors.contains("#7c3aed"), "{colors}");
    }

    #[test]
    fn the_manifest_reference_is_the_resource_that_was_written() {
        let dir = staging("the_manifest_reference_i");
        let res = generate(&source(&dir, 512), "#7c3aed", "#7c3aed", &dir).expect("generating");
        let reference = manifest_reference();
        let name = reference.strip_prefix("@mipmap/").expect("a mipmap reference");
        assert!(res.join("mipmap-anydpi-v26").join(format!("{name}.xml")).is_file());
    }

    #[test]
    fn the_theme_is_written_twice_and_the_qualified_one_carries_the_splash() {
        let dir = staging("the_theme_is_written_twi");
        let res = generate(&source(&dir, 512), "#7c3aed", "#101018", &dir).expect("generating");

        let base = std::fs::read_to_string(res.join("values").join("themes.xml"))
            .expect("reading the base theme");
        // Below API 31 there is no splash to configure, so the base theme must
        // not name the attributes that did not exist yet.
        assert!(!base.contains("windowSplashScreen"), "{base}");
        assert!(base.contains("android:windowBackground"), "{base}");

        let modern =
            std::fs::read_to_string(res.join(format!("values-v{SPLASH_API}")).join("themes.xml"))
                .expect("reading the v31 theme");
        assert!(modern.contains("windowSplashScreenBackground"), "{modern}");
        assert!(modern.contains(&format!("@mipmap/{NAME}")), "{modern}");
        // A qualified resource replaces the unqualified one outright rather
        // than merging, so anything the base sets has to be repeated here or it
        // is silently lost on exactly the devices that have a splash.
        assert!(modern.contains("android:windowBackground"), "{modern}");
    }

    #[test]
    fn both_themes_declare_the_style_the_manifest_asks_for() {
        let dir = staging("both_themes_declare_the_");
        let res = generate(&source(&dir, 512), "#7c3aed", "#101018", &dir).expect("generating");
        let reference = theme_reference();
        let name = reference.strip_prefix("@style/").expect("a style reference");
        for values in ["values".to_string(), format!("values-v{SPLASH_API}")] {
            let xml = std::fs::read_to_string(res.join(values).join("themes.xml"))
                .expect("reading a theme");
            assert!(xml.contains(&format!("name=\"{name}\"")), "{xml}");
        }
    }

    #[test]
    fn the_splash_colour_is_its_own_and_not_the_icons() {
        let dir = staging("the_splash_colour_is_its");
        let res = generate(&source(&dir, 512), "#7c3aed", "#101018", &dir).expect("generating");
        let colors = std::fs::read_to_string(res.join("values").join("ic_launcher.xml"))
            .expect("reading the colours");
        assert!(colors.contains("#7c3aed"), "the icon plate is missing: {colors}");
        assert!(colors.contains("#101018"), "the splash colour is missing: {colors}");
    }

    #[test]
    fn a_rectangular_icon_is_refused_and_says_what_it_measured() {
        let dir = staging("a_rectangular_icon_is_re");
        let path = dir.join("wide.png");
        image::RgbaImage::from_pixel(512, 256, image::Rgba([0, 0, 0, 255]))
            .save(&path)
            .expect("writing the test icon");
        let error = generate(&path, "#7c3aed", "#7c3aed", &dir).expect_err("a rectangle is not an icon");
        assert!(error.contains("512 by 256"), "{error}");
    }

    #[test]
    fn a_previous_icon_does_not_survive_into_the_next_build() {
        let dir = staging("a_previous_icon_does_not");
        let res = generate(&source(&dir, 512), "#7c3aed", "#7c3aed", &dir).expect("generating");
        // Something the generator would never write, standing in for a density
        // that used to be produced or a name that used to be used.
        let stale = res.join("mipmap-mdpi").join("ic_launcher_old.png");
        std::fs::write(&stale, "stale").expect("writing the stale file");
        generate(&source(&dir, 512), "#7c3aed", "#7c3aed", &dir).expect("regenerating");
        assert!(!stale.is_file(), "the previous tree was packed into the next build");
    }
}
