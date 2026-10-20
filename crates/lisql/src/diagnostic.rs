use std::ops::Range;

/// Half-open UTF-8 byte range in the original source.
pub type Span = Range<usize>;

/// A source error with a UTF-8 byte range. Render with the original source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Affected source bytes; rendering clamps out-of-bounds ranges safely.
    pub span: Span,
    /// Explanation of the error without filename or location formatting.
    pub message: String,
}

impl Diagnostic {
    pub(crate) fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }

    /// Render a one-based line/column and a marker on the first affected source
    /// line.
    pub fn render(&self, filename: &str, source: &str) -> String {
        // Public diagnostics can carry arbitrary ranges; clamp before slicing
        // UTF-8.
        let mut start = self.span.start.min(source.len());
        while !source.is_char_boundary(start) {
            start -= 1;
        }

        let prefix = &source[..start];
        let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
        let line_start = prefix.rfind('\n').map_or(0, |i| i + 1);
        let column = source[line_start..start].chars().count() + 1;
        let line_end = source[start..]
            .find('\n')
            .map_or(source.len(), |i| start + i);
        let snippet = &source[line_start..line_end];

        let mut end = self.span.end.min(line_end).max(start);
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        let width = source[start..end].chars().count().max(1);

        // Preserve tabs in the marker prefix so terminal tab stops stay
        // aligned.
        let padding: String = source[line_start..start]
            .chars()
            .map(|c| if c == '\t' { '\t' } else { ' ' })
            .collect();

        format!(
            "{filename}:{line}:{column}: error: {}\n{line} | {snippet}\n{} | {padding}{}",
            self.message,
            " ".repeat(line.to_string().len()),
            "^".repeat(width)
        )
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Diagnostic {}
