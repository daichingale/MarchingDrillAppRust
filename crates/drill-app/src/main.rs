//! DrillForge desktop executable bootstrap.
//!
//! Application state, controllers, and UI composition live in `app_state` so
//! this binary entry remains a stable boundary for platform startup.

mod app_state;

pub(crate) use app_state::{DrillApp, WorkspaceFocus};
#[cfg(test)]
pub(crate) use drill_core::{Locale, Point};

fn main() -> eframe::Result {
    app_state::run()
}
