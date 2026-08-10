//! Thin UI state for the "export practice viewer" button.
//!
//! Generation via `drill_mobile_viewer::build_practice_viewer` is synchronous
//! and fast even for large rosters (well under a second), so unlike video/PDF
//! export this needs no background job -- the existing `TextExportState`
//! already covers file-write latency. This struct only remembers enough to
//! show a helpful status line after the last export.

#[derive(Default)]
pub(crate) struct MobileViewerState {
    /// Number of performers embedded in the most recently generated viewer,
    /// shown next to the export button as a confirmation of scope.
    pub last_performer_count: usize,
}
