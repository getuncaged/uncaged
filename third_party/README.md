# third_party

Crates vendored from Warp's repositories so the build no longer fetches from
`github.com/warpdotdev`. They are path dependencies from the root
`[workspace.dependencies]`, and `third_party` is in `[workspace] exclude`, so
workspace-wide clippy and nextest skip them. `third_party/Cargo.toml` is their own
workspace root, so Cargo stops looking for one in parent directories, and
`third_party/.rustfmt.toml` sets `disable_all_formatting` because
`cargo fmt --all` also formats local path dependencies. Crate names are
unchanged. Each copy is the tracked tree of the pinned revision, minus the files
listed under "Local changes". Every copy also drops upstream's root workspace
`Cargo.toml`, `Cargo.lock`, READMEs, `rust-toolchain.toml`, CI config and
dotfiles; LICENSE and notice files are kept.

| Crate(s) | Origin | License | Local changes |
| --- | --- | --- | --- |
| `command-corrections` | [warpdotdev/command-corrections](https://github.com/warpdotdev/command-corrections) @ `eae08c8c51d9bc9741fcb17eef2c21f696aebbeb` | MIT (`command-corrections/LICENSE.md`; includes thefuck's notice) | Manifest `license` set to MIT to match `LICENSE.md` (upstream still said AGPL-3.0-only); `authors` added. |
| `mermaid_to_svg`, `dagre_rust` | [warpdotdev/mermaid-to-svg](https://github.com/warpdotdev/mermaid-to-svg) @ `8d3f789c2eb49335d7bf247a06bb649f59b6d4ed` | MIT (`mermaid_to_svg/LICENSE`, `THIRD_PARTY_NOTICES`); `dagre_rust` Apache-2.0 (`crates/dagre_rust/LICENCE`) | Dropped `samples/`, `docs/`, `scripts/`, `examples/`, the `render_mermaid` bin and `[dev-dependencies]`, so the test modules no longer build here. `dagre_rust` lost its cargo-generated manifest header, `Cargo.toml.orig`, `.cargo_vcs_info.json` and `.cargo-ok`. |
| `warp-command-signatures`, `warp-completion-metadata` | [warpdotdev/command-signatures](https://github.com/warpdotdev/command-signatures) @ `a937ae35dfe20eaed030664ad52b36657390b162` | MIT (`command-signatures/LICENSE.md`; specs adapted from withfig/autocomplete) | Literal `authors`/`license`/`publish` instead of workspace inheritance. Dropped `json/overrides/` and `src/bin/` (offline PowerShell spec generator only), plus repo tooling (`.agents/`, `script/`, `package*.json`, prettier config, `AGENTS.md`). `json/` is embedded by rust-embed. |
| `warp-workflows`, `warp-workflows-types` | [warpdotdev/workflows](https://github.com/warpdotdev/workflows) @ `793a98ddda6ef19682aed66364faebd2829f0e01` | Apache-2.0 (`workflows/LICENSE`) | `build.rs` writes one `generated_workflows.rs` into `$OUT_DIR`, which `src/lib.rs` `include!`s, instead of a gitignored `src/generated_workflows/` tree, and no longer runs `cargo fmt`. Uses crates.io `uneval` 0.2.4 instead of the warpdotdev/uneval fork (`df4d9fa`). The generated `workflows()` serializes to byte-identical JSON. Literal manifest metadata. Dropped `build_ts/`, `script/`, `FORMAT.md`. |

The crates marked `publish = false` are skipped by cargo-about, so their
license files are appended through `ADDITIONAL_LICENSES` in
`script/prepare_bundled_resources` and `script/windows/prepare_bundled_resources.ps1`.
