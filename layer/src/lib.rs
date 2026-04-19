//! rigor-layer: LD_PRELOAD shared library for intercepting LLM API connections.
//!
//! Uses frida-gum (same as mirrord) for inline function hooking:
//! - Hooks `getaddrinfo` — redirects DNS resolution for LLM API hosts to 127.0.0.1
//! - Hooks `freeaddrinfo` — properly frees our custom addrinfo structs
//! - Hooks `gethostbyname` — handles deprecated DNS resolution path
//! - Hooks `connect` / `connectx` (macOS) — port redirection
//! - Hooks `SecTrustEvaluateWithError` (macOS) — universal TLS bypass
//! - Hooks `dns_configuration_copy` (macOS) — prevents system DNS bypass
//!
//! Architecture follows mirrord pattern: intercept at DNS level, not socket level.
//! When the process resolves api.anthropic.com, we return 127.0.0.1:DAEMON_PORT.
//! The process then connects to our daemon regardless of what connect mechanism it uses.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::net::Ipv4Addr;
use std::ptr::null_mut;
use std::sync::{Mutex, OnceLock};

use frida_gum::{interceptor::Interceptor, Gum, Module, NativePointer};
use libc::{addrinfo, c_char, c_int, sockaddr_in, AF_INET, SOCK_STREAM};
use once_cell::sync::Lazy;

// =============================================================================
// Re-entrancy protection (mirrord DetourGuard pattern)
// =============================================================================

thread_local! {
    static DETOUR_BYPASS: Cell<bool> = const { Cell::new(false) };
}

/// Guard that prevents re-entrant hook calls on the same thread.
/// When a hooked function internally calls another hooked function
/// (e.g., logging triggers DNS lookup), the guard ensures we call
/// the original libc function instead of recursing.
struct DetourGuard;

impl DetourGuard {
    /// Returns `Some(guard)` if this is the outermost hook call.
    /// Returns `None` if already inside a hook (re-entrant call).
    fn new() -> Option<Self> {
        DETOUR_BYPASS.with(|b| {
            if b.get() {
                None
            } else {
                b.set(true);
                Some(DetourGuard)
            }
        })
    }
}

impl Drop for DetourGuard {
    fn drop(&mut self) {
        DETOUR_BYPASS.with(|b| b.set(false));
    }
}

// =============================================================================
// Configuration
// =============================================================================

/// Port 443 — we listen on 443 because getaddrinfo returns 127.0.0.1
/// and the app connects to 127.0.0.1:443 (original HTTPS port).
/// Bun bypasses connect() hooks so we can't redirect to a different port.
static DAEMON_PORT: Lazy<u16> = Lazy::new(|| {
    std::env::var("RIGOR_DAEMON_TLS_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(443)
});

static DEBUG: Lazy<bool> = Lazy::new(|| std::env::var("RIGOR_LAYER_DEBUG").is_ok());

/// When true, the connect() hook redirects ALL outbound port 443 connections
/// (not just 127.0.0.1:443) to the rigor daemon. This is the mirrord-style
/// transparent interception mode that catches Bun/Go HTTP clients which
/// bypass our DNS hook with their own resolvers.
static TRANSPARENT: Lazy<bool> = Lazy::new(|| std::env::var("RIGOR_TRANSPARENT").is_ok());

fn debug_log(msg: &str) {
    if *DEBUG {
        eprintln!("rigor-layer: {}", msg);
    }
}

/// Hosts to intercept at DNS level. ALL port 443 connections to these hosts
/// get redirected to the rigor daemon.
static INTERCEPT_HOSTS: Lazy<HashSet<String>> = Lazy::new(|| {
    let default_hosts = vec![
        // Anthropic
        "api.anthropic.com",
        // OpenAI
        "api.openai.com",
        // Google Vertex AI (all regions)
        "us-east5-aiplatform.googleapis.com",
        "us-central1-aiplatform.googleapis.com",
        "us-west1-aiplatform.googleapis.com",
        "europe-west1-aiplatform.googleapis.com",
        "europe-west4-aiplatform.googleapis.com",
        "asia-southeast1-aiplatform.googleapis.com",
        "aiplatform.googleapis.com",
        // Azure OpenAI (common endpoints)
        "openai.azure.com",
        // Ollama
        "localhost",
    ];

    let mut hosts: HashSet<String> = default_hosts.iter().map(|s| s.to_string()).collect();

    // Add custom hosts from env
    if let Ok(extra) = std::env::var("RIGOR_INTERCEPT_HOSTS") {
        for h in extra.split(',') {
            let h = h.trim();
            if !h.is_empty() {
                hosts.insert(h.to_string());
            }
        }
    }

    // Remove localhost if not explicitly requested (avoid breaking local services)
    hosts.remove("localhost");

    hosts
});

// =============================================================================
// Original function pointers (set once by frida-gum, read lock-free via OnceLock)
// =============================================================================

type GetaddrinfoFn = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    *const addrinfo,
    *mut *mut addrinfo,
) -> c_int;

type FreeaddrinfoFn = unsafe extern "C" fn(*mut addrinfo);
type ConnectFn = unsafe extern "C" fn(libc::c_int, *const libc::sockaddr, libc::socklen_t) -> libc::c_int;
type GethostbynameFn = unsafe extern "C" fn(*const c_char) -> *mut libc::hostent;

#[cfg(target_os = "macos")]
type ConnectxFn = unsafe extern "C" fn(
    libc::c_int, *const libc::sa_endpoints_t, libc::c_uint, libc::c_uint,
    *const libc::iovec, libc::c_uint, *mut usize, *mut libc::c_uint,
) -> libc::c_int;

#[cfg(target_os = "macos")]
type DnsConfigurationCopyFn = unsafe extern "C" fn() -> *mut std::ffi::c_void;
#[cfg(target_os = "macos")]
type DnsConfigurationFreeFn = unsafe extern "C" fn(*mut std::ffi::c_void);

// Lock-free OnceLock — set once during install_hooks(), read without locking in detours
static ORIGINAL_GETADDRINFO: OnceLock<GetaddrinfoFn> = OnceLock::new();
static ORIGINAL_FREEADDRINFO: OnceLock<FreeaddrinfoFn> = OnceLock::new();
static ORIGINAL_CONNECT: OnceLock<ConnectFn> = OnceLock::new();
static ORIGINAL_GETHOSTBYNAME: OnceLock<GethostbynameFn> = OnceLock::new();

#[cfg(target_os = "macos")]
static ORIGINAL_CONNECTX: OnceLock<ConnectxFn> = OnceLock::new();
#[cfg(target_os = "macos")]
static ORIGINAL_DNS_CONFIGURATION_COPY: OnceLock<DnsConfigurationCopyFn> = OnceLock::new();
#[cfg(target_os = "macos")]
static ORIGINAL_DNS_CONFIGURATION_FREE: OnceLock<DnsConfigurationFreeFn> = OnceLock::new();

type GetpeernameFn = unsafe extern "C" fn(libc::c_int, *mut libc::sockaddr, *mut libc::socklen_t) -> libc::c_int;
type GetsocknameFn = unsafe extern "C" fn(libc::c_int, *mut libc::sockaddr, *mut libc::socklen_t) -> libc::c_int;

static ORIGINAL_GETPEERNAME: OnceLock<GetpeernameFn> = OnceLock::new();
static ORIGINAL_GETSOCKNAME: OnceLock<GetsocknameFn> = OnceLock::new();

/// Track which addrinfo pointers we allocated (so freeaddrinfo knows to free ours)
static MANAGED_ADDRINFO: Lazy<Mutex<HashSet<usize>>> = Lazy::new(|| Mutex::new(HashSet::new()));

// =============================================================================
// Socket state tracking — remember which fd connects to which original address
// =============================================================================

// Track the last hostname resolved via getaddrinfo on this thread.
thread_local! {
    static LAST_RESOLVED_HOST: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Map fd → original sockaddr_in for intercepted connections.
/// When connect() redirects to 127.0.0.1:DAEMON_PORT, we store the ORIGINAL
/// destination here so getpeername() can return it instead of the redirected address.
/// This is the mirrord pattern — apps verify peer address matches intended server.
static INTERCEPTED_SOCKETS: Lazy<Mutex<HashMap<i32, sockaddr_in>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// =============================================================================
// Thread-local buffer for gethostbyname (returns pointer to static storage)
// =============================================================================

/// gethostbyname returns a pointer to a static struct per POSIX.
/// We use a thread-local to provide per-thread storage.
struct FakeHostent {
    hostent: libc::hostent,
    name: CString,
    addr: [u8; 4],
    addr_ptr: *mut u8,
    addr_list: [*mut c_char; 2],
}

thread_local! {
    static FAKE_HOSTENT: std::cell::RefCell<Option<FakeHostent>> = const { std::cell::RefCell::new(None) };
}

// =============================================================================
// Hook: getaddrinfo — the core interception mechanism (mirrord pattern)
// =============================================================================

unsafe extern "C" fn getaddrinfo_detour(
    node: *const c_char,
    service: *const c_char,
    hints: *const addrinfo,
    res: *mut *mut addrinfo,
) -> c_int {
    // Re-entrancy guard
    let _guard = match DetourGuard::new() {
        Some(g) => g,
        None => {
            return match ORIGINAL_GETADDRINFO.get() {
                Some(func) => func(node, service, hints, res),
                None => libc::getaddrinfo(node, service, hints, res),
            };
        }
    };

    // Get the hostname being resolved
    let hostname = if !node.is_null() {
        CStr::from_ptr(node).to_str().unwrap_or("")
    } else {
        ""
    };

    // Check if this host should be intercepted
    let should_intercept = INTERCEPT_HOSTS.iter().any(|h| {
        hostname == h.as_str() || hostname.ends_with(&format!(".{}", h))
    });

    if should_intercept {
        debug_log(&format!("INTERCEPT getaddrinfo({}) -> 127.0.0.1:{}", hostname, *DAEMON_PORT));

        // Track the hostname for socket state tracking
        LAST_RESOLVED_HOST.with(|cell| {
            *cell.borrow_mut() = Some(hostname.to_string());
        });

        // Build a fake addrinfo pointing to 127.0.0.1:DAEMON_PORT
        let mut addr: sockaddr_in = std::mem::zeroed();
        addr.sin_family = AF_INET as u8;
        addr.sin_port = (*DAEMON_PORT).to_be();
        addr.sin_addr.s_addr = u32::from(Ipv4Addr::LOCALHOST).to_be();
        #[cfg(target_os = "macos")]
        {
            addr.sin_len = std::mem::size_of::<sockaddr_in>() as u8;
        }

        let addr_ptr = Box::into_raw(Box::new(addr)) as *mut libc::sockaddr;
        let canonical = CString::new(hostname).unwrap_or_else(|_| CString::new("").unwrap());

        let info = Box::new(addrinfo {
            ai_flags: 0,
            ai_family: AF_INET,
            ai_socktype: SOCK_STREAM,
            ai_protocol: 6, // IPPROTO_TCP
            ai_addrlen: std::mem::size_of::<sockaddr_in>() as u32,
            ai_addr: addr_ptr,
            ai_canonname: canonical.into_raw(),
            ai_next: null_mut(),
        });

        let info_ptr = Box::into_raw(info);

        // Track this allocation so freeaddrinfo can free it properly
        MANAGED_ADDRINFO
            .lock()
            .expect("lock failed")
            .insert(info_ptr as usize);

        *res = info_ptr;
        return 0; // Success
    }

    // Not intercepted — call original getaddrinfo
    match ORIGINAL_GETADDRINFO.get() {
        Some(func) => func(node, service, hints, res),
        None => libc::getaddrinfo(node, service, hints, res),
    }
}

// =============================================================================
// Hook: freeaddrinfo — free our custom allocations
// =============================================================================

unsafe extern "C" fn freeaddrinfo_detour(info: *mut addrinfo) {
    let _guard = match DetourGuard::new() {
        Some(g) => g,
        None => {
            match ORIGINAL_FREEADDRINFO.get() {
                Some(func) => func(info),
                None => libc::freeaddrinfo(info),
            }
            return;
        }
    };

    let is_managed = MANAGED_ADDRINFO
        .lock()
        .expect("lock failed")
        .remove(&(info as usize));

    if is_managed {
        // Free our custom addrinfo chain
        let mut current = info;
        while !current.is_null() {
            let boxed = Box::from_raw(current);
            let next = boxed.ai_next;

            // Free the sockaddr
            if !boxed.ai_addr.is_null() {
                let _ = Box::from_raw(boxed.ai_addr as *mut sockaddr_in);
            }
            // Free the canonical name
            if !boxed.ai_canonname.is_null() {
                let _ = CString::from_raw(boxed.ai_canonname);
            }

            MANAGED_ADDRINFO
                .lock()
                .expect("lock failed")
                .remove(&(current as usize));

            current = next;
            drop(boxed);
        }
    } else {
        // Not ours — call original freeaddrinfo
        match ORIGINAL_FREEADDRINFO.get() {
            Some(func) => func(info),
            None => libc::freeaddrinfo(info),
        }
    }
}

// =============================================================================
// Hook: gethostbyname — deprecated DNS path (used by Erlang/Elixir, older C)
// =============================================================================

unsafe extern "C" fn gethostbyname_detour(name: *const c_char) -> *mut libc::hostent {
    let _guard = match DetourGuard::new() {
        Some(g) => g,
        None => {
            return match ORIGINAL_GETHOSTBYNAME.get() {
                Some(func) => func(name),
                None => null_mut(),
            };
        }
    };

    let hostname = if !name.is_null() {
        CStr::from_ptr(name).to_str().unwrap_or("")
    } else {
        ""
    };

    let should_intercept = INTERCEPT_HOSTS.iter().any(|h| {
        hostname == h.as_str() || hostname.ends_with(&format!(".{}", h))
    });

    if should_intercept {
        debug_log(&format!("INTERCEPT gethostbyname({}) -> 127.0.0.1", hostname));

        // Track hostname for socket state
        LAST_RESOLVED_HOST.with(|cell| {
            *cell.borrow_mut() = Some(hostname.to_string());
        });

        // Return a thread-local hostent pointing to 127.0.0.1
        return FAKE_HOSTENT.with(|cell| {
            let mut borrow = cell.borrow_mut();
            let fake = borrow.get_or_insert_with(|| {
                let name_cstr = CString::new("localhost").unwrap();
                FakeHostent {
                    hostent: std::mem::zeroed(),
                    name: name_cstr,
                    addr: [127, 0, 0, 1],
                    addr_ptr: null_mut(),
                    addr_list: [null_mut(); 2],
                }
            });

            // Update for this call
            fake.name = CString::new(hostname).unwrap_or_else(|_| CString::new("").unwrap());
            fake.addr = [127, 0, 0, 1];
            fake.addr_ptr = fake.addr.as_mut_ptr();
            fake.addr_list[0] = fake.addr.as_mut_ptr() as *mut c_char;
            fake.addr_list[1] = null_mut();

            fake.hostent.h_name = fake.name.as_ptr() as *mut c_char;
            fake.hostent.h_aliases = null_mut();
            fake.hostent.h_addrtype = AF_INET;
            fake.hostent.h_length = 4;
            fake.hostent.h_addr_list = fake.addr_list.as_mut_ptr();

            &mut fake.hostent as *mut libc::hostent
        });
    }

    match ORIGINAL_GETHOSTBYNAME.get() {
        Some(func) => func(name),
        None => null_mut(),
    }
}

// =============================================================================
// Hook: connect() — redirect 127.0.0.1:443 to 127.0.0.1:DAEMON_PORT
// =============================================================================

unsafe extern "C" fn connect_detour(
    sockfd: libc::c_int,
    addr: *const libc::sockaddr,
    addrlen: libc::socklen_t,
) -> libc::c_int {
    let _guard = match DetourGuard::new() {
        Some(g) => g,
        None => {
            return match ORIGINAL_CONNECT.get() {
                Some(func) => func(sockfd, addr, addrlen),
                None => libc::connect(sockfd, addr, addrlen),
            };
        }
    };

    if !addr.is_null() && (*addr).sa_family as i32 == AF_INET {
        let sin = &*(addr as *const sockaddr_in);
        let port = u16::from_be(sin.sin_port);
        let ip_bytes = sin.sin_addr.s_addr.to_ne_bytes();
        let target_ip = Ipv4Addr::new(ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3]);

        // Decide whether to redirect this connect() to rigor's daemon.
        // - Default: only redirect 127.0.0.1:443 (legacy DNS-redirect path)
        // - Transparent mode: redirect ALL outbound :443 (mirrord pattern)
        let is_localhost_443 = target_ip == Ipv4Addr::LOCALHOST && port == 443 && *DAEMON_PORT != 443;
        let is_transparent_443 = *TRANSPARENT
            && port == 443
            && !target_ip.is_loopback()
            && !target_ip.is_unspecified();

        if is_localhost_443 || is_transparent_443 {
            if is_transparent_443 {
                debug_log(&format!("INTERCEPT connect({}:{}) -> 127.0.0.1:{} [transparent]",
                    target_ip, port, *DAEMON_PORT));
            } else {
                debug_log(&format!("INTERCEPT connect(127.0.0.1:443) -> 127.0.0.1:{}", *DAEMON_PORT));
            }

            // Store the ORIGINAL destination address so getpeername() can return it
            // instead of the redirected 127.0.0.1:DAEMON_PORT. This is critical —
            // Bun/TLS libraries verify peer address matches intended server.
            if let Ok(mut sockets) = INTERCEPTED_SOCKETS.lock() {
                if sockets.len() > 1024 {
                    sockets.clear();
                }
                sockets.insert(sockfd, *sin);
                if *DEBUG {
                    debug_log(&format!("  fd {} -> original addr: {}:{}", sockfd, target_ip, port));
                }
            }

            let mut new_addr: sockaddr_in = std::mem::zeroed();
            new_addr.sin_family = AF_INET as u8;
            new_addr.sin_port = (*DAEMON_PORT).to_be();
            new_addr.sin_addr.s_addr = u32::from(Ipv4Addr::LOCALHOST).to_be();
            #[cfg(target_os = "macos")]
            {
                new_addr.sin_len = std::mem::size_of::<sockaddr_in>() as u8;
            }

            return match ORIGINAL_CONNECT.get() {
                Some(func) => func(
                    sockfd,
                    &new_addr as *const sockaddr_in as *const libc::sockaddr,
                    std::mem::size_of::<sockaddr_in>() as libc::socklen_t,
                ),
                None => libc::connect(
                    sockfd,
                    &new_addr as *const sockaddr_in as *const libc::sockaddr,
                    std::mem::size_of::<sockaddr_in>() as libc::socklen_t,
                ),
            };
        }
    }

    match ORIGINAL_CONNECT.get() {
        Some(func) => func(sockfd, addr, addrlen),
        None => libc::connect(sockfd, addr, addrlen),
    }
}

// =============================================================================
// Hook: connectx (macOS) — same redirect for BSD endpoints API
// =============================================================================

#[cfg(target_os = "macos")]
unsafe extern "C" fn connectx_detour(
    sockfd: libc::c_int,
    endpoints: *const libc::sa_endpoints_t,
    associd: libc::c_uint,
    flags: libc::c_uint,
    iov: *const libc::iovec,
    iovcnt: libc::c_uint,
    len: *mut usize,
    connid: *mut libc::c_uint,
) -> libc::c_int {
    let _guard = match DetourGuard::new() {
        Some(g) => g,
        None => {
            return match ORIGINAL_CONNECTX.get() {
                Some(func) => func(sockfd, endpoints, associd, flags, iov, iovcnt, len, connid),
                None => {
                    *libc::__error() = libc::ENOSYS;
                    -1
                }
            };
        }
    };

    if !endpoints.is_null() {
        let ep = &*endpoints;
        if !ep.sae_dstaddr.is_null() && (*ep.sae_dstaddr).sa_family as i32 == AF_INET {
            let sin = &*(ep.sae_dstaddr as *const sockaddr_in);
            let port = u16::from_be(sin.sin_port);
            let ip_bytes = sin.sin_addr.s_addr.to_ne_bytes();
            let target_ip = Ipv4Addr::new(ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3]);

            let is_localhost_443 = target_ip == Ipv4Addr::LOCALHOST && port == 443 && *DAEMON_PORT != 443;
            let is_transparent_443 = *TRANSPARENT
                && port == 443
                && !target_ip.is_loopback()
                && !target_ip.is_unspecified();

            if is_localhost_443 || is_transparent_443 {
                if is_transparent_443 {
                    debug_log(&format!("INTERCEPT connectx({}:{}) -> 127.0.0.1:{} [transparent]",
                        target_ip, port, *DAEMON_PORT));
                } else {
                    debug_log(&format!("INTERCEPT connectx(127.0.0.1:443) -> 127.0.0.1:{}", *DAEMON_PORT));
                }

                let mut new_addr: sockaddr_in = std::mem::zeroed();
                new_addr.sin_family = AF_INET as u8;
                new_addr.sin_port = (*DAEMON_PORT).to_be();
                new_addr.sin_addr.s_addr = u32::from(Ipv4Addr::LOCALHOST).to_be();
                new_addr.sin_len = std::mem::size_of::<sockaddr_in>() as u8;

                let mut new_ep: libc::sa_endpoints_t = *ep;
                new_ep.sae_dstaddr = &new_addr as *const sockaddr_in as *const libc::sockaddr;
                new_ep.sae_dstaddrlen = std::mem::size_of::<sockaddr_in>() as u32;

                return match ORIGINAL_CONNECTX.get() {
                    Some(func) => func(sockfd, &new_ep, associd, flags, iov, iovcnt, len, connid),
                    None => {
                        *libc::__error() = libc::ENOSYS;
                        -1
                    }
                };
            }
        }
    }

    match ORIGINAL_CONNECTX.get() {
        Some(func) => func(sockfd, endpoints, associd, flags, iov, iovcnt, len, connid),
        None => {
            *libc::__error() = libc::ENOSYS;
            -1
        }
    }
}

// =============================================================================
// =============================================================================
// Hook: getpeername — return original address for redirected sockets (mirrord pattern)
// =============================================================================

unsafe extern "C" fn getpeername_detour(
    sockfd: libc::c_int,
    addr: *mut libc::sockaddr,
    addrlen: *mut libc::socklen_t,
) -> libc::c_int {
    let _guard = match DetourGuard::new() {
        Some(g) => g,
        None => {
            return match ORIGINAL_GETPEERNAME.get() {
                Some(func) => func(sockfd, addr, addrlen),
                None => { *libc::__error() = libc::EBADF; -1 }
            };
        }
    };

    // If this fd was redirected by our connect hook, return the ORIGINAL
    // destination address (not 127.0.0.1:DAEMON_PORT).
    if let Ok(sockets) = INTERCEPTED_SOCKETS.lock() {
        if let Some(original_addr) = sockets.get(&sockfd) {
            if !addr.is_null() && !addrlen.is_null() {
                let size = std::mem::size_of::<sockaddr_in>();
                if (*addrlen as usize) >= size {
                    std::ptr::copy_nonoverlapping(
                        original_addr as *const sockaddr_in as *const u8,
                        addr as *mut u8,
                        size,
                    );
                    *addrlen = size as libc::socklen_t;
                    debug_log(&format!("INTERCEPT getpeername(fd={}) -> original addr", sockfd));
                    return 0;
                }
            }
        }
    }

    match ORIGINAL_GETPEERNAME.get() {
        Some(func) => func(sockfd, addr, addrlen),
        None => { *libc::__error() = libc::EBADF; -1 }
    }
}

// =============================================================================
// Hook: getsockname — return plausible local address for redirected sockets
// =============================================================================

unsafe extern "C" fn getsockname_detour(
    sockfd: libc::c_int,
    addr: *mut libc::sockaddr,
    addrlen: *mut libc::socklen_t,
) -> libc::c_int {
    let _guard = match DetourGuard::new() {
        Some(g) => g,
        None => {
            return match ORIGINAL_GETSOCKNAME.get() {
                Some(func) => func(sockfd, addr, addrlen),
                None => { *libc::__error() = libc::EBADF; -1 }
            };
        }
    };

    // For redirected sockets, call the real getsockname (which returns the local
    // end of the 127.0.0.1 connection) — that's fine, apps rarely check this.
    // But if needed in the future, we could return a fake local address here.
    match ORIGINAL_GETSOCKNAME.get() {
        Some(func) => func(sockfd, addr, addrlen),
        None => { *libc::__error() = libc::EBADF; -1 }
    }
}

// =============================================================================
// Hook: SecTrustEvaluateWithError (macOS) — universal TLS bypass
// =============================================================================

/// Bypasses ALL certificate validation on macOS. Returns true to indicate
/// the certificate chain is valid, regardless of actual validity.
/// This makes any macOS application accept the daemon's self-signed cert.
#[cfg(target_os = "macos")]
unsafe extern "C" fn sec_trust_evaluate_with_error_detour(
    _trust: *const std::ffi::c_void,
    error: *mut *mut std::ffi::c_void,
) -> bool {
    debug_log("INTERCEPT SecTrustEvaluateWithError -> true (TLS bypass)");
    // Clear the error output if provided
    if !error.is_null() {
        *error = null_mut();
    }
    true
}

// =============================================================================
// Hook: dns_configuration_copy (macOS) — prevent system DNS bypass
// =============================================================================

/// Returns null to force callers to fall back to getaddrinfo, which we intercept.
/// Without this, macOS system frameworks can bypass our getaddrinfo hook entirely.
#[cfg(target_os = "macos")]
unsafe extern "C" fn dns_configuration_copy_detour() -> *mut std::ffi::c_void {
    let _guard = match DetourGuard::new() {
        Some(g) => g,
        None => {
            return match ORIGINAL_DNS_CONFIGURATION_COPY.get() {
                Some(func) => func(),
                None => null_mut(),
            };
        }
    };

    debug_log("INTERCEPT dns_configuration_copy -> null (force getaddrinfo fallback)");
    null_mut()
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn dns_configuration_free_detour(config: *mut std::ffi::c_void) {
    if config.is_null() {
        return; // We returned null from copy, nothing to free
    }
    // Not ours — pass to original
    if let Some(func) = ORIGINAL_DNS_CONFIGURATION_FREE.get() {
        func(config);
    }
}

// =============================================================================
// Initialization — install hooks using frida-gum (same as mirrord)
// =============================================================================

fn install_hooks() {
    let gum = Gum::obtain();
    let mut interceptor = Interceptor::obtain(&gum);

    // Transaction batching: install all hooks atomically
    interceptor.begin_transaction();

    // Hook getaddrinfo
    if let Some(ptr) = Module::find_global_export_by_name("getaddrinfo") {
        match interceptor.replace(
            ptr,
            NativePointer(getaddrinfo_detour as *mut _),
            NativePointer(null_mut()),
        ) {
            Ok(original) => {
                let func: GetaddrinfoFn = unsafe { std::mem::transmute(original.0) };
                let _ = ORIGINAL_GETADDRINFO.set(func);
                debug_log("hooked getaddrinfo (frida-gum inline hook)");
            }
            Err(e) => {
                eprintln!("rigor-layer: FAILED to hook getaddrinfo: {:?}", e);
            }
        }
    } else {
        eprintln!("rigor-layer: getaddrinfo not found in exports");
    }

    // Hook connect
    if let Some(ptr) = Module::find_global_export_by_name("connect") {
        match interceptor.replace(
            ptr,
            NativePointer(connect_detour as *mut _),
            NativePointer(null_mut()),
        ) {
            Ok(original) => {
                let func: ConnectFn = unsafe { std::mem::transmute(original.0) };
                let _ = ORIGINAL_CONNECT.set(func);
                debug_log("hooked connect (frida-gum)");
            }
            Err(_) => {
                debug_log("failed to hook connect");
            }
        }
    }

    // Hook connectx (macOS)
    #[cfg(target_os = "macos")]
    if let Some(ptr) = Module::find_global_export_by_name("connectx") {
        match interceptor.replace(
            ptr,
            NativePointer(connectx_detour as *mut _),
            NativePointer(null_mut()),
        ) {
            Ok(original) => {
                let func: ConnectxFn = unsafe { std::mem::transmute(original.0) };
                let _ = ORIGINAL_CONNECTX.set(func);
                debug_log("hooked connectx (frida-gum)");
            }
            Err(_) => {
                debug_log("failed to hook connectx");
            }
        }
    }

    // Hook freeaddrinfo
    if let Some(ptr) = Module::find_global_export_by_name("freeaddrinfo") {
        match interceptor.replace(
            ptr,
            NativePointer(freeaddrinfo_detour as *mut _),
            NativePointer(null_mut()),
        ) {
            Ok(original) => {
                let func: FreeaddrinfoFn = unsafe { std::mem::transmute(original.0) };
                let _ = ORIGINAL_FREEADDRINFO.set(func);
                debug_log("hooked freeaddrinfo (frida-gum inline hook)");
            }
            Err(_) => {
                debug_log("failed to hook freeaddrinfo (non-critical)");
            }
        }
    }

    // Hook getpeername — return original address for redirected sockets
    if let Some(ptr) = Module::find_global_export_by_name("getpeername") {
        match interceptor.replace(
            ptr,
            NativePointer(getpeername_detour as *mut _),
            NativePointer(null_mut()),
        ) {
            Ok(original) => {
                let func: GetpeernameFn = unsafe { std::mem::transmute(original.0) };
                let _ = ORIGINAL_GETPEERNAME.set(func);
                debug_log("hooked getpeername (frida-gum)");
            }
            Err(_) => {
                debug_log("failed to hook getpeername (non-critical)");
            }
        }
    }

    // Hook getsockname
    if let Some(ptr) = Module::find_global_export_by_name("getsockname") {
        match interceptor.replace(
            ptr,
            NativePointer(getsockname_detour as *mut _),
            NativePointer(null_mut()),
        ) {
            Ok(original) => {
                let func: GetsocknameFn = unsafe { std::mem::transmute(original.0) };
                let _ = ORIGINAL_GETSOCKNAME.set(func);
                debug_log("hooked getsockname (frida-gum)");
            }
            Err(_) => {
                debug_log("failed to hook getsockname (non-critical)");
            }
        }
    }

    // Hook gethostbyname (deprecated but used by Erlang/Elixir, older C libraries)
    if let Some(ptr) = Module::find_global_export_by_name("gethostbyname") {
        match interceptor.replace(
            ptr,
            NativePointer(gethostbyname_detour as *mut _),
            NativePointer(null_mut()),
        ) {
            Ok(original) => {
                let func: GethostbynameFn = unsafe { std::mem::transmute(original.0) };
                let _ = ORIGINAL_GETHOSTBYNAME.set(func);
                debug_log("hooked gethostbyname (frida-gum)");
            }
            Err(_) => {
                debug_log("failed to hook gethostbyname (non-critical)");
            }
        }
    }

    // Hook SecTrustEvaluateWithError (macOS TLS bypass — universal cert acceptance)
    #[cfg(target_os = "macos")]
    {
        if let Some(ptr) = Module::find_global_export_by_name("SecTrustEvaluateWithError") {
            match interceptor.replace(
                ptr,
                NativePointer(sec_trust_evaluate_with_error_detour as *mut _),
                NativePointer(null_mut()),
            ) {
                Ok(_original) => {
                    // No need to store original — we never call it
                    debug_log("hooked SecTrustEvaluateWithError (TLS bypass)");
                }
                Err(_) => {
                    debug_log("failed to hook SecTrustEvaluateWithError (non-critical)");
                }
            }
        }
    }

    // Hook dns_configuration_copy / dns_configuration_free (macOS system DNS bypass prevention)
    #[cfg(target_os = "macos")]
    {
        if let Some(ptr) = Module::find_global_export_by_name("dns_configuration_copy") {
            match interceptor.replace(
                ptr,
                NativePointer(dns_configuration_copy_detour as *mut _),
                NativePointer(null_mut()),
            ) {
                Ok(original) => {
                    let func: DnsConfigurationCopyFn = unsafe { std::mem::transmute(original.0) };
                    let _ = ORIGINAL_DNS_CONFIGURATION_COPY.set(func);
                    debug_log("hooked dns_configuration_copy (frida-gum)");
                }
                Err(_) => {
                    debug_log("failed to hook dns_configuration_copy (non-critical)");
                }
            }
        }

        if let Some(ptr) = Module::find_global_export_by_name("dns_configuration_free") {
            match interceptor.replace(
                ptr,
                NativePointer(dns_configuration_free_detour as *mut _),
                NativePointer(null_mut()),
            ) {
                Ok(original) => {
                    let func: DnsConfigurationFreeFn = unsafe { std::mem::transmute(original.0) };
                    let _ = ORIGINAL_DNS_CONFIGURATION_FREE.set(func);
                    debug_log("hooked dns_configuration_free (frida-gum)");
                }
                Err(_) => {
                    debug_log("failed to hook dns_configuration_free (non-critical)");
                }
            }
        }
    }

    // Flush all hooks atomically
    interceptor.end_transaction();

    // Don't drop the interceptor — hooks must stay alive
    std::mem::forget(interceptor);
    std::mem::forget(gum);
}

// =============================================================================
// Constructor — runs when library is loaded via LD_PRELOAD / DYLD_INSERT_LIBRARIES
// =============================================================================

#[used]
#[cfg_attr(target_os = "linux", link_section = ".init_array")]
#[cfg_attr(target_os = "macos", link_section = "__DATA,__mod_init_func")]
static INIT: extern "C" fn() = {
    extern "C" fn init() {
        // Force lazy init
        let _ = &*DAEMON_PORT;
        let _ = &*DEBUG;
        let _ = &*TRANSPARENT;
        let _ = &*INTERCEPT_HOSTS;

        install_hooks();

        // Clear DYLD_INSERT_LIBRARIES so child processes (stop hooks, plugins,
        // shell scripts) don't try to load our arm64 dylib into arm64e system
        // shells (/bin/sh, /bin/bash, /bin/zsh) and crash. The hooks are already
        // installed in THIS process via frida-gum in-memory patching — removing
        // the env var doesn't affect them. Child processes won't be intercepted,
        // but the main process (where all LLM API calls happen) stays hooked.
        #[cfg(target_os = "macos")]
        std::env::remove_var("DYLD_INSERT_LIBRARIES");
        #[cfg(target_os = "linux")]
        std::env::remove_var("LD_PRELOAD");

        if *DEBUG {
            eprintln!(
                "rigor-layer: loaded — intercepting {} hosts via getaddrinfo -> 127.0.0.1:{}",
                INTERCEPT_HOSTS.len(),
                *DAEMON_PORT
            );
            for host in INTERCEPT_HOSTS.iter() {
                eprintln!("rigor-layer:   {}", host);
            }
        }
    }
    init
};
