//! Import binding, LoadLibrary / GetProcAddress over our pseudo modules, and abort thunks for
//! anything not implemented (so a missing function is reported by name instead of crashing).

use crate::k32 as k;
use crate::log;
use crate::other as o;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

macro_rules! table {
    ($( $dll:literal : [ $( $name:ident = $f:expr ),* $(,)? ] ),* $(,)?) => {
        fn build() -> HashMap<(String, String), usize> {
            let mut m = HashMap::new();
            $( $( m.insert(($dll.to_string(), stringify!($name).to_string()), $f as usize); )* )*
            m
        }
    };
}

table! {
    "kernel32.dll": [
        AreFileApisANSI = k::AreFileApisANSI, BuildCommDCBA = k::BuildCommDCBA, CancelIo = k::CancelIo,
        ClearCommError = k::ClearCommError, CloseHandle = k::CloseHandle, CompareStringW = k::CompareStringW,
        CreateDirectoryA = k::CreateDirectoryA, CreateDirectoryW = k::CreateDirectoryW, CreateEventA = k::CreateEventA,
        CreateFileA = k::CreateFileA, CreateFileW = k::CreateFileW, CreateProcessA = k::CreateProcessA,
        CreateThread = k::CreateThread, DecodePointer = k::DecodePointer, DeleteCriticalSection = k::DeleteCriticalSection,
        DeleteFileA = k::DeleteFileA, DeleteFileW = k::DeleteFileW, DisableThreadLibraryCalls = k::DisableThreadLibraryCalls,
        EncodePointer = k::EncodePointer, EnterCriticalSection = k::EnterCriticalSection, EnumSystemLocalesW = k::EnumSystemLocalesW,
        EscapeCommFunction = k::EscapeCommFunction, ExitProcess = k::ExitProcess, ExitThread = k::ExitThread,
        FileTimeToLocalFileTime = k::FileTimeToLocalFileTime, FileTimeToSystemTime = k::FileTimeToSystemTime,
        FindClose = k::FindClose, FindFirstFileA = k::FindFirstFileA, FindFirstFileExW = k::FindFirstFileExW,
        FindNextFileA = k::FindNextFileA, FlushConsoleInputBuffer = k::FlushConsoleInputBuffer,
        FlushFileBuffers = k::FlushFileBuffers, FormatMessageA = k::FormatMessageA,
        FreeEnvironmentStringsW = k::FreeEnvironmentStringsW, FreeLibrary = k::FreeLibrary, GetACP = k::GetACP,
        GetCPInfo = k::GetCPInfo, GetCommState = k::GetCommState, GetCommandLineA = k::GetCommandLineA,
        GetConsoleCP = k::GetConsoleCP, GetConsoleMode = k::GetConsoleMode, GetCurrentDirectoryW = k::GetCurrentDirectoryW,
        GetCurrentProcess = k::GetCurrentProcess, GetCurrentProcessId = k::GetCurrentProcessId,
        GetCurrentThreadId = k::GetCurrentThreadId, GetDateFormatW = k::GetDateFormatW, GetDriveTypeW = k::GetDriveTypeW,
        GetEnvironmentStringsW = k::GetEnvironmentStringsW, GetFileAttributesA = k::GetFileAttributesA,
        GetFileInformationByHandle = k::GetFileInformationByHandle, GetFileSize = k::GetFileSize, GetFileType = k::GetFileType,
        GetFullPathNameW = k::GetFullPathNameW, GetLastError = k::GetLastError, GetLocalTime = k::GetLocalTime,
        GetLocaleInfoW = k::GetLocaleInfoW, GetModuleFileNameA = k::GetModuleFileNameA, GetModuleFileNameW = k::GetModuleFileNameW,
        GetModuleHandleA = k::GetModuleHandleA, GetModuleHandleExW = k::GetModuleHandleExW, GetModuleHandleW = k::GetModuleHandleW,
        GetNumberOfConsoleInputEvents = k::GetNumberOfConsoleInputEvents, GetOEMCP = k::GetOEMCP,
        GetProcAddress = k::GetProcAddress, GetProcessHeap = k::GetProcessHeap, GetProcessTimes = k::GetProcessTimes,
        GetStartupInfoW = k::GetStartupInfoW, GetStdHandle = k::GetStdHandle, GetStringTypeW = k::GetStringTypeW,
        GetSystemTime = k::GetSystemTime, GetSystemTimeAsFileTime = k::GetSystemTimeAsFileTime,
        GetTempFileNameA = k::GetTempFileNameA, GetTempPathA = k::GetTempPathA, GetTickCount = k::GetTickCount,
        GetTimeFormatW = k::GetTimeFormatW, GetTimeZoneInformation = k::GetTimeZoneInformation,
        GetUserDefaultLCID = k::GetUserDefaultLCID, GetVersion = k::GetVersion, GetVersionExA = k::GetVersionExA,
        GlobalMemoryStatus = k::GlobalMemoryStatus, HeapAlloc = k::HeapAlloc, HeapFree = k::HeapFree,
        HeapReAlloc = k::HeapReAlloc, HeapSize = k::HeapSize, InitializeCriticalSection = k::InitializeCriticalSection,
        InitializeCriticalSectionAndSpinCount = k::InitializeCriticalSectionAndSpinCount,
        InitializeCriticalSectionEx = k::InitializeCriticalSectionEx,
        IsDebuggerPresent = k::IsDebuggerPresent, IsProcessorFeaturePresent = k::IsProcessorFeaturePresent,
        IsValidCodePage = k::IsValidCodePage, IsValidLocale = k::IsValidLocale, LCMapStringW = k::LCMapStringW,
        LeaveCriticalSection = k::LeaveCriticalSection, LoadLibraryA = k::LoadLibraryA, LoadLibraryExW = k::LoadLibraryExW,
        LoadLibraryW = k::LoadLibraryW, LocalFileTimeToFileTime = k::LocalFileTimeToFileTime, LocalFree = k::LocalFree,
        MoveFileA = k::MoveFileA, MultiByteToWideChar = k::MultiByteToWideChar, OutputDebugStringA = k::OutputDebugStringA,
        OutputDebugStringW = k::OutputDebugStringW, PeekConsoleInputA = k::PeekConsoleInputA, PeekNamedPipe = k::PeekNamedPipe,
        PurgeComm = k::PurgeComm, QueryPerformanceCounter = k::QueryPerformanceCounter,
        QueryPerformanceFrequency = k::QueryPerformanceFrequency, RaiseException = k::RaiseException,
        ReadConsoleInputA = k::ReadConsoleInputA, ReadConsoleW = k::ReadConsoleW, ReadFile = k::ReadFile,
        ReadFileEx = k::ReadFileEx, ResetEvent = k::ResetEvent, ResumeThread = k::ResumeThread,
        RtlCaptureContext = k::RtlCaptureContext, RtlLookupFunctionEntry = k::RtlLookupFunctionEntry,
        RtlPcToFileHeader = k::RtlPcToFileHeader, RtlUnwindEx = k::RtlUnwindEx, RtlVirtualUnwind = k::RtlVirtualUnwind,
        SetCommState = k::SetCommState, SetCommTimeouts = k::SetCommTimeouts, SetConsoleCtrlHandler = k::SetConsoleCtrlHandler,
        SetConsoleMode = k::SetConsoleMode, SetEndOfFile = k::SetEndOfFile, SetEnvironmentVariableA = k::SetEnvironmentVariableA,
        SetEvent = k::SetEvent, SetFilePointer = k::SetFilePointer, SetFilePointerEx = k::SetFilePointerEx,
        SetLastError = k::SetLastError, SetPriorityClass = k::SetPriorityClass, SetStdHandle = k::SetStdHandle,
        SetUnhandledExceptionFilter = k::SetUnhandledExceptionFilter, SetupComm = k::SetupComm, Sleep = k::Sleep,
        SleepEx = k::SleepEx, SystemTimeToFileTime = k::SystemTimeToFileTime, TerminateProcess = k::TerminateProcess,
        TlsAlloc = k::TlsAlloc, TlsFree = k::TlsFree, TlsGetValue = k::TlsGetValue, TlsSetValue = k::TlsSetValue,
        UnhandledExceptionFilter = k::UnhandledExceptionFilter, WaitForMultipleObjects = k::WaitForMultipleObjects,
        WaitForMultipleObjectsEx = k::WaitForMultipleObjectsEx, WaitForSingleObject = k::WaitForSingleObject,
        WaitForSingleObjectEx = k::WaitForSingleObjectEx, WideCharToMultiByte = k::WideCharToMultiByte,
        WriteConsoleW = k::WriteConsoleW, WriteFile = k::WriteFile, WriteFileEx = k::WriteFileEx,
        lstrcpyW = k::lstrcpyW, lstrlenW = k::lstrlenW,
    ],
    "user32.dll": [
        GetDesktopWindow = o::GetDesktopWindow, GetUserObjectInformationW = o::GetUserObjectInformationW,
        GetProcessWindowStation = o::GetProcessWindowStation, MessageBoxA = o::MessageBoxA,
    ],
    "advapi32.dll": [
        RegSetValueExA = o::RegSetValueExA, RegDeleteValueA = o::RegDeleteValueA, RegDeleteKeyA = o::RegDeleteKeyA,
        RegCreateKeyExA = o::RegCreateKeyExA, RegQueryInfoKeyA = o::RegQueryInfoKeyA, ReportEventA = o::ReportEventA,
        RegisterEventSourceA = o::RegisterEventSourceA, DeregisterEventSource = o::DeregisterEventSource,
        RegQueryValueExA = o::RegQueryValueExA, RegOpenKeyExA = o::RegOpenKeyExA, RegCloseKey = o::RegCloseKey,
    ],
    "ole32.dll": [
        StringFromCLSID = o::StringFromCLSID, CoCreateInstance = o::CoCreateInstance,
        CoInitializeEx = o::CoInitializeEx, CoUninitialize = o::CoUninitialize,
    ],
    "shell32.dll": [ SHGetSpecialFolderPathA = o::SHGetSpecialFolderPathA ],
    "ws2_32.dll": [
        WSACloseEvent = o::WSACloseEvent, WSAEnumNetworkEvents = o::sock_err, WSAResetEvent = o::WSAResetEvent,
        getprotobyname = o::getprotobyname, shutdown = o::sock_ok, recvfrom = o::recvfrom, select = o::select,
        WSAGetLastError = o::WSAGetLastError, inet_ntoa = o::inet_ntoa, gethostname = o::gethostname,
        WSAAddressToStringA = o::sock_err, getaddrinfo = o::getaddrinfo, WSAEventSelect = o::sock_err,
        freeaddrinfo = o::freeaddrinfo, connect = o::sock_ok, ntohl = o::ntohl, htonl = o::htonl, bind = o::sock_ok,
        gethostbyname = o::gethostbyname, socket = o::socket, sendto = o::sendto, closesocket = o::closesocket,
        WSACleanup = o::WSACleanup, WSAStartup = o::WSAStartup, inet_addr = o::inet_addr, htons = o::htons,
        recv = o::recv, send = o::send, WSACreateEvent = o::WSACreateEvent, ioctlsocket = o::sock_ok,
        WSASetLastError = o::WSASetLastError,
    ],
    "winhttp.dll": [
        WinHttpSendRequest = o::WinHttpFail, WinHttpOpenRequest = o::WinHttpFail, WinHttpReceiveResponse = o::WinHttpFail,
        WinHttpOpen = o::WinHttpOpen, WinHttpCrackUrl = o::WinHttpFail, WinHttpReadData = o::WinHttpFail,
        WinHttpCloseHandle = o::WinHttpCloseHandle, WinHttpQueryHeaders = o::WinHttpFail,
        WinHttpSetStatusCallback = o::WinHttpFail, WinHttpSetOption = o::WinHttpFail, WinHttpConnect = o::WinHttpFail,
    ],
    "gpio.dll": [
        GpioDllInit = k::GpioDllInit, GpioDllDeinit = k::GpioDllDeinit, GpioGet = k::GpioGet, GpioSet = k::GpioSet,
    ],
}

static TABLE: OnceLock<HashMap<(String, String), usize>> = OnceLock::new();
fn table() -> &'static HashMap<(String, String), usize> {
    TABLE.get_or_init(build)
}

const DLLS: &[&str] = &[
    "kernel32.dll", "user32.dll", "advapi32.dll", "ole32.dll", "shell32.dll", "ws2_32.dll", "winhttp.dll", "gpio.dll",
];
const PSEUDO_BASE: usize = 0x7ffe_0000_0000;
const EXE_HANDLE: usize = 0x7ffd_0000_0000;

fn canon(dll: &str) -> String {
    let base = dll.rsplit(['\\', '/']).next().unwrap_or(dll).to_ascii_lowercase();
    let base = if base.ends_with(".dll") { base } else { format!("{base}.dll") };
    match base.as_str() {
        // the engine's GPIO helpers all map to our built-in implementation
        b if b.contains("gpio") => "gpio.dll".into(),
        "kernelbase.dll" => "kernel32.dll".into(),
        b => b.into(),
    }
}

pub fn load_library(name: &str) -> usize {
    let c = canon(name);
    match DLLS.iter().position(|d| *d == c) {
        Some(i) => {
            log!(1, "LoadLibrary({name}) -> built-in {c}");
            PSEUDO_BASE + i * 0x10000
        }
        None => {
            log!(1, "LoadLibrary({name}) -> not available");
            crate::rt::set_err(126);
            0
        }
    }
}

pub fn module_handle(name: Option<String>) -> usize {
    match name {
        None => EXE_HANDLE,
        Some(n) => {
            let c = canon(&n);
            if n.eq_ignore_ascii_case("BcmGnss.dll") {
                return crate::k32::CONFIG.get().map(|c| c.image.start).unwrap_or(0);
            }
            DLLS.iter().position(|d| *d == c).map(|i| PSEUDO_BASE + i * 0x10000).unwrap_or_else(|| {
                crate::rt::set_err(126);
                0
            })
        }
    }
}

pub fn get_proc(h: usize, name: &str) -> usize {
    if let Some(cfg) = crate::k32::CONFIG.get() {
        if h == cfg.image.start {
            return crate::IMAGE.get().and_then(|i| i.export(name)).unwrap_or(0);
        }
    }
    let dll = if h >= PSEUDO_BASE && (h - PSEUDO_BASE) / 0x10000 < DLLS.len() {
        DLLS[(h - PSEUDO_BASE) / 0x10000]
    } else {
        "kernel32.dll"
    };
    let r = table().get(&(dll.to_string(), name.to_string())).copied().unwrap_or(0);
    log!(if r == 0 { 1 } else { 2 }, "GetProcAddress({dll}, {name}) -> {}", if r == 0 { "NULL" } else { "ok" });
    if r == 0 {
        crate::rt::set_err(127);
    }
    r
}

// ---- abort thunks for unimplemented imports --------------------------------------------------------
static MISSING: Mutex<Vec<String>> = Mutex::new(Vec::new());

extern "win64" fn missing_called(name: *const u8) -> ! {
    let s = unsafe { std::ffi::CStr::from_ptr(name as *const _) }.to_string_lossy();
    eprintln!("tabpro-s-gnss: called unimplemented Windows function {s} — please report");
    std::process::abort();
}

fn make_thunk(name: String) -> usize {
    static THUNKS: Mutex<(usize, usize)> = Mutex::new((0, 0)); // (page, used)
    let cname = Box::leak(format!("{name}\0").into_boxed_str()).as_ptr() as u64;
    let mut code = Vec::with_capacity(24);
    code.extend_from_slice(&[0x48, 0xb9]); // mov rcx, imm64
    code.extend_from_slice(&cname.to_le_bytes());
    code.extend_from_slice(&[0x48, 0xb8]); // mov rax, imm64
    code.extend_from_slice(&(missing_called as usize as u64).to_le_bytes());
    code.extend_from_slice(&[0xff, 0xe0]); // jmp rax
    let mut t = THUNKS.lock().unwrap();
    if t.0 == 0 || t.1 + 32 > 0x10000 {
        let p = unsafe {
            libc::mmap(std::ptr::null_mut(), 0x10000, libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                       libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0)
        };
        *t = (p as usize, 0);
    }
    let a = t.0 + t.1;
    unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), a as *mut u8, code.len()) };
    t.1 += 32;
    a
}

/// Resolver for the PE loader.
/// Standard WS2_32 export ordinals (BcmGnss.dll imports these by number).
fn ws2_ordinal(n: &str) -> Option<&'static str> {
    Some(match n.strip_prefix('#')?.parse::<u32>().ok()? {
        2 => "bind", 3 => "closesocket", 4 => "connect", 8 => "htonl", 9 => "htons", 10 => "ioctlsocket",
        11 => "inet_addr", 12 => "inet_ntoa", 14 => "ntohl", 16 => "recv", 17 => "recvfrom", 18 => "select",
        19 => "send", 20 => "sendto", 22 => "shutdown", 23 => "socket", 52 => "gethostbyname",
        53 => "getprotobyname", 57 => "gethostname", 111 => "WSAGetLastError", 112 => "WSASetLastError",
        115 => "WSAStartup", 116 => "WSACleanup",
        _ => return None,
    })
}

pub fn resolve_import(dll: &str, name: &str) -> usize {
    let c = canon(dll);
    let name = if c == "ws2_32.dll" { ws2_ordinal(name).unwrap_or(name) } else { name };
    if let Some(a) = table().get(&(c.clone(), name.to_string())) {
        return *a;
    }
    MISSING.lock().unwrap().push(format!("{c}!{name}"));
    make_thunk(format!("{c}!{name}"))
}

pub fn missing() -> Vec<String> {
    MISSING.lock().unwrap().clone()
}
