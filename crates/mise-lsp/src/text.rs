//! Conversions between byte offsets and LSP positions (UTF-16 code units).

pub(crate) struct LineIndex<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Self { text, starts }
    }

    /// The `(line, character)` of a byte offset.
    pub(crate) fn position(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.text.len());
        let line = self.starts.partition_point(|start| *start <= offset) - 1;
        let start = self.starts[line];
        let character = self
            .text
            .get(start..offset)
            .map_or(0, |s| s.encode_utf16().count());
        (line, character)
    }

    /// The byte offset of a `(line, character)`, clamped to the line and the text.
    pub(crate) fn offset(&self, line: usize, character: usize) -> usize {
        let Some(&start) = self.starts.get(line) else {
            return self.text.len();
        };
        let end = self
            .starts
            .get(line + 1)
            .map_or(self.text.len(), |next| next - 1);
        let mut units = 0;
        for (i, c) in self.text[start..end].char_indices() {
            if units >= character {
                return start + i;
            }
            units += c.len_utf16();
        }
        end
    }

    /// The text of a line without its newline.
    pub(crate) fn line(&self, line: usize) -> &'a str {
        let Some(&start) = self.starts.get(line) else {
            return "";
        };
        let end = self
            .starts
            .get(line + 1)
            .map_or(self.text.len(), |next| next - 1);
        self.text[start..end].trim_end_matches('\r')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_count_utf16_units() {
        let text = "a = \"é😀\"\nb = 1\n";
        let index = LineIndex::new(text);
        let after_emoji = text.find('"').unwrap() + 1 + "é😀".len();
        assert_eq!(index.position(after_emoji), (0, 4 + 1 + 1 + 2));
        assert_eq!(index.offset(0, 4 + 1 + 1 + 2), after_emoji);
        assert_eq!(index.position(text.find('b').unwrap()), (1, 0));
        assert_eq!(index.offset(1, 99), text.find("1\n").unwrap() + 1);
        assert_eq!(index.line(1), "b = 1");
    }
}
