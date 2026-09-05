#!/usr/bin/env python3
"""Generate and verify lling-llang's complete Raku NativeCall ABI.

``bindings/api.json`` is the language-neutral source of truth for public C
signatures, ABI-v2 layouts, provider callbacks, versions, capabilities,
ownership, nullability, and threading.  The public header and the Raku
provider shim are independent witnesses: generation fails if either drifts
from the model, and ``--check`` fails if the committed Raku module is stale.
"""

from __future__ import annotations

import argparse
import copy
import json
import re
import sys
from pathlib import Path
from typing import NoReturn

ROOT = Path(__file__).resolve().parents[1]
MODEL_PATH = ROOT / "bindings" / "api.json"
HEADER_PATH = ROOT / "include" / "lling_llang.h"
PROVIDER_PATH = ROOT / "bindings" / "raku" / "cbits" / "provider.c"
OUTPUT_PATH = (
    ROOT / "bindings" / "raku" / "lib" / "Lling" / "Llang" / "GeneratedAbi.rakumod"
)


def abort(message: str) -> NoReturn:
    raise SystemExit(f"generate-raku-abi: {message}")


def load_model() -> dict:
    try:
        model = json.loads(MODEL_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        abort(f"cannot read {MODEL_PATH.relative_to(ROOT)}: {error}")
    if not isinstance(model, dict):
        abort("bindings/api.json must contain a JSON object")
    return model


def normalize_c(declaration: str) -> str:
    declaration = re.sub(r"\s+", " ", declaration.strip())
    declaration = re.sub(r"\s*\*\s*", "*", declaration)
    declaration = re.sub(r"\s*,\s*", ",", declaration)
    declaration = re.sub(r"\(\s*", "(", declaration)
    declaration = re.sub(r"\s*\)", ")", declaration)
    return declaration


def c_parameter(parameter: dict) -> str:
    return f"{parameter['cType']} {parameter['name']}"


def c_prototype(function: dict, export_macro: str) -> str:
    parameters = ", ".join(c_parameter(item) for item in function["parameters"])
    return (
        f"{export_macro} {function['return']['cType']} {function['name']}"
        f"({parameters or 'void'});"
    )


def exported_prototypes(path: Path, export_macro: str, prefix: str) -> dict[str, str]:
    try:
        source = path.read_text(encoding="utf-8")
    except OSError as error:
        abort(f"cannot read {path.relative_to(ROOT)}: {error}")
    source = re.sub(r"/\*.*?\*/", " ", source, flags=re.DOTALL)
    pattern = (
        rf"{re.escape(export_macro)}\s+(?P<return>[^;(]+?)\s+"
        rf"(?P<name>{re.escape(prefix)}[a-z0-9_]+)\s*"
        r"\((?P<parameters>.*?)\)\s*;?\s*\{?"
    )
    result: dict[str, str] = {}
    for match in re.finditer(pattern, source, flags=re.DOTALL):
        name = match.group("name")
        if name in result:
            abort(f"duplicate declaration for {name} in {path.relative_to(ROOT)}")
        result[name] = normalize_c(
            f"{export_macro} {match.group('return')} {name}"
            f"({match.group('parameters')});"
        )
    return result


def callback_prototypes(path: Path, prefix: str) -> dict[str, str]:
    try:
        source = path.read_text(encoding="utf-8")
    except OSError as error:
        abort(f"cannot read {path.relative_to(ROOT)}: {error}")
    source = re.sub(r"/\*.*?\*/", " ", source, flags=re.DOTALL)
    pattern = (
        r"typedef\s+(?P<return>[^;(]+?)\s+\(\*"
        rf"(?P<name>{re.escape(prefix)}[A-Za-z0-9_]+)\)\s*"
        r"\((?P<parameters>.*?)\)\s*;"
    )
    result: dict[str, str] = {}
    for match in re.finditer(pattern, source, flags=re.DOTALL):
        name = match.group("name")
        raw_parameters = match.group("parameters").strip()
        parameters = []
        if raw_parameters and raw_parameters != "void":
            for raw_parameter in raw_parameters.split(","):
                raw_parameter = raw_parameter.strip()
                named = re.fullmatch(
                    r"(?P<type>.+(?:\s|\*))(?P<name>[A-Za-z_][A-Za-z0-9_]*)",
                    raw_parameter,
                )
                parameters.append(
                    normalize_c(named.group("type") if named else raw_parameter)
                )
        rendered_parameters = ",".join(parameters) or "void"
        result[name] = normalize_c(
            f"typedef {match.group('return')} (*{name})({rendered_parameters});"
        )
    return result


def callback_prototype(callback: dict) -> str:
    parameters = ", ".join(item["cType"] for item in callback["parameters"])
    return (
        f"typedef {callback['return']['cType']} (*{callback['name']})"
        f"({parameters or 'void'});"
    )


def header_enums(path: Path) -> dict[str, dict[str, int]]:
    try:
        source = path.read_text(encoding="utf-8")
    except OSError as error:
        abort(f"cannot read {path.relative_to(ROOT)}: {error}")
    result: dict[str, dict[str, int]] = {}
    pattern = (
        r"typedef\s+enum\s+(?P<tag>[A-Za-z_][A-Za-z0-9_]*)\s*"
        r"\{(?P<body>.*?)\}\s*(?P<alias>[A-Za-z_][A-Za-z0-9_]*)\s*;"
    )
    for match in re.finditer(pattern, source, flags=re.DOTALL):
        tag = match.group("tag")
        alias = match.group("alias")
        if tag != alias:
            abort(f"enum tag/alias drift in {path.relative_to(ROOT)}: {tag}/{alias}")
        values = {
            name: int(value)
            for name, value in re.findall(
                r"([A-Z][A-Z0-9_]*)\s*=\s*(\d+)", match.group("body")
            )
        }
        result[alias] = values
    return result


def require_text(item: dict, field: str, where: str) -> str:
    value = item.get(field)
    if not isinstance(value, str) or not value:
        abort(f"{where}.{field} must be a non-empty string")
    return value


def validate_parameter(parameter: dict, where: str) -> None:
    for field in ("name", "cType", "rakuType", "ownership", "nullability"):
        require_text(parameter, field, where)
    if parameter.get("direction") not in {"in", "out", "inout"}:
        abort(f"{where}.direction must be in, out, or inout")
    if parameter.get("rakuPassing", "value") not in {"value", "rw"}:
        abort(f"{where}.rakuPassing must be value or rw")


def validate_function(function: dict, where: str, api_revision: int) -> None:
    name = require_text(function, "name", where)
    require_text(function, "rakuName", where)
    require_text(function, "threading", where)
    require_text(function, "capability", where)
    since = function.get("sinceApiRevision")
    if not isinstance(since, int) or not 1 <= since <= api_revision:
        abort(f"{name}.sinceApiRevision must be within 1..{api_revision}")
    returned = function.get("return")
    if not isinstance(returned, dict):
        abort(f"{name}.return must be an object")
    for field in ("cType", "ownership", "nullability"):
        require_text(returned, field, f"{name}.return")
    raku_return = returned.get("rakuType")
    if raku_return is not None and (
        not isinstance(raku_return, str) or not raku_return
    ):
        abort(f"{name}.return.rakuType must be null or a non-empty string")
    parameters = function.get("parameters")
    if not isinstance(parameters, list):
        abort(f"{name}.parameters must be an array")
    names: set[str] = set()
    for index, parameter in enumerate(parameters):
        if not isinstance(parameter, dict):
            abort(f"{name}.parameters[{index}] must be an object")
        validate_parameter(parameter, f"{name}.parameters[{index}]")
        parameter_name = parameter["name"]
        if parameter_name in names:
            abort(f"{name} has duplicate parameter {parameter_name}")
        names.add(parameter_name)
    binding = function.get("raku")
    if not isinstance(binding, dict) or not isinstance(binding.get("bind"), bool):
        abort(f"{name}.raku.bind must be Boolean")
    if not binding["bind"]:
        require_text(binding, "reason", f"{name}.raku")


def validate_model(model: dict) -> None:
    api_revision = model.get("apiRevision")
    if not isinstance(api_revision, int) or api_revision < 1:
        abort("apiRevision must be a positive integer")
    raku = model.get("rakuAbi")
    if not isinstance(raku, dict):
        abort("rakuAbi must be an object")
    if raku.get("module") != "Lling::Llang::GeneratedAbi":
        abort("rakuAbi.module must be Lling::Llang::GeneratedAbi")
    for library_key in ("nativeLibrary", "providerLibrary"):
        library = raku.get(library_key)
        expected = {"environment", "linux", "macos", "windows"}
        if library_key == "providerLibrary":
            expected.add("resource")
        if not isinstance(library, dict) or set(library) != expected:
            abort(f"rakuAbi.{library_key} must define exactly {sorted(expected)}")
        for key in expected:
            require_text(library, key, f"rakuAbi.{library_key}")

    ffi_types = raku.get("ffiTypes")
    if not isinstance(ffi_types, dict) or not ffi_types:
        abort("rakuAbi.ffiTypes must be a non-empty object")
    for name, item in ffi_types.items():
        if not isinstance(item, dict):
            abort(f"rakuAbi.ffiTypes.{name} must be an object")
        for field in ("kind", "rakuType", "ownership"):
            require_text(item, field, f"rakuAbi.ffiTypes.{name}")

    structs = raku.get("structs")
    if not isinstance(structs, dict) or not structs:
        abort("rakuAbi.structs must be a non-empty object")
    layouts = model.get("typedAbiV2", {}).get("layouts", {})
    for name, item in structs.items():
        where = f"rakuAbi.structs.{name}"
        if not isinstance(item, dict):
            abort(f"{where} must be an object")
        require_text(item, "cType", where)
        require_text(item, "layoutKey", where)
        support = item.get("rakuSupport")
        if support not in {"cstruct", "pointer-only"}:
            abort(f"{where}.rakuSupport must be cstruct or pointer-only")
        layout_key = item["layoutKey"]
        if item.get("size") != layouts.get(layout_key):
            abort(f"{where}.size must match typedAbiV2.layouts.{layout_key}")
        fields = item.get("fields")
        if not isinstance(fields, list) or not fields:
            abort(f"{where}.fields must be a non-empty array")
        for index, field in enumerate(fields):
            if not isinstance(field, dict):
                abort(f"{where}.fields[{index}] must be an object")
            for key in ("name", "cName", "cType"):
                require_text(field, key, f"{where}.fields[{index}]")
            if support == "cstruct":
                require_text(field, "rakuType", f"{where}.fields[{index}]")
        if support == "pointer-only":
            require_text(item, "reason", where)

    enums = model.get("enums")
    if not isinstance(enums, dict) or not enums:
        abort("enums must be a non-empty object")
    for name, item in enums.items():
        where = f"enums.{name}"
        if not isinstance(item, dict):
            abort(f"{where} must be an object")
        for field in ("cType", "cPrefix", "rustType", "rakuName"):
            require_text(item, field, where)
        suffix = item.get("cSuffix")
        if not isinstance(suffix, str):
            abort(f"{where}.cSuffix must be a string")
        since = item.get("sinceApiRevision")
        if not isinstance(since, int) or not 1 <= since <= api_revision:
            abort(f"{where}.sinceApiRevision is invalid")
        values = item.get("values")
        if not isinstance(values, dict) or not values:
            abort(f"{where}.values must be a non-empty object")
        if not all(isinstance(value, int) for value in values.values()):
            abort(f"{where}.values must contain integer discriminants")

    actual_enums = header_enums(HEADER_PATH)
    modeled_enum_types = {item["cType"] for item in enums.values()}
    if set(actual_enums) != modeled_enum_types:
        abort(
            "C enum/model drift: "
            f"missing={sorted(modeled_enum_types - set(actual_enums))}, "
            f"extra={sorted(set(actual_enums) - modeled_enum_types)}"
        )
    for key, item in enums.items():
        expected_values = {
            f"{item['cPrefix']}{name}{item['cSuffix']}": value
            for name, value in item["values"].items()
        }
        actual_values = actual_enums[item["cType"]]
        if expected_values != actual_values:
            abort(
                f"C enum drift for {key}: model={expected_values}, "
                f"header={actual_values}"
            )

    core_functions = model.get("cFunctions")
    if not isinstance(core_functions, list) or not core_functions:
        abort("cFunctions must be a non-empty array")
    core_names: set[str] = set()
    for index, function in enumerate(core_functions):
        if not isinstance(function, dict):
            abort(f"cFunctions[{index}] must be an object")
        validate_function(function, f"cFunctions[{index}]", api_revision)
        name = function["name"]
        if not re.fullmatch(r"lling_[a-z0-9_]+", name):
            abort(f"{name} is not an lling-llang C symbol")
        if name in core_names:
            abort(f"duplicate modeled function {name}")
        core_names.add(name)

    provider = raku.get("providerShim")
    if not isinstance(provider, dict):
        abort("rakuAbi.providerShim must be an object")
    for field in ("abiVersion", "apiRevision"):
        if not isinstance(provider.get(field), int) or provider[field] < 1:
            abort(f"rakuAbi.providerShim.{field} must be a positive integer")
    capabilities = provider.get("capabilities")
    if not isinstance(capabilities, dict) or not capabilities:
        abort("rakuAbi.providerShim.capabilities must be a non-empty object")
    if not all(isinstance(value, int) and value > 0 for value in capabilities.values()):
        abort("provider capability values must be positive integers")
    callbacks = provider.get("callbacks")
    if not isinstance(callbacks, list) or not callbacks:
        abort("rakuAbi.providerShim.callbacks must be a non-empty array")
    callback_names: set[str] = set()
    for index, callback in enumerate(callbacks):
        where = f"rakuAbi.providerShim.callbacks[{index}]"
        if not isinstance(callback, dict):
            abort(f"{where} must be an object")
        name = require_text(callback, "name", where)
        require_text(callback, "threading", where)
        returned = callback.get("return")
        if not isinstance(returned, dict):
            abort(f"{where}.return must be an object")
        require_text(returned, "cType", f"{where}.return")
        require_text(returned, "ownership", f"{where}.return")
        require_text(returned, "nullability", f"{where}.return")
        raku_return = returned.get("rakuType")
        if raku_return is not None and (
            not isinstance(raku_return, str) or not raku_return
        ):
            abort(f"{where}.return.rakuType must be null or a non-empty string")
        parameters = callback.get("parameters")
        if not isinstance(parameters, list):
            abort(f"{where}.parameters must be an array")
        for parameter_index, parameter in enumerate(parameters):
            if not isinstance(parameter, dict):
                abort(f"{where}.parameters[{parameter_index}] must be an object")
            validate_parameter(parameter, f"{where}.parameters[{parameter_index}]")
        if name in callback_names:
            abort(f"duplicate provider callback {name}")
        callback_names.add(name)
    provider_functions = provider.get("functions")
    if not isinstance(provider_functions, list) or not provider_functions:
        abort("rakuAbi.providerShim.functions must be a non-empty array")
    provider_names: set[str] = set()
    for index, function in enumerate(provider_functions):
        if not isinstance(function, dict):
            abort(f"rakuAbi.providerShim.functions[{index}] must be an object")
        validate_function(function, f"providerShim.functions[{index}]", api_revision)
        name = function["name"]
        if name in provider_names:
            abort(f"duplicate provider function {name}")
        provider_names.add(name)
        for parameter in function["parameters"]:
            callback = parameter.get("callback")
            if callback is not None and callback not in callback_names:
                abort(
                    f"{name}.{parameter['name']} references unknown callback {callback}"
                )

    actual_core = exported_prototypes(HEADER_PATH, "LLING_LLANG_API", "lling_")
    if set(actual_core) != core_names:
        abort(
            "C header/model symbol drift: "
            f"missing={sorted(core_names - set(actual_core))}, "
            f"extra={sorted(set(actual_core) - core_names)}"
        )
    for function in core_functions:
        expected = normalize_c(c_prototype(function, "LLING_LLANG_API"))
        actual = actual_core[function["name"]]
        if expected != actual:
            abort(
                f"C signature drift for {function['name']}:\n"
                f"  model:  {expected}\n  header: {actual}"
            )

    actual_provider = exported_prototypes(
        PROVIDER_PATH, "LLING_RAKU_API", "lling_raku_"
    )
    if set(actual_provider) != provider_names:
        abort(
            "provider source/model symbol drift: "
            f"missing={sorted(provider_names - set(actual_provider))}, "
            f"extra={sorted(set(actual_provider) - provider_names)}"
        )
    for function in provider_functions:
        expected = normalize_c(c_prototype(function, "LLING_RAKU_API"))
        actual = actual_provider[function["name"]]
        if expected != actual:
            abort(
                f"provider signature drift for {function['name']}:\n"
                f"  model:   {expected}\n  provider: {actual}"
            )

    actual_callbacks = callback_prototypes(PROVIDER_PATH, "LlingRaku")
    if set(actual_callbacks) != callback_names:
        abort(
            "provider callback/model drift: "
            f"missing={sorted(callback_names - set(actual_callbacks))}, "
            f"extra={sorted(set(actual_callbacks) - callback_names)}"
        )
    for callback in callbacks:
        expected = normalize_c(callback_prototype(callback))
        actual = actual_callbacks[callback["name"]]
        if expected != actual:
            abort(
                f"provider callback drift for {callback['name']}:\n"
                f"  model:   {expected}\n  provider: {actual}"
            )


def raku_quote(value: str) -> str:
    return "'" + value.replace("\\", "\\\\").replace("'", "\\'") + "'"


def append_map(
    lines: list[str], name: str, entries: list[tuple[str, str | int]]
) -> None:
    lines.append(f"our constant {name} is export(:metadata) = Map.new(")
    for key, value in entries:
        rendered = str(value) if isinstance(value, int) else raku_quote(value)
        lines.append(f"    {raku_quote(key)} => {rendered},")
    lines.extend([");", ""])


def render_library(lines: list[str], function_name: str, library: dict) -> None:
    lines.extend(
        [
            f"sub {function_name}(--> Str:D) {{",
            (
                f"    return %*ENV<{library['environment']}> if "
                f"%*ENV<{library['environment']}>:exists;"
            ),
        ]
    )
    resource = library.get("resource")
    if resource is not None:
        lines.extend(
            [
                f"    return %?RESOURCES<{resource}>.IO.Str",
                f"        if %?RESOURCES<{resource}>:exists;",
            ]
        )
    lines.extend(
        [
            f"    $*DISTRO.is-win ?? {raku_quote(library['windows'])} !!",
            f"        $*KERNEL.name eq 'darwin' ?? {raku_quote(library['macos'])} !!",
            f"        {raku_quote(library['linux'])}",
            "}",
            "",
        ]
    )


def render_struct(lines: list[str], name: str, item: dict) -> None:
    if item["rakuSupport"] != "cstruct":
        return
    lines.append(f"our class {name} is repr('CStruct') is export(:abi) {{")
    for field in item["fields"]:
        keyword = "HAS" if field.get("embedded") else "has"
        lines.append(f"    {keyword} {field['rakuType']} $.{field['name']} is rw;")
    constructor = item.get("rakuConstructor")
    if constructor == "abi-header":
        lines.extend(
            [
                "    multi method new(",
                "        UInt:D :$struct-size!, UInt:D :$flags = 0",
                f"        --> {name}:D",
                "    ) {",
                "        self.bless(:$struct-size, abi-version => TYPED-ABI-VERSION,",
                "            :$flags, reserved => 0)",
                "    }",
            ]
        )
    elif constructor == "budget":
        lines.extend(
            [
                "    multi method new(",
                "        UInt:D :$max-states = 0, UInt:D :$max-arcs = 0,",
                "        UInt:D :$max-bytes = 0, UInt:D :$max-work = 0,",
                f"        --> {name}:D",
                "    ) {",
                "        my $flags = ($max-states ?? BUDGET-STATES !! 0) +|",
                "            ($max-arcs ?? BUDGET-ARCS !! 0) +|",
                "            ($max-bytes ?? BUDGET-BYTES !! 0) +|",
                "            ($max-work ?? BUDGET-WORK !! 0);",
                "        my $value = self.bless;",
                f"        $value.header.struct-size = nativesizeof({name});",
                "        $value.header.abi-version = TYPED-ABI-VERSION;",
                "        $value.header.flags = $flags;",
                "        $value.header.reserved = 0;",
                "        $value.max-states = $max-states;",
                "        $value.max-arcs = $max-arcs;",
                "        $value.max-bytes = $max-bytes;",
                "        $value.max-work = $max-work;",
                "        $value.reserved0 = 0;",
                "        $value.reserved1 = 0;",
                "        $value",
                "    }",
            ]
        )
    elif constructor == "outcome":
        lines.extend(
            [
                "    multi method new(",
                "        UInt:D :$precision!, UInt:D :$completeness!,",
                "        UInt:D :$applicability!, UInt:D :$termination!,",
                "        UInt:D :$evidence!, UInt:D :$states = 0,",
                "        UInt:D :$arcs = 0, UInt:D :$bytes = 0,",
                "        UInt:D :$work = 0, UInt:D :$limitations = 0,",
                f"        --> {name}:D",
                "    ) {",
                "        my $value = self.bless;",
                f"        $value.header.struct-size = nativesizeof({name});",
                "        $value.header.abi-version = TYPED-ABI-VERSION;",
                "        $value.header.flags = 0;",
                "        $value.header.reserved = 0;",
                "        $value.precision = $precision;",
                "        $value.completeness = $completeness;",
                "        $value.applicability = $applicability;",
                "        $value.termination = $termination;",
                "        $value.evidence = $evidence;",
                "        $value.reserved0 = 0;",
                "        $value.states = $states;",
                "        $value.arcs = $arcs;",
                "        $value.bytes = $bytes;",
                "        $value.work = $work;",
                "        $value.limitations = $limitations;",
                "        $value.reserved1 = 0;",
                "        $value",
                "    }",
            ]
        )
    lines.extend(["}", ""])


def callback_raku_type(callback: dict, parameter_name: str) -> str:
    parameters = [item["rakuType"] for item in callback["parameters"]]
    returned = callback["return"]["rakuType"]
    signature = ", ".join(parameters)
    if returned is not None:
        signature = f"{signature} --> {returned}" if signature else f"--> {returned}"
    raku_name = parameter_name.replace("_", "-")
    return f"&{raku_name} ({signature})"


def render_binding(
    lines: list[str], function: dict, library_function: str, callbacks: dict[str, dict]
) -> None:
    if not function["raku"]["bind"]:
        return
    parameters: list[str] = []
    for parameter in function["parameters"]:
        callback_name = parameter.get("callback")
        if callback_name is None:
            rendered = parameter["rakuType"]
        else:
            rendered = callback_raku_type(callbacks[callback_name], parameter["name"])
        if parameter.get("rakuPassing", "value") == "rw":
            rendered += " is rw"
        parameters.append(rendered)
    returned = function["return"].get("rakuType")
    parts = parameters[:]
    if returned is not None:
        parts.append(f"--> {returned}")
    if parts:
        lines.append(f"our sub {function['rakuName']}(")
        lines.append("    " + ",\n    ".join(parts))
        lines.append(")")
    else:
        lines.append(f"our sub {function['rakuName']}()")
    lines.extend(
        [
            f"    is native(&{library_function})",
            f"    is symbol({raku_quote(function['name'])})",
            "    is export(:native)",
            "{ * }",
            "",
        ]
    )


def render_raku(model: dict) -> str:
    raku = model["rakuAbi"]
    provider = raku["providerShim"]
    lines = [
        f"unit module {raku['module']};",
        "",
        "use NativeCall;",
        "need Vinary::Tree::Interop;",
        "",
        "# Code generated by scripts/generate-raku-abi.py from bindings/api.json",
        "# after validating include/lling_llang.h and the provider shim; DO NOT EDIT.",
        f"our constant ABI-VERSION is export(:abi) = {model['abiVersion']};",
        f"our constant API-REVISION is export(:abi) = {model['apiRevision']};",
        f"our constant TYPED-ABI-VERSION is export(:abi) = {model['typedAbiV2']['metadataVersion']};",
        f"our constant PROVIDER-ABI-VERSION is export(:abi) = {provider['abiVersion']};",
        f"our constant PROVIDER-API-REVISION is export(:abi) = {provider['apiRevision']};",
    ]
    for name, value in raku["constants"].items():
        lines.append(f"our constant {name} is export(:abi) = {value};")
    for name, value in provider["capabilities"].items():
        lines.append(
            f"our constant PROVIDER-CAPABILITY-{name.replace('_', '-')} "
            f"is export(:abi) = {value};"
        )
    lines.append("")

    for item in model["enums"].values():
        lines.append(f"our enum {item['rakuName']} is export(:abi) (")
        for name, value in item["values"].items():
            lines.append(f"    {name.replace('_', '-')} => {value},")
        lines.extend([");", ""])

    for name, item in raku["structs"].items():
        render_struct(lines, name, item)

    append_map(
        lines,
        "TYPE-KINDS",
        [(name, item["kind"]) for name, item in raku["ffiTypes"].items()],
    )
    append_map(
        lines,
        "TYPE-RAKU-REPRESENTATIONS",
        [(name, item["rakuType"]) for name, item in raku["ffiTypes"].items()],
    )
    append_map(
        lines,
        "TYPE-OWNERSHIP",
        [(name, item["ownership"]) for name, item in raku["ffiTypes"].items()],
    )
    append_map(
        lines,
        "STRUCT-SIZES",
        [(item["cType"], item["size"]) for item in raku["structs"].values()],
    )
    append_map(
        lines,
        "STRUCT-RAKU-SUPPORT",
        [(item["cType"], item["rakuSupport"]) for item in raku["structs"].values()],
    )

    functions = model["cFunctions"] + provider["functions"]
    append_map(
        lines,
        "C-SIGNATURES",
        [
            (
                function["name"],
                c_prototype(
                    function,
                    "LLING_RAKU_API"
                    if function["name"].startswith("lling_raku_")
                    else "LLING_LLANG_API",
                ),
            )
            for function in functions
        ],
    )
    append_map(
        lines,
        "CALLBACK-SIGNATURES",
        [
            (
                callback["name"],
                "typedef "
                + callback["return"]["cType"]
                + " (*"
                + callback["name"]
                + ")("
                + (
                    ", ".join(c_parameter(item) for item in callback["parameters"])
                    or "void"
                )
                + ");",
            )
            for callback in provider["callbacks"]
        ],
    )
    append_map(
        lines,
        "CALLBACK-THREADING",
        [
            (callback["name"], callback["threading"])
            for callback in provider["callbacks"]
        ],
    )
    for map_name, field in (
        ("FUNCTION-SINCE-API-REVISION", "sinceApiRevision"),
        ("FUNCTION-THREADING", "threading"),
        ("FUNCTION-CAPABILITIES", "capability"),
    ):
        append_map(
            lines,
            map_name,
            [(function["name"], function[field]) for function in functions],
        )
    ownership: list[tuple[str, str]] = []
    nullability: list[tuple[str, str]] = []
    for function in functions:
        name = function["name"]
        ownership.append((f"{name}:return", function["return"]["ownership"]))
        nullability.append((f"{name}:return", function["return"]["nullability"]))
        for parameter in function["parameters"]:
            key = f"{name}:{parameter['name']}"
            ownership.append((key, parameter["ownership"]))
            nullability.append((key, parameter["nullability"]))
    append_map(lines, "FUNCTION-OWNERSHIP", ownership)
    append_map(lines, "FUNCTION-NULLABILITY", nullability)
    callback_ownership: list[tuple[str, str]] = []
    callback_nullability: list[tuple[str, str]] = []
    for callback in provider["callbacks"]:
        name = callback["name"]
        callback_ownership.append((f"{name}:return", callback["return"]["ownership"]))
        callback_nullability.append(
            (f"{name}:return", callback["return"]["nullability"])
        )
        for parameter in callback["parameters"]:
            key = f"{name}:{parameter['name']}"
            callback_ownership.append((key, parameter["ownership"]))
            callback_nullability.append((key, parameter["nullability"]))
    append_map(lines, "CALLBACK-OWNERSHIP", callback_ownership)
    append_map(lines, "CALLBACK-NULLABILITY", callback_nullability)
    append_map(
        lines,
        "RAKU-BINDING-EXCLUSIONS",
        [
            (function["name"], function["raku"]["reason"])
            for function in functions
            if not function["raku"]["bind"]
        ],
    )

    render_library(lines, "native-library", raku["nativeLibrary"])
    render_library(lines, "provider-library", raku["providerLibrary"])
    callbacks = {item["name"]: item for item in provider["callbacks"]}
    for function in model["cFunctions"]:
        render_binding(lines, function, "native-library", callbacks)
    for function in provider["functions"]:
        render_binding(lines, function, "provider-library", callbacks)
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="verify generated output")
    mode.add_argument("--write", action="store_true", help="rewrite generated output")
    mode.add_argument(
        "--self-test",
        action="store_true",
        help="prove a model mutation makes the committed output stale",
    )
    args = parser.parse_args()

    model = load_model()
    validate_model(model)
    rendered = render_raku(model)
    if args.self_test:
        current = OUTPUT_PATH.read_text(encoding="utf-8")
        if current != rendered:
            abort("self-test requires a current generated module")
        mutated = copy.deepcopy(model)
        mutated["apiRevision"] += 1
        if render_raku(mutated) == current:
            abort("negative stale-generation control was not detected")
        print("negative control passed: a model mutation makes Raku output stale")
        return 0
    if args.write:
        OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
        OUTPUT_PATH.write_text(rendered, encoding="utf-8")
        print(f"wrote {OUTPUT_PATH.relative_to(ROOT)}")
        return 0
    try:
        current = OUTPUT_PATH.read_text(encoding="utf-8")
    except OSError as error:
        abort(f"cannot read {OUTPUT_PATH.relative_to(ROOT)}: {error}")
    if current != rendered:
        print(
            f"{OUTPUT_PATH.relative_to(ROOT)} is stale; run "
            "python3 scripts/generate-raku-abi.py --write",
            file=sys.stderr,
        )
        return 1
    print("lling-llang model, native headers, provider shim, and Raku ABI agree")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
