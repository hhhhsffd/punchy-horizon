# Credits and provenance

- Original Punchy: Dev Punchy Man. Original project:
  https://modrinth.com/mod/punchy-fpa. Analyzed version: 3.1b for Minecraft 26.3.
  This port adapts behavior recovered from the original compiled mod; embedded
  animation, geometry and tuning data originate from Punchy. Preserve LICENSE-Punchy.
- Horizon: Ludovic Marie and the Horizon contributors. MIT, LICENSE-Horizon.
  geometry.rs is adapted from Horizon's first-person geometry renderer at commit
  66f3981f522b24c250b21523a70080965b940424.
- Rust port development assisted by Codex.
- PunchyMoves 1.0.9 for Fabric 26.3: Punchy / Punchy Guys Studios,
  https://modrinth.com/mod/punchymoves. The user requested inspection of the
  shipped JAR to guide the Rust port. That reference and decompiled Java stay
  under ignored `.local/moves-reference/`, never in the mod bundle. Modrinth
  and fabric.mod.json declare ARR; the JAR's license file is an unselected
  template, not a grant of redistribution rights.
- Fluid & Foolish 1.0.1: Crafto,
  https://modrinth.com/resourcepack/fluid-foolish. Required externally for Moves
  animations; no pack assets, clips, or copied behavior JSON are bundled by this
  fork. Synthetic test data is original.
- data/oracles fixtures contain numerical results generated from original
  Punchy pure-class tests, not original Java classes or decompiled source.

The original Punchy archive, original Java classes, decompiler output, Ghidra
projects, Minecraft resources and game binaries are not part of this repository.
