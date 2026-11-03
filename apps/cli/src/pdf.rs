//! PDFs in the notes folder (v4 B, blueprint §4.6; registry `pdf-extract`). The text comes out in a
//! process of its own, `kestrel pdf-text FILE`: a broken or hostile PDF can panic or run long, and
//! neither may reach the service. It is kept in memory only: these are people's documents, and their
//! text must not stay on disk after the file is gone. English, numbers and dates come out well; Hindi
//! loses joined letters (evaluated 2026-11-03), and a scan has no text at all.

use std::collections::HashMap;
use std::error::Error;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// How long one PDF may take.
const TIME_LIMIT: Duration = Duration::from_secs(20);

/// `kestrel pdf-text FILE`: the PDF's text on stdout.
pub fn child(args: &[String], out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    let [file] = args else {
        return Err("usage: kestrel pdf-text FILE.pdf".into());
    };
    let bytes = std::fs::read(file)?;
    let text = pdf_extract::extract_text_from_mem(&bytes)?;
    out.write_all(clean(&text).as_bytes())?;
    Ok(())
}

/// Readable lines: no control characters (joined Hindi letters come out as NUL), no empty runs.
pub fn clean(text: &str) -> String {
    // A form feed ends a page.
    let text = text.replace('\u{c}', "\n\n");
    let mut out = String::new();
    let mut blank = false;
    for line in text.lines() {
        let line: String = line
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect();
        let line = line.trim();
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if blank {
            out.push('\n');
            blank = false;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

/// The text kept per file with the size and time it was read at, so a changed file is read again.
type Kept = HashMap<PathBuf, (u64, Option<SystemTime>, Result<String, String>)>;

static KEPT: Mutex<Option<Kept>> = Mutex::new(None);

/// `path`'s text through `extract`, once per version of the file; failures are remembered too.
fn remembered(
    path: &Path,
    extract: &dyn Fn(&Path) -> Result<String, String>,
) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let (len, time) = (meta.len(), meta.modified().ok());
    let lock = || KEPT.lock().map_err(|_| "the PDF text was lost".to_string());
    if let Some((l, t, text)) = lock()?.get_or_insert_default().get(path)
        && (*l, *t) == (len, time)
    {
        return text.clone();
    }
    let text = extract(path);
    let kept = (len, time, text.clone());
    lock()?
        .get_or_insert_default()
        .insert(path.to_path_buf(), kept);
    text
}

/// A PDF's text, in its own process (`kestrel_tools::notes::read_pdfs_with`).
pub fn read(path: &Path) -> Result<String, String> {
    remembered(path, &extract)
}

fn extract(path: &Path) -> Result<String, String> {
    let program = crate::sandbox::kestrel().map_err(|e| e.to_string())?;
    let mut child = Command::new(program)
        .arg("pdf-text")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let read_all = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut all = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut all);
            }
            String::from_utf8_lossy(&all).into_owned()
        })
    };
    let out = read_all(child.stdout.take().map(|p| Box::new(p) as _));
    let err = read_all(child.stderr.take().map(|p| Box::new(p) as _));
    let start = Instant::now();
    while child.try_wait().map_err(|e| e.to_string())?.is_none() {
        if start.elapsed() >= TIME_LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("reading it took over {} s", TIME_LIMIT.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let (text, why) = (
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    );
    if status.success() {
        // A note holds no more than this, and memory should not either.
        let end = text.floor_char_boundary(kestrel_tools::notes::MAX_DOC_BYTES as usize);
        Ok(text[..end].to_string())
    } else if why.to_lowercase().contains("crypt") {
        Err("it is locked with a password".into())
    } else {
        Err("it could not be read".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn joined_letters_and_empty_runs_are_cleaned() {
        assert_eq!(
            clean(
                "\n\nPassport / पासपोट\u{0}\n\n\n\n  Date of Expiry  11/03/2029  \r\n\u{c}x\ty\n"
            ),
            "Passport / पासपोट\n\nDate of Expiry  11/03/2029\n\nx\ty"
        );
        assert_eq!(clean(" \n\u{0}\n "), "");
    }

    #[test]
    fn a_pdf_is_read_once_until_its_file_changes_and_a_failure_is_remembered() {
        let f = std::env::temp_dir().join(format!("kestrel_pdf_memo_{}.pdf", std::process::id()));
        std::fs::write(&f, "one").unwrap();
        let reads = Cell::new(0);
        let extract = |_: &Path| {
            reads.set(reads.get() + 1);
            if reads.get() == 2 {
                Err("locked".to_string())
            } else {
                Ok("text".to_string())
            }
        };
        assert_eq!(remembered(&f, &extract), Ok("text".into()));
        assert_eq!(remembered(&f, &extract), Ok("text".into()));
        assert_eq!(reads.get(), 1);
        std::fs::write(&f, "a longer file").unwrap();
        assert_eq!(remembered(&f, &extract), Err("locked".into()));
        assert_eq!(remembered(&f, &extract), Err("locked".into()));
        assert_eq!(reads.get(), 2);
        let _ = std::fs::remove_file(&f);
    }
}
