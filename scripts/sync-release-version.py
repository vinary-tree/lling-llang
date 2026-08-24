#!/usr/bin/env python3
"""Write or validate the component's release-train coordinates."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODEL_PATH = ROOT / "release/version.json"


def replace(path: str, pattern: str, replacement: str, expected: int = 1) -> None:
    target = ROOT / path
    updated, count = re.subn(pattern, replacement, target.read_text(encoding="utf-8"), flags=re.MULTILINE)
    if count != expected:
        raise ValueError(f"{path}: expected {expected} matches for {pattern!r}, found {count}")
    target.write_text(updated, encoding="utf-8")


def update_json(path: str, mutate) -> None:
    target = ROOT / path
    value = json.loads(target.read_text(encoding="utf-8"))
    mutate(value)
    target.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def set_cargo_dependency(name: str, version: str) -> None:
    target = ROOT / "Cargo.toml"
    source = target.read_text(encoding="utf-8")
    pattern = rf'^({re.escape(name)} = \{{[^\n]*?version = ")[^"]+("[^\n]*\}})$'
    updated, count = re.subn(pattern, rf'\g<1>={version}\2', source, flags=re.MULTILINE)
    if count not in (1, 2):
        raise ValueError(f"Cargo.toml: expected one or two {name} dependencies, found {count}")
    target.write_text(updated, encoding="utf-8")


def write_versions(model: dict[str, object]) -> None:
    canonical = str(model["canonical"])
    component = str(model["component"])
    deps = model["dependencies"]
    assert isinstance(deps, dict)
    replace("Cargo.toml", r'^version = "[^"]+"$', f'version = "{canonical}"')
    for name in ("liblevenshtein", "libdictenstein", "lling-llang", "llattice"):
        if name in deps and re.search(rf'^{re.escape(name)} = ', (ROOT / "Cargo.toml").read_text(), re.MULTILINE):
            set_cargo_dependency(name, str(deps[name]))
    replace(
        "Cargo.toml", r'^vinary-tree-interop = \{[^\n]+\}$',
        f'vinary-tree-interop = {{ path = "../vinary-tree-interop", version = "={deps["vinary-tree-interop"]}", optional = true }}',
    )

    def api(value: dict) -> None:
        value["packageVersion"] = canonical
        value["javascript"]["version"] = canonical
        value["javascript"]["dependencies"]["@vinary-tree/interop"] = deps["@vinary-tree/interop"]
        value["javascript"]["dependencies"]["@vinary-tree/vinary-tree"] = deps["@vinary-tree/vinary-tree"]
        value["release"] = {
            "canonical": canonical,
            "registries": model["registries"],
            "distTag": model["publication"]["distTag"],
        }
    update_json("bindings/api.json", api)

    def npm(value: dict) -> None:
        value["version"] = canonical
        value["dependencies"]["@vinary-tree/interop"] = deps["@vinary-tree/interop"]
        value["dependencies"]["@vinary-tree/vinary-tree"] = deps["@vinary-tree/vinary-tree"]
        value.setdefault("publishConfig", {})["tag"] = model["publication"]["distTag"]
    update_json("bindings/javascript/package.json", npm)
    replace(
        "bindings/javascript/README.md",
        r'(\| `@vinary-tree/interop` \| exact `)[^`]+(` \(guards \+ shared types\) \|)',
        rf'\g<1>{deps["@vinary-tree/interop"]}\2',
    )
    replace(
        "bindings/javascript/deps.cljs",
        r'"@vinary-tree/lling-llang" "[^"]+"',
        f'"@vinary-tree/lling-llang" "{canonical}"',
    )
    test_path = "bindings/javascript/test/facades.test.mjs"
    source = (ROOT / test_path).read_text(encoding="utf-8")
    source = re.sub(r'(packageJson\.dependencies\["@vinary-tree/vinary-tree"\], )"[^"]+"', rf'\g<1>"{deps["@vinary-tree/vinary-tree"]}"', source)
    source = re.sub(r'(packageJson\.dependencies\["@vinary-tree/interop"\], )"[^"]+"', rf'\g<1>"{deps["@vinary-tree/interop"]}"', source)
    source = source.replace(r"/^\d+\.\d+\.\d+$/", r"/^\d+\.\d+\.\d+-rc\.\d+$/")
    (ROOT / test_path).write_text(source, encoding="utf-8")
    cmake_name = "lling-llang" if component == "lling-llang" else component
    replace(f"cmake/{cmake_name}ConfigVersion.cmake", r'^set\(PACKAGE_VERSION "[^"]+"\)$', f'set(PACKAGE_VERSION "{canonical}")')
    replace(f"pkgconfig/{component}.pc", r'^Version: \S+$', f'Version: {canonical}')
    replace(f"pkgconfig/{component}.pc", r'^Requires: vinary-tree-interop (?:>=|=) \S+$', f'Requires: vinary-tree-interop = {deps["vinary-tree-interop"]}')


def validate(model: dict[str, object]) -> list[str]:
    failures: list[str] = []
    canonical = str(model["canonical"])
    component = str(model["component"])
    deps = model["dependencies"]
    assert isinstance(deps, dict)
    expected_registries = {"cargo": canonical, "cmake": canonical, "npm": canonical, "pkgConfig": canonical}
    if model.get("registries") != expected_registries:
        failures.append("registry spellings do not equal the canonical component version")
    cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    package_match = re.search(r'^version = "([^"]+)"$', cargo, re.MULTILINE)
    if not package_match or package_match.group(1) != canonical:
        failures.append("Cargo package version is stale")
    api = json.loads((ROOT / "bindings/api.json").read_text(encoding="utf-8"))
    if api.get("packageVersion") != canonical or api.get("javascript", {}).get("version") != canonical:
        failures.append("binding model version is stale")
    package = json.loads((ROOT / "bindings/javascript/package.json").read_text(encoding="utf-8"))
    if package.get("version") != canonical or package.get("publishConfig", {}).get("tag") != "next":
        failures.append("npm package release identity is stale")
    readme = (ROOT / "bindings/javascript/README.md").read_text(encoding="utf-8")
    readme_interop = re.search(r'\| `@vinary-tree/interop` \| exact `([^`]+)`', readme)
    if not readme_interop or readme_interop.group(1) != deps["@vinary-tree/interop"]:
        failures.append("JavaScript README interop pin is stale")
    cmake_name = "lling-llang" if component == "lling-llang" else component
    for name, path, pattern in (
        ("CMake", f"cmake/{cmake_name}ConfigVersion.cmake", r'PACKAGE_VERSION "([^"]+)"'),
        ("pkg-config", f"pkgconfig/{component}.pc", r'^Version: (\S+)$'),
    ):
        match = re.search(pattern, (ROOT / path).read_text(encoding="utf-8"), re.MULTILINE)
        if not match or match.group(1) != canonical:
            failures.append(f"{name} version is stale")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()
    model = json.loads(MODEL_PATH.read_text(encoding="utf-8"))
    if args.write:
        write_versions(model)
    failures = validate(model)
    if failures:
        for failure in failures:
            print(f"release-version error: {failure}", file=sys.stderr)
        return 1
    print(f"release versions agree with {model['canonical']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
