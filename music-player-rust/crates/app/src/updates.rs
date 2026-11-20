//! Port of `GitHubUpdates.cs` and `ReleaseVersion.cs`.
//!
//! The original app uses `HttpClient`; this port keeps the same request shape
//! (user agent, GitHub media type, ETag revalidation, 8 second timeout) but
//! talks to WinHTTP so the shipping binary does not gain an HTTP stack.
//! Blocking calls run on a worker thread owned by `App`.

use serde_json::Value;
use std::ffi::c_void;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{GetLastError, ERROR_INSUFFICIENT_BUFFER};
use windows_sys::Win32::Networking::WinHttp as wh;

pub const REPOSITORY: &str = "kalabhaftu/music-player";
pub const REPOSITORY_URL: &str = "https://github.com/kalabhaftu/music-player";
pub const RELEASES_URL: &str = "https://github.com/kalabhaftu/music-player/releases";

const RELEASES_API_HOST: &str = "api.github.com";
const RELEASES_API_PATH: &str = "/repos/kalabhaftu/music-player/releases?per_page=10";
const RELEASES_PAGE_HOST: &str = "github.com";
const LATEST_STABLE_PATH: &str = "/kalabhaftu/music-player/releases/latest";
const RELEASE_TAG_PATH_PREFIX: &str = "/kalabhaftu/music-player/releases/tag/";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

// ---------------------------------------------------------------------------
// ReleaseVersion (1:1 with MusicPlayer.Core/ReleaseVersion.cs)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReleaseVersion {
    pub major: i32,
    pub minor: i32,
    pub patch: i32,
    pub preview: Option<i32>,
}

impl ReleaseVersion {
    pub fn is_prerelease(self) -> bool {
        self.preview.is_some()
    }

    /// Mirrors `int.TryParse(..., NumberStyles.None, InvariantCulture, ...)`:
    /// digits only, no sign, no whitespace, no overflow.
    fn parse_number(text: &str) -> Option<i32> {
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        text.parse::<i32>().ok()
    }

    pub fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut value = trimmed;
        if let Some(rest) = value.strip_prefix('v').or_else(|| value.strip_prefix('V')) {
            value = rest;
        }
        if let Some(index) = value.find('+') {
            value = &value[..index];
        }

        // C# uses IndexOf(..., OrdinalIgnoreCase) on the ASCII marker; scan on
        // bytes so slicing stays valid even for non-ASCII tag text.
        let marker = "-preview.";
        let mut preview = None;
        if let Some(index) = find_ascii_case_insensitive(value, marker) {
            let suffix = &value[index + marker.len()..];
            if suffix.contains('-') {
                return None;
            }
            let number = Self::parse_number(suffix)?;
            if number < 0 {
                return None;
            }
            preview = Some(number);
            value = &value[..index];
        } else if value.contains('-') {
            return None;
        }

        let mut parts = value.split('.');
        let major = Self::parse_number(parts.next()?)?;
        let minor = Self::parse_number(parts.next()?)?;
        let patch = Self::parse_number(parts.next()?)?;
        if parts.next().is_some() || major < 0 || minor < 0 || patch < 0 {
            return None;
        }
        Some(Self { major, minor, patch, preview })
    }

    fn cmp_preview(self, other: Self) -> std::cmp::Ordering {
        match (self.preview, other.preview) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (Some(left), Some(right)) => left.cmp(&right),
        }
    }
}

impl Ord for ReleaseVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
            .then_with(|| self.cmp_preview(*other))
    }
}

impl PartialOrd for ReleaseVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let bytes = haystack.as_bytes();
    let target = needle.as_bytes();
    if target.is_empty() || bytes.len() < target.len() {
        return None;
    }
    bytes
        .windows(target.len())
        .position(|window| window.eq_ignore_ascii_case(target))
}

/// Port of `ApplicationVersion.Compare`.
#[allow(dead_code)]
pub fn compare(current: &str, latest: Option<&str>) -> UpdateComparison {
    let Some(installed) = ReleaseVersion::parse(current) else {
        return UpdateComparison::UnknownVersion;
    };
    let Some(available) = latest.and_then(ReleaseVersion::parse) else {
        return UpdateComparison::NoEligibleRelease;
    };
    if available > installed {
        UpdateComparison::NewerRelease
    } else {
        UpdateComparison::CurrentRelease
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateComparison {
    NewerRelease,
    CurrentRelease,
    NoEligibleRelease,
    UnknownVersion,
}

// ---------------------------------------------------------------------------
// Release records and URL validation (GitHubUpdates.cs)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub url: String,
    pub draft: bool,
    pub prerelease: bool,
}

#[derive(Clone, Debug)]
pub struct UpdateCheck {
    pub release: Option<Release>,
    pub etag: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateErrorKind {
    RateLimited,
    Timeout,
    Network,
    InvalidData,
}

#[derive(Clone, Debug)]
pub struct UpdateError {
    pub kind: UpdateErrorKind,
    pub message: String,
}

impl UpdateError {
    fn new(kind: UpdateErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }
}

pub fn read_cached_release(tag: Option<&str>, url: Option<&str>) -> Option<Release> {
    let tag = tag?;
    let url = url?;
    ReleaseVersion::parse(tag)?;
    if !is_release_page(url) {
        return None;
    }
    Some(Release {
        tag: tag.to_string(),
        url: url.to_string(),
        draft: false,
        prerelease: tag.to_ascii_lowercase().contains("-preview."),
    })
}

fn split_https_url(value: &str) -> Option<(&str, &str)> {
    let rest = value.strip_prefix("https://")?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let host = authority.split(':').next().unwrap_or(authority);
    if !host.eq_ignore_ascii_case(RELEASES_PAGE_HOST) {
        return None;
    }
    let path = &rest[authority_end..];
    let path_end = path.find(['?', '#']).unwrap_or(path.len());
    Some((host, &path[..path_end]))
}

pub fn is_release_page(value: &str) -> bool {
    let Some((_, path)) = split_https_url(value) else {
        return false;
    };
    path.get(..RELEASE_TAG_PATH_PREFIX.len())
        .map(|prefix| prefix.eq_ignore_ascii_case(RELEASE_TAG_PATH_PREFIX))
        .unwrap_or(false)
}

#[allow(dead_code)]
pub fn is_repository_info_page(value: &str) -> bool {
    let Some((_, path)) = split_https_url(value) else {
        return false;
    };
    let path = path.trim_end_matches('/');
    path.eq_ignore_ascii_case(&format!("/{REPOSITORY}"))
        || path.eq_ignore_ascii_case(&format!("/{REPOSITORY}/releases"))
}

fn is_eligible(release: Option<&Release>, include_prerelease: bool) -> bool {
    release
        .map(|release| {
            !release.draft
                && (include_prerelease || !release.prerelease)
                && ReleaseVersion::parse(&release.tag).is_some()
                && is_release_page(&release.url)
        })
        .unwrap_or(false)
}

fn is_entity_tag(value: &str) -> bool {
    let value = value.trim();
    let value = value.strip_prefix("W/").unwrap_or(value);
    value.len() >= 2 && value.starts_with('"') && value.ends_with('"')
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// ---------------------------------------------------------------------------
// WinHTTP client
// ---------------------------------------------------------------------------

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { wh::WinHttpCloseHandle(self.0) };
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn http_error(kind: UpdateErrorKind, action: &str) -> UpdateError {
    let code = unsafe { GetLastError() };
    let kind = match code {
        wh::ERROR_WINHTTP_TIMEOUT => UpdateErrorKind::Timeout,
        wh::ERROR_WINHTTP_NAME_NOT_RESOLVED
        | wh::ERROR_WINHTTP_CANNOT_CONNECT
        | wh::ERROR_WINHTTP_CONNECTION_ERROR
        | wh::ERROR_WINHTTP_SECURE_FAILURE => UpdateErrorKind::Network,
        _ => kind,
    };
    UpdateError::new(kind, format!("{action} failed (WinHTTP error {code})"))
}

struct Response {
    status: u32,
    etag: Option<String>,
    body: Vec<u8>,
    final_url: Option<String>,
}

impl Response {
    fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    fn is_rate_limited(&self) -> bool {
        self.status == 403 || self.status == 429
    }
}

fn open_session() -> Result<Handle, UpdateError> {
    let agent = wide(&format!("MusicPlayer/{}", current_version()));
    let mut session = unsafe {
        wh::WinHttpOpen(
            agent.as_ptr(),
            wh::WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    };
    if session.is_null() {
        session = unsafe {
            wh::WinHttpOpen(
                agent.as_ptr(),
                wh::WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        };
    }
    if session.is_null() {
        return Err(http_error(UpdateErrorKind::Network, "Opening WinHTTP"));
    }
    let session = Handle(session);
    let timeout = REQUEST_TIMEOUT.as_millis() as i32;
    unsafe {
        wh::WinHttpSetTimeouts(session.0, timeout, timeout, timeout, timeout);
    }
    Ok(session)
}

fn query_u32(handle: *mut c_void, info: u32) -> Option<u32> {
    let mut value = 0u32;
    let mut length = std::mem::size_of::<u32>() as u32;
    let ok = unsafe {
        wh::WinHttpQueryHeaders(
            handle,
            info,
            std::ptr::null(),
            &mut value as *mut u32 as *mut c_void,
            &mut length,
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(value)
}

fn query_string(handle: *mut c_void, info: u32) -> Option<String> {
    let mut length = 0u32;
    unsafe {
        wh::WinHttpQueryHeaders(
            handle,
            info,
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut length,
            std::ptr::null_mut(),
        );
    }
    if length == 0 || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
        return None;
    }
    let mut buffer = vec![0u8; length as usize];
    let ok = unsafe {
        wh::WinHttpQueryHeaders(
            handle,
            info,
            std::ptr::null(),
            buffer.as_mut_ptr() as *mut c_void,
            &mut length,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return None;
    }
    let units: Vec<u16> = buffer
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    let text = String::from_utf16_lossy(&units);
    Some(text.trim().to_string())
}

fn read_body(request: *mut c_void) -> Result<Vec<u8>, UpdateError> {
    let mut body = Vec::new();
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        if Instant::now() > deadline {
            return Err(UpdateError::new(UpdateErrorKind::Timeout, "Reading GitHub response timed out"));
        }
        let mut available = 0u32;
        if unsafe { wh::WinHttpQueryDataAvailable(request, &mut available) } == 0 {
            return Err(http_error(UpdateErrorKind::Network, "Reading GitHub response"));
        }
        if available == 0 {
            break;
        }
        let mut chunk = vec![0u8; available as usize];
        let mut read = 0u32;
        if unsafe {
            wh::WinHttpReadData(
                request,
                chunk.as_mut_ptr() as *mut c_void,
                available,
                &mut read,
            )
        } == 0
        {
            return Err(http_error(UpdateErrorKind::Network, "Reading GitHub response"));
        }
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read as usize]);
    }
    Ok(body)
}

fn get(
    session: &Handle,
    host: &str,
    path: &str,
    headers: &str,
    follow_redirects: bool,
) -> Result<Response, UpdateError> {
    let host = wide(host);
    let connect = Handle(unsafe {
        wh::WinHttpConnect(session.0, host.as_ptr(), 443, 0)
    });
    if connect.0.is_null() {
        return Err(http_error(UpdateErrorKind::Network, "Connecting to GitHub"));
    }

    let verb = wide("GET");
    let target = wide(path);
    let request = Handle(unsafe {
        wh::WinHttpOpenRequest(
            connect.0,
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            wh::WINHTTP_FLAG_SECURE,
        )
    });
    if request.0.is_null() {
        return Err(http_error(UpdateErrorKind::Network, "Opening GitHub request"));
    }

    if follow_redirects {
        let policy = wh::WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS;
        unsafe {
            wh::WinHttpSetOption(
                request.0,
                wh::WINHTTP_OPTION_REDIRECT_POLICY,
                &policy as *const u32 as *const c_void,
                std::mem::size_of::<u32>() as u32,
            );
        }
    }

    let header_text = wide(headers);
    let send = unsafe {
        wh::WinHttpSendRequest(
            request.0,
            header_text.as_ptr(),
            header_text.len().saturating_sub(1) as u32,
            std::ptr::null(),
            0,
            0,
            0,
        )
    };
    if send == 0 {
        return Err(http_error(UpdateErrorKind::Network, "Sending GitHub request"));
    }
    if unsafe { wh::WinHttpReceiveResponse(request.0, std::ptr::null_mut()) } == 0 {
        return Err(http_error(UpdateErrorKind::Network, "Waiting for GitHub"));
    }

    let status = query_u32(request.0, wh::WINHTTP_QUERY_STATUS_CODE | wh::WINHTTP_QUERY_FLAG_NUMBER)
        .unwrap_or(0);
    let etag = query_string(request.0, wh::WINHTTP_QUERY_ETAG).filter(|value| is_entity_tag(value));
    let final_url = follow_redirects.then(|| query_string(request.0, wh::WINHTTP_OPTION_URL)).flatten();
    let body = read_body(request.0)?;
    Ok(Response { status, etag, body, final_url })
}

fn parse_releases(body: &[u8]) -> Result<Vec<Release>, UpdateError> {
    let values: Vec<Value> = serde_json::from_slice(body).map_err(|error| {
        UpdateError::new(
            UpdateErrorKind::InvalidData,
            format!("GitHub returned invalid release data: {error}"),
        )
    })?;
    Ok(values
        .iter()
        .map(|value| Release {
            tag: value.get("tag_name").and_then(Value::as_str).unwrap_or_default().to_string(),
            url: value.get("html_url").and_then(Value::as_str).unwrap_or_default().to_string(),
            draft: value.get("draft").and_then(Value::as_bool).unwrap_or(false),
            prerelease: value.get("prerelease").and_then(Value::as_bool).unwrap_or(false),
        })
        .collect())
}

fn select_latest(releases: &[Release], include_prerelease: bool) -> Option<Release> {
    let mut best: Option<(ReleaseVersion, &Release)> = None;
    for release in releases {
        if release.draft || (!include_prerelease && release.prerelease) {
            continue;
        }
        if !is_release_page(&release.url) {
            continue;
        }
        let Some(version) = ReleaseVersion::parse(&release.tag) else {
            continue;
        };
        if best.map(|(current, _)| version > current).unwrap_or(true) {
            best = Some((version, release));
        }
    }
    best.map(|(_, release)| release.clone())
}

fn stable_release_from_redirect(session: &Handle) -> Result<Option<Release>, UpdateError> {
    let response = get(session, RELEASES_PAGE_HOST, LATEST_STABLE_PATH, "", true)?;
    if !response.is_success() {
        return Err(UpdateError::new(
            UpdateErrorKind::Network,
            format!("GitHub returned HTTP {} for the stable release page", response.status),
        ));
    }
    let Some(final_url) = response.final_url else {
        return Ok(None);
    };
    let Some((_, path)) = split_https_url(&final_url) else {
        return Ok(None);
    };
    if !path
        .get(..RELEASE_TAG_PATH_PREFIX.len())
        .map(|prefix| prefix.eq_ignore_ascii_case(RELEASE_TAG_PATH_PREFIX))
        .unwrap_or(false)
    {
        return Ok(None);
    }
    let tag = percent_decode(&path[RELEASE_TAG_PATH_PREFIX.len()..]);
    let Some(version) = ReleaseVersion::parse(&tag) else {
        return Ok(None);
    };
    if version.is_prerelease() {
        return Ok(None);
    }
    let url = format!("{REPOSITORY_URL}/releases/tag/{tag}");
    if !is_release_page(&url) {
        return Ok(None);
    }
    Ok(Some(Release { tag, url, draft: false, prerelease: false }))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let digits = &value[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(digits, 16) {
                output.push(byte);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

/// Blocking port of `GitHubUpdates.GetLatestAsync`. Call from a worker thread.
pub fn get_latest(
    include_prerelease: bool,
    etag: Option<&str>,
    cached_release: Option<&Release>,
    channel_is_current: bool,
) -> Result<UpdateCheck, UpdateError> {
    let session = open_session()?;
    let cached_eligible = channel_is_current && is_eligible(cached_release, include_prerelease);
    let conditional = cached_eligible
        && etag
            .map(|value| is_entity_tag(value))
            .unwrap_or(false);

    let mut headers = String::from("Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n");
    if conditional {
        headers.push_str("If-None-Match: ");
        headers.push_str(etag.unwrap_or_default().trim());
        headers.push_str("\r\n");
    }

    let response = get(&session, RELEASES_API_HOST, RELEASES_API_PATH, &headers, false)?;
    if response.status == 304 {
        return Ok(UpdateCheck {
            release: if cached_eligible { cached_release.cloned() } else { None },
            etag: response.etag.or_else(|| etag.map(str::to_string)),
        });
    }

    // GitHub occasionally rate-limits unauthenticated clients. The stable
    // `releases/latest` page is a narrow fallback: the final redirect must
    // identify this repository and a tagged stable release.
    if response.is_rate_limited() && !include_prerelease {
        return Ok(UpdateCheck {
            release: stable_release_from_redirect(&session)?,
            etag: None,
        });
    }

    if !response.is_success() {
        let kind = if response.is_rate_limited() {
            UpdateErrorKind::RateLimited
        } else {
            UpdateErrorKind::Network
        };
        return Err(UpdateError::new(
            kind,
            format!("GitHub returned HTTP {} for the release listing", response.status),
        ));
    }

    let releases = parse_releases(&response.body)?;
    Ok(UpdateCheck {
        release: select_latest(&releases, include_prerelease),
        etag: response.etag,
    })
}

/// Test hook for the exact release-comparison behavior of the C# port.
#[allow(dead_code)]
pub fn compare_versions(current: &str, latest: Option<&str>) -> UpdateComparison {
    compare(current, latest)
}
