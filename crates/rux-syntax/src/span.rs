//! Where a piece of source is: byte offsets, and the line and column an author
//! reads them as.

/// A range of the source, as byte offsets. `end` is exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start: start as u32, end: end as u32 }
    }

    /// An empty span at `at`, for something that has a place but no text,
    /// such as the end of the input.
    pub fn at(at: usize) -> Self {
        Self::new(at, at)
    }

    /// From the start of `self` to the end of `other`.
    pub fn to(self, other: Span) -> Span {
        Span { start: self.start.min(other.start), end: self.end.max(other.end) }
    }

    pub fn range(self) -> std::ops::Range<usize> {
        self.start as usize..self.end as usize
    }

    /// The text this span covers.
    pub fn text(self, src: &str) -> &str {
        &src[self.range()]
    }
}

/// Turns byte offsets into 1-based lines and columns.
///
/// A column counts characters, not bytes, as rhai's positions do, so a
/// position read from either agrees on a line with an accent in it.
#[derive(Clone, Debug)]
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(src: &str) -> Self {
        let mut starts = vec![0];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Self { starts }
    }

    /// The 1-based line and column of byte offset `at` in `src`.
    pub fn line_col(&self, src: &str, at: usize) -> (usize, usize) {
        let line = match self.starts.binary_search(&at) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let start = self.starts[line];
        let col = src[start..at.min(src.len())].chars().count() + 1;
        (line + 1, col)
    }
}
