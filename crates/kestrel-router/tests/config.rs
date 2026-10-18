//! Contract: the router reads the config Python generates (experiments/router/make_config.py).

use std::path::Path;

use kestrel_router::RouterConfig;

#[test]
fn reads_the_committed_dev_config() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../experiments/configs/router_intents_v0.json");
    let cfg = RouterConfig::from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(cfg.multi_intent_to_generative); // ADR-017
    assert_eq!(cfg.accept[3], 0.0); // Tier 4 always answers
    assert!(cfg.accept[2] > 0.0 && cfg.accept[2] < 1.0);
}
