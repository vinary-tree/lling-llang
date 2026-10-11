#!/usr/bin/env python3
"""Check complete finite path-cursor models and transition-level mutants."""

import json
import os
import re
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODEL = ROOT / "proofs/tla/JuliaPathSearchLifecycle.tla"
CONFIG_DIR = ROOT / "proofs/tla/MC"
LEDGER = ROOT / "proofs/doc/julia-path-invariants.json"
OUTPUT = ROOT / "target/formal-verification/julia-path"

MUTATIONS = {
    "bad_initial_index": ("lastIndex |-> 0", "lastIndex |-> 4"),
    "drop_graph_retain": (
        '!.cursor = "Open", !.retained = TRUE]',
        '!.cursor = "Open", !.retained = FALSE]',
    ),
    "overrun_work_limit": (
        "!.work = st.work + 1,", "!.work = st.work + 4,"
    ),
    "overproduce_paths": (
        "!.produced = st.produced + 1,", "!.produced = st.produced + 2,"
    ),
    "skip_ranked_path": (
        '!.nextIndex = IF Mode = "Sample" THEN st.nextIndex ELSE st.nextIndex + 1,',
        '!.nextIndex = IF Mode = "Sample" THEN st.nextIndex ELSE st.nextIndex + 2,',
    ),
    "admit_out_of_window": (
        "Cost(st.nextIndex) > Cost1 + Beam", "FALSE"
    ),
    "ignore_draw_index": (
        "!.trace = Append(st.trace, NextIndex),",
        "!.trace = Append(st.trace, 1),",
    ),
    "mislabel_sample_cap": (
        '!.status = "Truncated"]', '!.status = "Exhausted"]'
    ),
    "count_after_cancel": (
        "!.cancelCount = st.produced]", "!.cancelCount = st.produced + 1]"
    ),
    "publish_numeric_failure": (
        '!.status = "NumericError"]',
        '!.status = "NumericError", !.produced = 1]'
    ),
    "retain_after_reduce": (
        '!.retained = FALSE,\n       !.status = "ReducerStop"]',
        '!.retained = TRUE,\n       !.status = "ReducerStop"]'
    ),
}


def command() -> list[str]:
    jar = os.environ.get("TLA2TOOLS_JAR")
    if jar:
        return ["java", "-jar", str(Path(jar).resolve())]
    if shutil.which("tlc"):
        return ["tlc"]
    raise SystemExit("TLC unavailable: set TLA2TOOLS_JAR or install tlc")


def check(name: str, cfg: Path, model: Path, expected_failure: str | None) -> None:
    metadir = OUTPUT / f"meta-{name}"
    metadir.mkdir(parents=True, exist_ok=True)
    log = OUTPUT / f"{name}.log"
    with log.open("w") as output:
        try:
            result = subprocess.run(
                [*command(), "-workers", "1", "-metadir", str(metadir),
                 "-config", str(cfg), str(model)],
                cwd=ROOT, stdout=output, stderr=subprocess.STDOUT,
                check=False, timeout=120,
            )
        except subprocess.TimeoutExpired as error:
            raise SystemExit(f"TLC timed out: {name}, {log}") from error
    text = log.read_text()
    if expected_failure:
        if result.returncode == 0 or f"Invariant {expected_failure} is violated" not in text:
            raise SystemExit(f"mutation escaped {expected_failure}: {name}, {log}")
    elif result.returncode != 0 or "Model checking completed. No error has been found." not in text:
        raise SystemExit(f"positive model failed: {name}, {log}")
    print(f"{name}: {'caught ' + expected_failure if expected_failure else 'passed'}")


def main() -> None:
    ledger = json.loads(LEDGER.read_text())
    rows = ledger["invariants"]
    names = [row["name"] for row in rows]
    if len(names) != 11 or len(set(names)) != 11:
        raise SystemExit("expected eleven unique path invariants")
    if set(MUTATIONS) != {row["mutation"] for row in rows}:
        raise SystemExit("mutation ledger differs from runner")
    source = MODEL.read_text()
    suffix = source.split("Spec == Init /\\ [][Next]_vars", 1)
    if len(suffix) != 2 or re.findall(
        r"^([A-Za-z][A-Za-z0-9]*) ==", suffix[1], re.MULTILINE
    ) != names:
        raise SystemExit("formal invariant ledger does not exactly cover the model")
    OUTPUT.mkdir(parents=True, exist_ok=True)
    for case in ("Ranked", "Pruned", "Sample", "Numeric"):
        cfg = CONFIG_DIR / f"JuliaPathSearch{case}.cfg"
        match = re.search(r"^INVARIANTS\n((?:  [A-Za-z][A-Za-z0-9]*\n)+)",
                          cfg.read_text(), re.MULTILINE)
        if match is None or [s.strip() for s in match.group(1).splitlines()] != names:
            raise SystemExit(f"invariant coverage differs from ledger: {cfg}")
        check(case, cfg, MODEL, None)

    mutant = OUTPUT / "mutant"
    mutant.mkdir(exist_ok=True)
    mutant_model = mutant / MODEL.name
    mutant_cfg = mutant / "check.cfg"
    for row in rows:
        before, after = MUTATIONS[row["mutation"]]
        if source.count(before) != 1:
            raise SystemExit(f"non-unique mutation anchor: {row['mutation']}")
        mutant_model.write_text(source.replace(before, after, 1))
        cfg = (CONFIG_DIR / f"JuliaPathSearch{row['case']}.cfg").read_text()
        cfg, count = re.subn(
            r"INVARIANTS\n(?:  [A-Za-z][A-Za-z0-9]*\n)+",
            f"INVARIANT {row['name']}\n", cfg,
        )
        if count != 1:
            raise SystemExit(f"cannot isolate invariant: {row['name']}")
        mutant_cfg.write_text(cfg)
        check(row["mutation"], mutant_cfg, mutant_model, row["name"])
    mutant_model.unlink()
    mutant_cfg.unlink()
    mutant.rmdir()


if __name__ == "__main__":
    main()
