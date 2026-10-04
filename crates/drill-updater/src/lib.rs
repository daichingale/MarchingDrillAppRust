//! Update policy, manifest verification, and a tightly scoped update transport.
//!
//! Policy and verification remain pure. The optional transport can fetch only the
//! compiled-in manifest endpoint; package download and installation stay explicit.

mod transport;
pub use transport::{
    MANIFEST_HOST, MANIFEST_PATH, MAX_ATTESTATION_BYTES, MAX_MANIFEST_BYTES, MAX_PACKAGE_BYTES,
    ResourceKind, Response, TransportError, UpdateTransport, fetch_manifest, fetch_resource,
};

pub use semver::Version;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    #[default]
    Stable,
    Beta,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub download_url: String,
    pub sha256: String,
    /// URL of a Sigstore/GitHub provenance bundle. It is a reference, not a
    /// claim that this process contacted or trusted that endpoint.
    pub attestation_url: String,
    pub attestation_sha256: String,
    /// Expected certificate identity/workflow recorded in the attestation.
    pub signer_identity: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: Version,
    pub artifact: Artifact,
    pub release_notes_ja: String,
    pub release_notes_en: String,
    #[serde(default)]
    pub emergency_hotfix: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u16,
    pub stable: Release,
    #[serde(default)]
    pub beta: Option<Release>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    Json(String),
    UnsupportedSchema(u16),
    InvalidUrl(&'static str),
    InvalidSha256(&'static str),
    MissingSigner,
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid update manifest: {e}"),
            Self::UnsupportedSchema(v) => write!(f, "unsupported manifest schema {v}"),
            Self::InvalidUrl(field) => write!(f, "invalid HTTPS URL in {field}"),
            Self::InvalidSha256(field) => write!(f, "invalid SHA-256 in {field}"),
            Self::MissingSigner => f.write_str("attestation signer identity is empty"),
        }
    }
}

impl std::error::Error for ManifestError {}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, ManifestError> {
        let manifest: Self = serde_json::from_slice(bytes)
            .map_err(|error| ManifestError::Json(error.to_string()))?;
        if manifest.schema_version != 1 {
            return Err(ManifestError::UnsupportedSchema(manifest.schema_version));
        }
        validate_release(&manifest.stable)?;
        if let Some(beta) = &manifest.beta {
            validate_release(beta)?;
        }
        Ok(manifest)
    }

    #[must_use]
    pub fn release(&self, channel: Channel) -> &Release {
        match channel {
            Channel::Stable => &self.stable,
            Channel::Beta => self.beta.as_ref().unwrap_or(&self.stable),
        }
    }
}

fn validate_release(release: &Release) -> Result<(), ManifestError> {
    let artifact = &release.artifact;
    for (value, field) in [
        (&artifact.download_url, "download_url"),
        (&artifact.attestation_url, "attestation_url"),
    ] {
        if !value.starts_with("https://") || value.contains(['\r', '\n']) {
            return Err(ManifestError::InvalidUrl(field));
        }
    }
    validate_digest(&artifact.sha256, "sha256")?;
    validate_digest(&artifact.attestation_sha256, "attestation_sha256")?;
    if artifact.signer_identity.trim().is_empty() {
        return Err(ManifestError::MissingSigner);
    }
    Ok(())
}

fn validate_digest(value: &str, field: &'static str) -> Result<(), ManifestError> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ManifestError::InvalidSha256(field));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateDecision {
    Available(Box<Release>),
    Current,
    DowngradeRejected,
    Skipped,
    BlackoutDeferred,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preferences {
    pub channel: Channel,
    pub skipped_version: Option<Version>,
}

/// 7/15 through 12/15 inclusive is the marching-season blackout.
#[must_use]
pub const fn is_blackout(month: u8, day: u8) -> bool {
    (month > 7 && month < 12) || (month == 7 && day >= 15) || (month == 12 && day <= 15)
}

/// During blackout normal checks are weekly; otherwise they may run daily.
#[must_use]
pub const fn minimum_check_interval_secs(month: u8, day: u8) -> u64 {
    if is_blackout(month, day) {
        7 * 24 * 60 * 60
    } else {
        24 * 60 * 60
    }
}

#[must_use]
pub fn decide(
    current: &Version,
    manifest: &Manifest,
    preferences: &Preferences,
    month: u8,
    day: u8,
) -> UpdateDecision {
    let release = manifest.release(preferences.channel);
    if release.version < *current {
        return UpdateDecision::DowngradeRejected;
    }
    if release.version == *current {
        return UpdateDecision::Current;
    }
    if preferences.skipped_version.as_ref() == Some(&release.version) {
        return UpdateDecision::Skipped;
    }
    if is_blackout(month, day) && !release.emergency_hotfix {
        return UpdateDecision::BlackoutDeferred;
    }
    UpdateDecision::Available(Box::new(release.clone()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyError {
    Sha256Mismatch,
    AttestationSha256Mismatch,
    SignerMismatch,
    ManifestSignature,
}

/// Signature verification boundary. Production and tests use this exact API;
/// private signing material is never accepted by updater production code.
pub trait ManifestSignatureVerifier {
    fn verify(&self, payload: &[u8], signature: &[u8]) -> bool;
}

pub fn verify_signed_manifest(
    payload: &[u8],
    signature: &[u8],
    verifier: &impl ManifestSignatureVerifier,
) -> Result<Manifest, VerifyError> {
    if !verifier.verify(payload, signature) {
        return Err(VerifyError::ManifestSignature);
    }
    Manifest::parse(payload).map_err(|_| VerifyError::ManifestSignature)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedUpdate {
    pub release: Release,
    pub package: Vec<u8>,
    pub attestation: Vec<u8>,
}

/// Downloads and verifies an available update. It deliberately returns inert
/// bytes: updater has no installer-launch or process-execution capability.
pub fn prepare_update(
    transport: &impl UpdateTransport,
    release: &Release,
    attested_signer: &str,
) -> Result<PreparedUpdate, PrepareError> {
    let package = fetch_resource(
        transport,
        &release.artifact.download_url,
        ResourceKind::Package,
    )
    .map_err(PrepareError::Transport)?;
    let attestation = fetch_resource(
        transport,
        &release.artifact.attestation_url,
        ResourceKind::Attestation,
    )
    .map_err(PrepareError::Transport)?;
    verify_download(&release.artifact, &package, &attestation, attested_signer)
        .map_err(PrepareError::Verify)?;
    Ok(PreparedUpdate {
        release: release.clone(),
        package,
        attestation,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrepareError {
    Transport(TransportError),
    Verify(VerifyError),
}

/// Verifies downloaded bytes plus the locally supplied attestation bundle.
/// Cryptographic verification of the bundle itself remains the platform's
/// Sigstore verifier responsibility; this binds its digest and identity to the
/// already-validated manifest before any installer may be offered.
pub fn verify_download(
    artifact: &Artifact,
    package: &[u8],
    attestation: &[u8],
    attested_signer: &str,
) -> Result<(), VerifyError> {
    if sha256_hex(package) != artifact.sha256.to_ascii_lowercase() {
        return Err(VerifyError::Sha256Mismatch);
    }
    if sha256_hex(attestation) != artifact.attestation_sha256.to_ascii_lowercase() {
        return Err(VerifyError::AttestationSha256Mismatch);
    }
    if attested_signer != artifact.signer_identity {
        return Err(VerifyError::SignerMismatch);
    }
    Ok(())
}

// Compact dependency-free SHA-256 keeps this pure crate usable offline.
#[must_use]
pub fn sha256_hex(input: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut data = input.to_vec();
    let bit_len = (data.len() as u64) * 8;
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());
    let mut h = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    for block in data.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in w[..16].iter_mut().enumerate() {
            *word = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().expect("four bytes"));
        }
        for i in 16..64 {
            let a = w[i - 15];
            let b = w[i - 2];
            w[i] = w[i - 16]
                .wrapping_add(a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3))
                .wrapping_add(w[i - 7])
                .wrapping_add(b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10));
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let t1 = hh
                .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22))
                .wrapping_add((a & b) ^ (a & c) ^ (b & c));
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        let v = [a, b, c, d, e, f, g, hh];
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|v| format!("{v:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(version: &str, hotfix: bool) -> Release {
        Release {
            version: Version::parse(version).unwrap(),
            artifact: Artifact {
                download_url: "https://example.test/app.msix".into(),
                sha256: sha256_hex(b"pkg"),
                attestation_url: "https://example.test/app.sigstore".into(),
                attestation_sha256: sha256_hex(b"proof"),
                signer_identity: "release.yml@github".into(),
            },
            release_notes_ja: "更新".into(),
            release_notes_en: "Update".into(),
            emergency_hotfix: hotfix,
        }
    }
    fn manifest(version: &str) -> Manifest {
        Manifest {
            schema_version: 1,
            stable: release(version, false),
            beta: Some(release("2.0.0-beta.1", false)),
        }
    }
    #[test]
    fn sha_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
    #[test]
    fn semver_and_skip() {
        let m = manifest("1.2.0");
        let mut p = Preferences::default();
        assert!(matches!(
            decide(&Version::new(1, 1, 0), &m, &p, 1, 1),
            UpdateDecision::Available(_)
        ));
        p.skipped_version = Some(Version::new(1, 2, 0));
        assert_eq!(
            decide(&Version::new(1, 1, 0), &m, &p, 1, 1),
            UpdateDecision::Skipped
        );
        assert_eq!(
            decide(&Version::new(2, 0, 0), &m, &p, 1, 1),
            UpdateDecision::DowngradeRejected
        );
    }
    #[test]
    fn channels_and_blackout() {
        let m = manifest("1.0.0");
        let p = Preferences {
            channel: Channel::Beta,
            skipped_version: None,
        };
        assert_eq!(
            m.release(p.channel).version,
            Version::parse("2.0.0-beta.1").unwrap()
        );
        assert_eq!(
            decide(&Version::new(0, 9, 0), &m, &Preferences::default(), 8, 1),
            UpdateDecision::BlackoutDeferred
        );
        assert_eq!(minimum_check_interval_secs(8, 1), 604800);
    }
    #[test]
    fn hotfix_crosses_blackout() {
        let mut m = manifest("1.1.0");
        m.stable.emergency_hotfix = true;
        assert!(matches!(
            decide(&Version::new(1, 0, 0), &m, &Preferences::default(), 12, 1),
            UpdateDecision::Available(_)
        ));
    }
    #[test]
    fn validates_manifest_references() {
        let json = serde_json::to_vec(&manifest("1.0.1")).unwrap();
        assert!(Manifest::parse(&json).is_ok());
        let mut bad = manifest("1.0.1");
        bad.stable.artifact.download_url = "http://unsafe".into();
        assert!(matches!(
            Manifest::parse(&serde_json::to_vec(&bad).unwrap()),
            Err(ManifestError::InvalidUrl(_))
        ));
    }
    #[test]
    fn verifies_both_digests_and_signer() {
        let a = manifest("1.0.1").stable.artifact;
        assert_eq!(
            verify_download(&a, b"pkg", b"proof", "release.yml@github"),
            Ok(())
        );
        assert_eq!(
            verify_download(&a, b"bad", b"proof", "release.yml@github"),
            Err(VerifyError::Sha256Mismatch)
        );
        assert_eq!(
            verify_download(&a, b"pkg", b"proof", "other"),
            Err(VerifyError::SignerMismatch)
        );
    }
}
