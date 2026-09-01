#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <rust-target> <cargo-release-directory> <output-directory>" >&2
  exit 2
fi

target=$1
release_dir=$2
output_dir=$3
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
package_name="lling-llang-${version}-${target}"
prefix="${output_dir}/${package_name}"

mkdir -p "${prefix}/bin" "${prefix}/include" \
  "${prefix}/lib/cmake/lling-llang" \
  "${prefix}/lib/cmake/vinary-tree-interop" "${prefix}/lib/pkgconfig"
cp include/lling_llang.h include/lling_llang.hpp "${prefix}/include/"
cp ../vinary-tree-interop/include/vinary_tree_interop.h "${prefix}/include/"
cp cmake/lling-llangConfig.cmake cmake/lling-llangConfigVersion.cmake \
  "${prefix}/lib/cmake/lling-llang/"
cp ../vinary-tree-interop/cmake/vinary-tree-interopConfig.cmake \
  ../vinary-tree-interop/cmake/vinary-tree-interopConfigVersion.cmake \
  "${prefix}/lib/cmake/vinary-tree-interop/"
cp pkgconfig/lling-llang.pc ../vinary-tree-interop/pkgconfig/vinary-tree-interop.pc \
  "${prefix}/lib/pkgconfig/"
cp LICENSE README.md "${prefix}/"

case "$target" in
  *-pc-windows-msvc)
    shared=$(find "$release_dir" -maxdepth 2 -type f -name 'lling_llang.dll' -print -quit)
    import_library=$(find "$release_dir" -maxdepth 2 -type f -name 'lling_llang.dll.lib' -print -quit)
    static_library=$(find "$release_dir" -maxdepth 2 -type f -name 'lling_llang.lib' -print -quit)
    test -n "$shared" && test -n "$import_library" && test -n "$static_library"
    cp "$shared" "${prefix}/bin/lling_llang.dll"
    cp "$import_library" "${prefix}/lib/lling_llang.dll.lib"
    cp "$static_library" "${prefix}/lib/lling_llang.lib"
    private_libs='-lbcrypt -luserenv -lws2_32 -lntdll -lsynchronization -ladvapi32'
    ;;
  *-apple-darwin)
    shared=$(find "$release_dir" -maxdepth 2 -type f -name 'liblling_llang.dylib' -print -quit)
    static_library=$(find "$release_dir" -maxdepth 2 -type f -name 'liblling_llang.a' -print -quit)
    test -n "$shared" && test -n "$static_library"
    cp "$shared" "${prefix}/lib/liblling_llang.dylib"
    cp "$static_library" "${prefix}/lib/liblling_llang.a"
    private_libs='-ldl -lpthread -lm -liconv -framework CoreFoundation -framework Security'
    ;;
  *-linux-gnu)
    shared=$(find "$release_dir" -maxdepth 2 -type f -name 'liblling_llang.so' -print -quit)
    static_library=$(find "$release_dir" -maxdepth 2 -type f -name 'liblling_llang.a' -print -quit)
    test -n "$shared" && test -n "$static_library"
    cp "$shared" "${prefix}/lib/liblling_llang.so"
    cp "$static_library" "${prefix}/lib/liblling_llang.a"
    private_libs='-ldl -lpthread -lm'
    ;;
  *)
    echo "unsupported release target: $target" >&2
    exit 1
    ;;
esac

sed -i.bak "s|^Libs.private:.*|Libs.private: ${private_libs}|" \
  "${prefix}/lib/pkgconfig/lling-llang.pc"
rm -f "${prefix}/lib/pkgconfig/lling-llang.pc.bak"
tar -czf "${output_dir}/${package_name}.tar.gz" -C "$output_dir" "$package_name"
printf '%s\n' "$prefix"
