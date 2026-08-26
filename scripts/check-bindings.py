#!/usr/bin/env python3
"""Dependency-free binding-surface drift gate for lling-llang.

Verifies, against the committed binding model ``bindings/api.json``:

  1. symbol parity      -- api.json cFunctions == ``pub extern "C" fn lling_*``
                           exports in src/ffi.rs == declarations in
                           include/lling_llang.h; the C++ facade
                           include/lling_llang.hpp references only declared
                           symbols.
  2. enum/constant      -- LlingStatus discriminants and the
     parity                LLING_ABI_VERSION / LLING_API_REVISION constants
                           agree across api.json, src/ffi.rs, and the header.
  3. JS facade parity   -- every export-map subpath in
                           bindings/javascript/package.json resolves to an
                           existing file; the d.ts/mjs/cjs/cljs facades'
                           exported names are mutually consistent; every
                           @vinary-tree/* dependency is an exact semver pin.
  4. workflow version   -- .github/workflows/release-bindings.yml must derive
     derivation           the staged ``dist/lling-llang-<version>`` prefix
                           from Cargo.toml (the same source
                           scripts/stage-native-package.sh uses); a hardcoded
                           semver literal is a release-corruption hazard
                           (finding LLING-B1).
  5. identity guard     -- no ``f1r3fly`` / ``universal-automata`` identity
                           strings and no sibling-owned symbol families
                           (``ldict_*``, ``dual_*``) in publishable facade,
                           header, or packaging files.

Scope: this gate inspects lling-llang only. Per the family placement rule,
sibling repositories run their own gates; nothing here reads outside this
repository.

Usage:
  python3 scripts/check-bindings.py          # human-readable report
  python3 scripts/check-bindings.py --json   # machine-readable report

Exit status: 0 = all checks pass, 1 = at least one failure, 2 = usage or
model-loading error.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODEL_PATH = ROOT / "bindings" / "api.json"
FFI_PATH = ROOT / "src" / "ffi.rs"
HEADER_PATH = ROOT / "include" / "lling_llang.h"
HPP_PATH = ROOT / "include" / "lling_llang.hpp"
JS_ROOT = ROOT / "bindings" / "javascript"
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "release-bindings.yml"

SKIP_DIR_PARTS = {".git", "build", "node_modules", "obj", "target"}
SEMVER_IDENTIFIER = r"(?:0|[1-9]\d*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
EXACT_SEMVER = re.compile(
    rf"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)"
    rf"(?:-{SEMVER_IDENTIFIER}(?:\.{SEMVER_IDENTIFIER})*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?\Z"
)


class MissingFile(Exception):
    """A file the gate depends on is absent; aborts the enclosing check."""

    def __init__(self, message: str) -> None:
        super().__init__(message)
        self.message = message


class Report:
    """Collects named check results for the human and JSON renderings."""

    def __init__(self) -> None:
        self.checks: list[dict[str, object]] = []

    def run(self, name: str, body) -> None:
        failures: list[str] = []
        info: dict[str, object] = {}
        try:
            body(failures, info)
        except MissingFile as error:
            failures.append(error.message)
        except OSError as error:
            failures.append(f"required file is unreadable: {error}")
        self.checks.append(
            {"name": name, "pass": not failures, "failures": failures, "info": info}
        )

    @property
    def passed(self) -> bool:
        return all(check["pass"] for check in self.checks)

    def render_human(self) -> str:
        lines = ["check-bindings: lling-llang binding-surface drift gate", ""]
        for check in self.checks:
            marker = "PASS" if check["pass"] else "FAIL"
            lines.append(f"[{marker}] {check['name']}")
            for key, value in sorted(check["info"].items()):  # type: ignore[union-attr]
                lines.append(f"       {key}: {value}")
            for failure in check["failures"]:  # type: ignore[union-attr]
                lines.append(f"       ! {failure}")
        failed = sum(1 for check in self.checks if not check["pass"])
        lines.append("")
        lines.append(
            f"{len(self.checks) - failed}/{len(self.checks)} checks passed"
            + ("" if not failed else f"; {failed} FAILED")
        )
        return "\n".join(lines)

    def render_json(self) -> str:
        return json.dumps({"pass": self.passed, "checks": self.checks}, indent=2)


def read(path: Path) -> str:
    if not path.is_file():
        raise MissingFile(f"required binding file is missing: {path.relative_to(ROOT)}")
    return path.read_text(encoding="utf-8")


def camel_to_screaming(name: str) -> str:
    return re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", name).upper()


def walk_export_targets(value: object) -> list[str]:
    if isinstance(value, str):
        return [value]
    if isinstance(value, dict):
        targets: list[str] = []
        for nested in value.values():
            targets.extend(walk_export_targets(nested))
        return targets
    return []


def publishable_files() -> list[Path]:
    files: list[Path] = []
    for relative in ("bindings", "include", "cmake", "pkgconfig"):
        root = ROOT / relative
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*")):
            if not path.is_file():
                continue
            if any(part in SKIP_DIR_PARTS for part in path.parts):
                continue
            if path == MODEL_PATH:
                # The binding model is this gate's own configuration (it names
                # the forbidden symbol prefixes); it is not a published facade.
                continue
            files.append(path)
    return files


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("--json", action="store_true", help="emit a JSON report")
    arguments = parser.parse_args()

    if not MODEL_PATH.is_file():
        print(
            f"error: binding model is missing: {MODEL_PATH.relative_to(ROOT)}",
            file=sys.stderr,
        )
        return 2
    try:
        model = json.loads(MODEL_PATH.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        print(f"error: bindings/api.json is not valid JSON: {error}", file=sys.stderr)
        return 2

    report = Report()

    # ── 0. model sanity + version coherence ─────────────────────────────────
    def model_sanity(failures: list[str], info: dict[str, object]) -> None:
        if model.get("name") != "lling-llang":
            failures.append("model name must be lling-llang")
        organization = model.get("organization", {})
        if organization.get("github") != "vinary-tree":
            failures.append("wrong GitHub organization")
        interop = model.get("interop", {})
        if interop.get("crate") != "vinary-tree-interop":
            failures.append("wrong interop crate")
        if interop.get("cPrefix") != "vt_":
            failures.append("shared ABI must use the vt_ prefix")
        if interop.get("scalarWfstInterfaceVersion") != 1:
            failures.append("wrong scalar WFST interface version")
        if model.get("cPrefix") != "lling_":
            failures.append("project C ABI must use the lling_ prefix")

        cargo = tomllib.loads(read(ROOT / "Cargo.toml"))
        cargo_version = cargo["package"]["version"]
        info["cargo_version"] = cargo_version
        if model.get("packageVersion") != cargo_version:
            failures.append(
                f"api.json packageVersion {model.get('packageVersion')!r} "
                f"!= Cargo.toml version {cargo_version!r}"
            )
        package = json.loads(read(JS_ROOT / "package.json"))
        if package.get("version") != cargo_version:
            failures.append(
                f"bindings/javascript/package.json version {package.get('version')!r} "
                f"!= Cargo.toml version {cargo_version!r}"
            )
        deps_cljs = read(JS_ROOT / "deps.cljs")
        pin = re.search(r'"@vinary-tree/lling-llang"\s+"([^"]+)"', deps_cljs)
        if pin is None:
            failures.append("deps.cljs does not pin @vinary-tree/lling-llang")
        elif pin.group(1) != cargo_version:
            failures.append(
                f"deps.cljs pins @vinary-tree/lling-llang {pin.group(1)!r} "
                f"!= Cargo.toml version {cargo_version!r}"
            )

    report.run("model sanity + version coherence", model_sanity)

    # ── 1. symbol parity across model, Rust exports, C header, C++ facade ───
    def symbol_parity(failures: list[str], info: dict[str, object]) -> None:
        names = [item["name"] for item in model.get("cFunctions", [])]
        modeled = set(names)
        if len(modeled) != len(names):
            failures.append("bindings/api.json cFunctions contains duplicate names")
        for name in sorted(modeled):
            if not re.fullmatch(r"lling_[a-z0-9_]+", name):
                failures.append(
                    f"modeled symbol {name!r} does not use the lling_ prefix"
                )
        info["modeled_symbols"] = len(modeled)

        ffi_source = read(FFI_PATH)
        exported = set(
            re.findall(
                r'pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+(lling_[a-z0-9_]+)\s*\(',
                ffi_source,
            )
        )
        info["rust_exports"] = len(exported)
        if exported != modeled:
            failures.append(
                "C symbol model / src/ffi.rs mismatch: "
                f"missing-from-ffi={sorted(modeled - exported)}, "
                f"unmodeled={sorted(exported - modeled)}"
            )

        header = read(HEADER_PATH)
        declared = set(re.findall(r"\b(lling_[a-z0-9_]+)\s*\(", header))
        info["header_declarations"] = len(declared)
        if declared != modeled:
            failures.append(
                "C symbol model / include/lling_llang.h mismatch: "
                f"missing-from-header={sorted(modeled - declared)}, "
                f"undeclared-model={sorted(declared - modeled)}"
            )

        hpp = read(HPP_PATH)
        referenced = set(re.findall(r"\b(lling_[a-z0-9_]+)\s*\(", hpp))
        info["hpp_references"] = len(referenced)
        undeclared = referenced - declared
        if undeclared:
            failures.append(
                f"include/lling_llang.hpp references undeclared symbols: {sorted(undeclared)}"
            )

    report.run(
        "symbol parity (api.json == ffi.rs == header; hpp subset)", symbol_parity
    )

    # ── 2. enum + ABI/API constant parity ────────────────────────────────────
    def enum_parity(failures: list[str], info: dict[str, object]) -> None:
        status = model.get("enums", {}).get("status", {})
        modeled = {
            str(name): int(value) for name, value in status.get("values", {}).items()
        }
        if (
            status.get("cType") != "LlingStatus"
            or status.get("cPrefix") != "LLING_STATUS_"
        ):
            failures.append("status enum model must name LlingStatus / LLING_STATUS_")

        ffi_source = read(FFI_PATH)
        enum_match = re.search(
            r"pub\s+enum\s+LlingStatus\s*\{(.*?)\n\}", ffi_source, re.DOTALL
        )
        if enum_match is None:
            failures.append("src/ffi.rs does not define pub enum LlingStatus")
            return
        rust_values = {
            camel_to_screaming(name): int(value)
            for name, value in re.findall(
                r"^\s*([A-Z][A-Za-z0-9]*)\s*=\s*(\d+)\s*,",
                enum_match.group(1),
                re.MULTILINE,
            )
        }
        info["rust_variants"] = len(rust_values)
        if rust_values != modeled:
            failures.append(
                f"LlingStatus model/ffi.rs mismatch: model={modeled}, ffi={rust_values}"
            )
        if not re.search(
            r"#\[repr\(u32\)\]\s*(?:#\[[^\]]*\]\s*)*pub\s+enum\s+LlingStatus",
            ffi_source,
        ):
            failures.append("LlingStatus must remain #[repr(u32)]")

        header = read(HEADER_PATH)
        header_enum = re.search(
            r"typedef\s+enum\s+LlingStatus\s*\{(.*?)\}", header, re.DOTALL
        )
        if header_enum is None:
            failures.append(
                "include/lling_llang.h does not declare typedef enum LlingStatus"
            )
            return
        header_values = {
            name: int(value)
            for name, value in re.findall(
                r"LLING_STATUS_([A-Z0-9_]+)\s*=\s*(\d+)", header_enum.group(1)
            )
        }
        info["header_enumerators"] = len(header_values)
        if header_values != modeled:
            failures.append(
                f"LlingStatus model/header mismatch: model={modeled}, header={header_values}"
            )

        for constant, key in (
            ("LLING_ABI_VERSION", "abiVersion"),
            ("LLING_API_REVISION", "apiRevision"),
        ):
            expected = model.get(key)
            rust_constant = re.search(
                rf"pub\s+const\s+{constant}:\s*u32\s*=\s*(\d+)\s*;", ffi_source
            )
            if rust_constant is None:
                failures.append(f"src/ffi.rs does not define pub const {constant}: u32")
            elif int(rust_constant.group(1)) != expected:
                failures.append(
                    f"{constant} mismatch: api.json {key}={expected}, "
                    f"ffi.rs={rust_constant.group(1)}"
                )
            header_constant = re.search(rf"#define\s+{constant}\s+(\d+)u\b", header)
            if header_constant is None:
                failures.append(
                    f"include/lling_llang.h does not define {constant} as an unsigned literal"
                )
            elif int(header_constant.group(1)) != expected:
                failures.append(
                    f"{constant} mismatch: api.json {key}={expected}, "
                    f"header={header_constant.group(1)}"
                )
            info[key] = expected

    report.run("enum + ABI/API constant parity", enum_parity)

    # ── 3. JS facade parity ──────────────────────────────────────────────────
    def js_parity(failures: list[str], info: dict[str, object]) -> None:
        js_model = model.get("javascript", {})
        package = json.loads(read(JS_ROOT / "package.json"))
        if package.get("name") != js_model.get("package"):
            failures.append(
                f"npm package name {package.get('name')!r} != modeled {js_model.get('package')!r}"
            )

        exports = package.get("exports", {})
        modeled_exports = set(js_model.get("exports", []))
        actual_exports = set(exports)
        if actual_exports != modeled_exports:
            failures.append(
                "export-map subpath mismatch: "
                f"missing={sorted(modeled_exports - actual_exports)}, "
                f"unmodeled={sorted(actual_exports - modeled_exports)}"
            )
        resolved = 0
        for subpath, target in exports.items():
            for relative in walk_export_targets(target):
                if not (JS_ROOT / relative).resolve().is_file():
                    failures.append(
                        f"export {subpath!r} target does not exist: {relative}"
                    )
                else:
                    resolved += 1
        info["export_targets_resolved"] = resolved

        dependencies = package.get("dependencies", {})
        modeled_dependencies = js_model.get("dependencies", {})
        if dependencies != modeled_dependencies:
            failures.append(
                f"npm dependency mismatch: package.json={dependencies}, "
                f"api.json={modeled_dependencies}"
            )
        for dependency, version in dependencies.items():
            if dependency.startswith("@vinary-tree/") and not EXACT_SEMVER.fullmatch(
                version
            ):
                failures.append(
                    f"@vinary-tree dependency {dependency} must be an exact pin, found {version!r}"
                )

        # Value-export consistency across the typed and runtime facades.
        expected_exports = set(js_model.get("facadeExports", []))
        dts = read(JS_ROOT / "index.d.ts")
        dts_values = set(
            re.findall(r"^export\s+(?:const|function)\s+(\w+)", dts, re.MULTILINE)
        )
        if dts_values != expected_exports:
            failures.append(
                f"index.d.ts value exports {sorted(dts_values)} != modeled {sorted(expected_exports)}"
            )
        if not re.search(r"^export\s+default\s+\w+;", dts, re.MULTILINE):
            failures.append("index.d.ts lacks a default export")

        runtime_imports = {
            "facades/native.mjs": '"@vinary-tree/javascript-runtime"',
            "facades/wasm.mjs": '"@vinary-tree/javascript-runtime/wasm"',
            "facades/wasi.mjs": '"@vinary-tree/javascript-runtime/wasi"',
        }
        for relative, runtime_import in runtime_imports.items():
            source = read(JS_ROOT / relative)
            named = set(
                re.findall(
                    r"^export\s+(?:const|function)\s+(\w+)", source, re.MULTILINE
                )
            )
            if named != expected_exports:
                failures.append(
                    f"{relative} exports {sorted(named)} != modeled {sorted(expected_exports)}"
                )
            if "export default" not in source:
                failures.append(f"{relative} lacks a default export")
            if runtime_import not in source:
                failures.append(
                    f"{relative} must import the shared runtime {runtime_import}"
                )
            for guard in ("assertSameRuntime", "assertWfstResource"):
                if guard not in source:
                    failures.append(f"{relative} lacks the {guard} guard")

        cjs = read(JS_ROOT / "facades" / "native.cjs")
        exports_match = re.search(r"module\.exports\s*=\s*\{([^}]*)\}", cjs)
        if exports_match is None:
            failures.append(
                "facades/native.cjs does not assign a module.exports object"
            )
        else:
            cjs_names = set()
            for entry in exports_match.group(1).split(","):
                entry = entry.strip()
                if not entry or entry.startswith("..."):
                    continue
                cjs_names.add(entry.split(":", 1)[0].strip())
            missing = (expected_exports | {"default"}) - cjs_names
            if missing:
                failures.append(
                    f"facades/native.cjs exports are missing {sorted(missing)}"
                )
        for relative in ("facades/typescript.mjs", "facades/clojurescript.mjs"):
            source = read(JS_ROOT / relative)
            if (
                'export * from "./native.mjs"' not in source
                or 'export { default } from "./native.mjs"' not in source
            ):
                failures.append(
                    f"{relative} must re-export ./native.mjs (names and default)"
                )
        for relative in ("facades/typescript.cjs", "facades/clojurescript.cjs"):
            source = read(JS_ROOT / relative)
            if 'module.exports = require("./native.cjs")' not in source:
                failures.append(f"{relative} must re-export ./native.cjs")

        cljs = read(JS_ROOT / "cljs" / "vinary_tree" / "lling_llang.cljs")
        defns = set(re.findall(r"\(defn\s+([a-z0-9!?-]+)", cljs))
        modeled_cljs = set(js_model.get("cljsExports", []))
        if defns != modeled_cljs:
            failures.append(
                "ClojureScript facade mismatch: "
                f"missing={sorted(modeled_cljs - defns)}, unmodeled={sorted(defns - modeled_cljs)}"
            )
        if f"(ns {js_model.get('cljsNamespace')}" not in cljs:
            failures.append(
                f"ClojureScript facade must declare (ns {js_model.get('cljsNamespace')} ...)"
            )
        native_references = set(re.findall(r"\(native/(\w+)", cljs))
        stray = native_references - expected_exports
        if stray:
            failures.append(
                f"ClojureScript facade calls unexported native names: {sorted(stray)}"
            )
        dts_members = set(
            re.findall(r"^\s+(?:readonly\s+)?(\w+)\s*\(", dts, re.MULTILINE)
        )
        method_references = set(re.findall(r"\(\.(\w+)\s", cljs))
        unknown = method_references - dts_members
        if unknown:
            failures.append(
                f"ClojureScript facade calls methods absent from index.d.ts: {sorted(unknown)}"
            )
        info["cljs_exports"] = len(defns)

    report.run("JS facade parity (exports, pins, d.ts/mjs/cjs/cljs)", js_parity)

    # ── 4. workflow version-derivation guard [LLING-B1] ─────────────────────
    def workflow_guard(failures: list[str], info: dict[str, object]) -> None:
        workflow = read(WORKFLOW_PATH)
        hardcoded = re.findall(r"dist/lling-llang-\d+\.\d+\.\d+", workflow)
        if hardcoded:
            failures.append(
                "release-bindings.yml hardcodes the staged package version instead of "
                f"deriving it from Cargo.toml: {sorted(set(hardcoded))} [LLING-B1]"
            )
        if re.search(r"version=\$\(.*Cargo\.toml.*\)", workflow) is None:
            failures.append(
                "release-bindings.yml must derive version from Cargo.toml "
                "(the same source scripts/stage-native-package.sh reads)"
            )
        if "stage-native-package.sh" not in workflow:
            failures.append(
                "release-bindings.yml must stage native packages via "
                "scripts/stage-native-package.sh"
            )
        info["hardcoded_dist_literals"] = len(hardcoded)

    report.run("workflow version-derivation guard [LLING-B1]", workflow_guard)

    # ── 5. identity + sibling-symbol guard over publishable files ───────────
    def identity_guard(failures: list[str], info: dict[str, object]) -> None:
        forbidden_identities = ("f1r3fly", "universal-automata", "universal_automata")
        forbidden_symbols = [
            re.compile(rf"\b{re.escape(prefix)}")
            for prefix in model.get("forbiddenFacadeSymbols", [])
        ]
        scanned = 0
        for path in publishable_files():
            source = path.read_text(encoding="utf-8", errors="ignore")
            lowered = source.lower()
            scanned += 1
            for identity in forbidden_identities:
                if identity in lowered:
                    failures.append(
                        f"unrelated identity {identity!r} in {path.relative_to(ROOT)}"
                    )
            for pattern in forbidden_symbols:
                if pattern.search(source):
                    failures.append(
                        f"sibling-owned symbol family {pattern.pattern!r} in {path.relative_to(ROOT)}"
                    )
        info["files_scanned"] = scanned

    report.run("identity + sibling-symbol guard", identity_guard)

    print(report.render_json() if arguments.json else report.render_human())
    return 0 if report.passed else 1


if __name__ == "__main__":
    sys.exit(main())
