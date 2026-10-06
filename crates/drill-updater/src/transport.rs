//! Minimal, bounded transport for the signed update manifest.

use crate::{Manifest, ManifestError};

/// Release builds provide these at compile time. Development builds intentionally
/// have no pretend server and report `NotConfigured`.
pub const MANIFEST_HOST: Option<&str> = option_env!("DRILLFORGE_UPDATE_HOST");
pub const MANIFEST_PATH: Option<&str> = option_env!("DRILLFORGE_UPDATE_PATH");
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;
pub const MAX_PACKAGE_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_ATTESTATION_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    Manifest,
    Package,
    Attestation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    /// Set only after the transport has authenticated TLS for the requested origin.
    pub tls_authenticated: bool,
    pub redirected: bool,
}

/// Pure transport seam used by production and release-readiness tests.
pub trait UpdateTransport {
    fn get(&self, url: &str, kind: ResourceKind) -> Result<Response, TransportError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransportError {
    NotConfigured,
    Unsupported,
    Offline,
    Timeout,
    Tls,
    InsecureUrl,
    HttpStatus(u16),
    RedirectRejected,
    InvalidContentType,
    TooLarge,
    InvalidManifest(ManifestError),
    Platform(u32),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => f.write_str("update endpoint is not configured in this build"),
            Self::Unsupported => f.write_str("update transport is unsupported on this platform"),
            Self::Offline => f.write_str("network is unavailable"),
            Self::Timeout => f.write_str("update server timed out"),
            Self::Tls => f.write_str("secure connection validation failed"),
            Self::InsecureUrl => f.write_str("update resource did not use authenticated HTTPS"),
            Self::HttpStatus(code) => write!(f, "update server returned HTTP {code}"),
            Self::RedirectRejected => f.write_str("update server redirect was rejected"),
            Self::InvalidContentType => f.write_str("update server returned a non-JSON response"),
            Self::TooLarge => f.write_str("update manifest exceeds the size limit"),
            Self::InvalidManifest(error) => write!(f, "{error}"),
            Self::Platform(code) => write!(f, "update transport failed (OS error {code})"),
        }
    }
}

pub fn fetch_resource(
    transport: &impl UpdateTransport,
    url: &str,
    kind: ResourceKind,
) -> Result<Vec<u8>, TransportError> {
    if !url.starts_with("https://") || url.contains(['\r', '\n']) {
        return Err(TransportError::InsecureUrl);
    }
    let response = transport.get(url, kind)?;
    if !response.tls_authenticated {
        return Err(TransportError::Tls);
    }
    validate_response_for(kind, response)
}

fn validate_response_for(
    kind: ResourceKind,
    response: Response,
) -> Result<Vec<u8>, TransportError> {
    if response.redirected || (300..400).contains(&response.status) {
        return Err(TransportError::RedirectRejected);
    }
    if response.status != 200 {
        return Err(TransportError::HttpStatus(response.status));
    }
    let media_type = response.content_type.split(';').next().unwrap_or("").trim();
    let type_ok = match kind {
        ResourceKind::Manifest => {
            media_type.eq_ignore_ascii_case("application/json")
                || media_type.eq_ignore_ascii_case("application/manifest+json")
        }
        ResourceKind::Package => media_type.eq_ignore_ascii_case("application/octet-stream"),
        ResourceKind::Attestation => {
            media_type.eq_ignore_ascii_case("application/json")
                || media_type.eq_ignore_ascii_case("application/vnd.dev.sigstore.bundle+json")
        }
    };
    if !type_ok {
        return Err(TransportError::InvalidContentType);
    }
    let limit = match kind {
        ResourceKind::Manifest => MAX_MANIFEST_BYTES,
        ResourceKind::Package => MAX_PACKAGE_BYTES,
        ResourceKind::Attestation => MAX_ATTESTATION_BYTES,
    };
    if response.body.len() > limit {
        return Err(TransportError::TooLarge);
    }
    Ok(response.body)
}

impl std::error::Error for TransportError {}

/// Fetches and validates the manifest from DrillForge's compiled-in endpoint.
/// There is intentionally no public URL parameter.
pub fn fetch_manifest() -> Result<Manifest, TransportError> {
    let host = MANIFEST_HOST.ok_or(TransportError::NotConfigured)?;
    let path = MANIFEST_PATH.ok_or(TransportError::NotConfigured)?;
    if host.is_empty()
        || !host.is_ascii()
        || host.contains(['/', ':', '\\'])
        || !path.starts_with('/')
        || !path.is_ascii()
    {
        return Err(TransportError::NotConfigured);
    }
    let bytes = platform::fetch_bytes(host, path)?;
    Manifest::parse(&bytes).map_err(TransportError::InvalidManifest)
}

#[cfg(any(windows, test))]
fn validate_response(
    status: u16,
    content_type: &str,
    body: Vec<u8>,
) -> Result<Vec<u8>, TransportError> {
    if (300..400).contains(&status) {
        return Err(TransportError::RedirectRejected);
    }
    if status != 200 {
        return Err(TransportError::HttpStatus(status));
    }
    let media_type = content_type.split(';').next().unwrap_or("").trim();
    if !media_type.eq_ignore_ascii_case("application/json")
        && !media_type.eq_ignore_ascii_case("application/manifest+json")
    {
        return Err(TransportError::InvalidContentType);
    }
    if body.len() > MAX_MANIFEST_BYTES {
        return Err(TransportError::TooLarge);
    }
    Ok(body)
}

#[cfg(not(windows))]
mod platform {
    use super::TransportError;
    pub(super) fn fetch_bytes(_host: &str, _path: &str) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::Unsupported)
    }
}

#[cfg(windows)]
mod platform {
    use super::{MAX_MANIFEST_BYTES, TransportError, validate_response};
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Networking::WinHttp::*;

    const TIMEOUT_MS: i32 = 10_000;

    struct Handle(*mut c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn last_error() -> TransportError {
        let code = unsafe { GetLastError() };
        match code {
            ERROR_WINHTTP_TIMEOUT => TransportError::Timeout,
            ERROR_WINHTTP_CANNOT_CONNECT
            | ERROR_WINHTTP_NAME_NOT_RESOLVED
            | ERROR_WINHTTP_CONNECTION_ERROR => TransportError::Offline,
            ERROR_WINHTTP_SECURE_FAILURE
            | ERROR_WINHTTP_SECURE_CHANNEL_ERROR
            | ERROR_WINHTTP_SECURE_CERT_CN_INVALID
            | ERROR_WINHTTP_SECURE_CERT_DATE_INVALID
            | ERROR_WINHTTP_SECURE_INVALID_CA
            | ERROR_WINHTTP_SECURE_CERT_REVOKED
            | ERROR_WINHTTP_SECURE_CERT_WRONG_USAGE => TransportError::Tls,
            _ => TransportError::Platform(code),
        }
    }

    unsafe fn query_number(request: *mut c_void, kind: u32) -> Result<u32, TransportError> {
        let mut value = 0u32;
        let mut size = size_of::<u32>() as u32;
        if unsafe {
            WinHttpQueryHeaders(
                request,
                kind | WINHTTP_QUERY_FLAG_NUMBER,
                null(),
                (&mut value as *mut u32).cast(),
                &mut size,
                null_mut(),
            )
        } == 0
        {
            return Err(last_error());
        }
        Ok(value)
    }

    unsafe fn query_string(request: *mut c_void, kind: u32) -> Result<String, TransportError> {
        let mut size = 0u32;
        unsafe { WinHttpQueryHeaders(request, kind, null(), null_mut(), &mut size, null_mut()) };
        if size == 0 {
            return Err(last_error());
        }
        let mut buffer = vec![0u16; size as usize / 2];
        if unsafe {
            WinHttpQueryHeaders(
                request,
                kind,
                null(),
                buffer.as_mut_ptr().cast(),
                &mut size,
                null_mut(),
            )
        } == 0
        {
            return Err(last_error());
        }
        let end = buffer
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(buffer.len());
        Ok(String::from_utf16_lossy(&buffer[..end]))
    }

    pub(super) fn fetch_bytes(
        host_name: &str,
        request_path: &str,
    ) -> Result<Vec<u8>, TransportError> {
        let agent = wide("DrillForge-Updater/1");
        let host = wide(host_name);
        let path = wide(request_path);
        let get = wide("GET");
        let accept = wide("application/json");
        let accept_types = [accept.as_ptr(), null()];
        let session = Handle(unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                null(),
                null(),
                0,
            )
        });
        if session.0.is_null() {
            return Err(last_error());
        }
        if unsafe { WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS) }
            == 0
        {
            return Err(last_error());
        }
        let connection = Handle(unsafe {
            WinHttpConnect(session.0, host.as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0)
        });
        if connection.0.is_null() {
            return Err(last_error());
        }
        let request = Handle(unsafe {
            WinHttpOpenRequest(
                connection.0,
                get.as_ptr(),
                path.as_ptr(),
                null(),
                null(),
                accept_types.as_ptr(),
                WINHTTP_FLAG_SECURE,
            )
        });
        if request.0.is_null() {
            return Err(last_error());
        }
        let redirect_policy = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
        if unsafe {
            WinHttpSetOption(
                request.0,
                WINHTTP_OPTION_REDIRECT_POLICY,
                (&redirect_policy as *const u32).cast(),
                size_of::<u32>() as u32,
            )
        } == 0
        {
            return Err(last_error());
        }
        if unsafe { WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0) } == 0
            || unsafe { WinHttpReceiveResponse(request.0, null_mut()) } == 0
        {
            return Err(last_error());
        }

        let status = unsafe { query_number(request.0, WINHTTP_QUERY_STATUS_CODE)? } as u16;
        let content_type = unsafe { query_string(request.0, WINHTTP_QUERY_CONTENT_TYPE)? };
        let mut output = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let mut read = 0u32;
            if unsafe {
                WinHttpReadData(
                    request.0,
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    &mut read,
                )
            } == 0
            {
                return Err(last_error());
            }
            if read == 0 {
                break;
            }
            if output.len() + read as usize > MAX_MANIFEST_BYTES {
                return Err(TransportError::TooLarge);
            }
            output.extend_from_slice(&chunk[..read as usize]);
        }
        validate_response(status, &content_type, output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_is_compile_time_only_and_has_no_user_input() {
        assert!(MANIFEST_HOST.is_none() || !MANIFEST_HOST.unwrap().contains('/'));
        assert!(MANIFEST_PATH.is_none() || MANIFEST_PATH.unwrap().starts_with('/'));
        if MANIFEST_HOST.is_none() || MANIFEST_PATH.is_none() {
            assert_eq!(fetch_manifest(), Err(TransportError::NotConfigured));
        }
    }
    #[test]
    fn errors_are_stable_and_nonempty() {
        for error in [
            TransportError::NotConfigured,
            TransportError::Offline,
            TransportError::Timeout,
            TransportError::Tls,
            TransportError::HttpStatus(503),
            TransportError::RedirectRejected,
            TransportError::InvalidContentType,
            TransportError::TooLarge,
        ] {
            assert!(!error.to_string().is_empty());
        }
    }

    #[test]
    fn mock_response_boundary_enforces_status_type_redirect_and_size() {
        assert_eq!(
            validate_response(200, "application/json; charset=utf-8", b"{}".to_vec()),
            Ok(b"{}".to_vec())
        );
        assert_eq!(
            validate_response(302, "application/json", vec![]),
            Err(TransportError::RedirectRejected)
        );
        assert_eq!(
            validate_response(503, "application/json", vec![]),
            Err(TransportError::HttpStatus(503))
        );
        assert_eq!(
            validate_response(200, "text/html", vec![]),
            Err(TransportError::InvalidContentType)
        );
        assert_eq!(
            validate_response(200, "application/json", vec![0; MAX_MANIFEST_BYTES + 1]),
            Err(TransportError::TooLarge)
        );
    }
}
