//! Romanized Hinglish as people type it (ADR-034, app-level; the research rules, templates and
//! datasets keep their own spellings). `normalize` turns chat spellings into the ones Kestrel's
//! rules know ("krna" -> "karna", "kl" -> "kal", "mujhai" -> "mujhe") for every request; only
//! lowercase words, and none that is also a common English word or likely a name ("aj", "pr" stay).
//! `task_request` takes the reminder wording out of a task request wherever it stands ("mujhe yaad
//! dila dena ki ...", "... karna hai"), so the template sees the title and the date: the template
//! only knows "yaad dilana" / "yaad dila do" at the end.

/// Chat spelling -> the spelling the rules know.
const SPELLINGS: &[(&str, &str)] = &[
    ("kr", "kar"),
    ("krna", "karna"),
    ("krne", "karne"),
    ("krni", "karni"),
    ("kro", "karo"),
    ("krdo", "kar do"),
    ("kardo", "kar do"),
    ("krde", "kar de"),
    ("krlo", "kar lo"),
    ("karlo", "kar lo"),
    ("krwa", "karwa"),
    ("kia", "kiya"),
    ("kl", "kal"),
    ("prso", "parso"),
    ("parsoo", "parso"),
    ("mujhai", "mujhe"),
    ("mujhay", "mujhe"),
    ("mujhy", "mujhe"),
    ("mujhey", "mujhe"),
    ("muje", "mujhe"),
    ("mjhe", "mujhe"),
    ("yad", "yaad"),
    ("dilado", "dila do"),
    ("diladena", "dila dena"),
    ("dilaana", "dilana"),
    ("h", "hai"),
    ("hy", "hai"),
    ("gya", "gaya"),
    ("gyi", "gayi"),
    ("gye", "gaye"),
    ("hogya", "ho gaya"),
    ("hogaya", "ho gaya"),
    ("hogyi", "ho gayi"),
    ("hogye", "ho gaye"),
    ("btao", "batao"),
    ("bta", "bata"),
    ("tk", "tak"),
    ("nhi", "nahi"),
    ("rkho", "rakho"),
    ("rkh", "rakh"),
    ("sham", "shaam"),
    ("bje", "baje"),
];

/// Ways of asking to be reminded, after the template's own spellings.
const REMIND: &[&[&str]] = &[
    &["yaad", "dila", "dena"],
    &["yaad", "dila", "do"],
    &["yaad", "dila", "dijiye"],
    &["yaad", "dila", "de"],
    &["yaad", "dilana"],
    &["yaad", "dilao"],
    &["yaad", "karwa", "dena"],
    &["yaad", "karwa", "do"],
    &["remind", "kar", "dena"],
    &["remind", "kar", "do"],
    &["remind", "kar", "dijiye"],
    &["remind", "karna"],
    &["remind", "karo"],
];
/// "mujhe yaad dila dena": who is to be reminded.
const ME: &[&str] = &["mujhe", "mujhko", "hume", "humein", "hamein"];

/// A word without the punctuation around it.
fn core(w: &str) -> &str {
    w.trim_matches(|c: char| !c.is_alphanumeric())
}

/// Chat spellings in `text` replaced by the ones the rules know; everything else as it was.
pub fn normalize(text: &str) -> String {
    text.split_whitespace()
        .map(|w| {
            let c = core(w);
            match SPELLINGS.iter().find(|(typed, _)| *typed == c) {
                // Keep the punctuation on either side ("kl!" -> "kal!").
                Some((_, known)) => {
                    let start = w.find(c).unwrap_or(0);
                    format!("{}{known}{}", &w[..start], &w[start + c.len()..])
                }
                None => w.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A task request without its reminder wording: "mujhe yaad dila dena dataset check karna hai kal"
/// -> "dataset check karna kal".
pub fn task_request(text: &str) -> String {
    let mut w: Vec<&str> = text.split_whitespace().collect();
    let low = |w: &[&str], i: usize| w.get(i).map(|x| core(x).to_lowercase());
    let found = (0..w.len()).find_map(|i| {
        REMIND
            .iter()
            .find(|p| (0..p.len()).all(|k| low(&w, i + k).as_deref() == Some(p[k])))
            .map(|p| (i, p.len()))
    });
    if let Some((i, n)) = found {
        w.drain(i..i + n);
        // "... yaad dila dena ki X": the "that" goes with it, and so does "mujhe" before it.
        if low(&w, i).as_deref() == Some("ki") {
            w.remove(i);
        }
        if i > 0 && ME.contains(&low(&w, i - 1).unwrap_or_default().as_str()) {
            w.remove(i - 1);
        }
        if low(&w, 0).is_some_and(|x| ME.contains(&x.as_str())) {
            w.remove(0);
        }
    }
    // "karna hai" -> "karna": the title is the thing to do.
    let mut k = 1;
    while k < w.len() {
        let before = low(&w, k - 1).unwrap_or_default();
        if before.len() > 2
            && before.ends_with("na")
            && matches!(low(&w, k).as_deref(), Some("hai" | "hain"))
        {
            w.remove(k);
        } else {
            k += 1;
        }
    }
    w.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_spellings_become_the_ones_the_rules_know() {
        let cases = [
            (
                "mujhai yaad dila dena dataset check krna hai kl",
                "mujhe yaad dila dena dataset check karna hai kal",
            ),
            ("task 3 ho gya", "task 3 ho gaya"),
            ("task 2 hogya", "task 2 ho gaya"),
            ("task 5 kl tk kr do", "task 5 kal tak kar do"),
            ("krna hai, kl!", "karna hai, kal!"),
            ("muje yad dilana", "mujhe yaad dilana"),
            ("mere tasks btao", "mere tasks batao"),
            ("parso bill bharna h", "parso bill bharna hai"),
        ];
        for (typed, known) in cases {
            assert_eq!(normalize(typed), known, "{typed}");
        }
    }

    #[test]
    fn english_names_and_capitals_stay() {
        for text in [
            "call AJ tomorrow",
            "call aj tomorrow",
            "fly to KL on friday",
            "review the pr",
            "remind me to call asha at 5 pm",
            "search my notes for budget",
        ] {
            assert_eq!(normalize(text), text);
        }
    }

    #[test]
    fn the_reminder_wording_leaves_the_task_request() {
        let cases = [
            (
                "mujhe yaad dila dena dataset check karna hai kal",
                "dataset check karna kal",
            ),
            (
                "kal mujhe yaad dila dena ki dataset check karna hai",
                "kal dataset check karna",
            ),
            (
                "mujhe kal dataset check karna yaad dilana",
                "kal dataset check karna",
            ),
            (
                "dataset check karna hai kal yaad dila dena",
                "dataset check karna kal",
            ),
            (
                "kal asha ko call karna yaad dila do",
                "kal asha ko call karna",
            ),
            ("mujhe remind kar dena ki bill bharna hai", "bill bharna"),
        ];
        for (said, request) in cases {
            assert_eq!(task_request(said), request, "{said}");
        }
        for text in [
            "remind me to call asha tomorrow",
            "maa ke liye gift lene ka task add karo",
            "yaad se doodh lana",
        ] {
            assert_eq!(task_request(text), text);
        }
    }
}
