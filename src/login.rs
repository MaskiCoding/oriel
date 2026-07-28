//! Start-at-login via `SMAppService.mainAppService`.

use std::sync::Once;

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::{NSBundle, NSError, NSString};

const STATUS_ENABLED: isize = 1;

fn ensure_framework() {
    static LOAD: Once = Once::new();
    LOAD.call_once(|| {
        let path = NSString::from_str("/System/Library/Frameworks/ServiceManagement.framework");
        if let Some(bundle) = NSBundle::bundleWithPath(&path) {
            let _ = unsafe { bundle.load() };
        }
    });
}

fn main_app_service() -> Option<Retained<AnyObject>> {
    ensure_framework();
    let cls = AnyClass::get(c"SMAppService")?;
    let service: Option<Retained<AnyObject>> = unsafe { msg_send![cls, mainAppService] };
    service
}

/// `true` when the main-app login item is enabled (`SMAppServiceStatusEnabled`).
pub fn enabled() -> bool {
    let Some(service) = main_app_service() else {
        return false;
    };
    let status: isize = unsafe { msg_send![&*service, status] };
    status == STATUS_ENABLED
}

/// Register or unregister the main-app login item. Returns `true` if the request
/// was accepted (or the desired state was already in effect).
pub fn set_enabled(enabled: bool) -> bool {
    let Some(service) = main_app_service() else {
        return false;
    };
    let status: isize = unsafe { msg_send![&*service, status] };
    if (status == STATUS_ENABLED) == enabled {
        return true;
    }
    let result: Result<(), Retained<NSError>> = if enabled {
        unsafe { msg_send![&*service, registerAndReturnError: _] }
    } else {
        unsafe { msg_send![&*service, unregisterAndReturnError: _] }
    };
    result.is_ok()
}
