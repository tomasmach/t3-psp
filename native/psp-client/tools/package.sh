#!/usr/bin/env bash
set -euo pipefail
[[ $# -eq 0 ]] || { echo "Usage: bash tools/package.sh" >&2; exit 1; }

client_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
repo_dir=$(cd "$client_dir/../.." && pwd)
cd "$client_dir"

for command in cargo rustc jq; do
  command -v "$command" >/dev/null || { echo "Missing command: $command" >&2; exit 1; }
done

metadata=$(cargo metadata --locked --offline --format-version 1)
target_dir=$(jq -er '.target_directory' <<<"$metadata")
sysroot=$(rustc --print sysroot)
eboot="$target_dir/mipsel-sony-psp/release/EBOOT.PBP"

require_file() {
  [[ -s "$1" ]] || { echo "Missing required distribution file: $1" >&2; exit 1; }
}

require_file "$eboot"
require_file "$sysroot/share/doc/rust/COPYRIGHT-library.html"
require_file "$sysroot/lib/rustlib/src/rust/library/compiler-builtins/LICENSE.txt"
require_file "$sysroot/lib/rustlib/src/rust/src/llvm-project/libunwind/LICENSE.TXT"
jq -e '.packages[] | select(.name == "psp" and .version == "0.3.13")' <<<"$metadata" >/dev/null || {
  echo "Update the vendored rust-psp notice before packaging a different PSP crate version" >&2
  exit 1
}

mkdir -p "$target_dir"
staging_dir=$(mktemp -d "$target_dir/psp-package.XXXXXX")
mkdir -p "$staging_dir/licenses/rust" "$staging_dir/licenses/psp-0.3.13"

copy_required() {
  require_file "$1"
  cp "$1" "$2"
}

copy_required "$eboot" "$staging_dir/EBOOT.PBP"
copy_required "$client_dir/gateway.cfg.example" "$staging_dir/gateway.cfg.example"
copy_required "$client_dir/README.md" "$staging_dir/README.md"
copy_required "$client_dir/THIRD_PARTY_NOTICES.md" "$staging_dir/THIRD_PARTY_NOTICES.md"
copy_required "$repo_dir/LICENSE" "$staging_dir/LICENSE"
copy_required "$client_dir/licenses/T3-PSP-LICENSE-MIT" "$staging_dir/licenses/T3-PSP-LICENSE-MIT"
copy_required "$client_dir/assets/FONT-LICENSE.txt" "$staging_dir/licenses/FONT-LICENSE.txt"
copy_required "$client_dir/licenses/rust-psp-0.3.13-LICENSE" "$staging_dir/licenses/psp-0.3.13/LICENSE"
copy_required "$client_dir/licenses/Rust-LICENSE-MIT" "$staging_dir/licenses/rust/LICENSE-MIT"
copy_required "$sysroot/share/doc/rust/COPYRIGHT-library.html" "$staging_dir/licenses/rust/COPYRIGHT-library.html"
copy_required "$sysroot/lib/rustlib/src/rust/library/compiler-builtins/LICENSE.txt" "$staging_dir/licenses/rust/compiler-builtins-LICENSE.txt"
copy_required "$sysroot/lib/rustlib/src/rust/src/llvm-project/libunwind/LICENSE.TXT" "$staging_dir/licenses/rust/libunwind-LICENSE.TXT"

# Include every locked crate's license, including build-only macros. No license-expression parser.
shopt -s nullglob
while IFS=$'\t' read -r name version manifest; do
  [[ "$name" == "psp" ]] && continue
  crate_dir=$(dirname "$manifest")
  destination="$staging_dir/licenses/$name-$version"
  license_files=("$crate_dir"/LICENSE*)
  (( ${#license_files[@]} > 0 )) || { echo "No license files for $name $version" >&2; exit 1; }
  mkdir -p "$destination"
  for license_file in "${license_files[@]}"; do
    copy_required "$license_file" "$destination/"
  done
  if [[ "$name" == "libm" ]]; then
    [[ -d "$crate_dir/src/math" ]] || { echo "Missing libm source notices" >&2; exit 1; }
    # The original files preserve all individual BSD, SunPro and CORE-MATH notices.
    cp -R "$crate_dir/src/math" "$destination/math-source"
  fi
done < <(jq -r '.packages[] | select(.source != null) | [.name, .version, .manifest_path] | @tsv' <<<"$metadata")

# Only an explicit allowlist is copied; never gateway.cfg, audio, logs, or developer state.
printf '%s\n' "$staging_dir"
