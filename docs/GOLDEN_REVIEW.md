# Golden approval workflow

DrillForge keeps representative SVG, coordinate CSV/HTML, Production TSV/PDF,
software-rendered RGBA, and message-catalog outputs under
`crates/drill-conformance/tests/golden/`.

Normal tests are check-only. They never create or replace a golden. A mismatch
reports the first byte and line plus bounded expected/actual context.

To approve an intentional output change:

1. Run the ordinary test and inspect the reported difference.
2. Confirm the fixture change is intentional in every affected format.
3. Run `pwsh ./scripts/update-goldens.ps1 -Approve reviewed` locally.
4. Review `git diff -- crates/drill-conformance/tests/golden` as product output,
   not as mechanical test noise.
5. Commit source and approved golden changes together.

`UPDATE_GOLDENS` accepts only the exact value `reviewed`, and update mode refuses
to run when `CI` is set. CI runs the same test without update permission and also
checks that no `.actual`, `.new`, or `.snap.new` drafts are committed.
