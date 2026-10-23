"""experiments/freeze.py -- the PREREG v1 freeze (Week 13, ADR-031): E1, E2, E3 and E5.

    python experiments/freeze.py        # then: python experiments/candidates.py; commit; git tag exp-E1-frozen

Changes nothing a run uses: each router config's `status` becomes FROZEN (keeping how it was chosen),
so its config_sha256 changes, and e3_candidates.json's references follow those hashes, after checking
that every point was chosen for exactly the config it now points to. E5's frozen selection is
candidates.py's (FROZEN_V1). E4 and E6 are frozen later, by a logged amendment (ADR-031).
"""
from __future__ import annotations

import copy
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "experiments" / "router"))
from make_config import config_hash  # noqa: E402

FROZEN = "FROZEN 2026-10-23 (PREREG v1, ADR-031)"
CONFIGS = ROOT / "experiments" / "configs"
ROUTERS = [CONFIGS / f"router_{d}{s}.json" for d in ("intents_v0", "clinc150") for s in ("", "_r0", "_r1")]


def freeze_config(cfg: dict) -> dict:
    """The config with a FROZEN status (and its new hash); everything else unchanged."""
    if cfg["status"].startswith(FROZEN):
        return cfg
    chosen = cfg["status"].removeprefix("CANDIDATE: ").removesuffix("; FROZEN at the PREREG freeze (Week 13)")
    new = cfg | {"status": f"{FROZEN}; {chosen}"}
    return new | {"config_sha256": config_hash(new)}


def retarget_e3(e3: dict, new_hash: dict[str, str]) -> dict:
    """e3_candidates with each router hash replaced by its frozen one; refuses an unknown hash."""
    out = copy.deepcopy(e3)
    for d, v in out["datasets"].items():
        if v["router_config_sha256"] not in new_hash:
            raise ValueError(f"e3_candidates {d}: its points were chosen for another router config")
        v["router_config_sha256"] = new_hash[v["router_config_sha256"]]
    return out


def main() -> int:
    new_hash = {}
    for path in ROUTERS:
        old = json.loads(path.read_text())
        new = freeze_config(old)
        new_hash[old["config_sha256"]] = new["config_sha256"]
        new_hash[new["config_sha256"]] = new["config_sha256"]
        path.write_text(json.dumps(new, indent=1, ensure_ascii=False) + "\n")
        print(f"{path.relative_to(ROOT)}: {old['config_sha256'][:12]} -> {new['config_sha256'][:12]}")
    e3_path = CONFIGS / "e3_candidates.json"
    e3 = retarget_e3(json.loads(e3_path.read_text()), new_hash)
    e3_path.write_text(json.dumps(e3, indent=1) + "\n")
    print(f"{e3_path.relative_to(ROOT)}: router hashes retargeted")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
