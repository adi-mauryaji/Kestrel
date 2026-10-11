//! Minimal CSV writer for result files (Ch. 40): RFC 4180 quoting, `\n` line ends.

use std::borrow::Cow;
use std::io::{self, Write};

pub struct CsvWriter<W: Write> {
    out: W,
    width: usize,
}

impl<W: Write> CsvWriter<W> {
    /// Writes the header row. Every later row must have the same number of fields.
    pub fn new(mut out: W, header: &[&str]) -> io::Result<Self> {
        write_record(&mut out, header)?;
        Ok(Self {
            out,
            width: header.len(),
        })
    }

    /// A row with the wrong number of fields is an `InvalidInput` error and nothing is written.
    pub fn write_row<S: AsRef<str>>(&mut self, fields: &[S]) -> io::Result<()> {
        if fields.len() != self.width {
            let msg = format!("row has {} fields, header has {}", fields.len(), self.width);
            return Err(io::Error::new(io::ErrorKind::InvalidInput, msg));
        }
        write_record(&mut self.out, fields)
    }

    /// Flushes and returns the underlying writer.
    pub fn finish(mut self) -> io::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }
}

/// Builds the whole line first, so a row is written in one call.
fn write_record<S: AsRef<str>>(out: &mut impl Write, fields: &[S]) -> io::Result<()> {
    let line: Vec<Cow<str>> = fields.iter().map(|f| escape(f.as_ref())).collect();
    writeln!(out, "{}", line.join(","))
}

/// Quotes a field containing a comma, quote or line break, and doubles inner quotes.
fn escape(field: &str) -> Cow<'_, str> {
    if field.contains([',', '"', '\n', '\r']) {
        Cow::Owned(format!("\"{}\"", field.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(header: &[&str], rows: &[&[&str]]) -> String {
        let mut w = CsvWriter::new(Vec::new(), header).unwrap();
        for r in rows {
            w.write_row(r).unwrap();
        }
        String::from_utf8(w.finish().unwrap()).unwrap()
    }

    #[test]
    fn writes_header_then_rows() {
        assert_eq!(
            written(&["a", "b"], &[&["1", "2"], &["3", "4"]]),
            "a,b\n1,2\n3,4\n"
        );
    }

    #[test]
    fn quotes_commas_quotes_and_line_breaks() {
        let out = written(
            &["text"],
            &[
                &["kal, 5 baje"],
                &["say \"hi\""],
                &["two\nlines"],
                &["plain"],
            ],
        );
        assert_eq!(
            out,
            "text\n\"kal, 5 baje\"\n\"say \"\"hi\"\"\"\n\"two\nlines\"\nplain\n"
        );
    }

    #[test]
    fn wrong_width_row_is_rejected_and_not_written() {
        let mut w = CsvWriter::new(Vec::new(), &["a", "b"]).unwrap();
        let err = w.write_row(&["1", "2", "3"]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(String::from_utf8(w.finish().unwrap()).unwrap(), "a,b\n");
    }
}
