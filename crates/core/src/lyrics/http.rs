//! Minimal blocking HTTPS GET built on WinHTTP.
//!
//! The original app uses `HttpClient` for LRCLIB. The shipping binary already
//! links WinHTTP for update checks, so lyrics searches use the same stack
//! instead of spawning PowerShell (which can be blocked by execution policy or
//! endpoint security and masked the real network error).

use std::ffi::c_void;
use std::time::Duration;
use windows_sys::Win32::Networking::WinHttp as wh;

pub struct HttpResponse {
    pub status: u32,
    pub body: Vec<u8>,
}

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                wh::WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn split_https_url(url: &str) -> Result<(&str, &str), String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| "Only HTTPS URLs are supported.".to_string())?;
    let split = rest
        .find('/')
        .ok_or_else(|| "The request URL is missing a path.".to_string())?;
    let host = &rest[..split];
    let path = &rest[split..];
    if host.is_empty() {
        return Err("The request URL is missing a host.".to_string());
    }
    Ok((host, path))
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

fn read_body(request: *mut c_void, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::time::Instant::now() > deadline {
            return Err("The lyrics request timed out.".to_string());
        }
        let mut available = 0u32;
        if unsafe { wh::WinHttpQueryDataAvailable(request, &mut available) } == 0 {
            return Err("Could not read the lyrics response.".to_string());
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
            return Err("Could not read the lyrics response.".to_string());
        }
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read as usize]);
    }
    Ok(body)
}

/// Performs a blocking HTTPS GET using WinHTTP.
pub fn get(url: &str, user_agent: &str, timeout: Duration) -> Result<HttpResponse, String> {
    get_once(url, user_agent, timeout)
}

fn get_once(url: &str, user_agent: &str, timeout: Duration) -> Result<HttpResponse, String> {
    let (host, path) = split_https_url(url)?;
    let agent = wide(user_agent);
    let mut raw_session = unsafe {
        wh::WinHttpOpen(
            agent.as_ptr(),
            wh::WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    };
    if raw_session.is_null() {
        raw_session = unsafe {
            wh::WinHttpOpen(
                agent.as_ptr(),
                wh::WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        };
    }
    if raw_session.is_null() {
        return Err("Could not open an HTTPS session.".to_string());
    }
    let session = Handle(raw_session);
    let millis = timeout.as_millis().min(i32::MAX as u128) as i32;
    unsafe {
        wh::WinHttpSetTimeouts(session.0, millis, millis, millis, millis);
    }

    let host = wide(host);
    let connect = Handle(unsafe { wh::WinHttpConnect(session.0, host.as_ptr(), 443, 0) });
    if connect.0.is_null() {
        return Err("Could not connect to the lyrics service.".to_string());
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
        return Err("Could not create the lyrics request.".to_string());
    }

    let policy = wh::WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS;
    unsafe {
        wh::WinHttpSetOption(
            request.0,
            wh::WINHTTP_OPTION_REDIRECT_POLICY,
            &policy as *const u32 as *const c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }

    let headers = wide("Accept: application/json\r\n");
    if unsafe {
        wh::WinHttpSendRequest(
            request.0,
            headers.as_ptr(),
            headers.len().saturating_sub(1) as u32,
            std::ptr::null(),
            0,
            0,
            0,
        )
    } == 0
    {
        return Err("Could not send the lyrics request.".to_string());
    }
    if unsafe { wh::WinHttpReceiveResponse(request.0, std::ptr::null_mut()) } == 0 {
        return Err("The lyrics service did not respond.".to_string());
    }

    let status = query_u32(
        request.0,
        wh::WINHTTP_QUERY_STATUS_CODE | wh::WINHTTP_QUERY_FLAG_NUMBER,
    )
    .unwrap_or(0);
    let body = read_body(request.0, timeout)?;
    Ok(HttpResponse { status, body })
}
