#![cfg(windows)]

//! Safe Rust bindings for the Windows Subsystem for Linux (WSL) API.
//!
//! Wraps [`wslapi.h`](https://learn.microsoft.com/en-us/windows/win32/api/wslapi/)
//! — manage WSL distributions: register, unregister, configure, launch processes.
//!
//! ## Example
//!
//! ```rust,no_run
//! use wsl2_api;
//!
//! if wsl2_api::is_distribution_registered("Ubuntu").unwrap_or(false) {
//!     println!("Ubuntu is installed");
//! }
//!
//! let code = wsl2_api::launch_interactive("Ubuntu", "ls /", false).unwrap();
//! ```

use std::os::windows::ffi::OsStrExt;

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use windows_sys::Win32::Foundation::HANDLE;
pub use windows_sys::Win32::System::SubsystemForLinux::{
    WSL_DISTRIBUTION_FLAGS, WSL_DISTRIBUTION_FLAGS_APPEND_NT_PATH,
    WSL_DISTRIBUTION_FLAGS_ENABLE_DRIVE_MOUNTING, WSL_DISTRIBUTION_FLAGS_ENABLE_INTEROP,
    WSL_DISTRIBUTION_FLAGS_NONE,
};
pub use windows_sys::core::HRESULT;

use windows_sys::Win32::System::SubsystemForLinux::{
    WslConfigureDistribution as RawWslConfigureDistribution,
    WslGetDistributionConfiguration as RawWslGetDistributionConfiguration,
    WslIsDistributionRegistered as RawWslIsDistributionRegistered, WslLaunch as RawWslLaunch,
    WslLaunchInteractive as RawWslLaunchInteractive,
    WslRegisterDistribution as RawWslRegisterDistribution,
    WslUnregisterDistribution as RawWslUnregisterDistribution,
};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Error returned by WSL API operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WslError {
    /// The WSL API returned a failure `HRESULT`.
    HResult(HRESULT),
    /// A distribution name contained interior null bytes.
    InvalidName,
}

impl std::fmt::Display for WslError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HResult(hr) => write!(f, "WSL API error: HRESULT 0x{hr:08X}"),
            Self::InvalidName => write!(f, "distribution name contains interior null bytes"),
        }
    }
}

impl std::error::Error for WslError {}

type Result<T> = std::result::Result<T, WslError>;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn to_wide(s: &str) -> std::result::Result<Vec<u16>, WslError> {
    if s.contains('\0') {
        return Err(WslError::InvalidName);
    }
    Ok(std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect())
}

fn check_hr(hr: HRESULT) -> Result<()> {
    if hr < 0 { Err(WslError::HResult(hr)) } else { Ok(()) }
}

// ---------------------------------------------------------------------------
// Distribution configuration
// ---------------------------------------------------------------------------

/// Configuration of a registered WSL distribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistributionConfig {
    pub version: u32,
    pub default_uid: u32,
    pub flags: WSL_DISTRIBUTION_FLAGS,
}

// ---------------------------------------------------------------------------
// Safe wrappers
// ---------------------------------------------------------------------------

/// Returns `true` if a distribution is registered with WSL.
///
/// Wraps [`WslIsDistributionRegistered`](
/// https://learn.microsoft.com/en-us/windows/win32/api/wslapi/nf-wslapi-wslisdistributionregistered).
pub fn is_distribution_registered(name: &str) -> Result<bool> {
    let name_w = to_wide(name)?;
    Ok(unsafe { RawWslIsDistributionRegistered(name_w.as_ptr()) } != 0)
}

/// Registers a new WSL distribution from a `.tar.gz` root filesystem image.
///
/// Wraps [`WslRegisterDistribution`](
/// https://learn.microsoft.com/en-us/windows/win32/api/wslapi/nf-wslapi-wslregisterdistribution).
///
/// `tar_gz_path` must be an absolute path to a `.tar.gz` file.
pub fn register_distribution(name: &str, tar_gz_path: &str) -> Result<()> {
    let name_w = to_wide(name)?;
    let path_w = to_wide(tar_gz_path)?;
    let hr = unsafe { RawWslRegisterDistribution(name_w.as_ptr(), path_w.as_ptr()) };
    check_hr(hr)
}

/// Unregisters (deletes) a WSL distribution.
///
/// Wraps [`WslUnregisterDistribution`](
/// https://learn.microsoft.com/en-us/windows/win32/api/wslapi/nf-wslapi-wslunregisterdistribution).
///
/// **Warning:** This permanently deletes the distribution and all its data.
pub fn unregister_distribution(name: &str) -> Result<()> {
    let name_w = to_wide(name)?;
    let hr = unsafe { RawWslUnregisterDistribution(name_w.as_ptr()) };
    check_hr(hr)
}

/// Configures the behavior flags for a registered WSL distribution.
///
/// Wraps [`WslConfigureDistribution`](
/// https://learn.microsoft.com/en-us/windows/win32/api/wslapi/nf-wslapi-wslconfiguredistribution).
pub fn configure_distribution(
    name: &str,
    default_uid: u32,
    flags: WSL_DISTRIBUTION_FLAGS,
) -> Result<()> {
    let name_w = to_wide(name)?;
    let hr = unsafe { RawWslConfigureDistribution(name_w.as_ptr(), default_uid, flags) };
    check_hr(hr)
}

/// Retrieves the current configuration of a registered WSL distribution.
///
/// Wraps [`WslGetDistributionConfiguration`](
/// https://learn.microsoft.com/en-us/windows/win32/api/wslapi/nf-wslapi-wslgetdistributionconfiguration).
pub fn get_distribution_configuration(name: &str) -> Result<DistributionConfig> {
    let name_w = to_wide(name)?;
    let mut version = 0u32;
    let mut default_uid = 0u32;
    let mut flags = WSL_DISTRIBUTION_FLAGS_NONE;
    let hr = unsafe {
        RawWslGetDistributionConfiguration(
            name_w.as_ptr(),
            &mut version,
            &mut default_uid,
            &mut flags,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    check_hr(hr)?;
    Ok(DistributionConfig { version, default_uid, flags })
}

/// Launches a process inside a WSL distribution and returns the process handle.
///
/// Wraps [`WslLaunch`](
/// https://learn.microsoft.com/en-us/windows/win32/api/wslapi/nf-wslapi-wsllaunch).
///
/// Pass `None` for `stdin`/`stdout`/`stderr` to inherit the calling process's handles.
///
/// Returns the process `HANDLE`. The caller must close it with `CloseHandle`.
pub fn launch(
    distribution: &str,
    command: Option<&str>,
    use_cwd: bool,
    stdin: Option<HANDLE>,
    stdout: Option<HANDLE>,
    stderr: Option<HANDLE>,
) -> Result<HANDLE> {
    let dist_w = to_wide(distribution)?;
    let cmd_w = command.map(|c| to_wide(c)).transpose()?;
    let cmd_ptr = cmd_w.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
    let mut process: HANDLE = std::ptr::null_mut();
    let hr = unsafe {
        RawWslLaunch(
            dist_w.as_ptr(),
            cmd_ptr,
            use_cwd as i32,
            stdin.unwrap_or(std::ptr::null_mut()),
            stdout.unwrap_or(std::ptr::null_mut()),
            stderr.unwrap_or(std::ptr::null_mut()),
            &mut process,
        )
    };
    check_hr(hr)?;
    Ok(process)
}

/// Launches an interactive process inside a WSL distribution and waits for it
/// to complete.
///
/// Wraps [`WslLaunchInteractive`](
/// https://learn.microsoft.com/en-us/windows/win32/api/wslapi/nf-wslapi-wsllaunchinteractive).
///
/// Returns the process exit code.
pub fn launch_interactive(distribution: &str, command: Option<&str>, use_cwd: bool) -> Result<u32> {
    let dist_w = to_wide(distribution)?;
    let cmd_w = command.map(|c| to_wide(c)).transpose()?;
    let cmd_ptr = cmd_w.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
    let mut exit_code = 0u32;
    let hr = unsafe {
        RawWslLaunchInteractive(dist_w.as_ptr(), cmd_ptr, use_cwd as i32, &mut exit_code)
    };
    check_hr(hr)?;
    Ok(exit_code)
}

// ---------------------------------------------------------------------------
// Raw bindings
// ---------------------------------------------------------------------------

/// Raw, unsafe WSL API functions from `windows-sys`.
///
/// Prefer the safe wrappers above unless you need full control over FFI details.
pub mod raw {
    pub use windows_sys::Win32::System::SubsystemForLinux::{
        WslConfigureDistribution, WslGetDistributionConfiguration, WslIsDistributionRegistered,
        WslLaunch, WslLaunchInteractive, WslRegisterDistribution, WslUnregisterDistribution,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bogus_distribution_is_not_registered() {
        let result = is_distribution_registered("__telos_nonexistent_distro_42__");
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }
}
