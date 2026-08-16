//! Stable serialized boundary for future DrillForge plugins.
//!
//! This crate never loads native libraries or executes plugin code. A sandboxed
//! runner exchanges these bounded DTOs with the host; Rust types are not an ABI.
#![forbid(unsafe_code)]

use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

pub mod runner;

pub const API_MAJOR: u16 = 1;
pub const API_MINOR: u16 = 0;
pub const MANIFEST_SCHEMA: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    ReadDocument,
    ReadPerformerLabels,
    ReadSelection,
    ProposeEdits,
    ReadAsset,
    WriteExport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiVersion {
    pub major: u16,
    pub minor: u16,
}
impl ApiVersion {
    pub const CURRENT: Self = Self {
        major: API_MAJOR,
        minor: API_MINOR,
    };
    #[must_use]
    pub const fn is_supported(&self) -> bool {
        self.major != 0 && self.major <= API_MAJOR && self.major.saturating_add(1) >= API_MAJOR
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityMetadata {
    /// Lowercase BLAKE3 of component bytes.
    pub component_blake3: String,
    /// Trust-store identifier, never public/private key material.
    pub signing_key_id: String,
    /// Detached signature in the package layer's documented encoding.
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub schema_version: u16,
    pub api: ApiVersion,
    pub id: String,
    pub version: Version,
    pub publisher: String,
    #[serde(default)]
    pub requested: BTreeSet<Capability>,
    pub integrity: IntegrityMetadata,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallKind {
    Interactive,
    Job,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PluginLimits {
    pub manifest_bytes: usize,
    pub request_bytes: usize,
    pub output_bytes: usize,
    pub points: usize,
    pub parameters: usize,
    pub string_bytes: usize,
    pub memory_bytes: usize,
    pub interactive_deadline: Duration,
    pub job_deadline: Duration,
}
impl PluginLimits {
    pub const DEFAULT: Self = Self {
        manifest_bytes: 256 * 1024,
        request_bytes: 4 * 1024 * 1024,
        output_bytes: 64 * 1024 * 1024,
        points: 1_000_000,
        parameters: 256,
        string_bytes: 4096,
        memory_bytes: 256 * 1024 * 1024,
        interactive_deadline: Duration::from_millis(250),
        job_deadline: Duration::from_secs(30),
    };
    #[must_use]
    pub const fn deadline(self, kind: CallKind) -> Duration {
        match kind {
            CallKind::Interactive => self.interactive_deadline,
            CallKind::Job => self.job_deadline,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ParamValue {
    Flag(bool),
    Number(f64),
    Text(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridInfo {
    pub width: f32,
    pub height: f32,
    pub horizontal_steps: u16,
    pub horizontal_units: f32,
    pub vertical_steps: u16,
    pub vertical_units: f32,
    pub major_line_interval: f32,
    pub hashes: Vec<f32>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShapeRequest {
    pub name: String,
    pub count: u32,
    pub grid: GridInfo,
    /// Open bag; unknown keys survive for forward compatibility.
    pub params: BTreeMap<String, ParamValue>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", content = "payload", rename_all = "snake_case")]
pub enum HostCommand {
    Describe,
    ListShapes,
    GenerateShape(ShapeRequest),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginInfo {
    pub provider_id: String,
    pub display_name_key: String,
    pub version: Version,
    pub api: ApiVersion,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "response", content = "payload", rename_all = "snake_case")]
pub enum PluginResponse {
    Description(PluginInfo),
    ShapeNames(Vec<String>),
    Points(Vec<Point>),
    Error { code: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    InputTooLarge,
    InvalidJson,
    UnsupportedManifestSchema(u16),
    UnsupportedApi(u16),
    InvalidProviderId,
    ReservedProviderId,
    InvalidIntegrity,
    HashMismatch,
    SignatureRejected,
    CapabilityNotGranted(Capability),
    LimitExceeded(&'static str),
    ProviderMismatch,
    ResponseMismatch,
    NonDeterministic,
}

/// Implemented by the product trust store. The protocol never invents trust.
pub trait SignatureVerifier {
    fn verify(&self, key_id: &str, message: &[u8], signature: &str) -> bool;
}

fn valid_provider_id(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 64
        && v.is_ascii()
        && v.split('.').all(|p| {
            !p.is_empty()
                && p.len() <= 32
                && p.bytes().enumerate().all(|(i, b)| {
                    b.is_ascii_lowercase()
                        || b.is_ascii_digit()
                        || (b == b'-' && i > 0 && i + 1 < p.len())
                })
        })
}
fn valid_hex(v: &str, bytes: usize) -> bool {
    v.len() == bytes * 2
        && v.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn canonical_unsigned(m: &PluginManifest) -> Vec<u8> {
    #[derive(Serialize)]
    struct Signed<'a> {
        schema_version: u16,
        api: &'a ApiVersion,
        id: &'a str,
        version: &'a Version,
        publisher: &'a str,
        requested: &'a BTreeSet<Capability>,
        component_blake3: &'a str,
        signing_key_id: &'a str,
    }
    serde_json::to_vec(&Signed {
        schema_version: m.schema_version,
        api: &m.api,
        id: &m.id,
        version: &m.version,
        publisher: &m.publisher,
        requested: &m.requested,
        component_blake3: &m.integrity.component_blake3,
        signing_key_id: &m.integrity.signing_key_id,
    })
    .expect("fixed DTO serialization")
}

pub fn validate_package(
    manifest_bytes: &[u8],
    component: &[u8],
    granted: &BTreeSet<Capability>,
    verifier: &dyn SignatureVerifier,
    limits: PluginLimits,
) -> Result<PluginManifest, ProtocolError> {
    if manifest_bytes.len() > limits.manifest_bytes {
        return Err(ProtocolError::InputTooLarge);
    }
    let m: PluginManifest =
        serde_json::from_slice(manifest_bytes).map_err(|_| ProtocolError::InvalidJson)?;
    if m.schema_version != MANIFEST_SCHEMA {
        return Err(ProtocolError::UnsupportedManifestSchema(m.schema_version));
    }
    if !m.api.is_supported() || (m.api.major == API_MAJOR && m.api.minor > API_MINOR) {
        return Err(ProtocolError::UnsupportedApi(m.api.major));
    }
    if !valid_provider_id(&m.id) {
        return Err(ProtocolError::InvalidProviderId);
    }
    if m.id == "builtin" {
        return Err(ProtocolError::ReservedProviderId);
    }
    if m.publisher.is_empty()
        || m.publisher.len() > limits.string_bytes
        || m.integrity.signing_key_id.is_empty()
        || m.integrity.signing_key_id.len() > limits.string_bytes
        || !valid_hex(&m.integrity.component_blake3, 32)
        || m.integrity.signature.is_empty()
        || m.integrity.signature.len() > limits.string_bytes
    {
        return Err(ProtocolError::InvalidIntegrity);
    }
    if let Some(cap) = m.requested.iter().find(|c| !granted.contains(c)) {
        return Err(ProtocolError::CapabilityNotGranted(*cap));
    }
    if blake3::hash(component).to_hex().as_str() != m.integrity.component_blake3 {
        return Err(ProtocolError::HashMismatch);
    }
    let mut signed = Vec::with_capacity(component.len() + manifest_bytes.len());
    signed.extend_from_slice(component);
    signed.extend_from_slice(&canonical_unsigned(&m));
    if !verifier.verify(&m.integrity.signing_key_id, &signed, &m.integrity.signature) {
        return Err(ProtocolError::SignatureRejected);
    }
    Ok(m)
}

pub fn decode_command(bytes: &[u8], limits: PluginLimits) -> Result<HostCommand, ProtocolError> {
    if bytes.len() > limits.request_bytes {
        return Err(ProtocolError::InputTooLarge);
    }
    let cmd: HostCommand = serde_json::from_slice(bytes).map_err(|_| ProtocolError::InvalidJson)?;
    if let HostCommand::GenerateShape(r) = &cmd {
        validate_request(r, limits)?
    }
    Ok(cmd)
}
fn validate_request(r: &ShapeRequest, l: PluginLimits) -> Result<(), ProtocolError> {
    if r.count as usize > l.points {
        return Err(ProtocolError::LimitExceeded("points"));
    }
    if r.params.len() > l.parameters {
        return Err(ProtocolError::LimitExceeded("parameters"));
    }
    if r.name.len() > l.string_bytes
        || r.params.iter().any(|(k, v)| {
            k.len() > l.string_bytes || matches!(v,ParamValue::Text(s) if s.len()>l.string_bytes)
        })
    {
        return Err(ProtocolError::LimitExceeded("string_bytes"));
    }
    if ![
        r.grid.width,
        r.grid.height,
        r.grid.horizontal_units,
        r.grid.vertical_units,
        r.grid.major_line_interval,
    ]
    .into_iter()
    .all(f32::is_finite)
        || !r.grid.hashes.iter().copied().all(f32::is_finite)
    {
        return Err(ProtocolError::LimitExceeded("finite_numbers"));
    }
    Ok(())
}

pub fn validate_response(
    command: &HostCommand,
    bytes: &[u8],
    manifest: &PluginManifest,
    limits: PluginLimits,
) -> Result<PluginResponse, ProtocolError> {
    if bytes.len() > limits.output_bytes {
        return Err(ProtocolError::InputTooLarge);
    }
    let r: PluginResponse =
        serde_json::from_slice(bytes).map_err(|_| ProtocolError::InvalidJson)?;
    match (command, &r) {
        (HostCommand::Describe, PluginResponse::Description(i))
            if i.provider_id == manifest.id && i.api.is_supported() => {}
        (HostCommand::ListShapes, PluginResponse::ShapeNames(n))
            if n.len() <= limits.parameters && n.iter().all(|s| s.len() <= limits.string_bytes) => {
        }
        (HostCommand::GenerateShape(q), PluginResponse::Points(p))
            if p.len() == q.count as usize
                && p.len() <= limits.points
                && p.iter().all(|v| v.x.is_finite() && v.y.is_finite()) => {}
        (_, PluginResponse::Error { code }) if code.len() <= limits.string_bytes => {}
        (HostCommand::Describe, PluginResponse::Description(_)) => {
            return Err(ProtocolError::ProviderMismatch);
        }
        _ => return Err(ProtocolError::ResponseMismatch),
    }
    Ok(r)
}

/// Records canonical request/result hashes; callers execute only in a sandbox.
#[derive(Default)]
pub struct DeterminismGuard {
    seen: BTreeMap<[u8; 32], [u8; 32]>,
}
impl DeterminismGuard {
    pub fn observe(&mut self, c: &HostCommand, r: &PluginResponse) -> Result<(), ProtocolError> {
        let cb = serde_json::to_vec(c).map_err(|_| ProtocolError::InvalidJson)?;
        let rb = serde_json::to_vec(r).map_err(|_| ProtocolError::InvalidJson)?;
        let ch = *blake3::hash(&cb).as_bytes();
        let rh = *blake3::hash(&rb).as_bytes();
        if self.seen.get(&ch).is_some_and(|old| old != &rh) {
            return Err(ProtocolError::NonDeterministic);
        }
        self.seen.insert(ch, rh);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Verifier;
    impl SignatureVerifier for Verifier {
        fn verify(&self, k: &str, _: &[u8], s: &str) -> bool {
            k == "review-2026" && s == "valid"
        }
    }
    fn manifest(c: &[u8]) -> PluginManifest {
        PluginManifest {
            schema_version: 1,
            api: ApiVersion::CURRENT,
            id: "org.example.shape".into(),
            version: Version::new(1, 2, 3),
            publisher: "Example".into(),
            requested: BTreeSet::new(),
            integrity: IntegrityMetadata {
                component_blake3: blake3::hash(c).to_hex().to_string(),
                signing_key_id: "review-2026".into(),
                signature: "valid".into(),
            },
        }
    }
    fn json(m: &PluginManifest) -> Vec<u8> {
        serde_json::to_vec(m).unwrap()
    }
    fn req(n: u32) -> HostCommand {
        HostCommand::GenerateShape(ShapeRequest {
            name: "arc".into(),
            count: n,
            grid: GridInfo {
                width: 160.,
                height: 84.,
                horizontal_steps: 8,
                horizontal_units: 5.,
                vertical_steps: 8,
                vertical_units: 5.,
                major_line_interval: 5.,
                hashes: vec![28., 56.],
            },
            params: BTreeMap::new(),
        })
    }
    #[test]
    fn capability_vocabulary_is_six() {
        assert_eq!(
            serde_json::to_value([
                Capability::ReadDocument,
                Capability::ReadPerformerLabels,
                Capability::ReadSelection,
                Capability::ProposeEdits,
                Capability::ReadAsset,
                Capability::WriteExport
            ])
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
            6
        )
    }
    #[test]
    fn validates_package() {
        let c = b"component";
        let m = manifest(c);
        assert_eq!(
            validate_package(
                &json(&m),
                c,
                &BTreeSet::new(),
                &Verifier,
                PluginLimits::DEFAULT
            )
            .unwrap(),
            m
        )
    }
    #[test]
    fn rejects_schema_api_hash_signature_capability() {
        let c = b"x";
        let mut m = manifest(c);
        m.schema_version = 2;
        assert!(matches!(
            validate_package(
                &json(&m),
                c,
                &BTreeSet::new(),
                &Verifier,
                PluginLimits::DEFAULT
            ),
            Err(ProtocolError::UnsupportedManifestSchema(2))
        ));
        m = manifest(c);
        m.api.major = 3;
        assert!(matches!(
            validate_package(
                &json(&m),
                c,
                &BTreeSet::new(),
                &Verifier,
                PluginLimits::DEFAULT
            ),
            Err(ProtocolError::UnsupportedApi(3))
        ));
        m = manifest(c);
        m.api.minor = API_MINOR + 1;
        assert!(matches!(
            validate_package(
                &json(&m),
                c,
                &BTreeSet::new(),
                &Verifier,
                PluginLimits::DEFAULT
            ),
            Err(ProtocolError::UnsupportedApi(1))
        ));
        m = manifest(c);
        assert_eq!(
            validate_package(
                &json(&m),
                b"y",
                &BTreeSet::new(),
                &Verifier,
                PluginLimits::DEFAULT
            ),
            Err(ProtocolError::HashMismatch)
        );
        m.integrity.signature = "bad".into();
        assert_eq!(
            validate_package(
                &json(&m),
                c,
                &BTreeSet::new(),
                &Verifier,
                PluginLimits::DEFAULT
            ),
            Err(ProtocolError::SignatureRejected)
        );
        m = manifest(c);
        m.requested.insert(Capability::ReadAsset);
        assert_eq!(
            validate_package(
                &json(&m),
                c,
                &BTreeSet::new(),
                &Verifier,
                PluginLimits::DEFAULT
            ),
            Err(ProtocolError::CapabilityNotGranted(Capability::ReadAsset))
        )
    }
    #[test]
    fn provider_ids_are_checked() {
        let c = b"x";
        for id in ["", "Builtin", "-bad", "a..b", "builtin"] {
            let mut m = manifest(c);
            m.id = id.into();
            assert!(
                validate_package(
                    &json(&m),
                    c,
                    &BTreeSet::new(),
                    &Verifier,
                    PluginLimits::DEFAULT
                )
                .is_err()
            )
        }
    }
    #[test]
    fn rejects_oversize_before_parse() {
        assert_eq!(
            decode_command(
                &vec![b' '; PluginLimits::DEFAULT.request_bytes + 1],
                PluginLimits::DEFAULT
            ),
            Err(ProtocolError::InputTooLarge)
        )
    }
    #[test]
    fn output_count_and_finite_are_checked() {
        let m = manifest(b"x");
        let c = req(2);
        let one =
            serde_json::to_vec(&PluginResponse::Points(vec![Point { x: 0., y: 0. }])).unwrap();
        assert_eq!(
            validate_response(&c, &one, &m, PluginLimits::DEFAULT),
            Err(ProtocolError::ResponseMismatch)
        );
        let nan = serde_json::to_vec(&PluginResponse::Points(vec![
            Point { x: f32::NAN, y: 0. };
            2
        ]))
        .unwrap();
        assert_eq!(
            validate_response(&c, &nan, &m, PluginLimits::DEFAULT),
            // JSON has no non-finite number representation; serde emits null,
            // which is rejected before a point can enter the host model.
            Err(ProtocolError::InvalidJson)
        )
    }
    #[test]
    fn spoofed_provider_is_rejected() {
        let m = manifest(b"x");
        let b = serde_json::to_vec(&PluginResponse::Description(PluginInfo {
            provider_id: "org.bad".into(),
            display_name_key: "name".into(),
            version: Version::new(1, 0, 0),
            api: ApiVersion::CURRENT,
        }))
        .unwrap();
        assert_eq!(
            validate_response(&HostCommand::Describe, &b, &m, PluginLimits::DEFAULT),
            Err(ProtocolError::ProviderMismatch)
        )
    }
    #[test]
    fn changed_answer_is_nondeterministic() {
        let c = req(1);
        let mut g = DeterminismGuard::default();
        g.observe(&c, &PluginResponse::Points(vec![Point { x: 1., y: 2. }]))
            .unwrap();
        assert_eq!(
            g.observe(&c, &PluginResponse::Points(vec![Point { x: 2., y: 2. }])),
            Err(ProtocolError::NonDeterministic)
        )
    }
}
