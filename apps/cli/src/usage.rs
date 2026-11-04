//! What Settings shows of the daily counts, and what a pilot participant may copy and send (v4, blueprint
//! §4.12; the counts themselves: kestrel-memory's usage.rs). Counts and each capability's state, and
//! nothing that says who this is or what was said: no name, no text, no device detail.

use kestrel_memory::Memory;
use serde_json::{Map, Value, json};

/// A day counts as used when Kestrel was asked something or given a task; a reminder that rang by
/// itself does not make one.
const USED: [&str; 2] = ["request", "task_added"];

/// `capabilities` is what `/api/capabilities` answers; only each card's state is taken from it.
pub fn page_json(memory: &Memory, capabilities: Option<&Value>) -> Result<Value, String> {
    let mut days: Vec<(String, Map<String, Value>)> = Vec::new();
    for row in memory.usage().map_err(|e| e.to_string())? {
        if days.last().is_none_or(|(day, _)| *day != row.day) {
            days.push((row.day.clone(), Map::new()));
        }
        if let Some((_, counts)) = days.last_mut() {
            counts.insert(row.what, json!(row.n));
        }
    }
    let used = days
        .iter()
        .filter(|(_, counts)| USED.iter().any(|what| counts.contains_key(*what)))
        .count();
    let states: Map<String, Value> = capabilities
        .and_then(|c| c["cards"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|card| Some((card["id"].as_str()?.to_string(), card["state"].clone())))
        .collect();
    let days: Vec<Value> = days
        .into_iter()
        .map(|(day, counts)| json!({"day": day, "counts": counts}))
        .collect();
    Ok(json!({
        "kestrel": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "days_used": used,
        "days": days,
        "capabilities": states,
    }))
}

/// `POST /api/usage`: forgets the counts; what is left, or why not.
pub fn change(
    memory: &mut Memory,
    change: &Value,
    capabilities: Option<&Value>,
) -> Result<Value, String> {
    if change["forget"] != true {
        return Err("send {\"forget\": true}".into());
    }
    memory.forget_usage().map_err(|e| e.to_string())?;
    page_json(memory, capabilities)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    const NOW: i64 = 20_000 * DAY;

    #[test]
    fn counts_come_by_day_and_only_asked_for_days_are_used_days() {
        let mut m = Memory::in_memory().unwrap();
        m.count_use(NOW, "request");
        m.count_use(NOW, "request");
        m.count_use(NOW, "chat");
        m.count_use(NOW + DAY, "reminder_rang");
        m.create_task("CANARY-7f3a call asha", None, NOW + 2 * DAY)
            .unwrap();
        let v = page_json(&m, None).unwrap();
        assert_eq!(v["days_used"], 2, "{v}");
        let days = v["days"].as_array().unwrap();
        assert_eq!(days.len(), 3, "{v}");
        assert_eq!(days[0]["counts"], json!({"request": 2, "chat": 1}));
        assert_eq!(days[1]["counts"], json!({"reminder_rang": 1}));
        assert_eq!(days[2]["counts"], json!({"task_added": 1}));
        assert_eq!(v["os"], std::env::consts::OS);
        assert_eq!(v["capabilities"], json!({}));
        assert!(!v.to_string().contains("CANARY"), "{v}");
    }

    #[test]
    fn of_the_capabilities_only_each_state_is_taken_and_nothing_about_the_device() {
        let m = Memory::in_memory().unwrap();
        let cards = json!({
            "device": {"os": "Linux", "ram": "SECRET-RAM", "gpus": [{"name": "SECRET-GPU"}]},
            "cards": [
                {"id": "notes", "name": {"en": "SECRET-NAME"}, "state": "ready", "notes": []},
                {"id": "code", "name": {"en": "SECRET-NAME"}, "state": "locked",
                 "notes": [{"en": "SECRET-NOTE"}]},
            ],
        });
        let v = page_json(&m, Some(&cards)).unwrap();
        assert_eq!(
            v["capabilities"],
            json!({"notes": "ready", "code": "locked"})
        );
        assert!(!v.to_string().contains("SECRET"), "{v}");
    }

    #[test]
    fn the_counts_can_be_forgotten_and_nothing_else_is_understood() {
        let mut m = Memory::in_memory().unwrap();
        m.count_use(NOW, "request");
        assert!(change(&mut m, &json!({"forget": false}), None).is_err());
        assert_eq!(m.usage().unwrap().len(), 1);
        let v = change(&mut m, &json!({"forget": true}), None).unwrap();
        assert_eq!(
            (v["days_used"].as_u64(), v["days"].as_array().map(Vec::len)),
            (Some(0), Some(0))
        );
        assert_eq!(m.usage().unwrap(), []);
    }
}
