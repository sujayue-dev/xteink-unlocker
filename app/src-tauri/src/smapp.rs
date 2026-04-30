//! Thin SMAppService bindings. Registers the privileged helper LaunchDaemon
//! that ships inside our app bundle at
//! `Contents/Library/LaunchDaemons/com.sofriendly.crosspoint.unlocker.helper.plist`.
//!
//! macOS will prompt the user the first time we register, directing them to
//! Login Items in System Settings to approve the daemon. After approval the
//! daemon stays loaded across reboots.

#![cfg(target_os = "macos")]

use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::{NSError, NSString};

const HELPER_PLIST_NAME: &str = "com.sofriendly.crosspoint.unlocker.helper.plist";

#[derive(Debug, serde::Serialize)]
pub struct ServiceStatus {
    pub status: i64,
    pub status_label: &'static str,
}

fn status_label(s: i64) -> &'static str {
    // SMAppServiceStatus enum values per Apple docs.
    match s {
        0 => "not_registered",
        1 => "enabled",
        2 => "requires_approval",
        3 => "not_found",
        _ => "unknown",
    }
}

unsafe fn daemon_service() -> *mut AnyObject {
    let cls = class!(SMAppService);
    let plist = NSString::from_str(HELPER_PLIST_NAME);
    msg_send![cls, daemonServiceWithPlistName: &*plist]
}

pub fn register() -> Result<(), String> {
    unsafe {
        let service = daemon_service();
        if service.is_null() {
            return Err("SMAppService.daemonServiceWithPlistName returned nil".into());
        }
        let mut err: *mut NSError = std::ptr::null_mut();
        let ok: bool = msg_send![service, registerAndReturnError: &mut err];
        if ok {
            Ok(())
        } else if !err.is_null() {
            let desc: *mut NSString = msg_send![err, localizedDescription];
            let s = if desc.is_null() {
                "unknown error".to_string()
            } else {
                (*desc).to_string()
            };
            Err(s)
        } else {
            Err("registration failed (no error returned)".into())
        }
    }
}

pub fn unregister() -> Result<(), String> {
    unsafe {
        let service = daemon_service();
        if service.is_null() {
            return Err("nil service".into());
        }
        let mut err: *mut NSError = std::ptr::null_mut();
        let ok: bool = msg_send![service, unregisterAndReturnError: &mut err];
        if ok {
            Ok(())
        } else if !err.is_null() {
            let desc: *mut NSString = msg_send![err, localizedDescription];
            let s = if desc.is_null() {
                "unknown error".to_string()
            } else {
                (*desc).to_string()
            };
            Err(s)
        } else {
            Err("unregister failed".into())
        }
    }
}

pub fn status() -> ServiceStatus {
    unsafe {
        let service = daemon_service();
        if service.is_null() {
            return ServiceStatus {
                status: 0,
                status_label: "not_registered",
            };
        }
        let s: i64 = msg_send![service, status];
        ServiceStatus {
            status: s,
            status_label: status_label(s),
        }
    }
}
