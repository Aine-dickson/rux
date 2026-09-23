package dev.ruxlang.shell;

import android.app.NativeActivity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ActivityInfo;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.content.res.TypedArray;
import android.graphics.Insets;
import android.graphics.Rect;
import android.os.Build;
import android.os.Bundle;
import android.text.Editable;
import android.text.Selection;
import android.text.SpannableStringBuilder;
import android.view.ActionMode;
import android.view.KeyEvent;
import android.view.Menu;
import android.view.MenuItem;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewTreeObserver;
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

    /** The on-screen keyboard's height in pixels, 0 when it is down. */
    private static native void nativeKeyboard(int bottom);

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

        clipboard = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);

        keepSplashUntilDrawn();

        // Hand the activity to the Rust side, which has no other way to get it.
        // `ndk_context`, which `android-activity` fills in, holds the
        // Application object rather than this, and an activity method called on
        // that fails with `NoSuchMethodError`.
        nativeActivityCreated(this);
    }

    /**
     * The system splash screen, held until Rux has actually drawn something.
     *
     * <p><b>Without this the splash is correct and useless.</b> The platform
     * takes the splash away as soon as the activity reports a first frame, and
     * a {@link NativeActivity} reports one the moment its surface exists, which
     * is long before anything has been rendered into it. Measured on the
     * emulator: splash for about half a second, then <b>1.1 seconds of black</b>
     * while the shell starts, then the app. The black is the exact stretch the
     * splash exists to cover.
     *
     * <p><b>The splash is held by refusing to draw, not by keeping its view.</b>
     * {@code setOnExitAnimationListener} looks like the answer: it hands the
     * splash view over and makes the app responsible for removing it. It was
     * built that way first, and it fails in a way that reads as success. The
     * listener fires, the view is held, the release arrives a second later
     * exactly as intended, and <em>the screen is black the whole time</em>,
     * because a {@link NativeActivity} renders into the window surface itself
     * and a view handed back by the platform is never composited over it.
     *
     * <p>So instead nothing lets the first draw happen until Rux has presented.
     * The platform takes the splash away when the activity reports a frame, and
     * an {@code OnPreDrawListener} that returns {@code false} is what stops it
     * reporting one. The splash is then never asked to leave.
     *
     * <p>API 31 and up only, because that is where the system splash exists at
     * all; below it there is nothing to hold and the window background is
     * already the right colour.
     */
    private void keepSplashUntilDrawn() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) {
            return;
        }
        final View content = findViewById(android.R.id.content);
        content.getViewTreeObserver()
                .addOnPreDrawListener(
                        new ViewTreeObserver.OnPreDrawListener() {
                            @Override
                            public boolean onPreDraw() {
                                if (!drawn && !deadlinePassed) {
                                    return false;
                                }
                                content.getViewTreeObserver().removeOnPreDrawListener(this);
                                return true;
                            }
                        });
        // **A deadline, because holding the splash forever is a hung app.** If
        // the shell fails before it draws, the honest failure is the app's own
        // window and a log, not a splash screen that never leaves. Generous on
        // purpose: it is a backstop, and a slow cold start on a slow device
        // must not trip it.
        content.postDelayed(
                () -> {
                    if (!drawn) {
                        deadlinePassed = true;
                        content.invalidate();
                    }
                },
                SPLASH_DEADLINE_MS);
    }

    /** How long the splash may be held before it is taken away regardless. */
    private static final long SPLASH_DEADLINE_MS = 5_000L;

    /** Whether Rux has presented a frame. */
    private boolean drawn;

    /** Whether the backstop above has given up waiting for one. */
    private boolean deadlinePassed;

    /**
     * Called from Rust the first time a frame reaches the surface.
     *
     * <p>Named like the other direction's calls and invoked the same way, by
     * name through JNI. It arrives on the render thread, and a splash view may
     * only be touched on the UI thread, hence the hop.
     */
    void ruxFirstFrame() {
        runOnUiThread(
                () -> {
                    drawn = true;
                    // The listener only runs when something asks for a draw, and
                    // nothing will: the view hierarchy is idle and the frame
                    // that matters was drawn by native code into the same
                    // surface. This is what releases it.
                    final View content = findViewById(android.R.id.content);
                    if (content != null) {
                        content.invalidate();
                    }
                });
    }

    /**
     * Report which option a picker settled on, or -1 if it was dismissed.
     *
     * <p>Resolved against the already-loaded library, like {@link #nativeSafeArea}.
     */
    private static native void nativeSelectChosen(int index);

    /** The picker currently up, so a second request cannot stack two dialogs. */
    private android.app.AlertDialog picker;

    /**
     * Report the day a date picker settled on, month counting from 1, or
     * -1 in all three if it was dismissed.
     */
    private static native void nativeDateChosen(int year, int month, int day);

    /**
     * Show the platform's date picker for a {@code type="date"}, starting on
     * {@code value} ({@code YYYY-MM-DD}, or empty for today) and offering only
     * the days between {@code min} and {@code max}, each empty for no limit.
     *
     * <p>Built like {@link #ruxOpenSelect}, and for the same reasons: the UI
     * thread, an answer through a native call rather than a return, and a
     * dismissal that is reported rather than left unanswered.
     */
    void ruxOpenDate(final String value, final String min, final String max) {
        runOnUiThread(
                () -> {
                    if (picker != null) {
                        return;
                    }
                    int[] start = parseDate(value);
                    if (start == null) {
                        java.util.Calendar today = java.util.Calendar.getInstance();
                        start =
                                new int[] {
                                    today.get(java.util.Calendar.YEAR),
                                    today.get(java.util.Calendar.MONTH) + 1,
                                    today.get(java.util.Calendar.DAY_OF_MONTH)
                                };
                    }
                    final boolean[] answered = {false};
                    android.app.DatePickerDialog dialog =
                            new android.app.DatePickerDialog(
                                    this,
                                    (view, year, month, day) -> {
                                        answered[0] = true;
                                        // Java's months count from 0; a date
                                        // written down counts from 1.
                                        nativeDateChosen(year, month + 1, day);
                                    },
                                    start[0],
                                    start[1] - 1,
                                    start[2]);
                    int[] low = parseDate(min);
                    if (low != null) {
                        dialog.getDatePicker().setMinDate(dayMillis(low));
                    }
                    int[] high = parseDate(max);
                    if (high != null) {
                        dialog.getDatePicker().setMaxDate(dayMillis(high));
                    }
                    dialog.setOnDismissListener(
                            d -> {
                                picker = null;
                                // Choosing dismisses too; report a dismissal
                                // only when nothing was chosen.
                                if (!answered[0]) {
                                    nativeDateChosen(-1, -1, -1);
                                }
                            });
                    picker = dialog;
                    dialog.show();
                });
    }

    /**
     * The decimal separator of the phone's language, which decides whether a
     * comma typed into a {@code type="number"} is the point or a thousands
     * separator. Called from the render thread; reading a locale needs no
     * view.
     */
    char ruxDecimalSeparator() {
        return java.text.DecimalFormatSymbols.getInstance().getDecimalSeparator();
    }

    // The text menu's items, as bits. The same numbers are in rux-shell.
    private static final int MENU_COPY = 1;
    private static final int MENU_CUT = 2;
    private static final int MENU_PASTE = 4;
    private static final int MENU_SELECT_ALL = 8;
    private static final int MENU_SHARE = 16;
    private static final int MENU_PROCESS = 32;
    private static final int MENU_READONLY = 64;
    private static final int MENU_SELECT = 128;

    // What {@link #nativeTextAction} reports. Also in rux-shell.
    private static final int MENU_ACTION_COPY = 0;
    private static final int MENU_ACTION_CUT = 1;
    private static final int MENU_ACTION_PASTE = 2;
    private static final int MENU_ACTION_SELECT_ALL = 3;
    private static final int MENU_ACTION_SELECT_WORD = 4;

    /** The menu item id for Share, which has no platform id of its own. */
    private static final int ID_SHARE = 0x52555801;

    /** Where a {@code PROCESS_TEXT} app's answer comes back. */
    private static final int REQUEST_PROCESS_TEXT = 0x5258;

    /** An item from the text menu was chosen: a {@code MENU_ACTION_} code. */
    private static native void nativeTextAction(int action);

    /**
     * The text menu closed without Rux asking. {@code collapse}: the
     * selection goes with it.
     */
    private static native void nativeTextMenuClosed(boolean collapse);

    /** An app given the selection answered with {@code text}, for focus {@code token}. */
    private static native void nativeProcessedText(long token, String text);

    /** The text menu that is up, or null. */
    private ActionMode textMenu;

    /** What the menu offers, {@code MENU_} bits. */
    private int menuFlags;

    /** The selected text, for Share and {@code PROCESS_TEXT}. */
    private String menuText = "";

    /** The selection, in this view's pixels, which the menu floats beside. */
    private final Rect menuRect = new Rect();

    /** A {@code PROCESS_TEXT} app is open, and the field it answers for. */
    private long processToken;
    private boolean processReadonly;

    /**
     * Show the platform's floating text menu beside a selection, move it, or
     * close it.
     *
     * <p><b>Why the platform's and not a drawn one.</b> Rux drew its own strip
     * of four buttons first, the one a browser needs because a page cannot
     * reach the platform's. On a phone that strip is the only one on the device
     * shaped like it, it has no overflow, and it cannot offer the apps that
     * register for {@code PROCESS_TEXT}, which is how Translate and every
     * dictionary appear in other apps' menus. A floating {@link ActionMode} is
     * that menu: {@code TextView} uses exactly this, and the platform draws it,
     * places it clear of the keyboard and handles its overflow.
     *
     * <p>Started on the input view because an action mode belongs to a view,
     * and that is the one view Rux has. Called from the render thread with the
     * whole state each time, and only when that state changed.
     *
     * <p><b>A menu Rux closes does not report closing.</b> {@code textMenu} is
     * cleared before {@code finish()}, so the callback can tell a close Rux
     * asked for from one the platform made, which is the only kind worth
     * telling Rux about.
     */
    void ruxTextMenu(
            final boolean show,
            final int left,
            final int top,
            final int right,
            final int bottom,
            final int flags,
            final String text) {
        runOnUiThread(
                () -> {
                    if (input == null) {
                        return;
                    }
                    if (!show) {
                        final ActionMode open = textMenu;
                        textMenu = null;
                        if (open != null) {
                            open.finish();
                        }
                        return;
                    }
                    // Rux measures in the window; the menu asks in the view.
                    final int[] at = new int[2];
                    input.getLocationInWindow(at);
                    menuRect.set(left - at[0], top - at[1], right - at[0], bottom - at[1]);
                    final boolean changed = flags != menuFlags;
                    menuFlags = flags;
                    menuText = text == null ? "" : text;
                    if (textMenu == null) {
                        textMenu = input.startActionMode(new TextMenu(), ActionMode.TYPE_FLOATING);
                    } else {
                        if (changed) {
                            textMenu.invalidate();
                        }
                        textMenu.invalidateContentRect();
                    }
                });
    }

    /**
     * The menu's items and what choosing one does.
     *
     * <p>Items in {@code TextView}'s order, with its ids and its strings, so
     * the menu reads the same as in any other field on the phone, in the
     * phone's language.
     */
    private final class TextMenu extends ActionMode.Callback2 {

        @Override
        public boolean onCreateActionMode(ActionMode mode, Menu menu) {
            fill(menu);
            return true;
        }

        @Override
        public boolean onPrepareActionMode(ActionMode mode, Menu menu) {
            menu.clear();
            fill(menu);
            return true;
        }

        private void fill(Menu menu) {
            int order = 0;
            if ((menuFlags & MENU_CUT) != 0) {
                add(menu, android.R.id.cut, order++, getString(android.R.string.cut));
            }
            if ((menuFlags & MENU_COPY) != 0) {
                add(menu, android.R.id.copy, order++, getString(android.R.string.copy));
            }
            // Paste only with something to paste, as in any other field: an
            // item that does nothing when tapped is worse than no item.
            if ((menuFlags & MENU_PASTE) != 0 && clipboardHasText()) {
                add(menu, android.R.id.paste, order++, getString(android.R.string.paste));
            }
            if ((menuFlags & MENU_SHARE) != 0) {
                add(menu, ID_SHARE, order++, platformString("share", "Share"));
            }
            if ((menuFlags & MENU_SELECT) != 0) {
                add(menu, android.R.id.selectTextMode, order++, platformString("selectTextMode", "Select"));
            }
            if ((menuFlags & MENU_SELECT_ALL) != 0) {
                add(menu, android.R.id.selectAll, order++, getString(android.R.string.selectAll));
            }
            if ((menuFlags & MENU_PROCESS) != 0) {
                // After everything the field itself offers, as TextView puts
                // them. Each carries the intent that reaches its app.
                final PackageManager pm = getPackageManager();
                for (ResolveInfo app : processTextApps()) {
                    final Intent intent =
                            new Intent(Intent.ACTION_PROCESS_TEXT)
                                    .setType("text/plain")
                                    .setClassName(
                                            app.activityInfo.packageName, app.activityInfo.name);
                    menu.add(Menu.NONE, Menu.NONE, 100 + order++, app.loadLabel(pm))
                            .setIntent(intent)
                            .setShowAsAction(MenuItem.SHOW_AS_ACTION_IF_ROOM);
                }
            }
        }

        private void add(Menu menu, int id, int order, CharSequence title) {
            menu.add(Menu.NONE, id, order, title).setShowAsAction(MenuItem.SHOW_AS_ACTION_IF_ROOM);
        }

        @Override
        public boolean onActionItemClicked(ActionMode mode, MenuItem item) {
            final int id = item.getItemId();
            if (id == android.R.id.copy) {
                nativeTextAction(MENU_ACTION_COPY);
            } else if (id == android.R.id.cut) {
                nativeTextAction(MENU_ACTION_CUT);
            } else if (id == android.R.id.paste) {
                nativeTextAction(MENU_ACTION_PASTE);
            } else if (id == android.R.id.selectAll) {
                nativeTextAction(MENU_ACTION_SELECT_ALL);
            } else if (id == android.R.id.selectTextMode) {
                nativeTextAction(MENU_ACTION_SELECT_WORD);
            } else if (id == ID_SHARE) {
                final Intent send =
                        new Intent(Intent.ACTION_SEND)
                                .setType("text/plain")
                                .putExtra(Intent.EXTRA_TEXT, menuText);
                closeMenu(true);
                startSafely(Intent.createChooser(send, null), -1);
            } else if (item.getIntent() != null) {
                // Taken before the menu closes: the answer must reach the
                // field the text came from and nothing else.
                processToken = nativeFieldToken();
                processReadonly = (menuFlags & MENU_READONLY) != 0;
                final Intent intent =
                        new Intent(item.getIntent())
                                .putExtra(Intent.EXTRA_PROCESS_TEXT, menuText)
                                .putExtra(Intent.EXTRA_PROCESS_TEXT_READONLY, processReadonly);
                closeMenu(false);
                startSafely(intent, REQUEST_PROCESS_TEXT);
            }
            return true;
        }

        @Override
        public void onDestroyActionMode(ActionMode mode) {
            // Only a close Rux did not ask for is news. See `ruxTextMenu`.
            if (mode == textMenu) {
                textMenu = null;
                nativeTextMenuClosed(true);
            }
        }

        @Override
        public void onGetContentRect(ActionMode mode, View view, Rect out) {
            out.set(menuRect);
        }
    }

    /** Whether the clipboard holds anything a field could paste. Never throws. */
    private boolean clipboardHasText() {
        try {
            return clipboard != null && clipboard.hasPrimaryClip();
        } catch (RuntimeException e) {
            return false;
        }
    }

    /**
     * Close the menu from Java's side, because an item handed the text to
     * another app, and tell Rux, which did not ask for it.
     */
    private void closeMenu(boolean collapse) {
        final ActionMode open = textMenu;
        textMenu = null;
        if (open != null) {
            open.finish();
        }
        nativeTextMenuClosed(collapse);
    }

    /**
     * Start another app's activity, and survive its not being there: an app
     * can be uninstalled between the menu being built and the item tapped.
     */
    private void startSafely(Intent intent, int request) {
        try {
            if (request >= 0) {
                startActivityForResult(intent, request);
            } else {
                startActivity(intent);
            }
        } catch (RuntimeException e) {
            android.util.Log.w("rux", "could not start " + intent, e);
        }
    }

    /**
     * The apps that take selected text, the way {@code TextView} finds them.
     *
     * <p>Visible at all only because the manifest declares a
     * {@code <queries>} entry for {@code PROCESS_TEXT}: since Android 11 an app
     * sees only the other apps it has said it will look for, and without the
     * entry this list is empty and nothing says why.
     */
    private java.util.List<ResolveInfo> processTextApps() {
        try {
            return getPackageManager()
                    .queryIntentActivities(
                            new Intent(Intent.ACTION_PROCESS_TEXT).setType("text/plain"), 0);
        } catch (RuntimeException e) {
            return java.util.Collections.emptyList();
        }
    }

    /** A string the platform has but does not publish, by name, or {@code fallback}. */
    private String platformString(String name, String fallback) {
        final int id = getResources().getIdentifier(name, "string", "android");
        if (id == 0) {
            return fallback;
        }
        try {
            return getString(id);
        } catch (RuntimeException e) {
            return fallback;
        }
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != REQUEST_PROCESS_TEXT || result != RESULT_OK || data == null) {
            return;
        }
        // A read-only field was only showing the text to the app; whatever
        // came back is not for it.
        if (processReadonly) {
            return;
        }
        final CharSequence answer = data.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT);
        if (answer != null) {
            nativeProcessedText(processToken, answer.toString());
        }
    }

    /**
     * The theme's text highlight and accent, as two ARGB ints: highlight in
     * the high half. Called from the render thread; reading a theme needs no
     * view.
     *
     * <p>{@code textColorHighlight} is what every {@code TextView} on the
     * phone paints a selection with, and {@code colorControlActivated} is
     * what its handles are tinted with. From {@code DeviceDefault}, which
     * the activity's theme extends, both are the phone's own: its maker's
     * accent, or the wallpaper's on Android 12 and up.
     */
    long ruxThemeColors() {
        final TypedArray a =
                getTheme()
                        .obtainStyledAttributes(
                                new int[] {
                                    android.R.attr.textColorHighlight,
                                    android.R.attr.colorControlActivated,
                                    android.R.attr.colorAccent
                                });
        try {
            final int highlight = a.getColor(0, 0);
            int accent = a.getColor(1, 0);
            if (accent == 0) {
                accent = a.getColor(2, 0);
            }
            return ((long) highlight << 32) | (accent & 0xffffffffL);
        } finally {
            a.recycle();
        }
    }

    /** {@code YYYY-MM-DD} as year, month from 1 and day, or null. Rux has checked it. */
    private static int[] parseDate(String text) {
        if (text == null) {
            return null;
        }
        String[] parts = text.split("-");
        if (parts.length != 3) {
            return null;
        }
        try {
            return new int[] {
                Integer.parseInt(parts[0]), Integer.parseInt(parts[1]), Integer.parseInt(parts[2])
            };
        } catch (NumberFormatException e) {
            return null;
        }
    }

    /** Midnight at the start of a day, local time, which is what a picker's limits are in. */
    private static long dayMillis(int[] date) {
        java.util.Calendar c = java.util.Calendar.getInstance();
        c.clear();
        c.set(date[0], date[1] - 1, date[2]);
        return c.getTimeInMillis();
    }

    /**
     * Show the platform's own picker for a {@code <select>}.
     *
     * <p><b>Why this is not the drawn dropdown.</b> Rux draws one on a desktop
     * and it is the right answer there. On a phone it is a browser emulation:
     * it does not fling, does not dismiss on Back, is not announced as a picker
     * by a screen reader, and does not look like the control the person uses in
     * every other app. The spec has asked for the platform control since before
     * there was a phone to run it on.
     *
     * <p>Called from the render thread, so it hops to the UI thread, and it
     * answers through {@link #nativeSelectChosen} rather than by returning: a
     * picker is not answered in the gesture that opened it.
     *
     * <p><b>Dismissal is an answer.</b> Back, or a tap outside, reports -1
     * rather than nothing, because a shell still waiting for a reply would
     * leave that field unable to open a picker ever again.
     */
    void ruxOpenSelect(final String[] options, final int selected) {
        runOnUiThread(
                () -> {
                    if (picker != null) {
                        return;
                    }
                    final boolean[] answered = {false};
                    picker =
                            new android.app.AlertDialog.Builder(this)
                                    .setSingleChoiceItems(
                                            options,
                                            selected,
                                            (dialog, which) -> {
                                                answered[0] = true;
                                                nativeSelectChosen(which);
                                                dialog.dismiss();
                                            })
                                    .setOnDismissListener(
                                            dialog -> {
                                                picker = null;
                                                // Only when nothing was chosen:
                                                // choosing dismisses too, and
                                                // reporting twice would apply an
                                                // edit and then undo it.
                                                if (!answered[0]) {
                                                    nativeSelectChosen(-1);
                                                }
                                            })
                                    .create();
                    picker.show();
                });
    }

    /**
     * The system clipboard, taken once in {@link #onCreate}.
     *
     * <p>Fetched on the UI thread and kept, rather than looked up by each call
     * below, because those calls arrive on the Rux render thread, and on older
     * platforms a system service built for the first time off the main thread
     * binds to a thread with no looper.
     */
    private ClipboardManager clipboard;

    /**
     * Put {@code text} on the system clipboard. Called from Rust for Copy and Cut.
     *
     * <p><b>Why this exists at all.</b> The first APK stubbed the clipboard out,
     * on the grounds that copy and paste doing nothing was better than half
     * working. That stopped being true when the drawn text toolbar reached the
     * phone: Cut removed the selection and stored it nowhere, which is not a
     * clipboard that does nothing but a delete with no undo.
     *
     * <p>Called on the render thread and not hopped to the UI thread. A
     * clipboard write is a binder call, not a view operation, and running it
     * here is what lets Cut be sure the text has landed before it removes it.
     *
     * <p><b>Returns whether the write happened</b>, and never throws: an
     * exception left pending across JNI poisons every later call from the same
     * thread, and Cut asks precisely so that it can refuse to delete when this
     * failed.
     */
    boolean ruxClipboardWrite(final String text) {
        try {
            if (clipboard == null) {
                return false;
            }
            clipboard.setPrimaryClip(ClipData.newPlainText("text", text));
            return true;
        } catch (RuntimeException e) {
            android.util.Log.w("rux", "clipboard write failed", e);
            return false;
        }
    }

    /**
     * The system clipboard as text, or null if it holds none. Called from Rust
     * for Paste.
     *
     * <p>{@code coerceToText} rather than {@code getText}, so that a copied link
     * or a styled span pastes as the text it shows, the same as it would into
     * any other field on the device. Never throws, for the reason on
     * {@link #ruxClipboardWrite}.
     */
    String ruxClipboardRead() {
        try {
            if (clipboard == null || !clipboard.hasPrimaryClip()) {
                return null;
            }
            final ClipData clip = clipboard.getPrimaryClip();
            if (clip == null || clip.getItemCount() == 0) {
                return null;
            }
            final CharSequence text = clip.getItemAt(0).coerceToText(this);
            return text == null ? null : text.toString();
        } catch (RuntimeException e) {
            android.util.Log.w("rux", "clipboard read failed", e);
            return null;
        }
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
            // The keyboard, as an inset: from API 30 that is the only way it
            // is reported, and `adjustResize` no longer shrinks the window.
            // Below 30 it still does, and the window's resize says it.
            nativeKeyboard(insets.getInsets(WindowInsets.Type.ime()).bottom);
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
            // **What the field is, not what every field is.** This was a
            // constant, and a constant is a lie for any input that is not a
            // one-line text box. An `EditorInfo` is the only thing an app ever
            // says to an input method about what is being edited: the keys it
            // offers, the action in the corner and whether autocorrect runs are
            // all decided here and nowhere else.
            //
            // No full-screen editor either way. A phone in landscape will
            // otherwise replace the whole app with the IME's own editor, which
            // would cover the Rux document being edited.
            // The kind in the low byte, `inputmode` and `enterkeyhint` above
            // it; the layout is written down on `FOCUSED_KIND` in rux-shell.
            int packed = nativeFieldKind();
            int kind = packed & 0xff;
            int keyboard = (packed >> 8) & 0xff;
            int enter = (packed >> 16) & 0xff;
            boolean multiline = kind == KIND_TEXTAREA;
            if (kind == KIND_PASSWORD) {
                // **The variation is not cosmetic and not about the glyphs.**
                // Rux draws its own bullets, so this is not what masks the
                // field. What it buys is everything an input method would
                // otherwise do with the text: no autocorrect, no suggestion
                // strip built from it, and no adding it to the personal
                // dictionary. `IME_FLAG_NO_PERSONALIZED_LEARNING` is the same
                // refusal said again, because the variation alone is advisory
                // and keyboards have differed on honouring it.
                //
                // The realistic leak this closes is not someone reading the
                // screen. It is the keyboard learning the password and then
                // offering it as a suggestion in a different app.
                out.inputType =
                        EditorInfo.TYPE_CLASS_TEXT | EditorInfo.TYPE_TEXT_VARIATION_PASSWORD;
                out.imeOptions =
                        EditorInfo.IME_FLAG_NO_FULLSCREEN
                                | EditorInfo.IME_ACTION_DONE
                                | EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING;
            } else if (kind == KIND_SEARCH) {
                // The whole of `search`: the action key says Search. The field
                // is an ordinary one-line text box in every other respect,
                // which is exactly why this is a keyboard hint and not a
                // control of its own.
                out.inputType = EditorInfo.TYPE_CLASS_TEXT;
                out.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN | EditorInfo.IME_ACTION_SEARCH;
            } else if (kind == KIND_NUMBER) {
                // A number keyboard that can type any number the field
                // accepts: a minus sign and a decimal point, the two keys a
                // bare number class leaves off. An `inputmode` still wins
                // below, so `inputmode="numeric"` gives a digits-only pad.
                out.inputType = EditorInfo.TYPE_CLASS_NUMBER
                        | EditorInfo.TYPE_NUMBER_FLAG_SIGNED
                        | EditorInfo.TYPE_NUMBER_FLAG_DECIMAL;
                out.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN | EditorInfo.IME_ACTION_DONE;
            } else if (multiline) {
                // `IME_FLAG_NO_ENTER_ACTION` is what turns the action key back
                // into a newline key. Without it Gboard shows Done and sends an
                // editor action, and a textarea cannot be given a second line.
                out.inputType = EditorInfo.TYPE_CLASS_TEXT | EditorInfo.TYPE_TEXT_FLAG_MULTI_LINE;
                out.imeOptions =
                        EditorInfo.IME_FLAG_NO_FULLSCREEN | EditorInfo.IME_FLAG_NO_ENTER_ACTION;
            } else {
                out.inputType = EditorInfo.TYPE_CLASS_TEXT;
                out.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN | EditorInfo.IME_ACTION_DONE;
            }
            applyKeyboard(out, kind, keyboard);
            if (!multiline) {
                applyEnterKey(out, enter);
            }
            String current = nativeFocusedText();
            if (current == null) {
                current = "";
            }
            // Where Rux has the selection, not the end of the text. Clamped,
            // because the two natives are read separately and a field can
            // change between them.
            final long selection = nativeFocusedSelection();
            final int anchor = Math.min((int) (selection >>> 32), current.length());
            final int caret = Math.min((int) selection, current.length());
            out.initialSelStart = Math.min(anchor, caret);
            out.initialSelEnd = Math.max(anchor, caret);
            connection = new RuxInputConnection(this, current, anchor, caret, multiline);
            return connection;
        }

        /**
         * {@code inputmode}: which keys the keyboard offers. Only the class
         * and variation change; the field is a text field whatever this says.
         *
         * <p>A password keeps its variation, because trading it for an email
         * keyboard would give up the no-learning guarantee for a key layout.
         * The one exception is {@code numeric}, which Android has a password
         * variation of its own for: a PIN pad that still learns nothing.
         */
        private void applyKeyboard(EditorInfo out, int kind, int keyboard) {
            boolean password = kind == KIND_PASSWORD;
            int multi = out.inputType & EditorInfo.TYPE_TEXT_FLAG_MULTI_LINE;
            switch (keyboard) {
                case KEYBOARD_NUMERIC:
                    out.inputType = EditorInfo.TYPE_CLASS_NUMBER
                            | (password ? EditorInfo.TYPE_NUMBER_VARIATION_PASSWORD : 0);
                    break;
                case KEYBOARD_DECIMAL:
                    if (!password) {
                        out.inputType = EditorInfo.TYPE_CLASS_NUMBER
                                | EditorInfo.TYPE_NUMBER_FLAG_DECIMAL;
                    }
                    break;
                case KEYBOARD_TEL:
                    if (!password) {
                        out.inputType = EditorInfo.TYPE_CLASS_PHONE;
                    }
                    break;
                case KEYBOARD_EMAIL:
                    if (!password) {
                        out.inputType = EditorInfo.TYPE_CLASS_TEXT
                                | EditorInfo.TYPE_TEXT_VARIATION_EMAIL_ADDRESS | multi;
                    }
                    break;
                case KEYBOARD_URL:
                    if (!password) {
                        out.inputType = EditorInfo.TYPE_CLASS_TEXT
                                | EditorInfo.TYPE_TEXT_VARIATION_URI | multi;
                    }
                    break;
                case KEYBOARD_SEARCH:
                    if (multi == 0) {
                        out.imeOptions = (out.imeOptions & ~EditorInfo.IME_MASK_ACTION)
                                | EditorInfo.IME_ACTION_SEARCH;
                    }
                    break;
                default:
                    break;
            }
        }

        /**
         * {@code enterkeyhint}: what the action key says. Pressing it still
         * sends Enter, which is {@code BaseInputConnection}'s own answer to
         * {@code performEditorAction}, and Rux decides what Enter does.
         */
        private void applyEnterKey(EditorInfo out, int enter) {
            int action;
            switch (enter) {
                case ENTER_ENTER:
                    // A plain return key: no action at all.
                    out.imeOptions = (out.imeOptions & ~EditorInfo.IME_MASK_ACTION)
                            | EditorInfo.IME_ACTION_UNSPECIFIED
                            | EditorInfo.IME_FLAG_NO_ENTER_ACTION;
                    return;
                case ENTER_DONE: action = EditorInfo.IME_ACTION_DONE; break;
                case ENTER_GO: action = EditorInfo.IME_ACTION_GO; break;
                case ENTER_NEXT: action = EditorInfo.IME_ACTION_NEXT; break;
                case ENTER_PREVIOUS: action = EditorInfo.IME_ACTION_PREVIOUS; break;
                case ENTER_SEARCH: action = EditorInfo.IME_ACTION_SEARCH; break;
                case ENTER_SEND: action = EditorInfo.IME_ACTION_SEND; break;
                default: return;
            }
            out.imeOptions = (out.imeOptions & ~EditorInfo.IME_MASK_ACTION) | action;
        }

        /** The connection most recently handed to an input method. */
        RuxInputConnection connection;

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

        /**
         * Which focused field this connection was built for.
         *
         * <p><b>A connection outlives the field it belongs to, and that is a
         * defect you can watch happen.</b> {@code restartInput} is
         * asynchronous: it asks the input method to fetch a new connection and
         * returns immediately. Between the tap that moves focus and the new
         * connection arriving, the input method still holds this one, and it
         * usually has something to say on the way out, because finishing a
         * composition is the first thing it does. That report carries the *old*
         * field's text and is applied to the field that now has focus.
         *
         * <p>Reported by the user watching the screen, which is the only place
         * it shows: tapping the second input made the first one's text appear
         * in it for about half a second before being cleared. Every screenshot
         * taken after that was of an empty field, so the capture agreed with
         * the fix while the eye did not.
         *
         * <p>So Rux stamps each focus with a token and ignores a report that
         * does not carry the current one.
         */
        private final long token;

        /** Whether Enter belongs in the text rather than to the keyboard. */
        private final boolean multiline;

        RuxInputConnection(
                View target, String initial, int anchor, int caret, boolean multiline) {
            // `true`: this is a full editor, so the base class maintains the
            // composing spans and the selection on the editable below, which is
            // the part of an input method's protocol worth not reimplementing.
            super(target, true);
            editable = new SpannableStringBuilder(initial);
            Selection.setSelection(editable, anchor, caret);
            token = nativeFieldToken();
            this.multiline = multiline;
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

        /**
         * The action key, handed to Rux directly, so it does what its label
         * said (Next, Go, Done).
         *
         * <p>Not through the base class, whose answer is to dispatch an Enter
         * from no device, and that Enter never reaches the native side: driven
         * with Gboard, the key went out and the window never saw it.
         */
        @Override
        public boolean performEditorAction(int action) {
            nativeEnter(true);
            return true;
        }

        @Override
        public boolean sendKeyEvent(KeyEvent event) {
            // **The base class does not edit anything here, and that is the
            // whole of why Backspace did nothing.** `BaseInputConnection`
            // implements this by dispatching the key to the target view, on the
            // assumption that the view is a `TextView` that will act on it.
            // Rux's view is not: it exists only to hold focus and returns false
            // from `onKeyDown`, so the key reached nothing, the editable was
            // untouched, and the report that followed carried text that had not
            // changed. Driven on a phone with Gboard: four presses of Backspace,
            // no change to the field.
            //
            // It only showed once the text was no longer being composed. While
            // an input method is composing a word it edits through
            // `setComposingText`, which does maintain the editable, so the
            // first Backspace of a fresh word works and the ones after a space,
            // a suggestion or a change of field do not. That is a hard shape to
            // catch by hand and it is why this was reported as "Backspace
            // doesn't work" rather than as anything narrower.
            if (event.getAction() == KeyEvent.ACTION_DOWN) {
                switch (event.getKeyCode()) {
                    case KeyEvent.KEYCODE_DEL:
                        deleteAround(true);
                        return true;
                    case KeyEvent.KEYCODE_FORWARD_DEL:
                        deleteAround(false);
                        return true;
                    case KeyEvent.KEYCODE_ENTER:
                        // A newline is an edit, in a field that takes one. In a
                        // one-line field it is not, and falls through to the
                        // dispatch below so that the action key keeps meaning
                        // what the `EditorInfo` said it means.
                        if (multiline) {
                            commitText("\n", 1);
                            return true;
                        }
                        // In a one-line field it is Enter, handed over the way
                        // the action key is, for the same reason: dispatched,
                        // it would never arrive.
                        nativeEnter(false);
                        return true;
                    default:
                        // A printable key sent this way would be dropped for the
                        // same reason, so it is committed rather than dispatched.
                        //
                        // **Printable, and control characters are not.** Tab
                        // and Enter both have a unicode value, and committing
                        // those would put a literal tab in the field instead of
                        // moving focus, and a newline in a single-line input.
                        // They are not edits, so they go to the dispatch below
                        // with the arrow keys.
                        int unicode = event.getUnicodeChar();
                        if (unicode >= 0x20 && unicode != 0x7f) {
                            commitText(String.valueOf((char) unicode), 1);
                            return true;
                        }
                }
            }
            // Anything else is not an edit: arrows, Enter, a hardware key. Those
            // do belong to the dispatch the base class performs.
            boolean handled = super.sendKeyEvent(event);
            report();
            return handled;
        }

        /**
         * Delete the selection, or one character to one side of the caret.
         *
         * <p>{@code before} is Backspace; false is Forward Delete. Counted in
         * <em>code points</em> rather than {@code char}s, so one press removes
         * one emoji instead of half of one and leaving a lone surrogate behind.
         */
        private void deleteAround(boolean before) {
            int start = Selection.getSelectionStart(editable);
            int end = Selection.getSelectionEnd(editable);
            if (start < 0 || end < 0) {
                return;
            }
            if (start != end) {
                editable.delete(Math.min(start, end), Math.max(start, end));
                report();
                return;
            }
            if (before && start > 0) {
                int from = Character.offsetByCodePoints(editable, start, -1);
                editable.delete(from, start);
            } else if (!before && start < editable.length()) {
                int to = Character.offsetByCodePoints(editable, start, 1);
                editable.delete(start, to);
            } else {
                // Nothing to remove, and reporting an unchanged field would be
                // one more edit for Rux to apply for no reason.
                return;
            }
            report();
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
                    token,
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

    /**
     * Move the selection in the connection's copy of the text, because Rux
     * moved it: a tap, a drag, a double tap, Select all.
     *
     * <p>Without this the connection kept the caret wherever the keyboard last
     * put it, and the next thing typed went in there, not where the caret was
     * drawn. {@code updateSelection} is how an editor tells the input method
     * its selection moved by itself, which is exactly what happened.
     *
     * <p>A text change does not come here: Rux rebuilds the connection for that
     * through {@link #ruxSetTextInput}. {@code token} is Rux's focus when it
     * asked; a connection built for any other is left alone.
     */
    void ruxSyncSelection(final long token, final int anchor, final int caret) {
        runOnUiThread(
                () -> {
                    if (input == null) {
                        return;
                    }
                    final RuxInputConnection connection = input.connection;
                    if (connection == null || connection.token != token) {
                        return;
                    }
                    final Editable editable = connection.getEditable();
                    final int length = editable.length();
                    final int a = Math.min(Math.max(anchor, 0), length);
                    final int c = Math.min(Math.max(caret, 0), length);
                    // Whatever was being composed is abandoned, as it is when
                    // the caret moves in any other editor.
                    BaseInputConnection.removeComposingSpans(editable);
                    Selection.setSelection(editable, a, c);
                    InputMethodManager imm =
                            (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
                    if (imm != null) {
                        imm.updateSelection(input, Math.min(a, c), Math.max(a, c), -1, -1);
                    }
                });
    }

    /** Hand this activity to the Rust side. See {@link #ruxSetTextInput}. */
    private static native void nativeActivityCreated(RuxActivity activity);

    /** Whether Rux currently has a text field focused. */
    private static native boolean nativeWantsText();

    /** A one-line text field, and what {@link #nativeFieldKind} falls back to. */
    private static final int KIND_TEXT = 0;

    /** {@code type="textarea"}, the one field that takes a newline. */
    private static final int KIND_TEXTAREA = 1;

    /** {@code type="password"}. */
    private static final int KIND_PASSWORD = 2;

    /** {@code type="search"}. */
    private static final int KIND_SEARCH = 3;

    /** {@code type="number"}. */
    private static final int KIND_NUMBER = 4;

    // `inputmode`, the second byte of {@link #nativeFieldKind}. 0 is text.
    private static final int KEYBOARD_NUMERIC = 1;
    private static final int KEYBOARD_DECIMAL = 2;
    private static final int KEYBOARD_TEL = 3;
    private static final int KEYBOARD_EMAIL = 4;
    private static final int KEYBOARD_URL = 5;
    private static final int KEYBOARD_SEARCH = 6;

    // `enterkeyhint`, the third byte. 0 is unset: the kind decides.
    private static final int ENTER_ENTER = 1;
    private static final int ENTER_DONE = 2;
    private static final int ENTER_GO = 3;
    private static final int ENTER_NEXT = 4;
    private static final int ENTER_PREVIOUS = 5;
    private static final int ENTER_SEARCH = 6;
    private static final int ENTER_SEND = 7;

    /**
     * What kind of field has focus, so the right keyboard can be asked for.
     *
     * <p>One of the {@code KIND_} constants above in the low byte, with a
     * {@code KEYBOARD_} in the next and an {@code ENTER_} in the third. Every
     * other input type Rux grows will arrive through here, because an
     * {@code EditorInfo} is the only thing that decides which keys an input
     * method offers.
     */
    private static native int nativeFieldKind();

    /** The focused field's current text, so an input method starts from it. */
    private static native String nativeFocusedText();

    /**
     * The focused field's selection in UTF-16 units: anchor in the high 32
     * bits, caret in the low. So a new connection starts where Rux's caret is.
     */
    private static native long nativeFocusedSelection();

    /**
     * Which field has focus right now, as a number that changes when it moves.
     *
     * <p>Taken once, when a connection is built, and handed back with every
     * report that connection makes. See {@link RuxInputConnection#token} for
     * what goes wrong without it.
     */
    private static native long nativeFieldToken();

    /** Enter from the input method: its action key, or a plain Enter. */
    private static native void nativeEnter(boolean action);

    /**
     * An input method edited the text. Offsets are UTF-16 code units.
     *
     * <p>{@code token} is the focus this edit belongs to; Rux drops the edit if
     * focus has moved on since. {@code composeStart} and {@code composeEnd} are
     * -1 when nothing is being composed, which is what
     * {@code getComposingSpanStart} reports.
     */
    private static native void nativeTextChanged(
            long token, String text, int caret, int anchor, int composeStart, int composeEnd);
}
