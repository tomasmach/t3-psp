use alloc::{
    ffi::CString,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::sync::atomic::{AtomicU32, Ordering};
use psp::sys::*;

static INITIALIZATION: AtomicU32 = AtomicU32::new(0);

// Keep rust-psp's HTTP import/NID reachable under LTO. Its public wrapper uses
// O32 for the fourth u64 argument, but the firmware expects EABI a4/a5.
#[used]
static HTTP_REQUEST_IMPORT: unsafe extern "C" fn(i32, HttpMethod, *mut u8, u64) -> i32 =
    sceHttpCreateRequestWithURL;

unsafe extern "C" {
    fn __sceHttpCreateRequestWithURL_stub();
}

// Requests are capped at 1 MiB, so a 32-bit length suffices at this boundary.
// Skip the broken wrapper and pass the aligned u64 directly to the import.
#[unsafe(naked)]
unsafe extern "C" fn create_http_request(
    connection: i32,
    method: HttpMethod,
    url: *mut u8,
    length: u32,
) -> i32 {
    core::arch::naked_asm!(
        ".set noreorder",
        "move $t0, $a3",
        "move $t1, $zero",
        "j {stub}",
        "nop",
        ".set reorder",
        stub = sym __sceHttpCreateRequestWithURL_stub,
    );
}

#[derive(Clone)]
pub struct Config {
    pub base_url: String,
    pub token: String,
    pub profile: i32,
}

pub struct Profile {
    pub id: i32,
    pub name: String,
    pub ssid: String,
}

fn profile_text(id: i32, parameter: NetParam) -> String {
    let mut data = UtilityNetData {
        as_string: [0; 128],
    };
    unsafe {
        if sceUtilityGetNetParam(id, parameter, &mut data) < 0 {
            return String::from("?");
        }
        let bytes = data.as_string;
        let length = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..length]).into_owned()
    }
}

/// Enumerate saved profile IDs without modifying PSP network settings.
pub fn profiles() -> Vec<Profile> {
    (1..=99)
        .filter(|id| unsafe { sceUtilityCheckNetParam(*id) >= 0 })
        .map(|id| Profile {
            id,
            name: profile_text(id, NetParam::Name),
            ssid: profile_text(id, NetParam::Ssid),
        })
        .collect()
}

fn checked(code: i32, operation: &str) -> Result<i32, String> {
    if code < 0 {
        Err(format!("{}: 0x{:08x}", operation, code as u32))
    } else {
        Ok(code)
    }
}

fn cstring(value: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| "Invalid zero byte in configuration".to_string())
}

pub fn load_config() -> Result<Config, String> {
    let path = cstring("ms0:/PSP/GAME/T3PSP/gateway.cfg")?;
    let fd = unsafe { sceIoOpen(path.as_ptr().cast(), IoOpenFlags::RD_ONLY, 0) };
    checked(fd.0, "Open gateway.cfg")?;
    let mut bytes = [0u8; 4097];
    let mut length = 0;
    let result = (|| {
        while length < bytes.len() {
            let count = checked(
                unsafe {
                    sceIoRead(
                        fd,
                        bytes[length..].as_mut_ptr().cast(),
                        (bytes.len() - length) as u32,
                    )
                },
                "Read gateway.cfg",
            )? as usize;
            if count == 0 {
                break;
            }
            length += count;
        }
        if length > 4096 {
            return Err("gateway.cfg exceeds 4 KiB".to_string());
        }
        let text = core::str::from_utf8(&bytes[..length])
            .map_err(|_| "gateway.cfg is not UTF-8".to_string())?;
        let mut config = Config {
            base_url: String::new(),
            token: String::new(),
            profile: 1,
        };
        for line in text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| "Invalid gateway.cfg line".to_string())?;
            match key.trim() {
                "url" => config.base_url = value.trim().trim_end_matches('/').to_string(),
                "token" => config.token = value.trim().to_string(),
                "profile" => {
                    config.profile = value
                        .trim()
                        .parse()
                        .map_err(|_| "Invalid Wi-Fi profile number".to_string())?
                }
                _ => return Err("Unknown gateway.cfg option".to_string()),
            }
        }
        validate(&config)?;
        Ok(config)
    })();
    unsafe {
        sceIoClose(fd);
    }
    result
}

fn validate(config: &Config) -> Result<(), String> {
    if !config.base_url.starts_with("http://")
        || config.base_url.len() <= 7
        || config.base_url.bytes().any(|b| b <= 32 || b == 127)
        || config.base_url.contains(['@', '?', '#'])
    {
        return Err("url must be a plain HTTP gateway address".to_string());
    }
    if config.token.is_empty() || config.token.bytes().any(|b| b <= 32 || b >= 127) {
        return Err("Missing or invalid gateway token".to_string());
    }
    if config.profile < 1 {
        return Err("Wi-Fi profile must be at least 1".to_string());
    }
    Ok(())
}

/// Initialize once; a failed attempt is cleaned up and may be retried.
pub fn init(config: &Config) -> Result<(), String> {
    validate(config)?;
    match INITIALIZATION.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => {}
        Err(2) => return Ok(()),
        Err(_) => return Err("Network initialization is already running".to_string()),
    }
    let mut modules = Vec::new();
    let mut stage = 0;
    let result = (|| {
        unsafe {
            for module in [
                NetModule::NetCommon,
                NetModule::NetInet,
                NetModule::NetParseUri,
                // psp 0.3.13 omits ParseHttp: its NetHttp=5 loads ParseHttp,
                // and NetSsl=6 loads HTTP (PSPSDK assigns SSL the value 7).
                NetModule::NetHttp,
                NetModule::NetSsl,
            ] {
                checked(sceUtilityLoadNetModule(module), "Load network module")?;
                modules.push(module);
            }
            checked(
                sceNetInit(256 * 1024, 42, 4096, 42, 4096),
                "Initialize network",
            )?;
            stage = 1;
            checked(sceNetInetInit(), "Initialize sockets")?;
            stage = 2;
            checked(sceNetApctlInit(0x8000, 48), "Initialize Wi-Fi")?;
            stage = 3;
            checked(sceNetResolverInit(), "Initialize DNS")?;
            stage = 4;
            checked(sceHttpInit(256 * 1024), "Initialize HTTP")?;
        }
        Ok(())
    })();
    if result.is_err() {
        unsafe {
            if stage >= 4 {
                sceNetResolverTerm();
            }
            if stage >= 3 {
                sceNetApctlTerm();
            }
            if stage >= 2 {
                sceNetInetTerm();
            }
            if stage >= 1 {
                sceNetTerm();
            }
            for module in modules.into_iter().rev() {
                sceUtilityUnloadNetModule(module);
            }
        }
    }
    INITIALIZATION.store(if result.is_ok() { 2 } else { 0 }, Ordering::Release);
    result
}

/// Read association state without changing a live connection.
pub fn disconnected() -> Result<bool, String> {
    let mut state = ApctlState::Disconnected;
    checked(
        unsafe { sceNetApctlGetState(&mut state) },
        "Read Wi-Fi state",
    )?;
    Ok(matches!(state, ApctlState::Disconnected))
}

/// Connect to an existing XMB Wi-Fi profile, with a 25-second deadline.
pub fn connect(config: &Config) -> Result<(), String> {
    unsafe {
        if sceUtilityCheckNetParam(config.profile) < 0 {
            return Err(format!(
                "Wi-Fi profile {} is not saved. Exit to Settings > Network Settings, create an Infrastructure connection, then retry.",
                config.profile
            ));
        }
        let mut state = ApctlState::Disconnected;
        checked(sceNetApctlGetState(&mut state), "Read Wi-Fi state")?;
        if !matches!(state, ApctlState::Disconnected) {
            checked(sceNetApctlDisconnect(), "Disconnect Wi-Fi")?;
            let started = sceKernelGetSystemTimeWide();
            loop {
                checked(sceNetApctlGetState(&mut state), "Read Wi-Fi state")?;
                if matches!(state, ApctlState::Disconnected) {
                    break;
                }
                if sceKernelGetSystemTimeWide() - started > 3_000_000 {
                    return Err("Wi-Fi disconnect timed out".to_string());
                }
                sceKernelDelayThread(100_000);
            }
        }
        checked(sceNetApctlConnect(config.profile), "Connect Wi-Fi profile")?;
        let started = sceKernelGetSystemTimeWide();
        loop {
            let code = sceNetApctlGetState(&mut state);
            if code < 0 {
                sceNetApctlDisconnect();
                return checked(code, "Read Wi-Fi state").map(|_| ());
            }
            if matches!(state, ApctlState::GotIp) {
                return Ok(());
            }
            if sceKernelGetSystemTimeWide() - started > 25_000_000 {
                sceNetApctlDisconnect();
                return Err(format!(
                    "Wi-Fi profile {} timed out at {:?}. Retry and select the connection tested in PSP Settings.",
                    config.profile, state
                ));
            }
            sceKernelDelayThread(100_000);
        }
    }
}

struct HttpHandles {
    template: i32,
    connection: i32,
    request: i32,
}
impl Drop for HttpHandles {
    fn drop(&mut self) {
        unsafe {
            if self.request >= 0 {
                sceHttpAbortRequest(self.request);
                sceHttpDeleteRequest(self.request);
            }
            if self.connection >= 0 {
                sceHttpDeleteConnection(self.connection);
            }
            if self.template >= 0 {
                sceHttpDeleteTemplate(self.template);
            }
        }
    }
}

pub fn request(config: &Config, path: &str, body: &[u8], post: bool) -> Result<String, String> {
    validate(config)?;
    if !path.starts_with('/') || path.bytes().any(|b| b <= 32 || b == 127) {
        return Err("Invalid gateway request path".to_string());
    }
    if body.len() > 1024 * 1024 {
        return Err("Request exceeds 1 MiB".to_string());
    }
    let url = cstring(&format!(
        "{}{}",
        config.base_url.trim_end_matches('/'),
        path
    ))?;
    let agent = cstring("T3PSP/0.1")?;
    let auth_name = cstring("Authorization")?;
    let auth = cstring(&format!("Bearer {}", config.token))?;
    let content_name = cstring("Content-Type")?;
    let transcription = path
        .split('?')
        .next()
        .unwrap_or(path)
        .ends_with("/transcriptions");
    let receive_timeout = if transcription {
        120_000_000
    } else {
        5_000_000
    };
    let content_type = cstring(if transcription {
        "application/octet-stream"
    } else {
        "text/plain;charset=utf-8"
    })?;
    let mut handles = HttpHandles {
        template: -1,
        connection: -1,
        request: -1,
    };
    unsafe {
        handles.template = checked(
            sceHttpCreateTemplate(agent.as_ptr() as *mut u8, 1, 0),
            "Create HTTP template",
        )?;
        let template = handles.template;
        checked(
            sceHttpSetResolveTimeOut(template, 5_000_000),
            "Set DNS timeout",
        )?;
        checked(sceHttpSetResolveRetry(template, 1), "Set DNS retries")?;
        checked(
            sceHttpSetConnectTimeOut(template, 5_000_000),
            "Set connection timeout",
        )?;
        checked(
            sceHttpSetSendTimeOut(template, 5_000_000),
            "Set send timeout",
        )?;
        checked(
            sceHttpSetRecvTimeOut(template, receive_timeout),
            "Set receive timeout",
        )?;
        checked(sceHttpDisableRedirect(template), "Disable redirects")?;
        handles.connection = checked(
            sceHttpCreateConnectionWithURL(template, url.as_ptr().cast(), 0),
            "Create HTTP connection",
        )?;
        handles.request = checked(
            create_http_request(
                handles.connection,
                if post {
                    HttpMethod::Post
                } else {
                    HttpMethod::Get
                },
                url.as_ptr() as *mut u8,
                if post { body.len() as u32 } else { 0 },
            ),
            "Create HTTP request",
        )?;
        let request = handles.request;
        checked(
            sceHttpAddExtraHeader(
                request,
                auth_name.as_ptr() as *mut u8,
                auth.as_ptr() as *mut u8,
                0,
            ),
            "Set authorization",
        )?;
        if post {
            checked(
                sceHttpAddExtraHeader(
                    request,
                    content_name.as_ptr() as *mut u8,
                    content_type.as_ptr() as *mut u8,
                    0,
                ),
                "Set content type",
            )?;
        }
        checked(
            sceHttpSendRequest(
                request,
                if post {
                    body.as_ptr() as *mut _
                } else {
                    core::ptr::null_mut()
                },
                if post { body.len() as u32 } else { 0 },
            ),
            "Send HTTP request",
        )?;
        let mut status = 0;
        checked(
            sceHttpGetStatusCode(request, &mut status),
            "Read HTTP status",
        )?;
        if status == 0 {
            return Err("The gateway returned an invalid HTTP response.".to_string());
        }
        let started = sceKernelGetSystemTimeWide();
        let mut response = Vec::new();
        let mut buffer = [0u8; 2048];
        loop {
            if sceKernelGetSystemTimeWide() - started
                > if transcription {
                    120_000_000
                } else {
                    15_000_000
                }
            {
                return Err("Gateway response timed out".to_string());
            }
            let count = checked(
                sceHttpReadData(request, buffer.as_mut_ptr().cast(), buffer.len() as u32),
                "Read HTTP response",
            )? as usize;
            if count == 0 {
                break;
            }
            if response.len() + count > 65536 {
                return Err("Gateway response exceeds 64 KiB".to_string());
            }
            response.extend_from_slice(&buffer[..count]);
        }
        let response =
            String::from_utf8(response).map_err(|_| "Gateway response is not UTF-8".to_string())?;
        if !(200..300).contains(&status) {
            let message = response
                .lines()
                .find_map(|line| line.strip_prefix("ERROR\t"));
            return Err(match message {
                Some(message) => format!(
                    "Gateway HTTP {status}: {}",
                    crate::model::decode_field(message)
                ),
                None => format!("Gateway HTTP {status}"),
            });
        }
        Ok(response)
    }
}
