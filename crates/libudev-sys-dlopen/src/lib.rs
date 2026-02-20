//! Drop-in replacement for `libudev-sys` that loads `libudev.so.1` at runtime
//! via `dlopen` instead of linking at compile time.
//!
//! If the library is not present on the system, all functions return safe
//! failure values (null pointers / zero / negative error codes).  This lets
//! binaries start and run normally on systems without libudev — gamepad
//! support simply won't be available.
//!
//! Type definitions and function signatures are derived from libudev-sys
//! (Copyright 2015 David Cuddeback, MIT license).  See LICENSE for details.

#![allow(non_camel_case_types)]

use std::sync::OnceLock;

use libc::{c_char, c_int, c_ulonglong, c_void, dev_t, size_t};
use libloading::Library;

// ---------------------------------------------------------------------------
// Opaque types — identical to the upstream libudev-sys crate
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct udev {
    __private: c_void,
}
#[repr(C)]
pub struct udev_list_entry {
    __private: c_void,
}
#[repr(C)]
pub struct udev_device {
    __private: c_void,
}
#[repr(C)]
pub struct udev_monitor {
    __private: c_void,
}
#[repr(C)]
pub struct udev_enumerate {
    __private: c_void,
}
#[repr(C)]
pub struct udev_queue {
    __private: c_void,
}

// ---------------------------------------------------------------------------
// Macro to declare the function-pointer table + thin wrapper functions
// ---------------------------------------------------------------------------

/// Each invocation declares:
/// 1. A field in `Lib` holding the resolved function pointer.
/// 2. A public `unsafe fn` with the same signature that delegates to the
///    dlopen'd symbol (or returns a safe default when libudev is absent).
macro_rules! udev_api {
    ($(
        pub fn $name:ident( $($arg:ident : $argty:ty),* $(,)? ) $( -> $ret:ty )?;
    )*) => {

        // -- function-pointer table -------------------------------------------

        struct Lib {
            _handle: Library,
            $( $name: unsafe extern "C" fn( $($argty),* ) $( -> $ret )?, )*
        }

        impl Lib {
            /// Try to load libudev and resolve every symbol we need.
            /// Returns `None` if the library or any symbol is missing.
            unsafe fn load() -> Option<Self> {
                let handle = Library::new("libudev.so.1").ok()?;
                $(
                    let $name = *handle
                        .get::<unsafe extern "C" fn( $($argty),* ) $( -> $ret )?>(
                            concat!(stringify!($name), "\0").as_bytes(),
                        )
                        .ok()?;
                )*
                Some(Lib { _handle: handle, $( $name, )* })
            }
        }

        // -- public wrapper functions -----------------------------------------

        $(
            #[inline]
            pub unsafe fn $name( $($arg : $argty),* ) $( -> $ret )? {
                match lib() {
                    Some(l) => (l.$name)( $($arg),* ),
                    None => udev_api!(@zeroed $( $ret )? ),
                }
            }
        )*
    };

    // Default return values when libudev is not loaded.
    // Void functions: return unit.
    (@zeroed) => { () };
    // All other returns (pointers → null, integers → 0): safe to zero.
    (@zeroed $_ret:ty) => { std::mem::zeroed() };
}

// ---------------------------------------------------------------------------
// Singleton loader
// ---------------------------------------------------------------------------

static LIB: OnceLock<Option<Lib>> = OnceLock::new();

fn lib() -> Option<&'static Lib> {
    LIB.get_or_init(|| unsafe { Lib::load() }).as_ref()
}

// ---------------------------------------------------------------------------
// API surface — matches libudev-sys 0.1.4 exactly (minus hwdb)
// ---------------------------------------------------------------------------

udev_api! {
    // udev context
    pub fn udev_new() -> *mut udev;
    pub fn udev_ref(udev: *mut udev) -> *mut udev;
    pub fn udev_unref(udev: *mut udev) -> *mut udev;
    pub fn udev_set_userdata(udev: *mut udev, userdata: *mut c_void);
    pub fn udev_get_userdata(udev: *mut udev) -> *mut c_void;

    // udev_list
    pub fn udev_list_entry_get_next(list_entry: *mut udev_list_entry) -> *mut udev_list_entry;
    pub fn udev_list_entry_get_by_name(list_entry: *mut udev_list_entry, name: *const c_char) -> *mut udev_list_entry;
    pub fn udev_list_entry_get_name(list_entry: *mut udev_list_entry) -> *const c_char;
    pub fn udev_list_entry_get_value(list_entry: *mut udev_list_entry) -> *const c_char;

    // udev_device
    pub fn udev_device_ref(udev_device: *mut udev_device) -> *mut udev_device;
    pub fn udev_device_unref(udev_device: *mut udev_device) -> *mut udev_device;
    pub fn udev_device_get_udev(udev_device: *mut udev_device) -> *mut udev;
    pub fn udev_device_new_from_syspath(udev: *mut udev, syspath: *const c_char) -> *mut udev_device;
    pub fn udev_device_new_from_devnum(udev: *mut udev, dev_type: c_char, devnum: dev_t) -> *mut udev_device;
    pub fn udev_device_new_from_subsystem_sysname(udev: *mut udev, subsystem: *const c_char, sysname: *const c_char) -> *mut udev_device;
    pub fn udev_device_new_from_device_id(udev: *mut udev, id: *const c_char) -> *mut udev_device;
    pub fn udev_device_new_from_environment(udev: *mut udev) -> *mut udev_device;
    pub fn udev_device_get_parent(udev_device: *mut udev_device) -> *mut udev_device;
    pub fn udev_device_get_parent_with_subsystem_devtype(udev_device: *mut udev_device, subsystem: *const c_char, devtype: *const c_char) -> *mut udev_device;
    pub fn udev_device_get_devpath(udev_device: *mut udev_device) -> *const c_char;
    pub fn udev_device_get_subsystem(udev_device: *mut udev_device) -> *const c_char;
    pub fn udev_device_get_devtype(udev_device: *mut udev_device) -> *const c_char;
    pub fn udev_device_get_syspath(udev_device: *mut udev_device) -> *const c_char;
    pub fn udev_device_get_sysname(udev_device: *mut udev_device) -> *const c_char;
    pub fn udev_device_get_sysnum(udev_device: *mut udev_device) -> *const c_char;
    pub fn udev_device_get_devnode(udev_device: *mut udev_device) -> *const c_char;
    pub fn udev_device_get_is_initialized(udev_device: *mut udev_device) -> c_int;
    pub fn udev_device_get_devlinks_list_entry(udev_device: *mut udev_device) -> *mut udev_list_entry;
    pub fn udev_device_get_properties_list_entry(udev_device: *mut udev_device) -> *mut udev_list_entry;
    pub fn udev_device_get_tags_list_entry(udev_device: *mut udev_device) -> *mut udev_list_entry;
    pub fn udev_device_get_property_value(udev_device: *mut udev_device, key: *const c_char) -> *const c_char;
    pub fn udev_device_get_driver(udev_device: *mut udev_device) -> *const c_char;
    pub fn udev_device_get_devnum(udev_device: *mut udev_device) -> dev_t;
    pub fn udev_device_get_action(udev_device: *mut udev_device) -> *const c_char;
    pub fn udev_device_get_sysattr_value(udev_device: *mut udev_device, sysattr: *const c_char) -> *const c_char;
    pub fn udev_device_set_sysattr_value(udev_device: *mut udev_device, sysattr: *const c_char, value: *mut c_char) -> c_int;
    pub fn udev_device_get_sysattr_list_entry(udev_device: *mut udev_device) -> *mut udev_list_entry;
    pub fn udev_device_get_seqnum(udev_device: *mut udev_device) -> c_ulonglong;
    pub fn udev_device_get_usec_since_initialized(udev_device: *mut udev_device) -> c_ulonglong;
    pub fn udev_device_has_tag(udev_device: *mut udev_device, tag: *const c_char) -> c_int;

    // udev_monitor
    pub fn udev_monitor_ref(udev_monitor: *mut udev_monitor) -> *mut udev_monitor;
    pub fn udev_monitor_unref(udev_monitor: *mut udev_monitor) -> *mut udev_monitor;
    pub fn udev_monitor_get_udev(udev_monitor: *mut udev_monitor) -> *mut udev;
    pub fn udev_monitor_new_from_netlink(udev: *mut udev, name: *const c_char) -> *mut udev_monitor;
    pub fn udev_monitor_enable_receiving(udev_monitor: *mut udev_monitor) -> c_int;
    pub fn udev_monitor_set_receive_buffer_size(udev_monitor: *mut udev_monitor, size: c_int) -> c_int;
    pub fn udev_monitor_get_fd(udev_monitor: *mut udev_monitor) -> c_int;
    pub fn udev_monitor_receive_device(udev_monitor: *mut udev_monitor) -> *mut udev_device;
    pub fn udev_monitor_filter_add_match_subsystem_devtype(udev_monitor: *mut udev_monitor, subsystem: *const c_char, devtype: *const c_char) -> c_int;
    pub fn udev_monitor_filter_add_match_tag(udev_monitor: *mut udev_monitor, tag: *const c_char) -> c_int;
    pub fn udev_monitor_filter_update(udev_monitor: *mut udev_monitor) -> c_int;
    pub fn udev_monitor_filter_remove(udev_monitor: *mut udev_monitor) -> c_int;

    // udev_enumerate
    pub fn udev_enumerate_ref(udev_enumerate: *mut udev_enumerate) -> *mut udev_enumerate;
    pub fn udev_enumerate_unref(udev_enumerate: *mut udev_enumerate) -> *mut udev_enumerate;
    pub fn udev_enumerate_get_udev(udev_enumerate: *mut udev_enumerate) -> *mut udev;
    pub fn udev_enumerate_new(udev: *mut udev) -> *mut udev_enumerate;
    pub fn udev_enumerate_add_match_subsystem(udev_enumerate: *mut udev_enumerate, subsystem: *const c_char) -> c_int;
    pub fn udev_enumerate_add_nomatch_subsystem(udev_enumerate: *mut udev_enumerate, subsystem: *const c_char) -> c_int;
    pub fn udev_enumerate_add_match_sysattr(udev_enumerate: *mut udev_enumerate, sysattr: *const c_char, value: *const c_char) -> c_int;
    pub fn udev_enumerate_add_nomatch_sysattr(udev_enumerate: *mut udev_enumerate, sysattr: *const c_char, value: *const c_char) -> c_int;
    pub fn udev_enumerate_add_match_property(udev_enumerate: *mut udev_enumerate, property: *const c_char, value: *const c_char) -> c_int;
    pub fn udev_enumerate_add_match_tag(udev_enumerate: *mut udev_enumerate, tag: *const c_char) -> c_int;
    pub fn udev_enumerate_add_match_parent(udev_enumerate: *mut udev_enumerate, parent: *mut udev_device) -> c_int;
    pub fn udev_enumerate_add_match_is_initialized(udev_enumerate: *mut udev_enumerate) -> c_int;
    pub fn udev_enumerate_add_match_sysname(udev_enumerate: *mut udev_enumerate, sysname: *const c_char) -> c_int;
    pub fn udev_enumerate_add_syspath(udev_enumerate: *mut udev_enumerate, syspath: *const c_char) -> c_int;
    pub fn udev_enumerate_scan_devices(udev_enumerate: *mut udev_enumerate) -> c_int;
    pub fn udev_enumerate_scan_subsystems(udev_enumerate: *mut udev_enumerate) -> c_int;
    pub fn udev_enumerate_get_list_entry(udev_enumerate: *mut udev_enumerate) -> *mut udev_list_entry;

    // udev_queue
    pub fn udev_queue_ref(udev_queue: *mut udev_queue) -> *mut udev_queue;
    pub fn udev_queue_unref(udev_queue: *mut udev_queue) -> *mut udev_queue;
    pub fn udev_queue_get_udev(udev_queue: *mut udev_queue) -> *mut udev;
    pub fn udev_queue_new(udev: *mut udev) -> *mut udev_queue;
    pub fn udev_queue_get_udev_is_active(udev_queue: *mut udev_queue) -> c_int;
    pub fn udev_queue_get_queue_is_empty(udev_queue: *mut udev_queue) -> c_int;
    pub fn udev_queue_get_fd(udev_queue: *mut udev_queue) -> c_int;
    pub fn udev_queue_flush(udev_queue: *mut udev_queue) -> c_int;

    // udev_util
    pub fn udev_util_encode_string(str: *const c_char, str_enc: *mut c_char, len: size_t) -> c_int;
}
