# ADR 0002: Image underlays are content-addressed project assets

Status: accepted

## Decision

`Document` persists only `ImageUnderlay` metadata: a BLAKE3 content address,
bounded byte length, original name, optional external-path hint and
`UnderlayPlacement`. Image bytes never enter document JSON. `.drillproj`
manifests use an optional `AssetEntry { kind: Image }`; embedded entries are
named from the content hash, while external entries retain a recovery hint.

Placement contains field-relative translation, independent X/Y scale,
rotation, opacity and visibility. `UnderlayRenderPolicy::Editor2dOnly` is the
default, excluding the image from 3D, printing and video.

Loading and decoding remain background jobs and retain the 32 MiB compressed,
dimension and decoded-pixel limits. Save is the existing atomic container
write. Hash mismatches fail save/load. Missing image bytes are non-fatal: the
document and placement remain available and the editor opens without a texture.

All metadata changes use `Edit::SetImageUnderlay`. Schema v4 adds the optional
field; v1-v3 migrate to `None` without changing drill geometry.

## Consequences

- Project identity is content-based rather than path-based.
- JSON remains small and inspectable.
- Relinking can be added without another schema change.
- Reference imagery cannot accidentally leak into published 3D output.

