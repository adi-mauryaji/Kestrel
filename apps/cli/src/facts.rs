//! What the person says about themselves (v4 B): "my sister's name is Meera", "mera favourite color
//! neela hai", "remember that I work at Infosys". Kept as they said it, in the memory file, and given
//! back word for word when asked ("what is my sister's name"): rules, no model, so a fact cannot be
//! changed on the way. A chat sees them too (src/conversation.rs). What names a password, PIN or OTP
//! is refused. Requests with a time or an action stay what they were: a task, a mail, a question for
//! the writer.

use kestrel_core::KestrelError;
use kestrel_memory::{Filter, Item, Kind, Memory};
use serde_json::{Value, json};

/// Longest memory key made here.
const KEY_CHARS: usize = 48;
/// Most facts given back at once.
const MOST: usize = 3;

/// What a request says about the person, if it does.
#[derive(Debug, Clone, PartialEq)]
pub enum Said {
    /// To keep: `key` is the subject (a new value replaces the old), `sentence` as they said it.
    Fact { key: String, sentence: String },
    /// A question that may be about them; `name`: asked about someone named, not "my ...".
    Question { name: Option<String> },
}

/// What `text` says about the person.
pub fn parse(text: &str) -> Option<Said> {
    let said = text.trim().trim_end_matches(['.', '!', '?']).trim();
    let lower = said.to_lowercase();
    // Told to keep it, whatever it says.
    for prefix in [
        "remember that ",
        "yaad rakhna ki ",
        "yaad rakho ki ",
        "yaad rakhiye ki ",
        "remember ",
    ] {
        if lower.starts_with(prefix) {
            let rest = said[prefix.len()..].trim();
            if rest.is_empty() || lower.starts_with("remember to ") {
                return None;
            }
            return Some(Said::Fact {
                key: key_of(rest),
                sentence: rest.to_string(),
            });
        }
    }
    let words: Vec<&str> = lower.split_whitespace().collect();
    let any = |c: &[&str]| words.iter().any(|w| c.contains(w));
    let first = *words.first()?;
    let mine = any(&["my", "mera", "meri", "mere", "mujhe"]);
    let asks_en = [
        "what", "what's", "whats", "who", "who's", "when", "where", "which",
    ]
    .contains(&first);
    let asks_hi = any(&["kya", "kaun", "kahan", "kaha", "kab", "kitna", "kitni"]);
    if lower.starts_with("do you remember") || (mine && (asks_en || asks_hi)) {
        return Some(Said::Question { name: None });
    }
    if ["where", "who", "who's"].contains(&first) {
        let name = said
            .split_whitespace()
            .skip(1)
            .find(|w| w.starts_with(char::is_uppercase))?;
        return Some(Said::Question {
            name: Some(name.to_lowercase()),
        });
    }
    if asks_hi || asks_en {
        return None;
    }
    // A time makes it a task; an action, a request for the writer.
    let not_a_fact = [
        "kal",
        "aaj",
        "parso",
        "baje",
        "tomorrow",
        "today",
        "tonight",
        "am",
        "pm",
        "morning",
        "evening",
        "subah",
        "shaam",
        "raat",
        "fix",
        "help",
        "please",
        "rewrite",
        "write",
        "translate",
        "summarize",
        "task",
        "tasks",
        "note",
        "notes",
        "mail",
        "email",
        "error",
        "bug",
        "karna",
        "karo",
        "remind",
        "yaad",
        "dena",
        "add",
        "delete",
        "rename",
    ];
    if words.len() > 14 || words.iter().any(|w| not_a_fact.contains(&word(w))) {
        return None;
    }
    let fact = |key: String| {
        Some(Said::Fact {
            key,
            sentence: said.to_string(),
        })
    };
    if first == "my" {
        let rest = &lower[3..];
        let at = [" is ", " are "]
            .iter()
            .filter_map(|v| rest.find(v))
            .min()?;
        return fact(key_of(&rest[..at]));
    }
    if first == "i" {
        return match words.get(1..3)? {
            ["live", "in" | "at"] => fact("live".into()),
            ["work", "at" | "in" | "for"] => fact("work".into()),
            ["study", "at" | "in"] => fact("study".into()),
            _ => None,
        };
    }
    // "mujhe chai pasand hai": what someone likes, under what it is, so a change of mind replaces it.
    if first == "mujhe" {
        let subject = LIKES.iter().find_map(|like| {
            let n = words.len().checked_sub(like.len())?;
            (n > 1 && words[n..] == **like).then(|| words[1..n].join(" "))
        })?;
        return fact(key_of(&subject));
    }
    let hindi = ["mera", "meri", "mere"].contains(&first)
        && ["hai", "hain", "hoon"].contains(&words[words.len() - 1])
        && words.len() >= 4;
    if hindi {
        return fact(key_of(&words[..words.len() - 2].join(" ")));
    }
    None
}

/// How "mujhe X ..." says someone likes it or does not.
const LIKES: [&[&str]; 8] = [
    &["pasand", "nahi", "hai"],
    &["pasand", "nahi"],
    &["pasand", "hai"],
    &["pasand", "hain"],
    &["acha", "lagta", "hai"],
    &["accha", "lagta", "hai"],
    &["achi", "lagti", "hai"],
    &["acchi", "lagti", "hai"],
];

/// The same thing in Hinglish and in English: a fact kept in one is found when asked in the other.
const SAME: [(&str, &str); 17] = [
    ("behen", "sister"),
    ("bahen", "sister"),
    ("bhai", "brother"),
    ("maa", "mother"),
    ("mummy", "mother"),
    ("papa", "father"),
    ("pita", "father"),
    ("dost", "friend"),
    ("biwi", "wife"),
    ("pati", "husband"),
    ("beta", "son"),
    ("beti", "daughter"),
    ("naam", "name"),
    ("shehar", "city"),
    ("sheher", "city"),
    ("ghar", "home"),
    ("kaam", "work"),
];

/// "mera naam kya hai", "what is my name": their own name, which the page greets them by.
pub fn asks_own_name(text: &str) -> bool {
    let said = text
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .trim()
        .to_lowercase();
    [
        "mera naam kya hai",
        "mera naam batao",
        "mera naam kya he",
        "what is my name",
        "what's my name",
        "whats my name",
        "do you know my name",
    ]
    .contains(&said.as_str())
}

/// A word without what clings to it: "it," is "it".
fn word(w: &str) -> &str {
    w.trim_matches(|c: char| !c.is_alphanumeric())
}

/// A memory key from words: "sister's name" is sister_s_name.
fn key_of(subject: &str) -> String {
    let lower = subject.to_lowercase();
    let parts: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect();
    let key: String = parts.join("_").chars().take(KEY_CHARS).collect();
    if key.starts_with(|c: char| c.is_ascii_lowercase()) {
        key
    } else {
        format!("fact_{key}")
    }
}

/// Facts among `items`, each with how many words it shares with `text`, best first.
fn sharing<'a>(items: &'a [Item], text: &str) -> Vec<(usize, &'a Item)> {
    let mut wanted = crate::conversation::words(text);
    for (hi, en) in SAME {
        if wanted.contains(hi) {
            wanted.insert(en.to_string());
        } else if wanted.contains(en) {
            wanted.insert(hi.to_string());
        }
    }
    let mut found: Vec<(usize, &Item)> = items
        .iter()
        .filter(|i| i.kind == Kind::Fact && i.approved)
        .map(|i| {
            (
                crate::conversation::words(&i.value)
                    .intersection(&wanted)
                    .count(),
                i,
            )
        })
        .filter(|(n, _)| *n > 0)
        .collect();
    found.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    found
}

/// The kept facts that answer `question`, word for word; None when none does, and the request goes
/// on as if never asked here.
pub fn answer(items: &[Item], question: &str, name: Option<&str>) -> Option<Vec<String>> {
    if asks_own_name(question) {
        let own = items
            .iter()
            .find(|i| i.kind == Kind::Fact && i.approved && i.key == "name")?;
        return Some(vec![format!("You told me: {}", own.value)]);
    }
    let found = sharing(items, question);
    let best = found.first()?.0;
    let lines: Vec<String> = found
        .iter()
        .filter(|(n, _)| *n == best)
        .filter(|(_, i)| name.is_none_or(|w| crate::conversation::words(&i.value).contains(w)))
        .take(MOST)
        .map(|(_, i)| format!("You told me: {}", i.value))
        .collect();
    (!lines.is_empty()).then_some(lines)
}

/// The kept facts that share words with `request`, for a chat.
pub fn related(items: &[Item], request: &str) -> Vec<String> {
    sharing(items, request)
        .into_iter()
        .take(MOST)
        .map(|(_, i)| i.value.clone())
        .collect()
}

/// What `GET /api/remembered` answers: the facts kept, and how much conversation.
pub fn page_json(memory: &Memory) -> Result<Value, KestrelError> {
    let facts: Vec<Value> = memory
        .items(Filter::Approved)?
        .iter()
        .filter(|i| i.kind == Kind::Fact)
        .map(|i| json!({"id": i.id, "text": i.value}))
        .collect();
    let (conversations, turns) = memory.conversation_counts()?;
    Ok(json!({"facts": facts, "conversations": conversations, "turns": turns}))
}

/// `POST /api/remembered`: forgets one fact or every conversation; what is kept after, or why not.
pub fn change(memory: &mut Memory, change: &Value, now: i64) -> Result<Value, String> {
    let text = |e: KestrelError| e.to_string();
    if change["forget_conversations"] == true {
        memory.forget_conversations().map_err(text)?;
    } else if let Some(id) = change["forget"].as_i64() {
        let items = memory.items(Filter::All).map_err(text)?;
        if !items.iter().any(|i| i.id == id && i.kind == Kind::Fact) {
            return Err(format!("no fact #{id}"));
        }
        memory.delete(id, now).map_err(text)?;
    } else {
        return Err("send {\"forget\": id} or {\"forget_conversations\": true}".into());
    }
    page_json(memory).map_err(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_memory::Source;

    #[test]
    fn hinglish_words_find_what_was_said_in_english_and_back() {
        let kept = [
            item(
                "sister_s_name",
                "my sister's name is Meera",
                Kind::Fact,
                true,
            ),
            item(
                "meri_maa_ka_naam",
                "meri maa ka naam Sunita hai",
                Kind::Fact,
                true,
            ),
        ];
        let got = answer(&kept, "meri behen ka naam kya hai", None).unwrap();
        assert_eq!(got, ["You told me: my sister's name is Meera"]);
        let got = answer(&kept, "what is my mother's name", None).unwrap();
        assert_eq!(got, ["You told me: meri maa ka naam Sunita hai"]);
    }

    #[test]
    fn asking_ones_own_name_finds_only_ones_own_name() {
        let sister = item(
            "sister_s_name",
            "my sister's name is Meera",
            Kind::Fact,
            true,
        );
        assert_eq!(
            answer(std::slice::from_ref(&sister), "mera naam kya hai", None),
            None
        );
        assert_eq!(
            answer(std::slice::from_ref(&sister), "what is my name", None),
            None
        );
        let mine = item("name", "my name is Ram", Kind::Fact, true);
        let got = answer(&[sister, mine], "mera naam kya hai", None).unwrap();
        assert_eq!(got, ["You told me: my name is Ram"]);
    }

    #[test]
    fn what_someone_likes_in_hinglish_is_kept() {
        assert_eq!(
            fact("mujhe chai pasand hai"),
            ("chai".to_string(), "mujhe chai pasand hai".to_string())
        );
        assert_eq!(
            fact("mujhe chai pasand nahi hai").0,
            "chai",
            "a change of mind replaces it"
        );
        assert_eq!(fact("mujhe garam chai acchi lagti hai").0, "garam_chai");
    }

    #[test]
    fn asking_ones_own_name_is_told_apart() {
        for text in [
            "mera naam kya hai",
            "what is my name",
            "What's my name?",
            "mera naam batao",
        ] {
            assert!(asks_own_name(text), "{text}");
        }
        for text in [
            "what is my sister's name",
            "meri behen ka naam kya hai",
            "name a color",
        ] {
            assert!(!asks_own_name(text), "{text}");
        }
    }

    fn fact(text: &str) -> (String, String) {
        match parse(text) {
            Some(Said::Fact { key, sentence }) => (key, sentence),
            other => panic!("{text}: {other:?}"),
        }
    }

    #[test]
    fn what_the_person_says_about_themselves_is_a_fact_under_its_subject() {
        for (text, key, sentence) in [
            (
                "my sister's name is Meera",
                "sister_s_name",
                "my sister's name is Meera",
            ),
            (
                "My favourite color is blue.",
                "favourite_color",
                "My favourite color is blue",
            ),
            (
                "my kids are Asha and Ravi",
                "kids",
                "my kids are Asha and Ravi",
            ),
            ("I live in Pune", "live", "I live in Pune"),
            ("i work at Infosys", "work", "i work at Infosys"),
            (
                "mera favourite color neela hai",
                "mera_favourite_color",
                "mera favourite color neela hai",
            ),
            (
                "meri behen ka naam Meera hai",
                "meri_behen_ka_naam",
                "meri behen ka naam Meera hai",
            ),
            (
                "remember that my blood group is O+",
                "my_blood_group_is_o",
                "my blood group is O+",
            ),
            (
                "Remember my wifi router is in the hall",
                "my_wifi_router_is_in_the_hall",
                "my wifi router is in the hall",
            ),
            (
                "yaad rakhna ki meri car ka number 1234 hai",
                "meri_car_ka_number_1234_hai",
                "meri car ka number 1234 hai",
            ),
        ] {
            assert_eq!(
                fact(text),
                (key.to_string(), sentence.to_string()),
                "{text}"
            );
        }
    }

    #[test]
    fn a_time_an_action_or_a_question_is_not_a_fact() {
        for text in [
            "my appointment is at 5 pm tomorrow",
            "mera kal doctor ka appointment hai",
            "remember to call Ravi",
            "my code is not working, fix it",
            "my mail is too long, please rewrite it",
            "mera laptop kahan hai",
            "mera naam kya hai",
            "mujhe 5 baje mummy ko call karna hai",
            "kal subah 9 baje doctor ko call karna yaad dila dena",
            "hello how are you",
            "why is the sky blue",
            "add task buy milk",
            "mere tasks btao",
        ] {
            assert!(
                !matches!(parse(text), Some(Said::Fact { .. })),
                "{text}: {:?}",
                parse(text)
            );
        }
    }

    #[test]
    fn questions_about_the_person_or_someone_named_are_told_from_the_rest() {
        for text in [
            "what is my sister's name",
            "what's my favourite color",
            "who is my doctor",
            "mera favourite color kya hai",
            "meri behen ka naam kya hai",
            "do you remember my blood group",
        ] {
            assert_eq!(parse(text), Some(Said::Question { name: None }), "{text}");
        }
        assert_eq!(
            parse("where does Meera live"),
            Some(Said::Question {
                name: Some("meera".into())
            })
        );
        assert_eq!(
            parse("who is Ravi?"),
            Some(Said::Question {
                name: Some("ravi".into())
            })
        );
        for text in [
            "what is recursion",
            "why is the sky blue",
            "write a mail to Ravi",
            "where is the file",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
    }

    fn item(key: &str, value: &str, kind: Kind, approved: bool) -> Item {
        Item {
            id: 1,
            kind,
            key: key.into(),
            value: value.into(),
            exception: None,
            source: Source::UserExplicit,
            importance: 3,
            approved,
            created_at: 0,
            updated_at: 0,
            expires_at: None,
        }
    }

    fn kept() -> Vec<Item> {
        vec![
            item(
                "sister_s_name",
                "my sister's name is Meera and she lives in Pune",
                Kind::Fact,
                true,
            ),
            item("name", "my name is Aditya", Kind::Fact, true),
            item(
                "mera_favourite_color",
                "mera favourite color neela hai",
                Kind::Fact,
                true,
            ),
            item("answer_style", "short answers", Kind::Preference, true),
            item("guess", "my sister likes tea", Kind::Fact, false),
        ]
    }

    #[test]
    fn a_question_gets_the_best_kept_facts_word_for_word() {
        let say = answer(&kept(), "what is my sister's name", None).unwrap();
        assert_eq!(
            say,
            ["You told me: my sister's name is Meera and she lives in Pune"]
        );
        let say = answer(&kept(), "mera favourite color kya hai", None).unwrap();
        assert_eq!(say, ["You told me: mera favourite color neela hai"]);
        let say = answer(&kept(), "where does Meera live", Some("meera")).unwrap();
        assert!(say[0].contains("Pune"), "{say:?}");
        assert_eq!(answer(&kept(), "what is my shoe size", None), None);
        assert_eq!(answer(&kept(), "who is Ravi", Some("ravi")), None);
        assert_eq!(
            answer(&kept(), "what is my answer style", None),
            None,
            "facts only"
        );
    }

    #[test]
    fn a_chat_sees_the_facts_that_share_its_words() {
        let r = related(&kept(), "write a birthday wish for my sister");
        assert_eq!(r, ["my sister's name is Meera and she lives in Pune"]);
        assert!(related(&kept(), "why is the sky blue").is_empty());
    }

    fn with_a_fact() -> Memory {
        let mut m = Memory::in_memory().unwrap();
        let draft = kestrel_memory::Draft::new(Kind::Fact, "name", "my name is Aditya");
        m.propose(&draft, Source::UserExplicit, 1).unwrap();
        let draft = kestrel_memory::Draft::new(Kind::Preference, "style", "short answers");
        m.propose(&draft, Source::UserExplicit, 1).unwrap();
        let c = m.current_conversation(1, 50).unwrap();
        m.add_turn(c, kestrel_memory::Role::Person, "hi", 1)
            .unwrap();
        m
    }

    #[test]
    fn the_page_sees_what_is_kept_and_can_forget_it() {
        let mut m = with_a_fact();
        let v = page_json(&m).unwrap();
        assert_eq!(v["facts"][0]["text"], "my name is Aditya");
        assert_eq!(
            v["facts"].as_array().unwrap().len(),
            1,
            "facts, not preferences"
        );
        assert_eq!(
            (v["conversations"].as_u64(), v["turns"].as_u64()),
            (Some(1), Some(1))
        );

        let id = v["facts"][0]["id"].clone();
        let v = change(&mut m, &json!({"forget": id}), 2).unwrap();
        assert!(v["facts"].as_array().unwrap().is_empty());
        let v = change(&mut m, &json!({"forget_conversations": true}), 3).unwrap();
        assert_eq!(v["turns"].as_u64(), Some(0));

        assert!(change(&mut m, &json!({"forget": 999}), 4).is_err());
        assert!(change(&mut m, &json!({"name": "x"}), 4).is_err());
        let pref = m.items(Filter::All).unwrap()[0].id;
        assert!(
            change(&mut m, &json!({"forget": pref}), 4).is_err(),
            "only a fact"
        );
    }
}
