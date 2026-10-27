//! v3: a request for code (Kinds-v2's `code`, ADR-044, ADR-046). Gemma writes it with CODE_SYSTEM (the
//! prompt Kestrel-CodeTasks-v0 measured: experiments/v3/code_eval.py), the sandbox checks it (src/sandbox.rs):
//! the code is imported (it compiles and its top level runs), and only compiled when it waits for input. A
//! syntax error, a crash or a time limit goes back to Gemma with the code, at most `RETRIES` times; the
//! model's own asserts are never asked for. The user sees the code as it is written, then what the check
//! found, and a caveat.

use std::time::Duration;

use crate::kinds::{Kind, Writer};

/// Gemma's instructions for code (one text for the harness and the app).
pub const SYSTEM: &str =
    include_str!("../../../datasets/contracts/code_system.txt").trim_ascii_end();

pub const RETRIES: usize = 2;

pub const MAX_TOKENS: usize = 640;

/// Under every code answer.
pub const CAVEAT: &str = "(Written offline by a small model: try it before you rely on it.)";

const CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// The first fenced block that is not a tests / input block, or the whole text when there is none
/// (as code_eval.py's `blocks`).
pub fn code_block(text: &str) -> &str {
    let mut rest = text;
    while let Some(open) = rest.find("```") {
        let after = &rest[open + 3..];
        let Some(nl) = after.find('\n') else { break };
        let name = after[..nl].trim().to_lowercase();
        let body = &after[nl + 1..];
        let Some(close) = body.find("```") else { break };
        if !matches!(name.as_str(), "tests" | "test" | "input") {
            return &body[..close];
        }
        rest = &body[close + 3..];
    }
    if text.contains("```") { "" } else { text }
}

/// Code that waits for input: only compiled, since the sandbox has no input to give it.
pub fn reads_input(code: &str) -> bool {
    code.contains("input(") || code.contains("sys.stdin") || code.contains("fileinput")
}

/// What the sandbox's check found: Ok(what was checked), or the error to send back.
pub enum Checked {
    Passed(&'static str),
    Failed(String),
    Unavailable(String),
}

/// Imported without an error (its top level ran; a `__main__` block does not run on import).
pub const RAN: &str = "(I checked it in a safe sandbox: no errors.)";

/// It waits for input at its top level, so it was only compiled.
pub const WAITS: &str = "(I checked it in a safe sandbox: no errors in how it is written. I didn't run \
                         it, because it waits for you to type input.)";

/// The code is imported in the sandbox (it compiles and its top level runs); when that stops at an
/// EOFError (it reads input at its top level), or times out while reading input, it is only compiled.
/// The human's prime function (2026-10-27) read input only under `if __name__ == "__main__":` and was
/// still only compiled by the earlier rule (any `input(` in the code).
pub fn check(code: &str) -> Checked {
    let run =
        |main: &str| crate::sandbox::run_python(main, &[("solution.py", code)], "", CHECK_TIMEOUT);
    let compile = "import py_compile\npy_compile.compile('solution.py', doraise=True)\n";
    match run("import solution\n") {
        Err(e) => Checked::Unavailable(e.0),
        Ok(r) if r.ok => Checked::Passed(RAN),
        Ok(r) if r.stderr.contains("EOFError") || (r.timed_out && reads_input(code)) => {
            match run(compile) {
                Err(e) => Checked::Unavailable(e.0),
                Ok(c) if c.ok => Checked::Passed(WAITS),
                Ok(c) => Checked::Failed(c.stderr.trim().to_string()),
            }
        }
        Ok(r) if r.timed_out => Checked::Failed("timed out".into()),
        Ok(r) => Checked::Failed(r.stderr.trim().to_string()),
    }
}

/// The next attempt's message: the request, the code, and the end of the error (as code_eval.py).
pub fn feedback(request: &str, code: &str, error: &str) -> String {
    // The last 1500 bytes, cut at a character's start.
    let start = (error.len().saturating_sub(1500)..=error.len())
        .find(|&i| error.is_char_boundary(i))
        .unwrap_or(error.len());
    let tail = &error[start..];
    format!(
        "{request}\n\nYour code:\n```python\n{}\n```\n\nRunning it gave this error:\n{tail}\n\nFix the code. \
         Reply in the same form as before.",
        code.trim_end()
    )
}

/// What the check found, the length note when the last text stopped at its limit, the caveat.
fn closing(found: String, cut: bool) -> Vec<String> {
    let mut out = vec![found];
    if cut {
        out.push(crate::kinds::CUT.into());
    }
    out.push(CAVEAT.into());
    out
}

/// The answer to a code request: written (streamed when `stream` is given), checked, retried.
pub fn write(
    writer: &dyn Writer,
    request: &str,
    mut stream: Option<&mut dyn FnMut(&str)>,
) -> Vec<String> {
    let mut lines = Vec::new();
    let say = |text: &str, stream: &mut Option<&mut dyn FnMut(&str)>, lines: &mut Vec<String>| {
        match stream.as_mut() {
            Some(show) => show(text),
            None => lines.extend(text.lines().map(String::from)),
        }
    };
    let mut ask = request.to_string();
    for attempt in 0..=RETRIES {
        let mut shown = String::new();
        let written = writer.write(Kind::Code, &ask, &mut |p| {
            if let Some(show) = stream.as_mut() {
                shown.push_str(p);
                show(p);
            }
        });
        let text = match written {
            Ok(t) => t,
            Err(e) => {
                lines.push(format!("I couldn't write that: {e}"));
                return lines;
            }
        };
        match text.strip_prefix(shown.as_str()) {
            Some(rest) if stream.is_some() => {
                if !rest.is_empty() {
                    say(rest, &mut stream, &mut lines)
                }
            }
            _ => say(&text, &mut stream, &mut lines),
        }
        let code = code_block(&text);
        match check(code) {
            Checked::Passed(what) => {
                lines.extend(closing(what.to_string(), writer.cut()));
                return lines;
            }
            Checked::Unavailable(why) => {
                lines.extend(closing(format!("(Not checked: {why}.)"), writer.cut()));
                return lines;
            }
            Checked::Failed(err) if attempt < RETRIES => {
                let last = err.lines().last().unwrap_or("an error");
                say(
                    &format!("\n\n(It had an error in the sandbox check: {last}. Fixing it.)\n\n"),
                    &mut stream,
                    &mut lines,
                );
                ask = feedback(request, code, &err);
            }
            Checked::Failed(err) => {
                let last = err.lines().last().unwrap_or("an error");
                lines.extend(closing(
                    format!("(It still has an error in the sandbox check: {last}.)"),
                    writer.cut(),
                ));
                return lines;
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::KestrelError;
    use std::cell::RefCell;

    #[test]
    fn the_code_is_the_first_block_not_named_tests_or_input() {
        let text =
            "Here:\n```python\ndef f():\n    return 1\n```\n```tests\nassert f() == 1\n```\n";
        assert_eq!(code_block(text), "def f():\n    return 1\n");
        assert_eq!(code_block("```\nprint(1)\n```"), "print(1)\n");
        assert_eq!(code_block("print(1)"), "print(1)");
        assert_eq!(
            code_block("```input\n5\n```\n```python\nx = 1\n```"),
            "x = 1\n"
        );
    }

    #[test]
    fn code_that_reads_input_is_told_apart() {
        assert!(reads_input("n = int(input())"));
        assert!(reads_input("import sys\nfor l in sys.stdin: pass"));
        assert!(!reads_input("def f(x):\n    return x"));
    }

    #[test]
    fn the_prompt_is_the_contract_and_asks_for_no_asserts() {
        assert!(SYSTEM.starts_with("You write small Python 3 programs") && !SYSTEM.ends_with('\n'));
        assert!(!SYSTEM.contains("assert"));
    }

    #[test]
    fn feedback_shows_the_code_and_the_end_of_the_error() {
        let m = feedback(
            "write f",
            "def f():\n    pass\n",
            &("x".repeat(3000) + "\nNameError: y"),
        );
        assert!(m.starts_with("write f\n\nYour code:\n```python\ndef f():\n    pass\n```\n"));
        assert!(m.ends_with("Fix the code. Reply in the same form as before."));
        assert!(m.contains("NameError: y") && !m.contains(&"x".repeat(1600)));
        // A long Hindi error is cut at a character, not inside one.
        let m = feedback("q", "x", &"ा".repeat(1000));
        assert!(m.contains("ा"));
    }

    /// Answers each request with the next reply; records what it was asked.
    struct Replies(RefCell<Vec<&'static str>>, RefCell<Vec<String>>);

    impl Writer for Replies {
        fn write(
            &self,
            kind: Kind,
            request: &str,
            on_part: &mut dyn FnMut(&str),
        ) -> Result<String, KestrelError> {
            assert_eq!(kind, Kind::Code);
            self.1.borrow_mut().push(request.into());
            let r = self.0.borrow_mut().remove(0);
            on_part(r);
            Ok(r.into())
        }
    }

    #[test]
    #[ignore = "needs bubblewrap (Fedora: installed)"]
    fn a_broken_answer_goes_back_once_and_the_fixed_one_is_checked() {
        let w = Replies(
            RefCell::new(vec![
                "```python\ndef f(:\n```",
                "```python\ndef f():\n    return 1\n```",
            ]),
            RefCell::new(Vec::new()),
        );
        let mut parts = Vec::new();
        let lines = write(
            &w,
            "write f",
            Some(&mut |p: &str| parts.push(p.to_string())),
        );
        assert_eq!(w.1.borrow().len(), 2);
        assert!(
            w.1.borrow()[1].contains("SyntaxError"),
            "{:?}",
            w.1.borrow()
        );
        assert!(
            parts
                .iter()
                .any(|p| p.contains("It had an error in the sandbox check")),
            "{parts:?}"
        );
        assert_eq!(lines, [RAN, CAVEAT]);
    }

    #[test]
    #[ignore = "needs bubblewrap (Fedora: installed)"]
    fn input_only_under_the_main_guard_still_runs_the_check() {
        // The human's prime function, 2026-10-27: it was only compiled, "Not run, as it reads input".
        let prime = "```python\ndef is_prime(n):\n    return n > 1 and all(n % i for i in range(2, n))\n\n\
                     if __name__ == \"__main__\":\n    print(is_prime(int(input())))\n```";
        let w = Replies(RefCell::new(vec![prime]), RefCell::new(Vec::new()));
        let lines = write(&w, "prime", None);
        assert_eq!(lines[lines.len() - 2..], [RAN, CAVEAT]);
    }

    #[test]
    #[ignore = "needs bubblewrap (Fedora: installed)"]
    fn code_that_waits_for_input_is_only_compiled_and_two_failures_end_the_loop() {
        let w = Replies(
            RefCell::new(vec!["```python\nprint(int(input()) * 2)\n```"]),
            RefCell::new(Vec::new()),
        );
        let lines = write(&w, "double a number", None);
        assert_eq!(lines[lines.len() - 2], WAITS);
        let broken = "```python\nx = (\n```";
        let w = Replies(RefCell::new(vec![broken; 3]), RefCell::new(Vec::new()));
        let lines = write(&w, "x", None);
        assert_eq!(w.1.borrow().len(), 1 + RETRIES);
        assert!(
            lines[lines.len() - 2].starts_with("(It still has an error in the sandbox check:"),
            "{lines:?}"
        );
    }
}
