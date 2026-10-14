//! `white-space`: which spaces in a `<text>` are real.
//!
//! CSS's rule, from CSS Text 3 §4.1.1. Under `normal` (and `nowrap`) a run of
//! spaces, tabs and newlines is one space, and spaces at the very start and
//! end of the paragraph are gone. That is what lets a sentence be written over
//! several indented lines of a template and still read as a sentence. `pre`
//! and `pre-wrap` keep every character; `pre-line` keeps newlines and
//! collapses the rest, which is the one to reach for when text from data has
//! line breaks that matter.
//!
//! Until v0.9 the parser trimmed every text run and nothing collapsed what was
//! left, so a newline inside a `<text>` was a line break and the indentation
//! after it was drawn. Inline runs made that untenable: `Read the <text>terms</text>`
//! needs the space before the tag, and the same rule has to apply to every
//! space or none.
//!
//! The work is done on the text as written *and* on `{{ }}` values, as CSS
//! does: the rule is about what is drawn, not where it came from.

use std::ops::Range;

/// The values of `white-space` Rux follows. `break-spaces` is not one of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WhiteSpace {
    #[default]
    Normal,
    Nowrap,
    Pre,
    PreWrap,
    PreLine,
}

impl WhiteSpace {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim() {
            "normal" => WhiteSpace::Normal,
            "nowrap" => WhiteSpace::Nowrap,
            "pre" => WhiteSpace::Pre,
            "pre-wrap" => WhiteSpace::PreWrap,
            "pre-line" => WhiteSpace::PreLine,
            _ => return None,
        })
    }

    /// Whether lines may break where they run out of room.
    pub fn wraps(self) -> bool {
        !matches!(self, WhiteSpace::Nowrap | WhiteSpace::Pre)
    }

    /// Whether spaces and tabs are kept as written.
    fn keeps_spaces(self) -> bool {
        matches!(self, WhiteSpace::Pre | WhiteSpace::PreWrap)
    }

    /// Whether a newline stays a line break.
    fn keeps_newlines(self) -> bool {
        !matches!(self, WhiteSpace::Normal | WhiteSpace::Nowrap)
    }
}

/// One run, collapsed as the only run of its paragraph.
pub fn collapse(text: &str, mode: WhiteSpace) -> String {
    let mut out = String::with_capacity(text.len());
    collapse_runs_into(&[(text, mode)], &mut out);
    out
}

/// The runs of one paragraph, each under its own `white-space`, collapsed as
/// one text: a space ending one run and a space starting the next are one
/// space, and it stays in the run it started in, as CSS keeps the first.
///
/// A collapsible space is held back until something follows it. That one
/// decision gives all three of CSS's trims: nothing is held at the start of
/// the paragraph or of a line, so leading spaces never appear, and whatever is
/// still held at the end is dropped, so trailing ones do not either.
pub fn collapse_runs(runs: &[(&str, WhiteSpace)]) -> Vec<String> {
    let mut joined = String::new();
    let ranges = collapse_runs_into(runs, &mut joined);
    ranges.into_iter().map(|r| joined[r].to_string()).collect()
}

/// [`collapse_runs`] written into one string, with where each run ended up:
/// what a sentence is joined with, without a string per run (it is done for
/// every sentence on every frame).
///
/// A held-back space is written when the next character arrives, which may be
/// in a later run. It still belongs to the run it started in, so it goes at
/// that run's end, and the runs in between, which are empty, move up by it.
pub fn collapse_runs_into(runs: &[(&str, WhiteSpace)], joined: &mut String) -> Vec<Range<usize>> {
    joined.reserve(runs.iter().map(|(t, _)| t.len()).sum());
    // Where each run starts in `joined`; each ends where the next starts.
    let mut starts: Vec<usize> = Vec::with_capacity(runs.len());
    // The run a held-back space belongs to.
    let mut held: Option<usize> = None;
    // At the start of the paragraph or just after a kept newline, where a
    // collapsible space is removed rather than held.
    let mut line_start = true;

    for (i, (text, mode)) in runs.iter().enumerate() {
        starts.push(joined.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            // A CRLF source (every `.rux` saved on Windows) is one newline,
            // and so is a lone CR, in every mode.
            let c = if c == '\r' {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                '\n'
            } else {
                c
            };
            match c {
                '\n' if mode.keeps_newlines() => {
                    // Spaces before a kept newline go with it (pre-line). Under
                    // pre and pre-wrap nothing is ever held, so this is a no-op.
                    held = None;
                    joined.push('\n');
                    line_start = true;
                }
                ' ' | '\t' | '\n' if !mode.keeps_spaces() => {
                    if !line_start && held.is_none() {
                        held = Some(i);
                    }
                }
                _ => {
                    if let Some(at) = held.take() {
                        // Nothing has been written since run `at` held it, so
                        // every run after it starts here, at the end.
                        joined.push(' ');
                        for s in &mut starts[at + 1..] {
                            *s += 1;
                        }
                    }
                    joined.push(c);
                    line_start = false;
                }
            }
        }
    }
    let end = joined.len();
    (0..starts.len()).map(|i| starts[i]..starts.get(i + 1).copied().unwrap_or(end)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use WhiteSpace::*;

    #[test]
    fn normal_collapses_a_sentence_written_over_lines() {
        let src = "\n      Read the\n      terms\tbefore   you sign.\n    ";
        assert_eq!(collapse(src, Normal), "Read the terms before you sign.");
        assert_eq!(collapse(src, Nowrap), "Read the terms before you sign.");
    }

    #[test]
    fn crlf_is_one_newline() {
        assert_eq!(collapse("a\r\n  b", Normal), "a b");
        assert_eq!(collapse("a\r\nb\rc", Pre), "a\nb\nc");
        assert_eq!(collapse("a  \r\n  b", PreLine), "a\nb");
    }

    #[test]
    fn pre_and_pre_wrap_keep_everything() {
        let src = "  two  spaces\n\tand a tab ";
        assert_eq!(collapse(src, Pre), src);
        assert_eq!(collapse(src, PreWrap), src);
    }

    #[test]
    fn pre_line_keeps_newlines_and_drops_the_spaces_around_them() {
        assert_eq!(collapse("  first   line  \n   second\n\nfourth  ", PreLine), "first line\nsecond\n\nfourth");
    }

    #[test]
    fn non_breaking_spaces_are_content() {
        assert_eq!(collapse("\u{a0} a \u{a0} b", Normal), "\u{a0} a \u{a0} b");
    }

    #[test]
    fn a_space_between_runs_is_kept_once_in_the_first() {
        let runs = collapse_runs(&[("Read the ", Normal), (" terms ", Normal), ("\n  now", Normal)]);
        assert_eq!(runs, ["Read the ", "terms ", "now"]);
    }

    #[test]
    fn a_space_only_between_runs_is_kept() {
        let runs = collapse_runs(&[("a", Normal), ("  ", Normal), ("b", Normal)]);
        assert_eq!(runs, ["a", " ", "b"]);
        // With nothing after it, it is a trailing space and goes.
        let runs = collapse_runs(&[("a", Normal), ("  ", Normal), ("", Normal)]);
        assert_eq!(runs, ["a", "", ""]);
    }

    /// A space held by one run and written during a later one is still the
    /// first run's, and the empty runs between move up past it.
    #[test]
    fn a_space_held_across_empty_runs_stays_in_its_own_run() {
        let mut joined = String::new();
        let ranges = collapse_runs_into(&[("a ", Normal), ("", Normal), (" ", Normal), ("b", Normal)], &mut joined);
        assert_eq!(joined, "a b");
        assert_eq!(ranges, [0..2, 2..2, 2..2, 2..3]);
    }

    #[test]
    fn a_kept_space_does_not_swallow_the_next_collapsible_one() {
        let runs = collapse_runs(&[("a ", Pre), (" b", Normal)]);
        assert_eq!(runs, ["a ", " b"]);
    }

    #[test]
    fn spaces_after_a_kept_newline_start_a_line_and_go() {
        let runs = collapse_runs(&[("a\n", Pre), ("   b", Normal)]);
        assert_eq!(runs, ["a\n", "b"]);
    }

    #[test]
    fn empty_and_blank_paragraphs() {
        assert_eq!(collapse("", Normal), "");
        assert_eq!(collapse(" \n\t ", Normal), "");
        assert_eq!(collapse(" \n ", PreLine), "\n");
    }
}
