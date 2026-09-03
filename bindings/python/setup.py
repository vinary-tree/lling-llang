from __future__ import annotations

import os
import platform
import shutil
import subprocess
from pathlib import Path

from setuptools import Distribution, setup
from setuptools.command.bdist_wheel import bdist_wheel
from setuptools.command.build_py import build_py

BINDING_DIRECTORY = Path(__file__).resolve().parent
REPOSITORY_ROOT = BINDING_DIRECTORY.parents[1]


def native_library_name() -> str:
    """Return the platform's lling-llang shared-library filename."""
    system = platform.system()
    if system == "Windows":
        return "lling_llang.dll"
    if system == "Darwin":
        return "liblling_llang.dylib"
    return "liblling_llang.so"


def native_library() -> Path:
    """Resolve a verified prebuilt library or build the exact source tree."""
    explicit = os.environ.get("LLING_LLANG_PREBUILT_LIBRARY")
    if explicit:
        library = Path(explicit).expanduser().resolve()
        if not library.is_file():
            raise FileNotFoundError(
                f"LLING_LLANG_PREBUILT_LIBRARY is not a file: {library}"
            )
        return library

    command = [
        "cargo",
        "build",
        "--manifest-path",
        str(REPOSITORY_ROOT / "Cargo.toml"),
        "--release",
        "--no-default-features",
        "--features",
        "python-bindings",
    ]
    target = os.environ.get("LLING_LLANG_RUST_TARGET")
    if target:
        command.extend(["--target", target])
    subprocess.run(command, cwd=REPOSITORY_ROOT, check=True)

    target_directory = Path(
        os.environ.get("CARGO_TARGET_DIR", REPOSITORY_ROOT / "target")
    )
    profile_directory = (
        target_directory / target / "release"
        if target
        else target_directory / "release"
    )
    library = profile_directory / native_library_name()
    if not library.is_file():
        raise FileNotFoundError(f"Cargo did not produce the native library: {library}")
    return library


class BuildWithNativeLibrary(build_py):
    """Stage the Rust library and license inside the import package."""

    def run(self) -> None:
        super().run()
        destination = Path(self.build_lib) / "lling_llang"
        native_destination = destination / "native"
        native_destination.mkdir(parents=True, exist_ok=True)
        shutil.copy2(native_library(), native_destination / native_library_name())
        shutil.copy2(REPOSITORY_ROOT / "LICENSE", destination / "LICENSE")


class PlatformDistribution(Distribution):
    """Mark wheels as platform-specific even though the facade is Python."""

    def has_ext_modules(self) -> bool:
        return True


class PortablePythonPlatformWheel(bdist_wheel):
    """Use one Python-3 ABI tag with the platform's native-library tag."""

    def finalize_options(self) -> None:
        super().finalize_options()
        self.root_is_pure = False

    def get_tag(self) -> tuple[str, str, str]:
        _, _, platform_tag = super().get_tag()
        return "py3", "none", platform_tag


setup(
    cmdclass={
        "build_py": BuildWithNativeLibrary,
        "bdist_wheel": PortablePythonPlatformWheel,
    },
    distclass=PlatformDistribution,
)
