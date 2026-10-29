//! `kestrel calibrate`'s pass rates (v4 A2, blueprint §4.3): the writer on the dev splits of
//! Kestrel-NotesQA-v0 and Kestrel-CodeTasks-v0, through the service's own notes Q&A and code loop,
//! graded as experiments/v3/notes_qa.py and code_eval.py grade them.
//!
//! Notes: a question the notes answer is right when the answer holds every fact (any of its ways),
//! wrong when an answer lacks one, abstained otherwise; a question they do not answer is right
//! unless it gets an answer. Code: a function or fix answer is imported as solution.py and the
//! hidden asserts run against it; a script passes when, for each case's stdin, the last non-empty
//! line of its output holds the expected output as whole words. Explain requests are `chat` in the
//! app, not the code loop, so they are not here.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kestrel_capability::{CodeTasks, NotesQa, Tally};

/// One run's time limit in the sandbox (experiments/v3/sandbox.py's default).
const RUN_TIMEOUT: Duration = Duration::from_secs(15);

pub fn notes_dev(root: &Path) -> PathBuf {
    root.join("datasets/kestrel_notesqa_v0/splits/dev.jsonl")
}

/// The notes the questions are about (Kestrel-NotesQ-v0's fixtures, not the user's notes).
pub fn notes_dir(root: &Path) -> PathBuf {
    root.join("datasets/kestrel_notesq_v0/notes")
}

pub fn code_dev(root: &Path) -> PathBuf {
    root.join("datasets/kestrel_codetasks_v0/splits/dev.jsonl")
}

fn rows(path: &Path) -> Result<Vec<serde_json::Value>, Box<dyn Error>> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    (text.lines().filter(|l| !l.trim().is_empty()))
        .enumerate()
        .map(|(n, l)| {
            serde_json::from_str(l).map_err(|e| format!("{}:{}: {e}", path.display(), n + 1).into())
        })
        .collect()
}

fn text_of(v: &serde_json::Value, k: &str) -> Result<String, Box<dyn Error>> {
    Ok(v[k]
        .as_str()
        .ok_or_else(|| format!("{}: no {k}", v["id"]))?
        .to_string())
}

/// A NotesQA question; `notes` empty when the notes do not answer it.
#[derive(Debug)]
pub struct Question {
    pub id: String,
    pub text: String,
    pub notes: Vec<String>,
    /// Every fact a right answer holds, each in any of its ways.
    pub facts: Vec<Vec<String>>,
}

pub fn read_questions(path: &Path) -> Result<Vec<Question>, Box<dyn Error>> {
    rows(path)?
        .iter()
        .map(|v| {
            Ok(Question {
                id: text_of(v, "id")?,
                text: text_of(v, "text")?,
                notes: serde_json::from_value(v["notes"].clone())?,
                facts: serde_json::from_value(v["facts"].clone()).unwrap_or_default(),
            })
        })
        .collect()
}

pub fn has_facts(text: &str, facts: &[Vec<String>]) -> bool {
    let t = text.to_lowercase();
    facts
        .iter()
        .all(|ways| ways.iter().any(|w| t.contains(&w.to_lowercase())))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    Answered,
    /// "The notes do not say this": no note close enough (`gated`), or the writer said so.
    NotInNotes,
    /// An answer not grounded in the notes is not shown.
    Withheld,
}

pub fn seen(gated: bool, text: &str, ungrounded: bool) -> Seen {
    if gated || crate::notes_qa::not_in_notes(text) {
        Seen::NotInNotes
    } else if ungrounded {
        Seen::Withheld
    } else {
        Seen::Answered
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grade {
    Right,
    Wrong,
    Abstained,
}

pub fn grade(q: &Question, seen: Seen, text: &str) -> Grade {
    match (q.notes.is_empty(), seen) {
        (true, Seen::Answered) => Grade::Wrong,
        (true, _) => Grade::Right,
        (false, Seen::Answered) if has_facts(text, &q.facts) => Grade::Right,
        (false, Seen::Answered) => Grade::Wrong,
        (false, _) => Grade::Abstained,
    }
}

pub fn notes_tally(graded: &[(&Question, Grade)]) -> NotesQa {
    let tally = |answerable: bool| {
        let mut t = Tally::default();
        for (_, g) in graded
            .iter()
            .filter(|(q, _)| q.notes.is_empty() != answerable)
        {
            t.n += 1;
            t.right += usize::from(*g == Grade::Right);
            t.wrong += usize::from(*g == Grade::Wrong);
        }
        t
    };
    NotesQa {
        answerable: tally(true),
        unanswerable: tally(false),
    }
}

/// A code request and its hidden check.
#[derive(Debug)]
pub struct CodeTask {
    pub id: String,
    pub text: String,
    pub check: Check,
}

#[derive(Debug)]
pub enum Check {
    /// Asserts run with `from solution import *` (function, fix).
    Tests(String),
    /// (stdin, expected output) per case (script).
    Cases(Vec<(String, String)>),
}

/// The dev split's function, fix and script requests.
pub fn read_code_tasks(path: &Path) -> Result<Vec<CodeTask>, Box<dyn Error>> {
    let mut out = Vec::new();
    for v in rows(path)? {
        let check = match v["type"].as_str() {
            Some("function" | "fix") => Check::Tests(text_of(&v, "tests")?),
            Some("script") => {
                let cases = v["cases"].as_array().ok_or("a script without cases")?;
                Check::Cases(
                    cases
                        .iter()
                        .map(|c| Ok((text_of(c, "stdin")?, text_of(c, "stdout")?)))
                        .collect::<Result<_, Box<dyn Error>>>()?,
                )
            }
            Some("explain") => continue,
            other => return Err(format!("{}: type {other:?}", v["id"]).into()),
        };
        out.push(CodeTask {
            id: text_of(&v, "id")?,
            text: text_of(&v, "text")?,
            check,
        });
    }
    Ok(out)
}

fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The last non-empty line holds `expected` as whole words: a number may carry a unit ("98.6F")
/// but never more digits, a word may not run into another (code_eval.py's `answers`).
pub fn answers(stdout: &str, expected: &str) -> bool {
    let Some(last) = stdout.lines().rfind(|l| !l.trim().is_empty()) else {
        return norm(expected).is_empty();
    };
    let (exp, line) = (norm(expected), norm(last));
    if exp.is_empty() {
        return false;
    }
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let numeric_start = exp.starts_with(|c: char| c.is_numeric() || c == '-');
    let numeric_end = exp.ends_with(char::is_numeric);
    // Every place it starts, overlapping ones too (as re.search tries them).
    let mut starts =
        (0..line.len()).filter(|&i| line.is_char_boundary(i) && line[i..].starts_with(&exp));
    starts.any(|i| {
        let before = line[..i].chars().next_back();
        let mut after = line[i + exp.len()..].chars();
        let free_before = match before {
            None => true,
            Some(c) if numeric_start => !(c.is_numeric() || c == '.' || c == '-'),
            Some(c) => !word(c),
        };
        let free_after = match after.next() {
            None => true,
            Some(c) if numeric_end => {
                !(c.is_numeric() || (c == '.' && after.next().is_some_and(char::is_numeric)))
            }
            Some(c) => !word(c),
        };
        free_before && free_after
    })
}

/// Does `code` pass `task`'s hidden check? Err when the sandbox cannot run.
pub fn passes(task: &CodeTask, code: &str) -> Result<bool, crate::sandbox::Unavailable> {
    use crate::sandbox::run_python;
    match &task.check {
        Check::Tests(tests) => {
            let main = format!("from solution import *\n{tests}");
            Ok(run_python(&main, &[("solution.py", code)], "", RUN_TIMEOUT)?.ok)
        }
        Check::Cases(cases) => {
            for (stdin, expected) in cases {
                let r = run_python(code, &[], stdin, RUN_TIMEOUT)?;
                if !(r.ok && answers(&r.stdout, expected)) {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
}

/// The tally over (first answer passes, last answer passes) per task.
pub fn code_tally(results: &[(bool, bool)]) -> CodeTasks {
    CodeTasks {
        n: results.len(),
        pass1: results.iter().filter(|r| r.0).count(),
        pass_loop: results.iter().filter(|r| r.1).count(),
    }
}

/// One question the service's way: the notes closest by meaning (none below θ), then the writer
/// answers from them only; a failed answer shows nothing.
pub fn run_question(
    meaning: &mut dyn crate::meaning::Meaning,
    writer: &dyn crate::kinds::Writer,
    q: &Question,
) -> (Seen, Grade) {
    let notes = meaning.closest(&q.text, crate::notes_qa::N);
    let (text, ungrounded) = if notes.is_empty() {
        (String::new(), false)
    } else {
        let pairs: Vec<(&str, &str)> = (notes.iter())
            .map(|n| (n.name.as_str(), n.text.as_str()))
            .collect();
        match writer.answer(&q.text, &pairs) {
            Ok(a) => (a.text, a.ungrounded.is_some()),
            Err(_) => (String::new(), true),
        }
    };
    let s = seen(notes.is_empty(), &text, ungrounded);
    (s, grade(q, s, &text))
}

/// The writer's texts, in order, as it hands them over.
struct Recording<'a> {
    inner: &'a dyn crate::kinds::Writer,
    texts: std::cell::RefCell<Vec<String>>,
}

impl crate::kinds::Writer for Recording<'_> {
    fn write(
        &self,
        kind: crate::kinds::Kind,
        request: &str,
        on_part: &mut dyn FnMut(&str),
    ) -> Result<String, kestrel_core::KestrelError> {
        let text = self.inner.write(kind, request, on_part)?;
        self.texts.borrow_mut().push(text.clone());
        Ok(text)
    }

    fn cut(&self) -> bool {
        self.inner.cut()
    }
}

/// One code request through the service's loop (src/code.rs); (first answer passes, last passes).
pub fn run_code_task(
    writer: &dyn crate::kinds::Writer,
    task: &CodeTask,
) -> Result<(bool, bool), crate::sandbox::Unavailable> {
    let rec = Recording {
        inner: writer,
        texts: Default::default(),
    };
    crate::code::write(&rec, &task.text, None);
    let texts = rec.texts.into_inner();
    let (Some(first), Some(last)) = (texts.first(), texts.last()) else {
        return Ok((false, false)); // the writer failed: nothing to judge
    };
    let judge = |t: &str| passes(task, crate::code::code_block(t));
    Ok((judge(first)?, judge(last)?))
}

/// What the run found, in words (`code` None: no sandbox to judge in).
pub fn report(notes: &NotesQa, code: Option<&CodeTasks>) -> String {
    let (a, u) = (&notes.answerable, &notes.unanswerable);
    let code = match code {
        Some(c) => format!(
            "code: {} of {} pass with the first answer, {} after the sandbox loop.",
            c.pass1, c.n, c.pass_loop
        ),
        None => "code: not measured (no sandbox here).".into(),
    };
    format!(
        "notes questions: {} of {} answered right ({} wrong, {} not answered); {} of {} the notes do \
         not answer got no answer.\n{code}",
        a.right,
        a.n,
        a.wrong,
        a.abstained(),
        u.right,
        u.n
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn json(rel: &str) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(root().join(rel)).unwrap()).unwrap()
    }

    #[test]
    fn facts_hold_in_any_of_their_ways_and_any_case() {
        // experiments/v3/test_code_eval.py's case.
        let facts = vec![vec!["square".to_string()], vec!["even".to_string()]];
        assert!(has_facts(
            "It makes a list of the squares of the EVEN numbers below 10.",
            &facts
        ));
        assert!(!has_facts("It makes a list of squares.", &facts));
        let either = vec![vec!["40".to_string(), "forty".to_string()]];
        assert!(has_facts("It goes down to Forty percent.", &either));
    }

    #[test]
    fn a_script_s_last_line_holds_the_answer_as_whole_words() {
        // experiments/v3/test_code_eval.py's cases, on the output those programs print.
        assert!(answers("  Even \n", "even"));
        assert!(answers("Enter a whole number: 4 is even.\n", "even"));
        assert!(!answers("uneven\n", "even"), "whole words only");
        assert!(!answers("even\ndone\n", "even"), "the last line");
        assert!(
            answers("37.0C is 98.6F\n", "98.6"),
            "a number may carry a unit"
        );
        assert!(!answers("198.6\n", "98.6") && !answers("98.65\n", "98.6"));
        assert!(answers("Result: 9 -2\n", "9 -2") && !answers("max 9, min -2\n", "9 -2"));
        assert!(answers("-40.0\n", "-40.0") && !answers("--40.0\n", "-40.0"));
        assert!(
            answers("it is 98.6.\n", "98.6"),
            "a full stop after a number"
        );
        assert!(!answers("", "even") && !answers("\n  \n", "even"));
        assert!(answers("a\n\n  \n", "A"), "blank lines after the last line");
        assert!(
            !answers("9 99 9\n", "9 9") && answers("99 9 9\n", "9 9"),
            "overlapping"
        );
    }

    #[test]
    fn what_the_user_sees_is_an_answer_not_in_notes_or_nothing() {
        assert_eq!(seen(false, "Rent is 12000.", false), Seen::Answered);
        assert_eq!(
            seen(true, "Rent is 12000.", false),
            Seen::NotInNotes,
            "below theta"
        );
        assert_eq!(seen(false, "NOT IN NOTES.", false), Seen::NotInNotes);
        assert_eq!(
            seen(false, "Rent is 12500.", true),
            Seen::Withheld,
            "not grounded"
        );
    }

    #[test]
    fn notes_questions_are_graded_and_tallied_as_the_v3_dev_run_did() {
        let qs = read_questions(&notes_dev(&root())).unwrap();
        assert_eq!(qs.len(), 26);
        let log = json("experiments/logs/v3/notes/qa_dev.json");
        let theta = log["theta"].as_f64().unwrap();
        let mut graded = Vec::new();
        for (q, l) in qs.iter().zip(log["questions"].as_array().unwrap()) {
            assert_eq!(q.id, l["id"].as_str().unwrap());
            let text = l["answer"].as_str().unwrap();
            let gated = l["top"][0][1].as_f64().unwrap() < theta;
            let s = seen(gated, text, !l["ungrounded"].is_null());
            let g = grade(q, s, text);
            assert_eq!(
                format!("{g:?}").to_lowercase(),
                l["grade"].as_str().unwrap(),
                "{}",
                q.id
            );
            graded.push((q, g));
        }
        let t = notes_tally(&graded);
        let want = &log["at_theta"];
        assert_eq!(
            (
                t.answerable.right,
                t.answerable.wrong,
                t.answerable.abstained()
            ),
            (want["answerable"]["right"].as_u64().unwrap() as usize, 0, 0)
        );
        assert_eq!(
            (t.unanswerable.n, t.unanswerable.right, t.unanswerable.wrong),
            (6, 4, 2)
        );
        assert_eq!(t.answerable.n + t.unanswerable.n, 26);
    }

    #[test]
    fn the_code_tasks_are_the_dev_split_s_code_requests_with_their_hidden_checks() {
        let tasks = read_code_tasks(&code_dev(&root())).unwrap();
        assert_eq!(tasks.len(), 18, "explain requests are chat in the app");
        let script = tasks.iter().find(|t| t.id == "sc-celsius/en/0").unwrap();
        let Check::Cases(cases) = &script.check else {
            panic!("a script has cases")
        };
        assert_eq!(cases[0], ("37\n".to_string(), "98.6".to_string()));
        assert!(
            tasks
                .iter()
                .any(|t| matches!(&t.check, Check::Tests(a) if a.contains("assert")))
        );
    }

    #[test]
    #[ignore = "needs bubblewrap (Fedora: installed)"]
    fn the_hidden_checks_judge_the_v3_dev_run_s_answers_as_it_did() {
        let tasks = read_code_tasks(&code_dev(&root())).unwrap();
        let log = json("experiments/logs/v3/code/dev_gemma-4-e2b_adr044.json");
        let results = log["results"].as_array().unwrap();
        let (mut pass1, mut pass_loop) = (0, 0);
        for t in &tasks {
            let r = results.iter().find(|r| r["id"] == t.id.as_str()).unwrap();
            let attempts = r["attempts"].as_array().unwrap();
            let code = |a: &serde_json::Value| {
                crate::code::code_block(a["text"].as_str().unwrap()).to_string()
            };
            let first = passes(t, &code(&attempts[0])).unwrap();
            let last = passes(t, &code(attempts.last().unwrap())).unwrap();
            assert_eq!(first, r["pass1"].as_bool().unwrap(), "{} first", t.id);
            assert_eq!(last, r["pass_loop"].as_bool().unwrap(), "{} last", t.id);
            pass1 += usize::from(first);
            pass_loop += usize::from(last);
        }
        assert_eq!((pass1, pass_loop), (17, 17));
    }

    #[test]
    #[ignore = "needs bubblewrap (Fedora: installed)"]
    fn a_right_function_passes_and_a_wrong_one_or_a_wrong_script_fails() {
        let f = CodeTask {
            id: "f".into(),
            text: String::new(),
            check: Check::Tests("assert double(2) == 4\n".into()),
        };
        assert!(passes(&f, "def double(x):\n    return 2 * x\n").unwrap());
        assert!(!passes(&f, "def double(x):\n    return x + 1\n").unwrap());
        assert!(
            !passes(&f, "while True:\n    pass\n").unwrap(),
            "a time limit"
        );
        let s = CodeTask {
            id: "s".into(),
            text: String::new(),
            check: Check::Cases(vec![
                ("4\n".into(), "even".into()),
                ("3\n".into(), "odd".into()),
            ]),
        };
        let right = "n = int(input())\nprint('even' if n % 2 == 0 else 'odd')\n";
        assert!(passes(&s, right).unwrap());
        assert!(!passes(&s, "print('even')\n").unwrap(), "every case");
    }

    use crate::kinds::{Kind, Writer};
    use crate::notes_qa::NotesAnswer;
    use kestrel_core::KestrelError;
    use kestrel_tools::tools::Note;
    use std::cell::RefCell;

    /// Answers from notes with `answer`, or writes the next of `code` replies.
    struct Fake {
        answer: Result<(&'static str, bool), ()>,
        code: RefCell<Vec<&'static str>>,
        asked: RefCell<Vec<String>>,
    }

    impl Writer for Fake {
        fn write(
            &self,
            kind: Kind,
            _: &str,
            on_part: &mut dyn FnMut(&str),
        ) -> Result<String, KestrelError> {
            assert_eq!(kind, Kind::Code);
            let r = self.code.borrow_mut().remove(0);
            on_part(r);
            Ok(r.into())
        }

        fn answer(&self, q: &str, notes: &[(&str, &str)]) -> Result<NotesAnswer, KestrelError> {
            self.asked
                .borrow_mut()
                .push(format!("{q} | {}", notes.len()));
            match self.answer {
                Ok((text, ungrounded)) => Ok(NotesAnswer {
                    text: text.into(),
                    ungrounded: ungrounded.then(|| "a number not in the notes".into()),
                }),
                Err(()) => Err(KestrelError::Invalid("the worker stopped".into())),
            }
        }
    }

    fn fake(answer: Result<(&'static str, bool), ()>, code: Vec<&'static str>) -> Fake {
        Fake {
            answer,
            code: RefCell::new(code),
            asked: RefCell::new(vec![]),
        }
    }

    /// Notes by meaning that give `notes` (none: below θ).
    struct Closest(Vec<Note>);

    impl crate::meaning::Meaning for Closest {
        fn find(&mut self, _: &str) -> Vec<Note> {
            unreachable!("the Q&A asks for the closest")
        }
        fn k(&self) -> usize {
            2
        }
        fn closest(&mut self, _: &str, n: usize) -> Vec<Note> {
            self.0.iter().take(n).cloned().collect()
        }
    }

    fn question(notes: &[&str], facts: &[&str]) -> Question {
        Question {
            id: "q".into(),
            text: "how much is my rent?".into(),
            notes: notes.iter().map(|n| n.to_string()).collect(),
            facts: facts.iter().map(|f| vec![f.to_string()]).collect(),
        }
    }

    #[test]
    fn a_question_goes_the_service_s_way_closest_notes_then_the_writer() {
        let budget = Note {
            name: "budget.md".into(),
            text: "Rent 12000.".into(),
        };
        let q = question(&["budget.md"], &["12000"]);
        let w = fake(Ok(("Your rent is 12000.", false)), vec![]);
        let mut m = Closest(vec![budget.clone(), budget.clone(), budget.clone()]);
        assert_eq!(run_question(&mut m, &w, &q), (Seen::Answered, Grade::Right));
        assert_eq!(
            w.asked.borrow().as_slice(),
            ["how much is my rent? | 2"],
            "N notes"
        );
        // Below θ the writer is not asked.
        let w = fake(Ok(("Your rent is 12000.", false)), vec![]);
        assert_eq!(
            run_question(&mut Closest(vec![]), &w, &q),
            (Seen::NotInNotes, Grade::Abstained)
        );
        assert!(w.asked.borrow().is_empty());
        // Not grounded, or the writer failed: nothing shown.
        let w = fake(Ok(("Your rent is 12500.", true)), vec![]);
        assert_eq!(
            run_question(&mut Closest(vec![budget.clone()]), &w, &q).0,
            Seen::Withheld
        );
        let w = fake(Err(()), vec![]);
        assert_eq!(
            run_question(&mut Closest(vec![budget.clone()]), &w, &q).0,
            Seen::Withheld
        );
        // A question the notes do not answer is right when it gets no answer.
        let none = question(&[], &[]);
        let w = fake(Ok(("NOT IN NOTES.", false)), vec![]);
        assert_eq!(
            run_question(&mut Closest(vec![budget]), &w, &none),
            (Seen::NotInNotes, Grade::Right)
        );
    }

    #[test]
    #[ignore = "needs bubblewrap (Fedora: installed)"]
    fn a_code_task_goes_through_the_service_s_loop_and_its_first_and_last_answers_are_judged() {
        let t = CodeTask {
            id: "f".into(),
            text: "write double(x)".into(),
            check: Check::Tests("assert double(2) == 4\n".into()),
        };
        // A syntax error goes back once; the fixed answer passes.
        let w = fake(
            Ok(("", false)),
            vec![
                "```python\ndef double(x)\n    return 2 * x\n```",
                "```python\ndef double(x):\n    return 2 * x\n```",
            ],
        );
        assert_eq!(run_code_task(&w, &t).unwrap(), (false, true));
        assert!(w.code.borrow().is_empty(), "both replies used");
        // Right at once: one attempt.
        let w = fake(
            Ok(("", false)),
            vec!["```python\ndef double(x):\n    return x * 2\n```", "unused"],
        );
        assert_eq!(run_code_task(&w, &t).unwrap(), (true, true));
        assert_eq!(w.code.borrow().len(), 1);
    }

    #[test]
    fn the_pass_rates_read_in_words() {
        let notes = NotesQa {
            answerable: Tally {
                n: 20,
                right: 19,
                wrong: 0,
            },
            unanswerable: Tally {
                n: 6,
                right: 4,
                wrong: 2,
            },
        };
        let code = CodeTasks {
            n: 18,
            pass1: 16,
            pass_loop: 17,
        };
        assert_eq!(
            report(&notes, Some(&code)),
            "notes questions: 19 of 20 answered right (0 wrong, 1 not answered); \
             4 of 6 the notes do not answer got no answer.\n\
             code: 16 of 18 pass with the first answer, 17 after the sandbox loop."
        );
        assert!(report(&notes, None).ends_with("code: not measured (no sandbox here)."));
    }
}
