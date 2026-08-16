# ADR 001: Image underlay ownership remains session-local

Status: accepted as an interim boundary, 2026-08-10.

DrillForge can safely decode and display PNG/JPEG formation references. The current
implementation keeps decoded pixels and placement controls in app session state. It
does not add a filesystem path or a large byte array to `Document`.

This preserves the existing promise that missing optional media never prevents a drill
from opening, and avoids freezing an incorrect schema before the field/3D coordinate
ownership described in design 52 is resolved. A local absolute path would be
non-portable; embedding raw pixels in JSON would make saves large and non-deterministic.

Before project persistence ships, one owner must define an `UnderlayPlacement` model
(field-space transform, crop, opacity, visibility and set range). The `.drillproj`
container should then store original compressed bytes as a content-addressed optional
asset, with a manifest reference from that model. Load must degrade to a warning and a
disabled underlay when the asset is absent or invalid. Migration, package round-trip,
asset-size limits, and 2D/3D/export visibility require tests. Until those requirements
are implemented together, session-local state is intentional rather than an implied
persistence guarantee.
