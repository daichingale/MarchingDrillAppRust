use drill_updater::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

// Test-only signing fixture. Production receives only ManifestSignatureVerifier.
const TEST_PRIVATE_KEY: &[u8] = b"drillforge-release-readiness-test-key-not-for-production";

struct FixtureVerifier;
impl ManifestSignatureVerifier for FixtureVerifier {
    fn verify(&self, payload: &[u8], signature: &[u8]) -> bool {
        fixture_sign(payload).as_bytes() == signature
    }
}
fn fixture_sign(payload: &[u8]) -> String {
    let mut input = TEST_PRIVATE_KEY.to_vec();
    input.extend_from_slice(payload);
    sha256_hex(&input)
}

#[derive(Clone)]
struct LocalhostTransport {
    address: Arc<String>,
    tls_authenticated: bool,
}
impl UpdateTransport for LocalhostTransport {
    fn get(&self, url: &str, _kind: ResourceKind) -> Result<Response, TransportError> {
        let path = url
            .strip_prefix("https://localhost")
            .ok_or(TransportError::InsecureUrl)?;
        let mut stream = (0..20)
            .find_map(|_| match TcpStream::connect(&**self.address) {
                Ok(stream) => Some(stream),
                Err(_) => {
                    thread::yield_now();
                    None
                }
            })
            .ok_or(TransportError::Offline)?;
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .map_err(|_| TransportError::Offline)?;
        stream.flush().map_err(|_| TransportError::Offline)?;
        let mut raw = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => {
                    raw.extend_from_slice(&chunk[..count]);
                    if response_is_complete(&raw) {
                        break;
                    }
                }
                // Windows may report reset after a test server has already sent
                // a complete Connection: close response.
                Err(error)
                    if error.kind() == std::io::ErrorKind::ConnectionReset && !raw.is_empty() =>
                {
                    break;
                }
                Err(_) => return Err(TransportError::Offline),
            }
        }
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or(TransportError::Offline)?;
        let headers = String::from_utf8_lossy(&raw[..split]);
        let status = headers
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(500);
        let content_type = headers
            .lines()
            .find_map(|line| line.strip_prefix("Content-Type: ").map(str::to_owned))
            .unwrap_or_default();
        Ok(Response {
            status,
            content_type,
            body: raw[split + 4..].to_vec(),
            tls_authenticated: self.tls_authenticated,
            redirected: (300..400).contains(&status),
        })
    }
}

fn response_is_complete(raw: &[u8]) -> bool {
    let Some(header_end) = raw.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let headers = String::from_utf8_lossy(&raw[..header_end]);
    let content_length = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    });
    content_length.is_some_and(|length| raw.len() >= header_end + 4 + length)
}

fn server(
    routes: Vec<(&'static str, u16, &'static str, Vec<u8>)>,
) -> (LocalhostTransport, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = Arc::new(listener.local_addr().unwrap().to_string());
    let handle = thread::spawn(move || {
        for (_, status, content_type, body) in routes {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 512];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0, "client closed before completing request headers");
                request.extend_from_slice(&chunk[..count]);
                assert!(request.len() <= 2048, "request headers exceeded test bound");
            }
            write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
            stream.write_all(&body).unwrap();
            stream.flush().unwrap();
            stream.shutdown(std::net::Shutdown::Write).unwrap();
        }
    });
    (
        LocalhostTransport {
            address,
            tls_authenticated: true,
        },
        handle,
    )
}

fn manifest(package: &[u8], attestation: &[u8]) -> Manifest {
    let artifact = Artifact {
        download_url: "https://localhost/package.bin".into(),
        sha256: sha256_hex(package),
        attestation_url: "https://localhost/attestation.json".into(),
        attestation_sha256: sha256_hex(attestation),
        signer_identity: "release.yml@github".into(),
    };
    Manifest {
        schema_version: 1,
        stable: Release {
            version: Version::new(1, 1, 0),
            artifact: artifact.clone(),
            release_notes_ja: "更新".into(),
            release_notes_en: "Update".into(),
            emergency_hotfix: false,
        },
        beta: Some(Release {
            version: Version::parse("1.2.0-beta.1").unwrap(),
            artifact,
            release_notes_ja: "ベータ".into(),
            release_notes_en: "Beta".into(),
            emergency_hotfix: true,
        }),
    }
}

#[test]
fn signed_manifest_to_inert_verified_download_e2e() {
    let package = b"signed installer bytes";
    let attestation = br#"{"certificateIdentity":"release.yml@github"}"#;
    let payload = serde_json::to_vec(&manifest(package, attestation)).unwrap();
    let signature = fixture_sign(&payload);
    let (transport, server) = server(vec![
        (
            "/manifest.json",
            200,
            "application/manifest+json",
            payload.clone(),
        ),
        (
            "/package.bin",
            200,
            "application/octet-stream",
            package.to_vec(),
        ),
        (
            "/attestation.json",
            200,
            "application/vnd.dev.sigstore.bundle+json",
            attestation.to_vec(),
        ),
    ]);
    let fetched = fetch_resource(
        &transport,
        "https://localhost/manifest.json",
        ResourceKind::Manifest,
    )
    .unwrap();
    let manifest =
        verify_signed_manifest(&fetched, signature.as_bytes(), &FixtureVerifier).unwrap();
    assert_eq!(
        decide(
            &Version::new(1, 0, 0),
            &manifest,
            &Preferences::default(),
            8,
            1
        ),
        UpdateDecision::BlackoutDeferred
    );
    let beta = Preferences {
        channel: Channel::Beta,
        skipped_version: None,
    };
    let release = match decide(&Version::new(1, 0, 0), &manifest, &beta, 8, 1) {
        UpdateDecision::Available(release) => release,
        other => panic!("unexpected {other:?}"),
    };
    let prepared = prepare_update(&transport, &release, "release.yml@github").unwrap();
    assert_eq!(prepared.package, package);
    assert_eq!(prepared.attestation, attestation);
    server.join().unwrap();
}

#[test]
fn transport_policy_rejects_redirect_type_size_and_unauthenticated_tls() {
    let cases = [
        (
            Response {
                status: 302,
                content_type: "application/json".into(),
                body: vec![],
                tls_authenticated: true,
                redirected: true,
            },
            TransportError::RedirectRejected,
        ),
        (
            Response {
                status: 200,
                content_type: "text/html".into(),
                body: vec![],
                tls_authenticated: true,
                redirected: false,
            },
            TransportError::InvalidContentType,
        ),
        (
            Response {
                status: 200,
                content_type: "application/json".into(),
                body: vec![0; MAX_MANIFEST_BYTES + 1],
                tls_authenticated: true,
                redirected: false,
            },
            TransportError::TooLarge,
        ),
        (
            Response {
                status: 200,
                content_type: "application/json".into(),
                body: b"{}".to_vec(),
                tls_authenticated: false,
                redirected: false,
            },
            TransportError::Tls,
        ),
    ];
    struct One(Response);
    impl UpdateTransport for One {
        fn get(&self, _: &str, _: ResourceKind) -> Result<Response, TransportError> {
            Ok(self.0.clone())
        }
    }
    for (response, expected) in cases {
        assert_eq!(
            fetch_resource(
                &One(response),
                "https://localhost/manifest",
                ResourceKind::Manifest
            ),
            Err(expected)
        );
    }
    assert_eq!(
        fetch_resource(
            &One(Response {
                status: 200,
                content_type: "application/json".into(),
                body: vec![],
                tls_authenticated: true,
                redirected: false
            }),
            "http://localhost/manifest",
            ResourceKind::Manifest
        ),
        Err(TransportError::InsecureUrl)
    );
}

#[test]
fn signed_manifest_hash_and_signer_fail_closed() {
    let package = b"package";
    let attestation = b"attestation";
    let payload = serde_json::to_vec(&manifest(package, attestation)).unwrap();
    assert_eq!(
        verify_signed_manifest(&payload, b"bad-signature", &FixtureVerifier),
        Err(VerifyError::ManifestSignature)
    );
    let artifact = manifest(package, attestation).stable.artifact;
    assert_eq!(
        verify_download(&artifact, b"tampered", attestation, "release.yml@github"),
        Err(VerifyError::Sha256Mismatch)
    );
    assert_eq!(
        verify_download(&artifact, package, attestation, "wrong-signer"),
        Err(VerifyError::SignerMismatch)
    );
}
