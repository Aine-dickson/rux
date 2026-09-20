package dev.ruxlang.shell;

import android.app.NativeActivity;
import android.content.Context;
import android.content.pm.ActivityInfo;
import android.content.pm.PackageManager;
import android.graphics.Insets;
import android.os.Build;
import android.os.Bundle;
import android.text.Editable;
import android.text.Selection;
import android.text.SpannableStringBuilder;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

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
 * <p><b>The input connection is not an override on this class.</b> It is an
 * override on a {@link View}, and {@code NativeActivity} owns its content view
 * and does not hand it over. So the second answer comes from {@link RuxInputView}
 * below: a one-pixel focusable view added above the native surface, which exists
 * only to be the thing an input method attaches to.
 */
public class RuxActivity extends NativeActivity {

    /** The view an input method talks to. See {@link RuxInputView}. */
    private RuxInputView input;

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

        // Above the native surface, and holding focus from here on. An input
        // method attaches to the focused view, and the view Rux draws into is
        // not one it can attach to.
        input = new RuxInputView(this);
        addContentView(
                input,
                new ViewGroup.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT,
                        ViewGroup.LayoutParams.MATCH_PARENT));
        input.requestFocus();

        // Hand the activity to the Rust side, which has no other way to get it.
        // `ndk_context`, which `android-activity` fills in, holds the
        // Application object rather than this, and an activity method called on
        // that fails with `NoSuchMethodError`.
        nativeActivityCreated(this);
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

    /**
     * The view an input method attaches to.
     *
     * <p>Rux draws into the surface {@code NativeActivity} owns, and that view is
     * private to it and is not a text editor. An input method never talks to an
     * activity: it asks the <em>focused view</em> for an
     * {@link InputConnection}. So this exists to be that view, and does nothing
     * else at all.
     *
     * <p><b>It fills the window, and that is not cosmetic.</b> It was one pixel
     * first, on the reasoning that a view which only receives typing needs no
     * area. An input method declines to open for an editor that small:
     * {@code showSoftInput} returns {@code true}, meaning the request was
     * delivered, and no keyboard ever appears. That failure reads as success in
     * every log there is. At full size the same code opens the keyboard every
     * time.
     *
     * <p>It never consumes a touch. A plain view with no click listener returns
     * false from {@code onTouchEvent}, so events pass to the surface below and
     * the tap handling Rux already had is untouched. Driven rather than assumed,
     * because this view now covers the whole app: tapping a field, a button and
     * a tab all still reach Rux, and a task added through it appears in the
     * list.
     */
    private static final class RuxInputView extends View {

        RuxInputView(Context context) {
            super(context);
            setFocusable(true);
            setFocusableInTouchMode(true);
        }

        /**
         * Only while Rux has a text field focused.
         *
         * <p>This view holds focus for the whole life of the app, because taking
         * it on demand would need a call from native code into Java and this
         * answer does not. But Android reads "the focused view is a text editor"
         * as a reason to raise the keyboard, so answering an unconditional yes
         * puts the keyboard up before anything has been tapped and leaves it
         * there. Verified rather than assumed: force-stopping the app with the
         * keyboard down and relaunching brought it straight back up.
         *
         * <p>So the honest answer is the one Rux would give, and it is asked
         * fresh every time an input method starts.
         */
        @Override
        public boolean onCheckIsTextEditor() {
            return nativeWantsText();
        }

        @Override
        public InputConnection onCreateInputConnection(EditorInfo out) {
            // Nothing focused means there is nothing to type into, and returning
            // null is how a view says so.
            if (!nativeWantsText()) {
                return null;
            }
            // A plain single-line text field, and no full-screen editor. A phone
            // in landscape will otherwise replace the whole app with the IME's
            // own editor, which would cover the Rux document being edited.
            out.inputType = EditorInfo.TYPE_CLASS_TEXT;
            out.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN | EditorInfo.IME_ACTION_DONE;
            String current = nativeFocusedText();
            if (current == null) {
                current = "";
            }
            out.initialSelStart = current.length();
            out.initialSelEnd = current.length();
            return new RuxInputConnection(this, current);
        }

        /**
         * Let every key that is not text carry on to the native side.
         *
         * <p>Holding focus means this view is offered the hardware keys first,
         * including Back. Returning false leaves them to the dispatch they took
         * before this view existed, so the back button, the volume keys and a
         * physical keyboard all behave as they did.
         */
        @Override
        public boolean onKeyDown(int code, KeyEvent event) {
            return false;
        }

        @Override
        public boolean onKeyUp(int code, KeyEvent event) {
            return false;
        }
    }

    /**
     * Where an input method does its editing, and the one copy that is authoritative
     * while it does.
     *
     * <p>The IME composes into an {@link Editable} here, not into the Rux
     * document, and every change is reported whole: the full text, the selection
     * and the composing region. Rux replaces the field's value with it rather
     * than applying keystrokes.
     *
     * <p><b>This is deliberately the same shape as the web shell's.</b> In a
     * browser the soft keyboard edits a hidden {@code <input>} and Rux hears the
     * new contents as {@code RuxEvent::WebText}, for exactly this reason: on a
     * phone, text does not arrive as key presses, it arrives as the result of an
     * edit that something else performed. Reusing that shape means Android and
     * the web reach the same code with the same four values.
     */
    private static final class RuxInputConnection extends BaseInputConnection {

        private final Editable editable;

        RuxInputConnection(View target, String initial) {
            // `true`: this is a full editor, so the base class maintains the
            // composing spans and the selection on the editable below, which is
            // the part of an input method's protocol worth not reimplementing.
            super(target, true);
            editable = new SpannableStringBuilder(initial);
            Selection.setSelection(editable, initial.length());
        }

        @Override
        public Editable getEditable() {
            return editable;
        }

        // Every mutating call goes through the base class, which maintains the
        // editable, and then reports the whole state. They are listed one by one
        // rather than funnelled, because there is no single hook: the base class
        // has no "something changed" callback.

        @Override
        public boolean commitText(CharSequence text, int position) {
            boolean handled = super.commitText(text, position);
            report();
            return handled;
        }

        @Override
        public boolean setComposingText(CharSequence text, int position) {
            boolean handled = super.setComposingText(text, position);
            report();
            return handled;
        }

        @Override
        public boolean finishComposingText() {
            boolean handled = super.finishComposingText();
            report();
            return handled;
        }

        @Override
        public boolean deleteSurroundingText(int before, int after) {
            boolean handled = super.deleteSurroundingText(before, after);
            report();
            return handled;
        }

        @Override
        public boolean setSelection(int start, int end) {
            boolean handled = super.setSelection(start, end);
            report();
            return handled;
        }

        @Override
        public boolean sendKeyEvent(KeyEvent event) {
            boolean handled = super.sendKeyEvent(event);
            report();
            return handled;
        }

        /**
         * Hand the whole editing state to Rux.
         *
         * <p>Offsets are in Java {@code char}s, which are UTF-16 code units, and
         * Rust counts bytes in UTF-8. Converting is the Rust side's job, because
         * it is the side that knows what it is indexing into; sending anything
         * other than the platform's own offsets here would mean two conversions
         * and two chances to disagree.
         */
        private void report() {
            int start = getComposingSpanStart(editable);
            int end = getComposingSpanEnd(editable);
            nativeTextChanged(
                    editable.toString(),
                    Selection.getSelectionEnd(editable),
                    Selection.getSelectionStart(editable),
                    start,
                    end);
        }
    }

    /**
     * Make the input method ask again what the focused view is.
     *
     * <p>Called from Rust whenever Rux focuses or blurs a text field. An input
     * method asks {@link RuxInputView#onCheckIsTextEditor} once and caches the
     * answer for as long as that view holds focus, and this view holds it for
     * the whole life of the app, so without this the first answer is the only
     * one and the keyboard never opens.
     *
     * <p>Posted to the UI thread, because it arrives on whichever thread the Rux
     * event loop is running on and every one of these calls must be made on the
     * thread that owns the view.
     */
    public void ruxSetTextInput(final boolean on) {
        runOnUiThread(
                () -> {
                    if (input == null) {
                        return;
                    }
                    InputMethodManager imm =
                            (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
                    if (imm == null) {
                        return;
                    }
                    if (on) {
                        input.requestFocus();
                        // Throws away what the input method cached about this
                        // view, so it asks `onCheckIsTextEditor` again and gets
                        // the new answer.
                        imm.restartInput(input);
                        // **Shown against this view, not the decor view.**
                        // `InputMethodManager` only raises the keyboard for the
                        // view it is currently serving, and since this view took
                        // focus that is no longer the decor view winit asks for.
                        // So `set_ime_allowed` alone stopped working the moment
                        // this class started taking focus, and the field focused
                        // with no keyboard behind it.
                        // Posted rather than called straight away. `restartInput`
                        // above is asynchronous: it tells the input method to
                        // fetch a new connection, and asking for the keyboard
                        // before that has landed is a request against the old
                        // state, which is accepted and then quietly dropped.
                        // `showSoftInput` returning true only means the request
                        // was delivered, never that a keyboard appeared, so the
                        // failure looks like success in every log.
                        input.post(
                                () -> imm.showSoftInput(input, 0));
                    } else {
                        imm.hideSoftInputFromWindow(input.getWindowToken(), 0);
                    }
                });
    }

    /** Hand this activity to the Rust side. See {@link #ruxSetTextInput}. */
    private static native void nativeActivityCreated(RuxActivity activity);

    /** Whether Rux currently has a text field focused. */
    private static native boolean nativeWantsText();

    /** The focused field's current text, so an input method starts from it. */
    private static native String nativeFocusedText();

    /**
     * An input method edited the text. Offsets are UTF-16 code units.
     *
     * <p>{@code composeStart} and {@code composeEnd} are -1 when nothing is being
     * composed, which is what {@code getComposingSpanStart} reports.
     */
    private static native void nativeTextChanged(
            String text, int caret, int anchor, int composeStart, int composeEnd);
}
