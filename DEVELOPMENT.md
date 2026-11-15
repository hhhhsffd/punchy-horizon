# Development

## Compatibility

- Minecraft: **26.3**.
- Bundle: **0.2.0**; see PORTING_STATUS.md for incomplete original parity.
- Latest published Horizon nightly checked 2026-11-15: `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0`.
- horizon-mixins: `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`.

Other revisions require validation. This native mod runs with the same account
permissions as Horizon; review its source before building it.

## Develop and package

The current implementation is unfinished. Testing resumed at the user's request
on 2026-11-15. See VALIDATION.md for results; engine tests do not establish a
working combined game or full visual parity.

Rust and Python 3.11+ are needed for development. Engine tests use bundled data
and checked-in numerical fixtures; no original JAR or decompiler is needed.

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
rustfmt --edition 2024 --check mixins/punchy/geometry.rs mixins/punchy/render.rs
```

To build a bundle, check out the pinned Horizon source and run:

```sh
git clone https://github.com/horizon-rust/horizon.git horizon-source
git -C horizon-source checkout 9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0
python3 tools/package.py --horizon-source horizon-source --validate
```

The tool validates the exact `.hnmod` with Horizon's actual Bundle::read parser.
Output is in `dist/`. Add `--offline` to use cached Cargo dependencies.
The original license notices are included with the bundle. Source exports with
Horizon's `.horizon-revision` marker are accepted too.

Build the current official client together with Spatial GUI with the pinned
horizon-mixins executable (shown as `horizon-mixins` below):

```sh
python3 tools/build.py check --horizon-source horizon-source --mixins /path/to/horizon-mixins --spatial-gui ../spatialgui
python3 tools/build.py build --horizon-source horizon-source --mixins /path/to/horizon-mixins --spatial-gui ../spatialgui
```

Omit `--spatial-gui` to build Punchy alone. The output stage is under `.build/`;
the original Horizon and Spatial GUI checkouts are preserved. The executable is
`horizon-official` in the Cargo target's `fast/` directory. Set `--target-dir`
to reuse a Cargo build cache. Rust/Cargo must be on PATH; Windows GNU builds also
need their toolchain's self-contained runtime libraries on `LIBRARY_PATH`.

`tools/runtime_smoke.py` can exercise a compiled native game with `--binary`,
`--ctl`, `--resources` and `--report`. It creates fresh temporary homes and a
throwaway world; it never uses the player's saves. See VALIDATION.md for scope.


Add `--viewmodel` to the runtime smoke command to exercise the real mod-settings
buttons, verify independent arm/item matrices, restart the game to check
persistence, and test both reset actions. It uses the same scratch game directory
for that restart. Numeric defaults, input validation, bounds and transform order
are also covered by engine tests in `viewmodel.rs`.

`tools/moves_smoke.py` additionally exercises an enabled external pack, body
poses, shoulder/orbit camera, facial skin upload/restoration, Spatial GUI camera
handoff, right-click controls, back navigation, and the Moves switch.
Pass `--binary`, `--ctl`, `--resources`, `--pack` (an unpacked external pack),
and `--report`. It also uses only a new temporary game folder.
