//! Turning one drawing into the resource tree Android wants for a launcher icon.
//!
//! **This is the whole of Rux's resource story, and it is deliberately small.**
//! A `res/` directory is where Gradle normally enters the picture: resource
//! merging, generated `R` classes, library manifests. None of that is here.
//! `aapt2 compile --dir` takes a tree in one invocation and `aapt2 link` takes
//! what it produced, and both tools are already in the pipeline for the
//! manifest, so an icon costs the build one extra call and no new dependency.
//!
//! What the author supplies is one PNG and one colour ([`crate::manifest::Icon`]
//! says why two rather than one). What Android wants is five copies of that PNG
//! at five densities, an XML file naming the two layers, and a colour resource.
//! Generating them is not a convenience: hand-maintaining five rescales of the
//! same art is how an icon ends up subtly different on one class of device.

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

/// What `<application android:icon>` is set to when an app has one.
pub fn manifest_reference() -> String {
    format!("@mipmap/{NAME}")
}

/// Write the `res/` tree for `foreground` on `background` under `staging`.
///
/// Returns the directory to hand to `aapt2 compile --dir`.
pub fn generate(foreground: &Path, background: &str, staging: &Path) -> Result<PathBuf, String> {
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
</resources>
"#
        ),
    )?;

    Ok(res)
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
        let res = generate(&source(&dir, 512), "#7c3aed", &dir).expect("generating");
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
        let res = generate(&source(&dir, 512), "#7c3aed", &dir).expect("generating");
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
        let res = generate(&source(&dir, 512), "#7c3aed", &dir).expect("generating");
        let reference = manifest_reference();
        let name = reference.strip_prefix("@mipmap/").expect("a mipmap reference");
        assert!(res.join("mipmap-anydpi-v26").join(format!("{name}.xml")).is_file());
    }

    #[test]
    fn a_rectangular_icon_is_refused_and_says_what_it_measured() {
        let dir = staging("a_rectangular_icon_is_re");
        let path = dir.join("wide.png");
        image::RgbaImage::from_pixel(512, 256, image::Rgba([0, 0, 0, 255]))
            .save(&path)
            .expect("writing the test icon");
        let error = generate(&path, "#7c3aed", &dir).expect_err("a rectangle is not an icon");
        assert!(error.contains("512 by 256"), "{error}");
    }

    #[test]
    fn a_previous_icon_does_not_survive_into_the_next_build() {
        let dir = staging("a_previous_icon_does_not");
        let res = generate(&source(&dir, 512), "#7c3aed", &dir).expect("generating");
        // Something the generator would never write, standing in for a density
        // that used to be produced or a name that used to be used.
        let stale = res.join("mipmap-mdpi").join("ic_launcher_old.png");
        std::fs::write(&stale, "stale").expect("writing the stale file");
        generate(&source(&dir, 512), "#7c3aed", &dir).expect("regenerating");
        assert!(!stale.is_file(), "the previous tree was packed into the next build");
    }
}
