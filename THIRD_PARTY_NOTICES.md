# Third-party notices

DrillForge is distributed under the MIT OR Apache-2.0 license. The complete
license texts are provided in `LICENSE-MIT` and `LICENSE-APACHE`.

## Noto Sans Japanese

The bundled `assets/NotoSansJP.ttf` font is distributed under the SIL Open
Font License 1.1. The complete license text is provided in
`assets/OFL-NotoSansJP.txt`.

## Symphonia

The `symphonia` audio decoding crates are used under the Mozilla Public
License 2.0. DrillForge links to the unmodified crates through Cargo; their
source, copyright notices, and license metadata are available from the
upstream Symphonia project and the Cargo package metadata.

## FFmpeg

FFmpeg is not bundled with DrillForge. Video export invokes an FFmpeg and
ffprobe installation selected from the user's computer. The applicable
FFmpeg license depends on that external build and its enabled codecs.

## Complete dependency inventory

Release packages include `THIRD_PARTY_LICENSES.md`, generated deterministically
from the committed `Cargo.lock` and Cargo metadata. CI rejects a missing or
stale inventory, and cargo-deny enforces advisory, license, ban, and source
policy for every direct and transitive Rust dependency.
