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
GENERATED_TREE_PARTS = frozenset(
    {".git", ".venv", "_build", "build", "dist", "node_modules", "target", "venv"}
)
HISTORICAL_DOC_TREE_PARTS = frozenset({"archive", "releases"})
NPM_PACKAGE = "@vinary-tree/lling-llang"
NPM_INTEROP_PACKAGE = "@vinary-tree/vinary-tree-interop"
NPM_RUNTIME_PACKAGE = "@vinary-tree/javascript-runtime"
DEPRECATED_NPM_COORDINATES = {
    "@vinary-tree/" + "interop",
    "@vinary-tree/" + "vinary-tree",
    "@vinary-tree/" + "javascript-runtime-interop",
}
DEPRECATED_NPM_PATTERNS = {
    coordinate: re.compile(re.escape(coordinate) + r"(?=$|[^A-Za-z0-9._~-])")
    for coordinate in DEPRECATED_NPM_COORDINATES
}


def replace(path: str, pattern: str, replacement: str, expected: int = 1) -> None:
    target = ROOT / path
    updated, count = re.subn(
        pattern, replacement, target.read_text(encoding="utf-8"), flags=re.MULTILINE
    )
    if count != expected:
        raise ValueError(
            f"{path}: expected {expected} matches for {pattern!r}, found {count}"
        )
    target.write_text(updated, encoding="utf-8")


def rewrite_candidate_tokens(patterns: tuple[str, ...], canonical: str) -> None:
    base, candidate = canonical.split("-rc.", 1)
    escaped = re.escape(base)
    replacements = (
        (rf"{escaped}\.rc\.\d+", f"{base}.rc.{candidate}"),
        (rf"{escaped}~rc\d+", f"{base}~rc{candidate}"),
        (rf"{escaped}rc\d+-\d+", f"{base}rc{candidate}-1"),
        (rf"{escaped}rc\d+", f"{base}rc{candidate}"),
        (rf"{escaped}-rc\.\d+", canonical),
    )
    for pattern in patterns:
        for target in ROOT.glob(pattern):
            relative = target.relative_to(ROOT)
            if (
                not target.is_file()
                or GENERATED_TREE_PARTS.intersection(relative.parts)
                or HISTORICAL_DOC_TREE_PARTS.intersection(relative.parts)
            ):
                continue
            source = target.read_text(encoding="utf-8")
            for version_pattern, replacement in replacements:
                source = re.sub(version_pattern, replacement, source)
            target.write_text(source, encoding="utf-8")


def update_json(path: str, mutate) -> None:
    target = ROOT / path
    value = json.loads(target.read_text(encoding="utf-8"))
    mutate(value)
    target.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def set_cargo_dependency(name: str, version: str) -> None:
    target = ROOT / "Cargo.toml"
    source = target.read_text(encoding="utf-8")
    pattern = rf'^({re.escape(name)} = \{{[^\n]*?version = ")[^"]+("[^\n]*\}})$'
    updated, count = re.subn(pattern, rf"\g<1>={version}\2", source, flags=re.MULTILINE)
    if count not in (1, 2):
        raise ValueError(
            f"Cargo.toml: expected one or two {name} dependencies, found {count}"
        )
    target.write_text(updated, encoding="utf-8")


def write_versions(model: dict[str, object]) -> None:
    canonical = str(model["canonical"])
    component = str(model["component"])
    coordinates = model["coordinates"]
    deps = model["dependencies"]
    publication = model["publication"]
    assert isinstance(coordinates, dict)
    assert isinstance(deps, dict)
    assert isinstance(publication, dict)
    npm_package = str(coordinates["npmPackage"])
    replace("Cargo.toml", r'^version = "[^"]+"$', f'version = "{canonical}"')
    for name in ("liblevenshtein", "libdictenstein", "lling-llang", "llattice"):
        if name in deps and re.search(
            rf"^{re.escape(name)} = ", (ROOT / "Cargo.toml").read_text(), re.MULTILINE
        ):
            set_cargo_dependency(name, str(deps[name]))
    replace(
        "Cargo.toml",
        r"^vinary-tree-interop = \{[^\n]+\}$",
        f'vinary-tree-interop = {{ path = "../vinary-tree-interop", version = "={deps["vinary-tree-interop"]}", optional = true }}',
    )
    for package, version in {
        "lling-llang": canonical,
        "liblevenshtein": deps["liblevenshtein"],
        "libdictenstein": deps["libdictenstein"],
        "vinary-tree-interop": deps["vinary-tree-interop"],
    }.items():
        replace(
            "Cargo.lock",
            rf'(\[\[package\]\]\nname = "{re.escape(package)}"\nversion = ")[^"]+',
            rf"\g<1>{version}",
        )

    def api(value: dict) -> None:
        value["packages"]["npm"] = npm_package
        value["interop"]["npm"] = NPM_INTEROP_PACKAGE
        value["packageVersion"] = canonical
        value["javascript"]["package"] = npm_package
        value["javascript"]["version"] = canonical
        value["javascript"]["dependencies"] = {
            NPM_INTEROP_PACKAGE: deps[NPM_INTEROP_PACKAGE],
            NPM_RUNTIME_PACKAGE: deps[NPM_RUNTIME_PACKAGE],
        }
        value["wasm"].pop("umbrellaPackage", None)
        value["wasm"]["runtimePackage"] = NPM_RUNTIME_PACKAGE
        value["release"] = {
            "canonical": canonical,
            "registries": model["registries"],
            "distTag": publication["distTag"],
            "sourceTag": publication["sourceTag"],
        }

    update_json("bindings/api.json", api)

    def npm(value: dict) -> None:
        value["name"] = npm_package
        value["version"] = canonical
        value["dependencies"] = {
            NPM_INTEROP_PACKAGE: deps[NPM_INTEROP_PACKAGE],
            NPM_RUNTIME_PACKAGE: deps[NPM_RUNTIME_PACKAGE],
        }
        value.setdefault("publishConfig", {})["tag"] = publication["distTag"]

    update_json("bindings/javascript/package.json", npm)
    replace(
        "bindings/javascript/README.md",
        r"(\| `@vinary-tree/vinary-tree-interop` \| exact `)[^`]+(` \(guards \+ shared types\) \|)",
        rf"\g<1>{deps[NPM_INTEROP_PACKAGE]}\2",
    )
    replace(
        "bindings/javascript/deps.cljs",
        r'"@vinary-tree/lling-llang" "[^"]+"',
        f'"@vinary-tree/lling-llang" "{canonical}"',
    )
    test_path = "bindings/javascript/test/facades.test.mjs"
    source = (ROOT / test_path).read_text(encoding="utf-8")
    source = re.sub(
        r'(packageJson\.dependencies\["@vinary-tree/javascript-runtime"\], )"[^"]+"',
        rf'\g<1>"{deps[NPM_RUNTIME_PACKAGE]}"',
        source,
    )
    source = re.sub(
        r'(packageJson\.dependencies\["@vinary-tree/vinary-tree-interop"\], )"[^"]+"',
        rf'\g<1>"{deps[NPM_INTEROP_PACKAGE]}"',
        source,
    )
    source = source.replace(r"/^\d+\.\d+\.\d+$/", r"/^\d+\.\d+\.\d+-rc\.\d+$/")
    (ROOT / test_path).write_text(source, encoding="utf-8")
    cmake_name = "lling-llang" if component == "lling-llang" else component
    replace(
        f"cmake/{cmake_name}ConfigVersion.cmake",
        r'^set\(PACKAGE_VERSION "[^"]+"\)$',
        f'set(PACKAGE_VERSION "{canonical}")',
    )
    replace(f"pkgconfig/{component}.pc", r"^Version: \S+$", f"Version: {canonical}")
    replace(
        f"pkgconfig/{component}.pc",
        r"^Requires: vinary-tree-interop (?:>=|=) \S+$",
        f"Requires: vinary-tree-interop = {deps['vinary-tree-interop']}",
    )
    rewrite_candidate_tokens(
        (".github/workflows/*.yml", "README.md", "bindings/**/*.md", "docs/**/*.md"),
        canonical,
    )


def validate(model: dict[str, object]) -> list[str]:
    failures: list[str] = []
    canonical = str(model["canonical"])
    component = str(model["component"])
    coordinates = model.get("coordinates")
    deps = model["dependencies"]
    publication = model.get("publication")
    if coordinates != {"npmPackage": NPM_PACKAGE}:
        failures.append(f"npm package coordinate must be exactly {NPM_PACKAGE}")
    if not isinstance(publication, dict):
        failures.append("publication model is missing")
        publication = {}
    assert isinstance(deps, dict)
    expected_npm_dependencies = {
        NPM_INTEROP_PACKAGE: canonical,
        NPM_RUNTIME_PACKAGE: canonical,
    }
    actual_npm_dependencies = {
        name: version for name, version in deps.items() if name.startswith("@")
    }
    if actual_npm_dependencies != expected_npm_dependencies:
        failures.append(
            f"npm dependency coordinates must be {expected_npm_dependencies}, "
            f"found {actual_npm_dependencies}"
        )
    source_tag = publication.get("sourceTag")
    immutable_tag_pattern = rf"v{re.escape(canonical)}(?:-release\.[1-9][0-9]*)?"
    if (
        not isinstance(source_tag, str)
        or re.fullmatch(immutable_tag_pattern, source_tag) is None
    ):
        failures.append(
            "source tag must be canonical or an append-only numbered correction"
        )
    for pattern in (
        "README.md",
        "bindings/api.json",
        "bindings/javascript/**/*",
        "docs/**/*.md",
    ):
        for target in ROOT.glob(pattern):
            if not target.is_file():
                continue
            relative = target.relative_to(ROOT)
            if HISTORICAL_DOC_TREE_PARTS.intersection(relative.parts):
                continue
            source = target.read_text(encoding="utf-8")
            deprecated = next(
                (
                    coordinate
                    for coordinate, pattern in DEPRECATED_NPM_PATTERNS.items()
                    if pattern.search(source)
                ),
                None,
            )
            if deprecated:
                failures.append(
                    f"{relative} contains forbidden npm coordinate {deprecated}"
                )
    expected_registries = {
        "cargo": canonical,
        "cmake": canonical,
        "npm": canonical,
        "pkgConfig": canonical,
    }
    if model.get("registries") != expected_registries:
        failures.append(
            "registry spellings do not equal the canonical component version"
        )
    cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    package_match = re.search(r'^version = "([^"]+)"$', cargo, re.MULTILINE)
    if not package_match or package_match.group(1) != canonical:
        failures.append("Cargo package version is stale")
    cargo_lock = (ROOT / "Cargo.lock").read_text(encoding="utf-8")
    for package, version in {
        "lling-llang": canonical,
        "liblevenshtein": deps["liblevenshtein"],
        "libdictenstein": deps["libdictenstein"],
        "vinary-tree-interop": deps["vinary-tree-interop"],
    }.items():
        match = re.search(
            rf'\[\[package\]\]\nname = "{re.escape(package)}"\nversion = "([^"]+)"',
            cargo_lock,
        )
        if not match or match.group(1) != version:
            failures.append(f"Cargo lock {package} version is stale")
    api = json.loads((ROOT / "bindings/api.json").read_text(encoding="utf-8"))
    if (
        api.get("packageVersion") != canonical
        or api.get("javascript", {}).get("version") != canonical
        or api.get("packages", {}).get("npm") != NPM_PACKAGE
        or api.get("javascript", {}).get("package") != NPM_PACKAGE
        or api.get("interop", {}).get("npm") != NPM_INTEROP_PACKAGE
        or api.get("wasm", {}).get("runtimePackage") != NPM_RUNTIME_PACKAGE
        or "umbrellaPackage" in api.get("wasm", {})
        or api.get("javascript", {}).get("dependencies") != expected_npm_dependencies
    ):
        failures.append("binding model release identity is stale")
    package = json.loads(
        (ROOT / "bindings/javascript/package.json").read_text(encoding="utf-8")
    )
    if (
        package.get("name") != NPM_PACKAGE
        or package.get("version") != canonical
        or package.get("publishConfig", {}).get("tag") != "next"
        or package.get("dependencies") != expected_npm_dependencies
    ):
        failures.append("npm package release identity is stale")
    readme = (ROOT / "bindings/javascript/README.md").read_text(encoding="utf-8")
    readme_interop = re.search(
        r"\| `@vinary-tree/vinary-tree-interop` \| exact `([^`]+)`", readme
    )
    if not readme_interop or readme_interop.group(1) != deps[NPM_INTEROP_PACKAGE]:
        failures.append("JavaScript README interop pin is stale")
    cmake_name = "lling-llang" if component == "lling-llang" else component
    for name, path, pattern in (
        (
            "CMake",
            f"cmake/{cmake_name}ConfigVersion.cmake",
            r'PACKAGE_VERSION "([^"]+)"',
        ),
        ("pkg-config", f"pkgconfig/{component}.pc", r"^Version: (\S+)$"),
    ):
        match = re.search(
            pattern, (ROOT / path).read_text(encoding="utf-8"), re.MULTILINE
        )
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
