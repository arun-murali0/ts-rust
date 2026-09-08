pub struct LineIndex {
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

    fn line_of(&self, byte_offset: u32) -> usize {
        self.line_starts
            .partition_point(|&start| start <= byte_offset)
            - 1
    }

    pub fn line_col_utf8(&self, byte_offset: u32, source: &str) -> (u32, u32) {
        let line = self.line_of(byte_offset);
        let line_start = self.line_starts[line] as usize;
        let column = source[line_start..byte_offset as usize].chars().count() as u32 + 1;
        (line as u32 + 1, column)
    }

    pub fn line_col_utf16(&self, byte_offset: u32, source: &str) -> (u32, u32) {
        let line = self.line_of(byte_offset);
        let line_start = self.line_starts[line] as usize;
        let column = source[line_start..byte_offset as usize]
            .encode_utf16()
            .count() as u32
            + 1;
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

        let b_offset = source
            .find("b = 2")
            .map(|offset| offset as u32)
            .unwrap_or_default();
        assert_eq!(index.line_col_utf8(b_offset, source), (2, 5));
    }

    #[test]
    fn codepoint_and_utf16_columns_agree_on_pure_ascii() {
        let source = "const value = 42;";
        let index = LineIndex::new(source);
        let offset = source
            .find("42")
            .map(|offset| offset as u32)
            .unwrap_or_default();
        assert_eq!(
            index.line_col_utf8(offset, source),
            index.line_col_utf16(offset, source)
        );
    }

    #[test]
    fn codepoint_and_utf16_columns_diverge_after_an_astral_character() {
        let source = "const s = \"\u{1F600}\"; const after = 1;";
        let index = LineIndex::new(source);
        let after_offset = source
            .find("after")
            .map(|offset| offset as u32)
            .unwrap_or_default();

        let (line_utf8, col_utf8) = index.line_col_utf8(after_offset, source);
        let (line_utf16, col_utf16) = index.line_col_utf16(after_offset, source);

        assert_eq!(line_utf8, 1);
        assert_eq!(line_utf16, 1);

        assert_eq!(col_utf16, col_utf8 + 1);
    }
}
