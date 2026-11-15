# Private development references

The user requested decompiling the published mod as a source reference for the
Rust port. These files stay in ignored `.local/moves-reference/` and are never
included in a source commit or `.hnmod`.

- PunchyMoves **1.0.9**, Fabric, Minecraft 26.3; Modrinth version `lRMHnY4W`.
- SHA-512:
  `1639877e68b361738efd21083d3009ae043f92f68b908181e377fae99eb00642215f38f6b99fb0cd49a0f4de142ae59dab980f513934a78a3639ccd2188edfba`.
- Decompiled using Vineflower **1.12.0**, development tooling only.
- Main reference: `.local/moves-reference/decompiled/punchymoves/`,
  203 Java files, 32,729 lines.
- Bundled Punchy Expressions **1.0.9** reference:
  `.local/moves-reference/expressions-decompiled/punchymoves/`,
  54 Java files, 17,446 lines.
- External resource-pack format reference: Fluid & Foolish **1.0.1**,
  Modrinth version `UAp4jRDk`; not redistributed or embedded.

Java is not part of the native mod runtime. Source-derived Rust does not change
the original project's all-rights-reserved status. Preserve the original notices;
public redistribution rights should not be inferred from decompilation.

The earlier testing hold was lifted by the user on 2026-11-15.
See VALIDATION.md for checks actually performed and the remaining validation scope.
