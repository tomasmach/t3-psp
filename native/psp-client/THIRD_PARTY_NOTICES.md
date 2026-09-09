# Third-party notices

T3 PSP is an unofficial client derived from T3 Code. The root `LICENSE` retains the MIT license and copyright notice of T3 Tools Inc.

The PSP client and gateway additions are copyright (c) 2026 Tomáš Mach, under the MIT license in `licenses/T3-PSP-LICENSE-MIT`. This attribution does not replace the original notices for T3 Code or the assets and dependencies listed below.

The XMB icon (`assets/ICON0.png`) is derived from T3 Code's `assets/prod/logo.svg`, with changed colors and a PSP label. Its origin is T3 Tools Inc., under the repository's MIT license. `tools/generate_assets.py` records the conversion.

The embedded `font12.bin` and `font14.bin` glyphs are rasterized from DejaVu Sans. Their copyright and permission notices are in `assets/FONT-LICENSE.txt`, copied to `licenses/FONT-LICENSE.txt` in the distribution.

## Native dependencies

The release package includes license texts for every crate resolved by the locked Cargo dependency graph. This includes build-only procedural macros for completeness; inclusion does not imply that every crate is linked into the EBOOT.

| Component                                   | License and distribution notice                                                                                                         |
| ------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| rust-psp 0.3.13                             | MIT, plus the retained PSPSDK BSD notice, in `licenses/psp-0.3.13/LICENSE`                                                              |
| bitflags                                    | MIT or Apache-2.0; original texts included                                                                                              |
| libm                                        | MIT with third-party notices; original `LICENSE.txt` and complete `src/math` files included under `licenses/libm-<version>/math-source` |
| num_enum and num_enum_derive                | BSD-3-Clause, MIT, or Apache-2.0; original texts included                                                                               |
| paste, proc-macro2, quote, rustversion, syn | MIT or Apache-2.0; original texts included                                                                                              |
| unicode-ident                               | (MIT or Apache-2.0) and Unicode-3.0; the Unicode notice is included in addition to the other texts                                      |
| unstringify                                 | Zlib, MIT, or Apache-2.0; original texts included                                                                                       |

The rust-psp crate archive omits its upstream license file. The vendored copy comes from the [exact source commit for 0.3.13](https://github.com/overdrivenpotato/rust-psp/blob/a8ee353309528d7d05a85bbb55976117e9527d71/LICENSE), recorded in the crate's `.cargo_vcs_info.json`. It retains Marko Mijalkovic's MIT notice and the PSPSDK authors' BSD notice together.

## Rust runtime

The EBOOT also uses Rust runtime code outside the application's Cargo.lock. `licenses/rust/COPYRIGHT-library.html` comes from the toolchain used for packaging and records the standard library's notices, including its third-party dependencies. The Rust MIT license is included separately, with attribution to The Rust Project Contributors. Its vendored text follows [Rust's LICENSE-MIT](https://github.com/rust-lang/rust/blob/main/LICENSE-MIT).

The package also retains the complete `compiler-builtins-LICENSE.txt` (MIT and Apache-2.0 with LLVM exception) and `libunwind-LICENSE.TXT` (Apache-2.0 with LLVM exceptions and its included notices), taken from the same toolchain's Rust source component.

## Packaging

After building the release EBOOT, run `bash native/psp-client/tools/package.sh` from the repository root. The script prints a fresh staging directory under Cargo's target directory. Distribute that complete directory so the notices accompany the executable.

Packaging requires Bash, Cargo, rustc, jq, the locked dependency sources already fetched, and the pinned toolchain's `rust-src` and `rust-docs` components. Missing required files stop packaging. The script copies only the EBOOT, the example configuration, the installation README, this document, and licenses. It never copies the active gateway configuration, recordings, logs, or developer state.
