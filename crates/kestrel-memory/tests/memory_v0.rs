//! Kestrel-Memory-v0 consistency (ADR-026), on both splits: no recall is computed here, only
//! whether the data says what its labels claim. The store loads through the public write path
//! (no secret refused, every row gets its built id); a "lexical" query shares a retrieval term with
//! every memory it expects and a "paraphrase" shares none, by the baseline's own `terms`; pending
//! and expired memories never come back, whatever the query.

use std::collections::HashMap;
use std::path::Path;

use kestrel_memory::eval::{self, Dataset};
use kestrel_memory::retrieval::{score, terms};
use kestrel_memory::{Filter, Item, Memory, Weights};

const BUILD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../datasets/kestrel_memory_v0/build"
);

fn loaded() -> (Dataset, Memory, HashMap<i64, String>) {
    let data = eval::load(Path::new(BUILD)).unwrap();
    let mut m = Memory::in_memory().unwrap();
    let ids = eval::seed(&mut m, &data.store).unwrap();
    (data, m, ids)
}

#[test]
fn the_store_loads_through_the_public_write_path() {
    let (data, m, ids) = loaded();
    assert_eq!(ids.len(), data.store.len());
    assert_eq!(m.items(Filter::All).unwrap().len(), data.store.len());
    let pending = m.items(Filter::Pending).unwrap().len();
    assert_eq!(pending, data.store.iter().filter(|r| !r.approved).count());
}

#[test]
fn lexical_and_paraphrase_labels_match_the_baselines_terms() {
    let (data, m, ids) = loaded();
    let by_name: HashMap<String, Item> = m
        .items(Filter::All)
        .unwrap()
        .into_iter()
        .map(|i| (ids[&i.id].clone(), i))
        .collect();
    let w = Weights::default();
    let mut wrong = Vec::new();
    for q in &data.queries {
        let t = terms(&q.text);
        for mid in &q.expect {
            let overlaps = score(&by_name[mid], &t, data.now, &w).is_some();
            let ok = match q.kind.as_str() {
                "lexical" => overlaps,
                "paraphrase" => !overlaps,
                other => panic!("{}: {other} with an expected memory", q.id),
            };
            if !ok {
                wrong.push(format!("{} ({}) vs {mid}: terms {t:?}", q.id, q.kind));
            }
        }
    }
    assert!(wrong.is_empty(), "mislabelled:\n{}", wrong.join("\n"));
}

#[test]
fn pending_and_expired_memories_never_come_back() {
    let (data, m, ids) = loaded();
    let all: Vec<_> = data.queries.iter().collect();
    let rows = eval::rank_all(&m, &ids, &all, data.now, &Weights::default()).unwrap();
    let gone: Vec<&str> = data
        .store
        .iter()
        .filter(|r| !r.approved || r.expires_at.is_some_and(|at| at <= data.now))
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(
        gone.len(),
        4,
        "3 pending guesses and 1 expired task: {gone:?}"
    );
    for r in &rows {
        assert!(
            r.retrieved.iter().all(|x| !gone.contains(&x.as_str())),
            "{}: {:?}",
            r.id,
            r.retrieved
        );
        assert_eq!(eval::leaks(r), 0, "{}", r.id);
    }
}
