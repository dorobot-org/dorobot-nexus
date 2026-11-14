//! Desktop appearance, queried on the UI thread.
#[cfg(target_os = "macos")]
pub fn system_light() -> f32 {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    unsafe {
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let appearance: *mut Object = msg_send![app, effectiveAppearance];
        let light: *mut Object =
            msg_send![class!(NSString),stringWithUTF8String:b"NSAppearanceNameAqua\0".as_ptr()];
        let dark: *mut Object =
            msg_send![class!(NSString),stringWithUTF8String:b"NSAppearanceNameDarkAqua\0".as_ptr()];
        let names = [light, dark];
        let array: *mut Object =
            msg_send![class!(NSArray),arrayWithObjects:names.as_ptr() count:2_usize];
        let matched: *mut Object = msg_send![appearance,bestMatchFromAppearancesWithNames:array];
        let is_light: bool = msg_send![matched,isEqualToString:light];
        if is_light {
            1.0
        } else {
            0.0
        }
    }
}
#[cfg(not(target_os = "macos"))]
pub fn system_light() -> f32 {
    0.0
}
