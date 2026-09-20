package dev.ruxlang.shell;

import android.app.NativeActivity;
import android.content.pm.ActivityInfo;
import android.content.pm.PackageManager;
import android.graphics.Insets;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.WindowInsets;

/**
 * The one Java class in a Rux app.
 *
 * <p>Rux builds an APK with four command-line tools and no Gradle, and this file
 * is the reason that is still true while the app has Java in it at all. It is
 * compiled by {@code javac} and dexed by {@code d8}, both of which an Android
 * build already needs for other reasons, and it depends on nothing but the
 * platform. No AndroidX, no AAR, no resources, so no resource merging, which is
 * the step that turns a build back into Gradle.
 *
 * <p><b>Why there is Java here at all.</b> A plain {@code NativeActivity} cannot
 * answer two questions that native code has no other way to ask. Both are
 * virtual methods on the activity, and JNI can call into Java but cannot
 * subclass it, so the override point has to exist in a real class:
 *
 * <ul>
 *   <li>{@code onApplyWindowInsets}, which is where a phone says how much of the
 *       display belongs to the status bar, the gesture bar and the camera
 *       cutout. Without it a Rux app draws its header underneath the clock.
 *   <li>{@code onCreateInputConnection}, which is what an input method attaches
 *       to. Without it a soft keyboard falls back to sending plain key events:
 *       enough for typing Latin text, not enough for composing Chinese,
 *       Japanese or Korean, and not enough for autocorrect or swipe typing.
 * </ul>
 *
 * <p>Only the first is implemented here. The second is the next piece of work,
 * and it lands in this same class.
 */
public class RuxActivity extends NativeActivity {

    /**
     * Hand the safe-area insets to the Rux shell, in physical pixels.
     *
     * <p>Resolved by the JVM against the already-loaded native library, so the
     * Rust side exports {@code Java_dev_ruxlang_shell_RuxActivity_nativeSafeArea}
     * and nothing has to call {@code RegisterNatives}. The library is loaded by
     * {@code NativeActivity.onCreate}, which runs before any of this can fire.
     */
    private static native void nativeSafeArea(int top, int right, int bottom, int left);

    @Override
    protected void onCreate(Bundle state) {
        loadNativeLibrary();
        super.onCreate(state);
        final View root = getWindow().getDecorView();
        root.setOnApplyWindowInsetsListener(
                (view, insets) -> {
                    report(insets);
                    // Passed on rather than consumed. Rux draws its own edges,
                    // but swallowing the insets here would leave anything else
                    // in the window unable to see them.
                    return view.onApplyWindowInsets(insets);
                });
    }

    /**
     * Load the Rux shared object under this class's own class loader.
     *
     * <p><b>This is not redundant, and leaving it out fails in a way that looks
     * like a missing symbol.</b> {@code NativeActivity} does load the library,
     * but it does so through its own private {@code loadNativeCode}, which
     * {@code dlopen}s the file directly. That is enough to find
     * {@code ANativeActivity_onCreate} and start the app, and it is not enough
     * for JNI: the runtime resolves a {@code native} method against libraries
     * registered for the class loader of the class that declares it, and a
     * {@code dlopen} registers with nobody.
     *
     * <p>So without this, {@link #nativeSafeArea} throws
     * {@code UnsatisfiedLinkError: No implementation found}, while
     * {@code llvm-nm} shows the symbol present and exported in the very
     * {@code .so} that is loaded. Calling {@code System.loadLibrary} here is
     * what registers it. The library ends up opened twice, which costs nothing:
     * {@code dlopen} of an already-open library returns the same handle.
     *
     * <p>It runs before {@code super.onCreate}, so the library is registered
     * before anything can call into it.
     */
    private void loadNativeLibrary() {
        try {
            ActivityInfo info =
                    getPackageManager()
                            .getActivityInfo(getComponentName(), PackageManager.GET_META_DATA);
            String name = info.metaData == null ? null : info.metaData.getString("android.app.lib_name");
            if (name != null) {
                System.loadLibrary(name);
            }
        } catch (PackageManager.NameNotFoundException e) {
            // Nothing useful to do, and nothing lost: super.onCreate is about to
            // look for the same library and fail with a clearer message.
        }
    }

    /**
     * Read the insets in whichever way this Android understands.
     *
     * <p>Rux's floor is API 26 and {@code WindowInsets.getInsets(int)} arrived in
     * API 30, so there are two paths and the old one is not optional. Compiling
     * against a recent {@code android.jar} hides this completely: the call
     * compiles and then throws {@code NoSuchMethodError} on the older phone that
     * the manifest says is supported.
     */
    private void report(WindowInsets insets) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            Insets bars =
                    insets.getInsets(
                            WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
            nativeSafeArea(bars.top, bars.right, bars.bottom, bars.left);
        } else {
            // API 26 to 29. Deprecated in 30 and correct before it. A display
            // cutout is API 28 and up, and on 26 and 27 there were no cutouts to
            // report, so the system window insets are the whole answer.
            nativeSafeArea(
                    insets.getSystemWindowInsetTop(),
                    insets.getSystemWindowInsetRight(),
                    insets.getSystemWindowInsetBottom(),
                    insets.getSystemWindowInsetLeft());
        }
    }
}
