//! Converts Oxc's UTF-8 byte-offset spans into human- or tool-facing
//! `(line, column)` positions.
//!
//! Two different consumers need two different notions of "column" from
//! the same underlying byte offset, and conflating them is a real
//! correctness bug, not a style choice:
//!
//!   - A native CLI/terminal consumer wants a **Unicode codepoint**
//!     count from the start of the line — the convention rustc and most
//!     compilers use, and the natural fit for "how many characters
//!     over" a human reading a monospace terminal would say.
//!   - A WASM/JS consumer — and the Language Server Protocol, by
//!     default — wants a **UTF-16 code unit** count instead, because JS
//!     strings are UTF-16 internally. Reporting a codepoint count to a
//!     JS caller that then does `sourceText.slice(start, end)`, or an
//!     editor that positions a cursor from it, would silently misplace
//!     every diagnostic after the first astral-plane character on the
//!     line (many emoji, some CJK extension blocks: 1 codepoint, but 2
//!     UTF-16 code units) — not crash, just point at the wrong spot.
//!
//! Both share the same line-start table. Finding where each line begins
//! is a single scan for the byte `b'\n'`, and that's safe purely at the
//! byte level even inside multi-byte UTF-8 text: `\n` (0x0A) can never
//! appear as a continuation byte of a multi-byte sequence, so scanning
//! raw bytes for it can't misfire on non-ASCII content. What differs
//! between the two consumers is only how the byte range *within* a
//! line gets counted, not where lines start.
//!
//! Deliberately not stored on `Diagnostic` itself: `Diagnostic` keeps
//! raw byte offsets (matching Oxc's own `Span` representation) as its
//! one source of truth, and callers convert at the point they actually
//! need a position — a native formatter calling `line_col_utf8`, the
//! WASM boundary calling `line_col_utf16`. That keeps `Diagnostic`
//! itself presentation-format-agnostic, the same way it's already
//! decoupled from Oxc's own diagnostic types (see `diagnostics.rs`).

pub struct LineIndex {
    /// Byte offset of the start of each line. Always non-empty:
    /// `line_starts[0] == 0`.
    line_starts: Vec<u32>,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i as u32 + 1);
            }
        }
        Self { line_starts }
    }

    /// Index into `line_starts` of the line containing `byte_offset`.
    fn line_of(&self, byte_offset: u32) -> usize {
        self.line_starts.partition_point(|&start| start <= byte_offset) - 1
    }

    /// 1-based line, 1-based column counted in Unicode codepoints — the
    /// convention a native CLI/terminal consumer expects.
    ///
    /// `source` must be the exact same string the `LineIndex` was built
    /// from, and `byte_offset` must fall on a UTF-8 char boundary
    /// within it (true by construction for any offset that came from
    /// Oxc's own `Span`, since a real parser never produces a span that
    /// splits a multi-byte character).
    pub fn line_col_utf8(&self, byte_offset: u32, source: &str) -> (u32, u32) {
        let line = self.line_of(byte_offset);
        let line_start = self.line_starts[line] as usize;
        let column = source[line_start..byte_offset as usize].chars().count() as u32 + 1;
        (line as u32 + 1, column)
    }

    /// 1-based line, 1-based column counted in UTF-16 code units — what
    /// a WASM/JS consumer, and the LSP spec by default, expect, so a JS
    /// caller can index into its own UTF-16 source string without the
    /// two representations silently diverging on non-ASCII input.
    ///
    /// Same preconditions on `source`/`byte_offset` as `line_col_utf8`.
    pub fn line_col_utf16(&self, byte_offset: u32, source: &str) -> (u32, u32) {
        let line = self.line_of(byte_offset);
        let line_start = self.line_starts[line] as usize;
        let column = source[line_start..byte_offset as usize].encode_utf16().count() as u32 + 1;
        (line as u32 + 1, column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_first_column_is_one_one() {
        let index = LineIndex::new("let x = 1;");
        assert_eq!(index.line_col_utf8(0, "let x = 1;"), (1, 1));
    }

    #[test]
    fn finds_the_right_line_across_multiple_lines() {
        let source = "let a = 1;\nlet b = 2;\nlet c = 3;";
        let index = LineIndex::new(source);
        // 'b' in "let b = 2;" is at byte offset 15 (line 2, column 5).
        let b_offset = source.find("b = 2").unwrap() as u32;
        assert_eq!(index.line_col_utf8(b_offset, source), (2, 5));
    }

    #[test]
    fn codepoint_and_utf16_columns_agree_on_pure_ascii() {
        let source = "const value = 42;";
        let index = LineIndex::new(source);
        let offset = source.find("42").unwrap() as u32;
        assert_eq!(index.line_col_utf8(offset, source), index.line_col_utf16(offset, source));
    }

    #[test]
    fn codepoint_and_utf16_columns_diverge_after_an_astral_character() {
        // U+1F600 (😀) is 4 bytes in UTF-8, 1 Unicode codepoint, but 2
        // UTF-16 code units — exactly the divergence this module exists
        // to handle correctly instead of silently getting wrong.
        let source = "const s = \"😀\"; const after = 1;";
        let index = LineIndex::new(source);
        let after_offset = source.find("after").unwrap() as u32;

        let (line_utf8, col_utf8) = index.line_col_utf8(after_offset, source);
        let (line_utf16, col_utf16) = index.line_col_utf16(after_offset, source);

        assert_eq!(line_utf8, 1);
        assert_eq!(line_utf16, 1);
        // The UTF-16 column must be exactly one greater than the
        // codepoint column, because exactly one codepoint before this
        // point (😀) costs 2 UTF-16 units instead of 1.
        assert_eq!(col_utf16, col_utf8 + 1);
    }
}
