#!/usr/bin/env python3
"""Binding-surface drift gate for lling-llang.

Verifies, against the committed binding model ``bindings/api.json``:

  1. symbol parity      -- api.json cFunctions == ``pub extern "C" fn lling_*``
                           exports in src/ffi.rs == declarations in
                           include/lling_llang.h; the C++ facade
                           include/lling_llang.hpp references only declared
                           symbols.
  2. enum/constant      -- LlingLlangStatus discriminants and the
     parity                LLING_ABI_VERSION / LLING_LLANG_API_REVISION constants
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
  python3 scripts/check-bindings.py --self-test-stack-safety

Exit status: 0 = all checks pass, 1 = at least one failure, 2 = usage or
model-loading error.
"""

from __future__ import annotations

import argparse
import ast
import json
import random
import re
import subprocess
import sys
import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.10 compatibility
    import tomli as tomllib

ROOT = Path(__file__).resolve().parents[1]
MODEL_PATH = ROOT / "bindings" / "api.json"
FFI_PATH = ROOT / "src" / "ffi.rs"
FFI_MODULE_ROOT = ROOT / "src" / "ffi"
HEADER_PATH = ROOT / "include" / "lling_llang.h"
HPP_PATH = ROOT / "include" / "lling_llang.hpp"
JS_ROOT = ROOT / "bindings" / "javascript"
PYTHON_ROOT = ROOT / "bindings" / "python"
JULIA_ROOT = ROOT / "bindings" / "julia" / "LlingLlang"
RAKU_ROOT = ROOT / "bindings" / "raku"
RAKU_ABI_GENERATOR = ROOT / "scripts" / "generate-raku-abi.py"
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "release-bindings.yml"

SKIP_DIR_PARTS = {
    ".git",
    ".precomp",
    "build",
    "node_modules",
    "obj",
    "target",
}


def read_ffi_surface() -> str:
    """Read the root C facade and every deliberately split child module."""
    children = sorted(FFI_MODULE_ROOT.glob("*.rs")) if FFI_MODULE_ROOT.is_dir() else []
    return "\n".join(read(path) for path in [FFI_PATH, *children])


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
    targets: list[str] = []
    pending = [value]
    while pending:
        current = pending.pop()
        if isinstance(current, str):
            targets.append(current)
        elif isinstance(current, dict):
            pending.extend(reversed(current.values()))
    return targets


def recursive_walk_export_targets_reference(value: object) -> list[str]:
    """Bounded test-only specification for export-target traversal."""
    if isinstance(value, str):
        return [value]
    if isinstance(value, dict):
        targets: list[str] = []
        for nested in value.values():
            targets.extend(recursive_walk_export_targets_reference(nested))
        return targets
    return []


def generated_export_value(generator: random.Random, depth: int) -> object:
    """Generate a bounded JSON-like export value with deterministic entropy."""
    choice = generator.randrange(4 if depth else 3)
    if choice == 0:
        return f"./target-{generator.randrange(32)}.js"
    if choice == 1:
        return generator.randrange(1024)
    if choice == 2:
        return None
    return {
        f"condition-{index}": generated_export_value(generator, depth - 1)
        for index in range(generator.randrange(5))
    }


def self_test_stack_safety() -> int:
    """Check ordered refinement and 100,000-level bounded-stack traversal."""
    generator = random.Random(0x11_1A_6B)
    for case in range(4096):
        value = generated_export_value(generator, case % 9)
        expected = recursive_walk_export_targets_reference(value)
        actual = walk_export_targets(value)
        if actual != expected:
            raise AssertionError(
                f"export traversal mismatch in generated case {case}: "
                f"expected {expected!r}, got {actual!r}"
            )

    def deep_worker() -> None:
        value: object = "./deep-target.js"
        for _ in range(100_000):
            value = {"default": value}
        if walk_export_targets(value) != ["./deep-target.js"]:
            raise AssertionError("deep export traversal changed its leaf or order")

        # Dismantle the synthetic ownership chain iteratively so this test
        # measures the traversal rather than CPython container finalization.
        while isinstance(value, dict) and value:
            nested = next(iter(value.values()))
            value.clear()
            value = nested

    previous_stack_size = threading.stack_size()
    threading.stack_size(256 * 1024)
    try:
        with ThreadPoolExecutor(
            max_workers=1, thread_name_prefix="deep-binding-export-walk"
        ) as executor:
            executor.submit(deep_worker).result()
    finally:
        threading.stack_size(previous_stack_size)

    print(
        "check-bindings: 4096 ordered properties and 100,000-level/256-KiB traversal verified"
    )
    return 0


def publishable_files() -> list[Path]:
    files: list[Path] = []
    for relative in ("bindings", "include", "cmake", "pkgconfig"):
        root = ROOT / relative
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*")):
            if not path.is_file():
                continue
            # Only repository-relative build/cache directories are excluded.
            # Absolute paths may legitimately place the entire checkout below
            # a parent called ``target`` (for example an isolated validation
            # clone); treating those parent components as in-repository parts
            # silently reduced this security scan to zero files.
            if any(part in SKIP_DIR_PARTS for part in path.relative_to(ROOT).parts):
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
    parser.add_argument(
        "--self-test-stack-safety",
        action="store_true",
        help="run ordered-refinement and bounded-stack traversal controls",
    )
    arguments = parser.parse_args()

    if arguments.self_test_stack_safety:
        return self_test_stack_safety()

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
        python = tomllib.loads(read(PYTHON_ROOT / "pyproject.toml"))
        python_project = python.get("project", {})
        python_version = cargo_version.replace("-rc.", "rc")
        if python_project.get("name") != model["packages"].get("pypi"):
            failures.append("Python distribution name does not match bindings/api.json")
        if python_project.get("version") != python_version:
            failures.append(
                f"Python version {python_project.get('version')!r} "
                f"!= PEP 440 spelling {python_version!r}"
            )
        if python_project.get("dependencies") != [
            f"vinary-tree-interop=={python_version}"
        ]:
            failures.append(
                "Python distribution must exact-pin the coordinated "
                "vinary-tree-interop release"
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

        ffi_source = read_ffi_surface()
        exported = set(
            re.findall(
                r'pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+(lling_[a-z0-9_]+)\s*\(',
                ffi_source,
            )
        )
        info["rust_exports"] = len(exported)
        if exported != modeled:
            failures.append(
                "C symbol model / src/ffi surface mismatch: "
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
            status.get("cType") != "LlingLlangStatus"
            or status.get("cPrefix") != "LLING_STATUS_"
        ):
            failures.append(
                "status enum model must name LlingLlangStatus / LLING_STATUS_"
            )

        ffi_source = read(FFI_PATH)
        enum_match = re.search(
            r"pub\s+enum\s+LlingLlangStatus\s*\{(.*?)\n\}", ffi_source, re.DOTALL
        )
        if enum_match is None:
            failures.append("src/ffi.rs does not define pub enum LlingLlangStatus")
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
                f"LlingLlangStatus model/ffi.rs mismatch: model={modeled}, ffi={rust_values}"
            )
        if not re.search(
            r"#\[repr\(u32\)\]\s*(?:#\[[^\]]*\]\s*)*pub\s+enum\s+LlingLlangStatus",
            ffi_source,
        ):
            failures.append("LlingLlangStatus must remain #[repr(u32)]")

        header = read(HEADER_PATH)
        header_enum = re.search(
            r"typedef\s+enum\s+LlingLlangStatus\s*\{(.*?)\}", header, re.DOTALL
        )
        if header_enum is None:
            failures.append(
                "include/lling_llang.h does not declare typedef enum LlingLlangStatus"
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
                f"LlingLlangStatus model/header mismatch: model={modeled}, header={header_values}"
            )

        for constant, key in (
            ("LLING_ABI_VERSION", "abiVersion"),
            ("LLING_LLANG_API_REVISION", "apiRevision"),
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
                    f"{relative} must import the umbrella runtime {runtime_import}"
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

    # ── 4. Python/Julia/Raku facade and generated-ABI parity ────────────────
    def julia_raku_parity(failures: list[str], info: dict[str, object]) -> None:
        cargo = tomllib.loads(read(ROOT / "Cargo.toml"))
        version = cargo["package"]["version"]
        expected_status = {
            str(name): int(value)
            for name, value in model["enums"]["status"]["values"].items()
        }

        python_abi = read(PYTHON_ROOT / "src" / "lling_llang" / "_abi.py")
        python_symbols = set(re.findall(r'"(lling_[a-z0-9_]+)"', python_abi)) - {
            "lling_llang"
        }
        modeled = {item["name"] for item in model["cFunctions"]}
        if python_symbols != modeled:
            failures.append(
                "Python native symbol drift: "
                f"missing={sorted(modeled - python_symbols)}, "
                f"unmodeled={sorted(python_symbols - modeled)}"
            )
        python_status_block = re.search(
            r"class Status\(IntEnum\):(.*?)(?:\n\nclass )",
            python_abi,
            re.DOTALL,
        )
        if python_status_block is None:
            failures.append("Python ABI does not define Status")
        else:
            python_status = {
                name: int(value)
                for name, value in re.findall(
                    r"^\s+([A-Z][A-Z0-9_]+)\s*=\s*(\d+)",
                    python_status_block.group(1),
                    re.MULTILINE,
                )
            }
            if python_status != expected_status:
                failures.append(
                    f"Python Status mismatch: {python_status} != {expected_status}"
                )
        python_init = read(PYTHON_ROOT / "src" / "lling_llang" / "__init__.py")
        init_tree = ast.parse(python_init)
        exported: set[str] = set()
        for statement in init_tree.body:
            if not isinstance(statement, ast.Assign):
                continue
            if not any(
                isinstance(target, ast.Name) and target.id == "__all__"
                for target in statement.targets
            ):
                continue
            if isinstance(statement.value, (ast.List, ast.Tuple)):
                exported = {
                    element.value
                    for element in statement.value.elts
                    if isinstance(element, ast.Constant)
                    and isinstance(element.value, str)
                }
        required_python_exports = {
            "Wfst",
            "WfstBuilder",
            "compose",
            "import_wfst",
            "ScalarWfstResource",
            "LatticeResource",
            "LatticeValue",
            "SemiringResource",
            "SemiringContext",
            "Cancellation",
        }
        if not required_python_exports <= exported:
            failures.append(
                "Python facade exports are missing "
                f"{sorted(required_python_exports - exported)}"
            )
        for relative in (
            "LICENSE",
            "MANIFEST.in",
            "README.md",
            "examples/custom_providers.py",
            "pyproject.toml",
            "pyrightconfig.json",
            "setup.py",
            "src/lling_llang/py.typed",
            "tests/test_api.py",
        ):
            read(PYTHON_ROOT / relative)

        julia_project = tomllib.loads(read(JULIA_ROOT / "Project.toml"))
        if julia_project.get("name") != model["packages"].get("julia"):
            failures.append("Julia project name does not match bindings/api.json")
        if julia_project.get("version") != version:
            failures.append(
                f"Julia version {julia_project.get('version')!r} != Cargo {version!r}"
            )
        julia_abi = read(JULIA_ROOT / "src" / "GeneratedAbi.jl")
        julia_status = {
            name: int(value)
            for name, value in re.findall(
                r"^\s*STATUS_([A-Z0-9_]+)\s*=\s*(\d+)", julia_abi, re.MULTILINE
            )
        }
        if julia_status != expected_status:
            failures.append(
                f"Julia generated Status mismatch: {julia_status} != {expected_status}"
            )
        for constant, key in (
            ("ABI_VERSION", "abiVersion"),
            ("API_REVISION", "apiRevision"),
        ):
            match = re.search(rf"const\s+{constant}\s*=\s*UInt32\((\d+)\)", julia_abi)
            if match is None or int(match.group(1)) != model[key]:
                failures.append(f"Julia {constant} does not match api.json {key}")
        julia_source = read(JULIA_ROOT / "src" / "LlingLlang.jl")
        julia_symbols = set(re.findall(r"native\(:(lling_[a-z0-9_]+)\)", julia_source))

        raku_meta = json.loads(read(RAKU_ROOT / "META6.json"))
        if raku_meta.get("name") != model["packages"].get("zef"):
            failures.append("Raku distribution name does not match bindings/api.json")
        if raku_meta.get("version") != version:
            failures.append(
                f"Raku version {raku_meta.get('version')!r} != Cargo {version!r}"
            )
        raku_family_version = version.replace("-rc.", ".rc.")
        if (
            f"Vinary-Tree-Interop:ver<{raku_family_version}>:auth<zef:vinary-tree>"
            not in raku_meta.get("depends", [])
        ):
            failures.append(
                "Raku distribution does not exact-pin the coordinated "
                "Vinary-Tree-Interop release candidate"
            )
        raku_abi = read(RAKU_ROOT / "lib" / "Lling" / "Llang" / "GeneratedAbi.rakumod")
        status_match = re.search(
            r"our enum Status[^\(]*\((.*?)\);", raku_abi, re.DOTALL
        )
        if status_match is None:
            failures.append("Raku generated ABI does not define Status")
            status_source = ""
        else:
            status_source = status_match.group(1)
        raku_status = {
            name.replace("-", "_"): int(value)
            for name, value in re.findall(
                r"^\s*([A-Z][A-Z0-9-]+)\s*=>\s*(\d+)",
                status_source,
                re.MULTILINE,
            )
        }
        if raku_status != expected_status:
            failures.append(
                f"Raku generated Status mismatch: {raku_status} != {expected_status}"
            )
        for constant, key in (
            ("ABI-VERSION", "abiVersion"),
            ("API-REVISION", "apiRevision"),
        ):
            match = re.search(
                rf"constant\s+{constant}\s+is\s+export(?:\(:abi\))?\s*=\s*(\d+)",
                raku_abi,
            )
            if match is None or int(match.group(1)) != model[key]:
                failures.append(f"Raku {constant} does not match api.json {key}")
        raku_source = read(RAKU_ROOT / "lib" / "Lling" / "Llang.rakumod")
        raku_symbols = set(re.findall(r"symbol\('(lling_[a-z0-9_]+)'\)", raku_abi))
        facade_symbols = set(re.findall(r"symbol\('(lling_[a-z0-9_]+)'\)", raku_source))
        if facade_symbols:
            failures.append(
                "Raku facade must not handwrite project NativeCall declarations: "
                f"{sorted(facade_symbols)}"
            )
        if "repr('CStruct')" in raku_source:
            failures.append("Raku facade must not handwrite generated CStruct layouts")

        expected_julia_symbols = modeled - {
            "lling_resource_release",
            "lling_wfst_import_ref",
            "lling_wfst_compose_refs",
        }
        if julia_symbols != expected_julia_symbols:
            failures.append(
                "Julia native symbol drift: "
                f"missing={sorted(expected_julia_symbols - julia_symbols)}, "
                f"unmodeled={sorted(julia_symbols - expected_julia_symbols)}"
            )
        expected_raku_symbols = {
            function["name"]
            for function in model["cFunctions"]
            if function["raku"]["bind"]
        } | {
            function["name"]
            for function in model["rakuAbi"]["providerShim"]["functions"]
            if function["raku"]["bind"]
        }
        if raku_symbols != expected_raku_symbols:
            failures.append(
                "Raku native symbol drift: "
                f"missing={sorted(expected_raku_symbols - raku_symbols)}, "
                f"unmodeled={sorted(raku_symbols - expected_raku_symbols)}"
            )
        for relative in (
            "Build.rakumod",
            "build-provider.raku",
            "cbits/provider.c",
            "doc/Lling-Llang.rakudoc",
            "t/01-conformance.rakutest",
        ):
            read(RAKU_ROOT / relative)
        info["julia_native_symbols"] = len(julia_symbols)
        info["raku_native_symbols"] = len(raku_symbols)
        expected_provider_languages = ["Python", "Julia", "Raku"]
        provider_languages = model["objects"]["HostScalarWfstProvider"]["languages"]
        if provider_languages != expected_provider_languages:
            failures.append(
                "host-provider language model must be exactly "
                f"{expected_provider_languages}, found {provider_languages}"
            )
        info["python_native_symbols"] = len(python_symbols)
        info["host_provider_languages"] = len(provider_languages)

    report.run("Python/Julia/Raku facade + generated ABI parity", julia_raku_parity)

    def raku_generation_gate(failures: list[str], info: dict[str, object]) -> None:
        for mode in ("--check", "--self-test"):
            result = subprocess.run(
                [sys.executable, str(RAKU_ABI_GENERATOR), mode],
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
            )
            if result.returncode != 0:
                details = (result.stderr or result.stdout).strip()
                failures.append(f"Raku ABI generator {mode} failed: {details}")
        info["modeled_core_functions"] = len(model["cFunctions"])
        info["modeled_provider_functions"] = len(
            model["rakuAbi"]["providerShim"]["functions"]
        )
        info["modeled_provider_callbacks"] = len(
            model["rakuAbi"]["providerShim"]["callbacks"]
        )

    report.run(
        "Raku authoritative ABI generation + negative control", raku_generation_gate
    )

    # ── 5. workflow version-derivation guard [LLING-B1] ─────────────────────
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

    # ── 6. identity + sibling-symbol guard over publishable files ───────────
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
