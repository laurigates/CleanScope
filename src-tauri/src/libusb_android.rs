//! Android-specific libusb integration
//!
//! This module provides safe wrappers around the libusb FFI bindings
//! needed for Android USB camera access via file descriptors from
//! Android's UsbManager.
//!
//! On Android, we cannot enumerate USB devices directly due to SELinux
//! policies. Instead, we:
//! 1. Get a file descriptor from Android's UsbManager via JNI
//! 2. Use `libusb_wrap_sys_device()` to create a libusb device handle
//! 3. Perform USB transfers on that handle
//!
//! For video streaming from UVC cameras, we use asynchronous isochronous
//! transfers which provide guaranteed bandwidth for real-time video data.

use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// libusb error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum LibusbError {
    Success = 0,
    IoError = -1,
    InvalidParam = -2,
    Access = -3,
    NoDevice = -4,
    NotFound = -5,
    Busy = -6,
    Timeout = -7,
    Overflow = -8,
    Pipe = -9,
    Interrupted = -10,
    NoMem = -11,
    NotSupported = -12,
    Other = -99,
}

impl From<i32> for LibusbError {
    fn from(code: i32) -> Self {
        match code {
            0 => LibusbError::Success,
            -1 => LibusbError::IoError,
            -2 => LibusbError::InvalidParam,
            -3 => LibusbError::Access,
            -4 => LibusbError::NoDevice,
            -5 => LibusbError::NotFound,
            -6 => LibusbError::Busy,
            -7 => LibusbError::Timeout,
            -8 => LibusbError::Overflow,
            -9 => LibusbError::Pipe,
            -10 => LibusbError::Interrupted,
            -11 => LibusbError::NoMem,
            -12 => LibusbError::NotSupported,
            _ => LibusbError::Other,
        }
    }
}

impl std::fmt::Display for LibusbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LibusbError::Success => write!(f, "Success"),
            LibusbError::IoError => write!(f, "I/O error"),
            LibusbError::InvalidParam => write!(f, "Invalid parameter"),
            LibusbError::Access => write!(f, "Access denied"),
            LibusbError::NoDevice => write!(f, "No such device"),
            LibusbError::NotFound => write!(f, "Entity not found"),
            LibusbError::Busy => write!(f, "Resource busy"),
            LibusbError::Timeout => write!(f, "Operation timed out"),
            LibusbError::Overflow => write!(f, "Overflow"),
            LibusbError::Pipe => write!(f, "Pipe error"),
            LibusbError::Interrupted => write!(f, "System call interrupted"),
            LibusbError::NoMem => write!(f, "Insufficient memory"),
            LibusbError::NotSupported => write!(f, "Operation not supported"),
            LibusbError::Other => write!(f, "Other error"),
        }
    }
}

impl std::error::Error for LibusbError {}

/// libusb option for disabling device discovery (needed for Android)
const LIBUSB_OPTION_NO_DEVICE_DISCOVERY: u32 = 2;

/// Wrapper to allow sending a libusb context pointer across threads
///
/// # Safety
/// The libusb context is thread-safe when used with the async event handling API.
/// The caller must ensure the context outlives any threads using this pointer.
pub struct SendableContextPtr {
    ptr: usize,
}

impl SendableContextPtr {
    /// Create a new sendable context pointer
    pub fn new(ptr: *mut libusb1_sys::libusb_context) -> Self {
        Self { ptr: ptr as usize }
    }

    /// Get the raw pointer
    pub fn as_ptr(&self) -> *mut libusb1_sys::libusb_context {
        self.ptr as *mut _
    }
}

// SAFETY: The libusb context is thread-safe for event handling
unsafe impl Send for SendableContextPtr {}
unsafe impl Sync for SendableContextPtr {}

/// libusb transfer status codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum TransferStatus {
    /// Transfer completed without error
    Completed = 0,
    /// Transfer failed due to I/O error
    Error = 1,
    /// Transfer timed out
    TimedOut = 2,
    /// Transfer was cancelled
    Cancelled = 3,
    /// Endpoint stalled
    Stall = 4,
    /// Device disconnected
    NoDevice = 5,
    /// Data overflow (device sent more data than requested)
    Overflow = 6,
}

impl From<i32> for TransferStatus {
    fn from(status: i32) -> Self {
        match status {
            0 => TransferStatus::Completed,
            1 => TransferStatus::Error,
            2 => TransferStatus::TimedOut,
            3 => TransferStatus::Cancelled,
            4 => TransferStatus::Stall,
            5 => TransferStatus::NoDevice,
            6 => TransferStatus::Overflow,
            _ => TransferStatus::Error,
        }
    }
}

/// libusb transfer type constants
pub mod transfer_type {
    pub const CONTROL: u8 = 0;
    pub const ISOCHRONOUS: u8 = 1;
    pub const BULK: u8 = 2;
    pub const INTERRUPT: u8 = 3;
}

/// USB transfer types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransferType {
    Control = 0,
    Isochronous = 1,
    Bulk = 2,
    Interrupt = 3,
}

impl TransferType {
    /// Convert from raw u8 value
    pub fn from_u8(value: u8) -> Self {
        match value & 0x03 {
            0 => TransferType::Control,
            1 => TransferType::Isochronous,
            2 => TransferType::Bulk,
            3 => TransferType::Interrupt,
            _ => unreachable!(),
        }
    }
}

/// Information about a USB endpoint for streaming
#[derive(Debug, Clone)]
pub struct EndpointInfo {
    /// Endpoint address (includes direction bit)
    pub address: u8,
    /// Transfer type (Isochronous, Bulk, etc.)
    pub transfer_type: TransferType,
    /// Maximum packet size in bytes
    pub max_packet_size: u16,
    /// Transactions per microframe (1-3 for high-speed isochronous)
    pub transactions_per_microframe: u16,
    /// Interface number this endpoint belongs to
    pub interface_number: u8,
    /// Alternate setting that enables this endpoint
    pub alt_setting: u8,
}

/// Wrapper around libusb context
pub struct LibusbContext {
    ctx: *mut libusb1_sys::libusb_context,
}

// SAFETY: The libusb context is thread-safe when used properly
unsafe impl Send for LibusbContext {}
unsafe impl Sync for LibusbContext {}

impl LibusbContext {
    /// Create a new libusb context configured for Android (no device discovery)
    pub fn new_android() -> Result<Self, LibusbError> {
        unsafe {
            // Set the no-discovery option before init
            // This is required on Android where we can't enumerate devices
            let ret =
                libusb1_sys::libusb_set_option(ptr::null_mut(), LIBUSB_OPTION_NO_DEVICE_DISCOVERY);
            if ret < 0 {
                log::error!("libusb_set_option failed: {}", ret);
                return Err(LibusbError::from(ret));
            }

            let mut ctx: *mut libusb1_sys::libusb_context = ptr::null_mut();
            let ret = libusb1_sys::libusb_init(&mut ctx);
            if ret < 0 {
                log::error!("libusb_init failed: {}", ret);
                return Err(LibusbError::from(ret));
            }

            log::info!("libusb context initialized for Android");
            Ok(LibusbContext { ctx })
        }
    }

    /// Get the raw libusb context pointer
    ///
    /// # Safety
    /// The caller must ensure the pointer is not used after the context is dropped.
    pub fn get_context_ptr(&self) -> *mut libusb1_sys::libusb_context {
        self.ctx
    }

    /// Wrap an Android file descriptor as a libusb device handle
    ///
    /// # Note on FFI
    /// The libusb1-sys crate incorrectly declares `libusb_wrap_sys_device` with
    /// `sys_dev: *mut c_int` but the actual libusb C library expects `intptr_t`.
    /// On Android/Linux, the file descriptor should be passed as `(intptr_t)fd`,
    /// not as a pointer to the fd. We work around this by casting the fd value
    /// to `isize` (Rust's `intptr_t` equivalent) and transmuting the pointer.
    ///
    /// See: <https://github.com/libusb/libusb/blob/master/android/examples/unrooted_android.c>
    pub fn wrap_fd(&self, fd: i32) -> Result<LibusbDeviceHandle, LibusbError> {
        unsafe {
            let mut dev_handle: *mut libusb1_sys::libusb_device_handle = ptr::null_mut();

            // The libusb C API expects: libusb_wrap_sys_device(ctx, (intptr_t)fd, &handle)
            // The fd value itself (not a pointer to it) should be passed as intptr_t.
            // Since libusb1-sys incorrectly declares sys_dev as *mut c_int, we need to
            // transmute our isize (which is intptr_t in Rust) to the expected pointer type.
            let fd_as_intptr: isize = fd as isize;
            let ret = libusb1_sys::libusb_wrap_sys_device(
                self.ctx,
                fd_as_intptr as *mut libc::c_int,
                &mut dev_handle,
            );

            if ret < 0 {
                log::error!(
                    "libusb_wrap_sys_device failed with error {}: {}",
                    ret,
                    LibusbError::from(ret)
                );
                return Err(LibusbError::from(ret));
            }

            if dev_handle.is_null() {
                log::error!("libusb_wrap_sys_device returned null handle");
                return Err(LibusbError::NoDevice);
            }

            log::info!("Successfully wrapped Android FD {} as libusb device", fd);
            Ok(LibusbDeviceHandle { handle: dev_handle })
        }
    }
}

impl Drop for LibusbContext {
    fn drop(&mut self) {
        unsafe {
            if !self.ctx.is_null() {
                libusb1_sys::libusb_exit(self.ctx);
            }
        }
    }
}

/// Wrapper around libusb device handle
pub struct LibusbDeviceHandle {
    handle: *mut libusb1_sys::libusb_device_handle,
}

// SAFETY: The device handle is thread-safe when used properly
unsafe impl Send for LibusbDeviceHandle {}
unsafe impl Sync for LibusbDeviceHandle {}

impl LibusbDeviceHandle {
    /// Get the raw libusb device handle pointer
    ///
    /// # Safety
    /// The caller must ensure the pointer is not used after the handle is dropped.
    pub fn get_handle_ptr(&self) -> *mut libusb1_sys::libusb_device_handle {
        self.handle
    }

    /// Detach kernel driver from an interface if one is attached
    pub fn detach_kernel_driver(&self, interface_number: i32) -> Result<bool, LibusbError> {
        unsafe {
            // Check if a kernel driver is active
            let active = libusb1_sys::libusb_kernel_driver_active(self.handle, interface_number);
            if active == 1 {
                log::info!(
                    "Kernel driver active on interface {}, detaching...",
                    interface_number
                );
                let ret = libusb1_sys::libusb_detach_kernel_driver(self.handle, interface_number);
                if ret < 0 {
                    log::error!("libusb_detach_kernel_driver failed: {}", ret);
                    return Err(LibusbError::from(ret));
                }
                log::info!("Kernel driver detached from interface {}", interface_number);
                return Ok(true);
            } else if active < 0 {
                // Error checking - but LIBUSB_ERROR_NOT_SUPPORTED (-12) is common on Android
                if active == -12 {
                    log::debug!("Kernel driver check not supported on this platform");
                    return Ok(false);
                }
                log::warn!("libusb_kernel_driver_active returned error: {}", active);
            }
            Ok(false)
        }
    }

    /// Claim an interface on the device, detaching kernel driver if necessary
    pub fn claim_interface(&self, interface_number: i32) -> Result<(), LibusbError> {
        unsafe {
            // Try to detach kernel driver first (ignore errors - may not be supported)
            let _ = self.detach_kernel_driver(interface_number);

            let ret = libusb1_sys::libusb_claim_interface(self.handle, interface_number);
            if ret < 0 {
                log::error!("libusb_claim_interface failed: {}", ret);
                return Err(LibusbError::from(ret));
            }
            log::info!("Claimed interface {}", interface_number);
            Ok(())
        }
    }

    /// Release an interface
    pub fn release_interface(&self, interface_number: i32) -> Result<(), LibusbError> {
        unsafe {
            let ret = libusb1_sys::libusb_release_interface(self.handle, interface_number);
            if ret < 0 {
                log::error!("libusb_release_interface failed: {}", ret);
                return Err(LibusbError::from(ret));
            }
            Ok(())
        }
    }

    /// Set the alternate setting for an interface
    ///
    /// For UVC devices, alternate setting 0 typically has zero bandwidth (no streaming),
    /// while alternate setting 1+ enables the streaming endpoint with actual bandwidth.
    pub fn set_interface_alt_setting(
        &self,
        interface_number: i32,
        alt_setting: i32,
    ) -> Result<(), LibusbError> {
        unsafe {
            let ret = libusb1_sys::libusb_set_interface_alt_setting(
                self.handle,
                interface_number,
                alt_setting,
            );
            if ret < 0 {
                log::error!(
                    "libusb_set_interface_alt_setting failed for interface {} alt {}: {}",
                    interface_number,
                    alt_setting,
                    ret
                );
                return Err(LibusbError::from(ret));
            }
            log::info!(
                "Set interface {} to alternate setting {}",
                interface_number,
                alt_setting
            );
            Ok(())
        }
    }

    /// Perform a control transfer
    ///
    /// # Arguments
    /// * `request_type` - Request type (direction | type | recipient)
    /// * `request` - Request code
    /// * `value` - Value for the request
    /// * `index` - Index for the request
    /// * `data` - Data buffer for the transfer
    /// * `timeout_ms` - Timeout in milliseconds
    pub fn control_transfer(
        &self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: &mut [u8],
        timeout_ms: u32,
    ) -> Result<usize, LibusbError> {
        unsafe {
            let ret = libusb1_sys::libusb_control_transfer(
                self.handle,
                request_type,
                request,
                value,
                index,
                data.as_mut_ptr(),
                data.len() as u16,
                timeout_ms,
            );

            if ret < 0 {
                return Err(LibusbError::from(ret));
            }
            Ok(ret as usize)
        }
    }

    /// Perform a bulk transfer
    ///
    /// # Arguments
    /// * `endpoint` - Endpoint address (with direction bit)
    /// * `data` - Data buffer for the transfer
    /// * `timeout_ms` - Timeout in milliseconds
    ///
    /// # Returns
    /// Number of bytes transferred
    pub fn bulk_transfer(
        &self,
        endpoint: u8,
        data: &mut [u8],
        timeout_ms: u32,
    ) -> Result<usize, LibusbError> {
        unsafe {
            let mut transferred: i32 = 0;
            let ret = libusb1_sys::libusb_bulk_transfer(
                self.handle,
                endpoint,
                data.as_mut_ptr(),
                data.len() as i32,
                &mut transferred,
                timeout_ms,
            );

            if ret < 0 {
                return Err(LibusbError::from(ret));
            }
            Ok(transferred as usize)
        }
    }

    /// Get the device associated with this handle
    pub fn get_device(&self) -> *mut libusb1_sys::libusb_device {
        unsafe { libusb1_sys::libusb_get_device(self.handle) }
    }

    /// Get the device descriptor
    pub fn get_device_descriptor(&self) -> Result<DeviceDescriptor, LibusbError> {
        unsafe {
            let device = self.get_device();
            let mut desc = std::mem::zeroed::<libusb1_sys::libusb_device_descriptor>();
            let ret = libusb1_sys::libusb_get_device_descriptor(device, &mut desc);
            if ret < 0 {
                return Err(LibusbError::from(ret));
            }
            Ok(DeviceDescriptor {
                vendor_id: desc.idVendor,
                product_id: desc.idProduct,
                device_class: desc.bDeviceClass,
                device_subclass: desc.bDeviceSubClass,
                device_protocol: desc.bDeviceProtocol,
                num_configurations: desc.bNumConfigurations,
            })
        }
    }

    /// Enumerate and log all endpoint descriptors for the device.
    /// Returns the streaming endpoint info if found (endpoint address, transfer type, max packet size).
    pub fn find_streaming_endpoint(&self) -> Result<Option<EndpointInfo>, LibusbError> {
        unsafe {
            let device = self.get_device();
            let mut cfg_desc: *const libusb1_sys::libusb_config_descriptor = std::ptr::null();

            let ret = libusb1_sys::libusb_get_active_config_descriptor(device, &mut cfg_desc);
            if ret < 0 {
                log::error!("Failed to get config descriptor: {}", ret);
                return Err(LibusbError::from(ret));
            }

            let cfg = &*cfg_desc;
            log::info!(
                "Config descriptor: {} interfaces, config value {}",
                cfg.bNumInterfaces,
                cfg.bConfigurationValue
            );

            let mut streaming_endpoint: Option<EndpointInfo> = None;

            // Iterate through interfaces
            for i in 0..cfg.bNumInterfaces as usize {
                let interface = &*cfg.interface.add(i);

                // Iterate through alternate settings
                for j in 0..interface.num_altsetting as usize {
                    let altsetting = &*interface.altsetting.add(j);

                    let is_video_class = altsetting.bInterfaceClass == 0x0E; // USB_CLASS_VIDEO
                    let is_streaming = altsetting.bInterfaceSubClass == 0x02; // VIDEO_STREAMING

                    log::info!(
                        "Interface {}.{}: class={:02x} subclass={:02x} protocol={:02x} endpoints={}{}",
                        altsetting.bInterfaceNumber,
                        altsetting.bAlternateSetting,
                        altsetting.bInterfaceClass,
                        altsetting.bInterfaceSubClass,
                        altsetting.bInterfaceProtocol,
                        altsetting.bNumEndpoints,
                        if is_video_class && is_streaming { " [VIDEO STREAMING]" } else { "" }
                    );

                    // Parse UVC format descriptors from class-specific extra bytes
                    if is_video_class && is_streaming && altsetting.extra_length > 0 {
                        let extra_bytes = std::slice::from_raw_parts(
                            altsetting.extra,
                            altsetting.extra_length as usize,
                        );
                        let formats = uvc::parse_format_descriptors(extra_bytes);
                        for fmt in &formats {
                            log::info!("  Available format: {:?}", fmt);
                        }
                    }

                    // Iterate through endpoints
                    for k in 0..altsetting.bNumEndpoints as usize {
                        let ep = &*altsetting.endpoint.add(k);

                        let ep_addr = ep.bEndpointAddress;
                        let ep_attr = ep.bmAttributes;
                        let transfer_type = ep_attr & 0x03;
                        let transfer_type_str = match transfer_type {
                            0 => "Control",
                            1 => "Isochronous",
                            2 => "Bulk",
                            3 => "Interrupt",
                            _ => "Unknown",
                        };

                        // For isochronous, extract sync type and usage type
                        let (sync_type, usage_type) = if transfer_type == 1 {
                            let sync = (ep_attr >> 2) & 0x03;
                            let usage = (ep_attr >> 4) & 0x03;
                            let sync_str = match sync {
                                0 => "None",
                                1 => "Async",
                                2 => "Adaptive",
                                3 => "Sync",
                                _ => "?",
                            };
                            let usage_str = match usage {
                                0 => "Data",
                                1 => "Feedback",
                                2 => "Implicit",
                                _ => "?",
                            };
                            (sync_str, usage_str)
                        } else {
                            ("N/A", "N/A")
                        };

                        // Extract max packet size (bits 0-10) and transactions per microframe (bits 11-12)
                        let max_packet_size = ep.wMaxPacketSize & 0x7FF;
                        let transactions = ((ep.wMaxPacketSize >> 11) & 0x03) + 1;

                        log::info!(
                            "  Endpoint 0x{:02x}: {} maxPacket={} x{} interval={}{}",
                            ep_addr,
                            transfer_type_str,
                            max_packet_size,
                            transactions,
                            ep.bInterval,
                            if transfer_type == 1 {
                                format!(" sync={} usage={}", sync_type, usage_type)
                            } else {
                                String::new()
                            }
                        );

                        // Look for video streaming IN endpoint
                        if is_video_class
                            && is_streaming
                            && (ep_addr & 0x80) != 0  // IN endpoint
                            && altsetting.bAlternateSetting > 0
                        // Non-zero bandwidth alt setting
                        {
                            let info = EndpointInfo {
                                address: ep_addr,
                                transfer_type: TransferType::from_u8(transfer_type),
                                max_packet_size,
                                transactions_per_microframe: transactions,
                                interface_number: altsetting.bInterfaceNumber,
                                alt_setting: altsetting.bAlternateSetting,
                            };

                            log::info!(
                                "  >>> Found streaming endpoint: 0x{:02x} ({}) on interface {}.{}",
                                ep_addr,
                                transfer_type_str,
                                altsetting.bInterfaceNumber,
                                altsetting.bAlternateSetting
                            );

                            // Prefer isochronous if available, otherwise take bulk
                            if streaming_endpoint.is_none()
                                || matches!(info.transfer_type, TransferType::Isochronous)
                            {
                                streaming_endpoint = Some(info);
                            }
                        }
                    }
                }
            }

            // libusb_free_config_descriptor expects *mut but we have *const
            // This is safe because we're freeing the descriptor we just got
            libusb1_sys::libusb_free_config_descriptor(cfg_desc as *mut _);

            Ok(streaming_endpoint)
        }
    }

    /// Get UVC format descriptors from the device.
    /// Returns a list of all formats and their frame descriptors (resolutions).
    pub fn get_format_descriptors(&self) -> Result<Vec<uvc::UvcFormatInfo>, LibusbError> {
        unsafe {
            let device = self.get_device();
            let mut cfg_desc: *const libusb1_sys::libusb_config_descriptor = std::ptr::null();

            let ret = libusb1_sys::libusb_get_active_config_descriptor(device, &mut cfg_desc);
            if ret < 0 {
                return Err(LibusbError::from(ret));
            }

            let cfg = &*cfg_desc;
            let mut all_formats = Vec::new();

            // Iterate through interfaces looking for video streaming interface
            for i in 0..cfg.bNumInterfaces as usize {
                let interface = &*cfg.interface.add(i);

                for j in 0..interface.num_altsetting as usize {
                    let altsetting = &*interface.altsetting.add(j);

                    let is_video_class = altsetting.bInterfaceClass == 0x0E;
                    let is_streaming = altsetting.bInterfaceSubClass == 0x02;

                    if is_video_class && is_streaming && altsetting.extra_length > 0 {
                        let extra_bytes = std::slice::from_raw_parts(
                            altsetting.extra,
                            altsetting.extra_length as usize,
                        );
                        let formats = uvc::parse_format_descriptors(extra_bytes);
                        all_formats.extend(formats);
                        // Only parse from first matching interface
                        if !all_formats.is_empty() {
                            break;
                        }
                    }
                }
                if !all_formats.is_empty() {
                    break;
                }
            }

            libusb1_sys::libusb_free_config_descriptor(cfg_desc as *mut _);
            Ok(all_formats)
        }
    }
}

impl Drop for LibusbDeviceHandle {
    fn drop(&mut self) {
        unsafe {
            if !self.handle.is_null() {
                libusb1_sys::libusb_close(self.handle);
            }
        }
    }
}

/// USB device descriptor information
#[derive(Debug, Clone)]
pub struct DeviceDescriptor {
    pub vendor_id: u16,
    pub product_id: u16,
    pub device_class: u8,
    pub device_subclass: u8,
    pub device_protocol: u8,
    pub num_configurations: u8,
}

/// UVC Video Class constants
pub mod uvc {
    /// UVC class code
    pub const USB_CLASS_VIDEO: u8 = 0x0E;

    /// UVC subclass codes
    pub const UVC_SC_UNDEFINED: u8 = 0x00;
    pub const UVC_SC_VIDEOCONTROL: u8 = 0x01;
    pub const UVC_SC_VIDEOSTREAMING: u8 = 0x02;

    /// UVC interface protocol
    pub const UVC_PC_PROTOCOL_UNDEFINED: u8 = 0x00;

    /// UVC request codes
    pub const UVC_SET_CUR: u8 = 0x01;
    pub const UVC_GET_CUR: u8 = 0x81;
    pub const UVC_GET_MIN: u8 = 0x82;
    pub const UVC_GET_MAX: u8 = 0x83;
    pub const UVC_GET_RES: u8 = 0x84;
    pub const UVC_GET_LEN: u8 = 0x85;
    pub const UVC_GET_INFO: u8 = 0x86;
    pub const UVC_GET_DEF: u8 = 0x87;

    /// UVC video streaming interface control selectors
    pub const UVC_VS_PROBE_CONTROL: u16 = 0x01;
    pub const UVC_VS_COMMIT_CONTROL: u16 = 0x02;

    /// USB request types
    pub const USB_TYPE_CLASS: u8 = 0x01 << 5;
    pub const USB_RECIP_INTERFACE: u8 = 0x01;
    pub const USB_DIR_OUT: u8 = 0x00;
    pub const USB_DIR_IN: u8 = 0x80;

    /// Endpoint direction
    pub const USB_ENDPOINT_IN: u8 = 0x80;
    pub const USB_ENDPOINT_OUT: u8 = 0x00;

    /// UVC Video Streaming Interface Descriptor Subtypes
    pub const VS_UNDEFINED: u8 = 0x00;
    pub const VS_INPUT_HEADER: u8 = 0x01;
    pub const VS_OUTPUT_HEADER: u8 = 0x02;
    pub const VS_STILL_IMAGE_FRAME: u8 = 0x03;
    pub const VS_FORMAT_UNCOMPRESSED: u8 = 0x04;
    pub const VS_FRAME_UNCOMPRESSED: u8 = 0x05;
    pub const VS_FORMAT_MJPEG: u8 = 0x06;
    pub const VS_FRAME_MJPEG: u8 = 0x07;
    pub const VS_FORMAT_MPEG2TS: u8 = 0x0A;
    pub const VS_FORMAT_DV: u8 = 0x0C;
    pub const VS_COLORFORMAT: u8 = 0x0D;
    pub const VS_FORMAT_FRAME_BASED: u8 = 0x10;
    pub const VS_FRAME_FRAME_BASED: u8 = 0x11;
    pub const VS_FORMAT_STREAM_BASED: u8 = 0x12;

    /// UVC format GUID for YUY2
    pub const YUY2_GUID: [u8; 16] = [
        0x59, 0x55, 0x59, 0x32, // "YUY2"
        0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
    ];

    /// UVC format GUID for NV12
    pub const NV12_GUID: [u8; 16] = [
        0x4E, 0x56, 0x31, 0x32, // "NV12"
        0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
    ];

    /// Parsed UVC frame descriptor (resolution info)
    #[derive(Debug, Clone, Copy)]
    pub struct UvcFrameInfo {
        pub frame_index: u8,
        pub width: u16,
        pub height: u16,
        pub max_frame_size: u32,
    }

    /// Parsed UVC format information
    #[derive(Debug, Clone)]
    pub struct UvcFormatInfo {
        pub format_index: u8,
        pub format_type: UvcFormatType,
        pub num_frame_descriptors: u8,
        pub guid: Option<[u8; 16]>,
        pub bits_per_pixel: Option<u8>,
        pub frames: Vec<UvcFrameInfo>,
    }

    /// UVC format types
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum UvcFormatType {
        Mjpeg,
        Uncompressed,
        FrameBased,
        Unknown(u8),
    }

    /// Parse UVC class-specific descriptors from interface extra bytes
    pub fn parse_format_descriptors(extra: &[u8]) -> Vec<UvcFormatInfo> {
        let mut formats = Vec::new();
        let mut offset = 0;

        while offset + 2 < extra.len() {
            let desc_len = extra[offset] as usize;
            if desc_len < 3 || offset + desc_len > extra.len() {
                break;
            }

            let desc_type = extra[offset + 1];
            let desc_subtype = extra[offset + 2];

            // Check if this is a class-specific Video Streaming descriptor (0x24 = CS_INTERFACE)
            if desc_type == 0x24 {
                match desc_subtype {
                    VS_FORMAT_MJPEG if desc_len >= 11 => {
                        let format_index = extra[offset + 3];
                        let num_frame_descs = extra[offset + 4];
                        log::info!(
                            "Found MJPEG format: index={}, frame_descriptors={}",
                            format_index,
                            num_frame_descs
                        );
                        formats.push(UvcFormatInfo {
                            format_index,
                            format_type: UvcFormatType::Mjpeg,
                            num_frame_descriptors: num_frame_descs,
                            guid: None,
                            bits_per_pixel: None,
                            frames: Vec::new(),
                        });
                    }
                    VS_FORMAT_UNCOMPRESSED if desc_len >= 27 => {
                        let format_index = extra[offset + 3];
                        let num_frame_descs = extra[offset + 4];
                        let mut guid = [0u8; 16];
                        guid.copy_from_slice(&extra[offset + 5..offset + 21]);
                        let bits_per_pixel = extra[offset + 21];

                        // Try to identify the format from GUID
                        let format_name = if guid == YUY2_GUID {
                            "YUY2"
                        } else if guid == NV12_GUID {
                            "NV12"
                        } else {
                            "Unknown"
                        };

                        log::info!(
                            "Found Uncompressed format: index={}, type={}, bpp={}, frame_descriptors={}",
                            format_index, format_name, bits_per_pixel, num_frame_descs
                        );
                        log::debug!("  GUID: {:02x?}", &guid);

                        formats.push(UvcFormatInfo {
                            format_index,
                            format_type: UvcFormatType::Uncompressed,
                            num_frame_descriptors: num_frame_descs,
                            guid: Some(guid),
                            bits_per_pixel: Some(bits_per_pixel),
                            frames: Vec::new(),
                        });
                    }
                    VS_FORMAT_FRAME_BASED if desc_len >= 28 => {
                        let format_index = extra[offset + 3];
                        let num_frame_descs = extra[offset + 4];
                        let mut guid = [0u8; 16];
                        guid.copy_from_slice(&extra[offset + 5..offset + 21]);
                        let bits_per_pixel = extra[offset + 21];

                        log::info!(
                            "Found Frame-Based format: index={}, bpp={}, frame_descriptors={}",
                            format_index,
                            bits_per_pixel,
                            num_frame_descs
                        );
                        log::debug!("  GUID: {:02x?}", &guid);

                        formats.push(UvcFormatInfo {
                            format_index,
                            format_type: UvcFormatType::FrameBased,
                            num_frame_descriptors: num_frame_descs,
                            guid: Some(guid),
                            bits_per_pixel: Some(bits_per_pixel),
                            frames: Vec::new(),
                        });
                    }
                    VS_INPUT_HEADER => {
                        if desc_len >= 13 {
                            let num_formats = extra[offset + 3];
                            log::info!("VS Input Header: {} format(s) available", num_formats);
                        }
                    }
                    VS_FRAME_UNCOMPRESSED | VS_FRAME_MJPEG => {
                        // Parse frame descriptor to get resolution info
                        // Offset 3: frame index
                        // Offset 5-6: wWidth (little-endian)
                        // Offset 7-8: wHeight (little-endian)
                        // Offset 9-12: dwMinBitRate
                        // Offset 13-16: dwMaxBitRate
                        // Offset 17-20: dwMaxVideoFrameBufferSize
                        if desc_len >= 21 {
                            let frame_index = extra[offset + 3];
                            let width = u16::from_le_bytes([extra[offset + 5], extra[offset + 6]]);
                            let height = u16::from_le_bytes([extra[offset + 7], extra[offset + 8]]);
                            let max_frame_size = u32::from_le_bytes([
                                extra[offset + 17],
                                extra[offset + 18],
                                extra[offset + 19],
                                extra[offset + 20],
                            ]);
                            let format_type_name = if desc_subtype == VS_FRAME_UNCOMPRESSED {
                                "Uncompressed"
                            } else {
                                "MJPEG"
                            };
                            log::info!(
                                "  Frame {}: {}x{} ({}) max_size={}",
                                frame_index,
                                width,
                                height,
                                format_type_name,
                                max_frame_size
                            );

                            // Add this frame to the most recently added format
                            if let Some(format) = formats.last_mut() {
                                format.frames.push(UvcFrameInfo {
                                    frame_index,
                                    width,
                                    height,
                                    max_frame_size,
                                });
                            }
                        }
                    }
                    _ => {
                        log::debug!(
                            "UVC VS descriptor subtype {:02x}, len={}",
                            desc_subtype,
                            desc_len
                        );
                    }
                }
            }

            offset += desc_len;
        }

        formats
    }
}

// ============================================================================
// Isochronous Transfer Support
// ============================================================================

/// Number of isochronous packets per transfer
/// Higher values = better throughput, lower values = lower latency
const ISO_PACKETS_PER_TRANSFER: i32 = 32;

/// Number of transfers to keep in flight simultaneously
/// This ensures continuous streaming without gaps
const NUM_TRANSFERS: usize = 4;

/// Timeout for event handling in milliseconds
const EVENT_TIMEOUT_MS: i32 = 100;

/// Known YUY2 frame sizes for common resolutions (2 bytes per pixel)
const YUY2_FRAME_SIZES: &[(usize, u32, u32)] = &[
    (1843200, 1280, 720), // 720p
    (921600, 640, 720),   // Half 720p width
    (614400, 640, 480),   // VGA
    (460800, 640, 360),   // 360p
    (153600, 320, 240),   // QVGA
];

/// Minimum acceptable frame size for uncompressed video (~75% of QVGA)
const MIN_UNCOMPRESSED_FRAME_SIZE: usize = 115200;

/// Check if frame_size represents a complete uncompressed frame
fn is_complete_uncompressed_frame(frame_size: usize) -> bool {
    // Check against known frame sizes with 5% tolerance
    for &(expected_size, _width, _height) in YUY2_FRAME_SIZES {
        let lower = expected_size * 95 / 100;
        let upper = expected_size * 105 / 100;
        if frame_size >= lower && frame_size <= upper {
            return true;
        }
    }

    // Fallback: accept any frame >= 90% of 720p size
    let min_720p = 1843200 * 90 / 100; // ~1.66MB
    frame_size >= min_720p
}

/// Expected YUY2 frame size for 720p (1280 * 720 * 2)
const EXPECTED_YUY2_720P_SIZE: usize = 1843200;

/// Shared state for frame accumulation across all transfers
struct SharedFrameState {
    /// Buffer to accumulate frame data across packets
    frame_buffer: Vec<u8>,
    /// Last seen frame ID (FID bit) for detecting frame boundaries
    last_frame_id: Option<bool>,
    /// Whether we've synced to a frame boundary
    synced: bool,
    /// Detected format: true = MJPEG, false = uncompressed (YUY2)
    is_mjpeg: Option<bool>,
    /// Expected frame size for uncompressed video (from descriptor, not probe)
    expected_frame_size: usize,
}

// Forward declaration for capture module
use crate::capture::CaptureState;

/// Context passed to the isochronous transfer callback
struct IsoCallbackContext {
    /// Channel to send received frame data
    frame_sender: std::sync::mpsc::Sender<Vec<u8>>,
    /// Flag to signal when streaming should stop
    stop_flag: Arc<AtomicBool>,
    /// Shared frame state (protected by mutex for thread-safety)
    shared_state: Arc<std::sync::Mutex<SharedFrameState>>,
    /// Max packet size for this endpoint
    max_packet_size: u16,
    /// Expected frame size for uncompressed video (from descriptor)
    expected_frame_size: usize,
    /// Optional capture state for recording raw packets (E2E testing)
    capture_state: Option<Arc<CaptureState>>,
}

/// Manages isochronous USB transfers for video streaming
pub struct IsochronousStream {
    /// libusb context (needed for event handling)
    ctx: *mut libusb1_sys::libusb_context,
    /// Device handle
    handle: *mut libusb1_sys::libusb_device_handle,
    /// Endpoint address
    endpoint: u8,
    /// Maximum packet size
    max_packet_size: u16,
    /// Pre-allocated transfer structures
    transfers: Vec<*mut libusb1_sys::libusb_transfer>,
    /// Buffers for each transfer
    buffers: Vec<Vec<u8>>,
    /// Callback contexts (boxed to ensure stable addresses)
    contexts: Vec<Box<IsoCallbackContext>>,
    /// Flag to signal stop (public for external access)
    pub stop_flag: Arc<AtomicBool>,
    /// Receiver for completed frames
    frame_receiver: Option<std::sync::mpsc::Receiver<Vec<u8>>>,
}

impl IsochronousStream {
    /// Create a new isochronous stream for the given endpoint
    ///
    /// # Safety
    /// The caller must ensure the device handle and context remain valid
    /// for the lifetime of this stream.
    ///
    /// # Arguments
    /// * `ctx` - libusb context pointer
    /// * `handle` - libusb device handle pointer
    /// * `endpoint` - Endpoint address
    /// * `max_packet_size` - Maximum packet size for the endpoint
    /// * `expected_frame_size` - Expected frame size from descriptor (e.g., 614400 for 640x480 YUY2)
    /// * `capture_state` - Optional capture state for recording raw packets (E2E testing)
    pub unsafe fn new(
        ctx: *mut libusb1_sys::libusb_context,
        handle: *mut libusb1_sys::libusb_device_handle,
        endpoint: u8,
        max_packet_size: u16,
        expected_frame_size: usize,
        capture_state: Option<Arc<CaptureState>>,
    ) -> Result<Self, LibusbError> {
        let (frame_sender, frame_receiver) = std::sync::mpsc::channel();
        let stop_flag = Arc::new(AtomicBool::new(false));

        // Use provided expected_frame_size, fall back to 720p if 0
        let frame_size = if expected_frame_size > 0 {
            expected_frame_size
        } else {
            EXPECTED_YUY2_720P_SIZE
        };

        log::info!(
            "Creating isochronous stream with expected frame size: {} bytes",
            frame_size
        );

        // Create shared state for frame accumulation (shared across all transfers)
        let shared_state = Arc::new(std::sync::Mutex::new(SharedFrameState {
            frame_buffer: Vec::with_capacity(frame_size + 1024), // Frame size + margin
            last_frame_id: None,
            synced: false,
            is_mjpeg: None, // Will be detected from first frame data
            expected_frame_size: frame_size,
        }));

        let buffer_size = (max_packet_size as usize) * (ISO_PACKETS_PER_TRANSFER as usize);

        let mut transfers = Vec::with_capacity(NUM_TRANSFERS);
        let mut buffers = Vec::with_capacity(NUM_TRANSFERS);
        let mut contexts = Vec::with_capacity(NUM_TRANSFERS);

        for i in 0..NUM_TRANSFERS {
            // Allocate transfer with space for ISO packet descriptors
            let transfer = libusb1_sys::libusb_alloc_transfer(ISO_PACKETS_PER_TRANSFER);
            if transfer.is_null() {
                // Clean up already allocated transfers
                for t in &transfers {
                    libusb1_sys::libusb_free_transfer(*t);
                }
                log::error!("Failed to allocate transfer {}", i);
                return Err(LibusbError::NoMem);
            }

            // Allocate buffer for this transfer
            let buffer = vec![0u8; buffer_size];

            // Create callback context
            let context = Box::new(IsoCallbackContext {
                frame_sender: frame_sender.clone(),
                stop_flag: Arc::clone(&stop_flag),
                shared_state: Arc::clone(&shared_state),
                max_packet_size,
                expected_frame_size: frame_size,
                capture_state: capture_state.clone(),
            });

            transfers.push(transfer);
            buffers.push(buffer);
            contexts.push(context);
        }

        log::info!(
            "Allocated {} isochronous transfers, {} packets each, {} bytes per packet (buffer {})",
            NUM_TRANSFERS,
            ISO_PACKETS_PER_TRANSFER,
            max_packet_size,
            buffer_size
        );

        Ok(Self {
            ctx,
            handle,
            endpoint,
            max_packet_size,
            transfers,
            buffers,
            contexts,
            stop_flag,
            frame_receiver: Some(frame_receiver),
        })
    }

    /// Start streaming by submitting all transfers
    pub fn start(&mut self) -> Result<(), LibusbError> {
        log::info!(
            "Starting isochronous streaming on endpoint 0x{:02x}",
            self.endpoint
        );

        for i in 0..NUM_TRANSFERS {
            self.setup_and_submit_transfer(i)?;
        }

        log::info!("All {} transfers submitted", NUM_TRANSFERS);
        Ok(())
    }

    /// Set up a transfer and submit it
    fn setup_and_submit_transfer(&mut self, index: usize) -> Result<(), LibusbError> {
        unsafe {
            let transfer = self.transfers[index];
            let buffer = self.buffers[index].as_mut_ptr();
            let buffer_len = self.buffers[index].len() as i32;
            let context_ptr = self.contexts[index].as_mut() as *mut IsoCallbackContext;

            // Fill the transfer structure
            (*transfer).dev_handle = self.handle;
            (*transfer).endpoint = self.endpoint;
            (*transfer).transfer_type = transfer_type::ISOCHRONOUS;
            (*transfer).timeout = 0; // No timeout for isochronous
            (*transfer).length = buffer_len;
            (*transfer).buffer = buffer;
            (*transfer).num_iso_packets = ISO_PACKETS_PER_TRANSFER;
            (*transfer).callback = iso_transfer_callback;
            (*transfer).user_data = context_ptr as *mut libc::c_void;

            // Set packet lengths for each isochronous packet
            libusb1_sys::libusb_set_iso_packet_lengths(transfer, self.max_packet_size as u32);

            // Submit the transfer
            let ret = libusb1_sys::libusb_submit_transfer(transfer);
            if ret < 0 {
                log::error!("Failed to submit transfer {}: {}", index, ret);
                return Err(LibusbError::from(ret));
            }

            log::debug!("Submitted transfer {}", index);
            Ok(())
        }
    }

    /// Take the frame receiver (can only be called once)
    pub fn take_frame_receiver(&mut self) -> Option<std::sync::mpsc::Receiver<Vec<u8>>> {
        self.frame_receiver.take()
    }

    /// Run the event loop to process USB transfers
    /// This should be called from a dedicated thread
    pub fn run_event_loop(&self) -> Result<(), LibusbError> {
        log::info!("Starting isochronous event loop");

        let mut timeval = libc::timeval {
            tv_sec: 0,
            tv_usec: (EVENT_TIMEOUT_MS * 1000) as libc::suseconds_t,
        };

        while !self.stop_flag.load(Ordering::Relaxed) {
            unsafe {
                let ret = libusb1_sys::libusb_handle_events_timeout(self.ctx, &mut timeval);
                if ret < 0 {
                    let err = LibusbError::from(ret);
                    if err != LibusbError::Interrupted {
                        log::error!("Event handling error: {}", err);
                        return Err(err);
                    }
                }
            }
        }

        log::info!("Isochronous event loop stopped");
        Ok(())
    }

    /// Signal the stream to stop
    pub fn stop(&self) {
        log::info!("Stopping isochronous stream");
        self.stop_flag.store(true, Ordering::Relaxed);
    }

    /// Check if streaming is stopped
    pub fn is_stopped(&self) -> bool {
        self.stop_flag.load(Ordering::Relaxed)
    }
}

impl Drop for IsochronousStream {
    fn drop(&mut self) {
        log::info!("Cleaning up isochronous stream");

        // Signal stop
        self.stop_flag.store(true, Ordering::Relaxed);

        // Cancel all pending transfers
        for (i, transfer) in self.transfers.iter().enumerate() {
            unsafe {
                let ret = libusb1_sys::libusb_cancel_transfer(*transfer);
                if ret < 0 && ret != -5 {
                    // -5 is LIBUSB_ERROR_NOT_FOUND (transfer not pending)
                    log::warn!("Failed to cancel transfer {}: {}", i, ret);
                }
            }
        }

        // Handle remaining events to complete cancellations
        unsafe {
            let mut timeval = libc::timeval {
                tv_sec: 0,
                tv_usec: 100_000 as libc::suseconds_t, // 100ms
            };
            let _ = libusb1_sys::libusb_handle_events_timeout(self.ctx, &mut timeval);
        }

        // Free all transfers
        for transfer in &self.transfers {
            unsafe {
                libusb1_sys::libusb_free_transfer(*transfer);
            }
        }

        log::info!("Isochronous stream cleanup complete");
    }
}

/// Callback function invoked when an isochronous transfer completes
///
/// # Safety
/// This is called from libusb's event handling thread. The transfer pointer
/// and user_data must be valid.
extern "system" fn iso_transfer_callback(transfer: *mut libusb1_sys::libusb_transfer) {
    // SAFETY: libusb guarantees transfer is valid in callback
    unsafe { iso_transfer_callback_inner(transfer) }
}

/// Inner implementation of the isochronous transfer callback
unsafe fn iso_transfer_callback_inner(transfer: *mut libusb1_sys::libusb_transfer) {
    log::debug!(">>> ISO CALLBACK INVOKED <<<");

    let xfr = &mut *transfer;
    let context = &mut *(xfr.user_data as *mut IsoCallbackContext);

    // Check if we should stop
    if context.stop_flag.load(Ordering::Relaxed) {
        log::debug!("Transfer callback: stop flag set, not resubmitting");
        return;
    }

    let status = TransferStatus::from(xfr.status);
    log::debug!("Transfer status: {:?}", status);

    match status {
        TransferStatus::Completed => {
            // Process each isochronous packet
            process_iso_packets(xfr, context);
        }
        TransferStatus::TimedOut => {
            log::trace!("Transfer timeout (normal for isochronous)");
        }
        TransferStatus::Cancelled => {
            log::debug!("Transfer cancelled");
            return; // Don't resubmit
        }
        TransferStatus::NoDevice => {
            log::error!("Device disconnected");
            context.stop_flag.store(true, Ordering::Relaxed);
            return;
        }
        _ => {
            log::warn!("Transfer error: {:?}", status);
        }
    }

    // Resubmit the transfer for continuous streaming
    let ret = libusb1_sys::libusb_submit_transfer(transfer);
    if ret < 0 {
        log::error!("Failed to resubmit transfer: {}", ret);
        context.stop_flag.store(true, Ordering::Relaxed);
    }
}

/// Validates a potential UVC payload header and returns the header length if valid.
///
/// Per USB Video Class spec, payload headers have this structure:
/// - Byte 0: Header length (includes this byte)
/// Round a frame size to the nearest common YUY2 frame size.
/// YUY2 uses 2 bytes per pixel, so frame_size = width * height * 2.
/// Common resolutions: 640x480, 1280x720, 1920x1080, 320x240, 800x600
fn round_to_yuy2_frame_size(actual_size: usize) -> usize {
    // Common YUY2 frame sizes
    const FRAME_SIZES: &[(usize, &str)] = &[
        (320 * 240 * 2, "320x240"),
        (640 * 480 * 2, "640x480"),
        (800 * 600 * 2, "800x600"),
        (1280 * 720 * 2, "1280x720"),
        (1920 * 1080 * 2, "1920x1080"),
        (1280 * 960 * 2, "1280x960"),
        (1600 * 1200 * 2, "1600x1200"),
        // Also include non-standard sizes that cameras might use
        (960 * 480 * 2, "960x480"),   // 3:1 aspect
        (1920 * 480 * 2, "1920x480"), // Wide sensor
    ];

    // Find the closest standard size
    let mut best_match = actual_size;
    let mut best_diff = usize::MAX;

    for &(size, name) in FRAME_SIZES {
        let diff = if size > actual_size {
            size - actual_size
        } else {
            actual_size - size
        };

        // Only match if within 5% tolerance
        if diff < best_diff && diff < size / 20 {
            best_diff = diff;
            best_match = size;
            log::debug!("Frame size {} matches {} ({})", actual_size, size, name);
        }
    }

    // If no close match, just use the actual size (rounded to be even)
    if best_match == actual_size {
        (actual_size / 2) * 2
    } else {
        best_match
    }
}

/// - Byte 1: BFH flags (bit 7 = EOH must be 1)
/// - Bytes 2-5: PTS (4 bytes, optional, present if bit 2 set)
/// - Bytes 6-11: SCR (6 bytes, optional, present if bit 3 set)
///
/// This function uses RELAXED validation - many cheap cameras don't strictly
/// follow the spec (they may set reserved bits, or declare 12-byte headers
/// without setting the PTS/SCR flags). If EOH is set and the length is in
/// the valid range (2-12), we trust the declared length.
///
/// Returns `Some(header_len)` if valid, `None` if this is not a UVC header.
#[inline]
fn validate_uvc_header(data: &[u8]) -> Option<usize> {
    if data.len() < 2 {
        return None;
    }

    let header_len = data[0] as usize;
    let header_flags = data[1];

    // EOH (End of Header) bit MUST be set for valid headers
    // This is the most reliable indicator
    if (header_flags & 0x80) == 0 {
        return None;
    }

    // Basic sanity check on length:
    // - Must be at least 2 (minimum header)
    // - Must be at most 12 (maximum with PTS + SCR)
    // - Must fit within packet
    if header_len < 2 || header_len > 12 || header_len > data.len() {
        return None;
    }

    // RELAXED VALIDATION:
    // Previously we rejected if reserved bits were set or if length didn't match flags.
    // Many cheap cameras set reserved bits or have inconsistent flags.
    // If EOH is set and length is reasonable, we trust the length byte.

    // Optional: Log trace when length doesn't match flags (for debugging)
    let pts_flag = (header_flags & 0x04) != 0;
    let scr_flag = (header_flags & 0x08) != 0;
    let expected_len = 2 + if pts_flag { 4 } else { 0 } + if scr_flag { 6 } else { 0 };

    if header_len != expected_len {
        log::trace!(
            "UVC header length mismatch: declared={}, expected from flags={}",
            header_len,
            expected_len
        );
    }

    Some(header_len)
}

/// Process individual isochronous packets from a completed transfer
unsafe fn process_iso_packets(
    xfr: &mut libusb1_sys::libusb_transfer,
    context: &IsoCallbackContext,
) {
    let num_packets = xfr.num_iso_packets as usize;
    let mut packets_with_data = 0;
    let mut total_bytes = 0usize;

    // Lock the shared state for the duration of packet processing
    let mut state = match context.shared_state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::error!("Shared state mutex poisoned, recovering");
            poisoned.into_inner()
        }
    };

    // Log first transfer's packet layout once
    static LAYOUT_LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let should_log_layout = !LAYOUT_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed);

    for i in 0..num_packets {
        // Get packet descriptor
        // The iso_packet_desc array is at the end of the transfer struct
        let pkt_desc_ptr = xfr.iso_packet_desc.as_ptr().add(i);
        let pkt_desc = &*pkt_desc_ptr;

        let pkt_status = TransferStatus::from(pkt_desc.status);
        let actual_length = pkt_desc.actual_length as usize;

        if pkt_status != TransferStatus::Completed {
            if i == 0 {
                log::debug!("Packet 0 status: {:?}", pkt_status);
            }
            continue;
        }

        if actual_length == 0 {
            continue;
        }

        packets_with_data += 1;
        total_bytes += actual_length;

        // Calculate packet buffer offset
        let offset = i * (context.max_packet_size as usize);
        let pkt_data = std::slice::from_raw_parts(xfr.buffer.add(offset), actual_length);

        // Record raw packet for E2E testing (before any parsing)
        // Fast path: atomic check avoids allocation when not capturing
        if let Some(capture_state) = &context.capture_state {
            if capture_state.is_capturing() {
                // Endpoint is stored in the transfer's endpoint field
                capture_state.add_packet(pkt_data, xfr.endpoint);
            }
        }

        // UVC payloads have a header (typically 2-12 bytes)
        // Use validate_uvc_header() to properly detect headers with all valid lengths
        // (2, 6, 8, or 12 bytes depending on PTS/SCR flags)
        let validated_header = validate_uvc_header(pkt_data);
        let is_uvc_header = validated_header.is_some();
        let header_len = validated_header.unwrap_or(0);

        // Log packet layout for debugging (first transfer only)
        if should_log_layout && packets_with_data <= 20 {
            let first_bytes: Vec<u8> = pkt_data.iter().take(24).copied().collect();
            let flags_str = if is_uvc_header {
                let flags = pkt_data[1];
                format!(
                    "hdr=12 EOF={} FID={}",
                    (flags & 0x02) != 0,
                    (flags & 0x01) != 0
                )
            } else {
                "no-hdr".to_string()
            };
            // Also show what payload bytes we'll actually use
            let payload_start = if is_uvc_header { header_len } else { 0 };
            let payload_preview: Vec<u8> = pkt_data
                .iter()
                .skip(payload_start)
                .take(8)
                .copied()
                .collect();
            log::info!(
                "Pkt[{}]: len={}, {}, payload[{}..]: {:02x?}",
                i,
                actual_length,
                flags_str,
                payload_start,
                payload_preview
            );
        }

        // Only parse UVC flags if we have a valid header
        let (end_of_frame, frame_id, error) = if let Some(_hdr_len) = validated_header {
            let header_flags = pkt_data[1];
            (
                (header_flags & 0x02) != 0, // EOF
                (header_flags & 0x01) != 0, // FID
                (header_flags & 0x40) != 0, // Error
            )
        } else {
            // No header - these bytes are payload, not flags
            // Use last known FID to maintain sync
            (false, state.last_frame_id.unwrap_or(false), false)
        };

        // Handle UVC error flag (only if this is actually a header)
        if error {
            // For MJPEG: Error likely means corrupted JPEG, need to clear
            let is_mjpeg = state.is_mjpeg.unwrap_or(false);
            if is_mjpeg {
                log::warn!("UVC error in MJPEG packet {} - clearing buffer", i);
                state.frame_buffer.clear();
                state.synced = false;
                continue;
            }
            // For YUY2: Skip this packet but don't clear buffer
            log::debug!("UVC error flag in YUY2 packet {} - skipping packet", i);
            continue;
        }

        // Detect format from first substantial data
        if state.is_mjpeg.is_none() && state.frame_buffer.len() >= 2 {
            let is_jpeg = state.frame_buffer[0] == 0xFF && state.frame_buffer[1] == 0xD8;
            state.is_mjpeg = Some(is_jpeg);
            if is_jpeg {
                log::info!("Detected MJPEG format from JPEG SOI marker");
            } else {
                log::info!(
                    "Detected uncompressed (YUY2) format - using size-based frame detection"
                );
            }
        }

        // FID toggle handling depends on format
        let is_mjpeg = state.is_mjpeg.unwrap_or(false);

        if let Some(last_fid) = state.last_frame_id {
            if frame_id != last_fid {
                // FID toggled - this indicates a new frame is starting
                if is_mjpeg {
                    // MJPEG: FID toggle is reliable frame boundary
                    let frame_size = state.frame_buffer.len();
                    if frame_size > 0 && state.synced {
                        let has_jpeg_marker = frame_size >= 2
                            && state.frame_buffer[0] == 0xFF
                            && state.frame_buffer[1] == 0xD8;
                        if has_jpeg_marker {
                            log::info!(
                                "Complete MJPEG frame: {} bytes (trigger: FID toggle)",
                                frame_size
                            );
                            let frame = std::mem::take(&mut state.frame_buffer);
                            if let Err(e) = context.frame_sender.send(frame) {
                                log::warn!("Failed to send frame: {}", e);
                            }
                        }
                    }
                    state.frame_buffer.clear();
                } else {
                    // YUY2: FID toggle is UNRELIABLE for frame boundaries on cheap cameras
                    // Many cameras toggle FID mid-frame. Only use FID for logging/calibration,
                    // NOT for sending frames. Size-based detection is the primary mechanism.
                    let buffer_size = state.frame_buffer.len();
                    if buffer_size > 0 && state.synced {
                        // Log FID-based frame size for calibration (but don't send!)
                        static FID_FRAME_LOG: std::sync::atomic::AtomicU32 =
                            std::sync::atomic::AtomicU32::new(0);
                        let log_count =
                            FID_FRAME_LOG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if log_count < 20 {
                            log::info!(
                                "FID toggle (info only): buffer={} bytes, expected={} bytes, diff={}",
                                buffer_size,
                                state.expected_frame_size,
                                buffer_size as i64 - state.expected_frame_size as i64
                            );
                        }
                    }
                    // NOTE: Do NOT send frame here! FID toggle is unreliable.
                    // Size-based detection (below) handles actual frame sending.
                }
                state.synced = true;
            }
        }
        state.last_frame_id = Some(frame_id);

        // Only accumulate data if we're synced
        if !state.synced {
            // Skip until we see a FID toggle (frame boundary)
            continue;
        }

        // Extract payload (skip header if present)
        // We already determined is_uvc_header earlier
        let bytes_added = if is_uvc_header {
            // Log header packets for debugging
            static HEADER_LOGGED: std::sync::atomic::AtomicU32 =
                std::sync::atomic::AtomicU32::new(0);
            let log_count = HEADER_LOGGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if log_count < 20 {
                let preview: Vec<u8> = pkt_data.iter().take(16).copied().collect();
                log::info!(
                    "Header packet {}: {} bytes, hdr_len={}, EOF={}, first 16: {:02x?}",
                    log_count,
                    actual_length,
                    header_len,
                    end_of_frame,
                    preview
                );
            }

            // This packet has a UVC header - skip it to get to payload
            if header_len <= actual_length {
                let payload = &pkt_data[header_len..];
                // Skip zero-filled payloads (incomplete/dropped data)
                if payload.len() > 8 && payload[0..8].iter().all(|&b| b == 0) {
                    0
                } else {
                    state.frame_buffer.extend_from_slice(payload);
                    payload.len()
                }
            } else {
                log::warn!(
                    "Header length {} > packet size {}, skipping",
                    header_len,
                    actual_length
                );
                0
            }
        } else {
            // No UVC header detected - this might be:
            // 1. Pure payload continuation (valid)
            // 2. A header we failed to detect (would corrupt data)

            // DIAGNOSTIC: Check if this looks like a header we missed
            // If byte 1 has EOH set (0x80) but we still rejected it, something is wrong
            if actual_length >= 2 && (pkt_data[1] & 0x80) != 0 {
                static MISSED_HEADER_LOG: std::sync::atomic::AtomicU32 =
                    std::sync::atomic::AtomicU32::new(0);
                if MISSED_HEADER_LOG.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 50 {
                    log::warn!(
                        "SUSPICIOUS: No header detected, but byte[1]={:02x} (EOH set). \
                         Len={}, byte[0]={:02x}. First 16: {:02x?}. Treating as payload!",
                        pkt_data[1],
                        actual_length,
                        pkt_data[0],
                        &pkt_data[..std::cmp::min(16, actual_length)]
                    );
                }
            }

            // Log first few bytes to help debug
            static PAYLOAD_LOGGED: std::sync::atomic::AtomicU32 =
                std::sync::atomic::AtomicU32::new(0);
            let log_count = PAYLOAD_LOGGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if log_count < 20 {
                let preview: Vec<u8> = pkt_data.iter().take(16).copied().collect();
                log::info!(
                    "Non-header packet {}: {} bytes, first 16: {:02x?}",
                    log_count,
                    actual_length,
                    preview
                );
            }

            // Pure payload data - but skip zero-filled packets (dropped/incomplete transfers)
            if actual_length > 8 && pkt_data[0..8].iter().all(|&b| b == 0) {
                // Zero-filled packet - skip it
                0
            } else {
                state.frame_buffer.extend_from_slice(pkt_data);
                pkt_data.len()
            }
        };

        // Suppress unused variable warning
        let _ = bytes_added;

        // For YUY2/uncompressed: Check if buffer has reached expected frame size
        // This is the primary frame detection mechanism for uncompressed video
        if !is_mjpeg {
            let buffer_size = state.frame_buffer.len();
            let expected_size = state.expected_frame_size;
            if buffer_size >= expected_size {
                // Buffer has accumulated a complete frame - send it
                let overflow = buffer_size - expected_size;
                if overflow > 0 {
                    log::debug!(
                        "Complete YUY2 frame: {} bytes ({} overflow bytes preserved for next frame)",
                        expected_size,
                        overflow
                    );
                }
                // Take exactly the expected frame size
                let frame: Vec<u8> = state.frame_buffer.drain(..expected_size).collect();
                // NOTE: Do NOT clear the buffer after drain()! The remaining bytes are
                // valid payload data for the NEXT frame. Clearing them causes frame
                // boundaries to misalign, creating an "interlaced" artifact where each
                // frame contains data from two different actual frames.
                if let Err(e) = context.frame_sender.send(frame) {
                    log::warn!("Failed to send frame: {}", e);
                }
            }
        }

        // For MJPEG: EOF is reliable, send immediately
        if is_mjpeg && end_of_frame && !state.frame_buffer.is_empty() {
            let frame_size = state.frame_buffer.len();

            // Check for JPEG SOI marker (0xFFD8)
            let has_jpeg_marker =
                frame_size >= 2 && state.frame_buffer[0] == 0xFF && state.frame_buffer[1] == 0xD8;

            if has_jpeg_marker {
                log::info!("Complete MJPEG frame: {} bytes (trigger: EOF)", frame_size);
                let frame = std::mem::take(&mut state.frame_buffer);
                if let Err(e) = context.frame_sender.send(frame) {
                    log::warn!("Failed to send frame: {}", e);
                }
            } else {
                // Scan for SOI marker in case it's offset
                let mut soi_offset: Option<usize> = None;
                for j in 0..frame_size.saturating_sub(1).min(100) {
                    if state.frame_buffer[j] == 0xFF && state.frame_buffer[j + 1] == 0xD8 {
                        soi_offset = Some(j);
                        break;
                    }
                }

                if let Some(offset) = soi_offset {
                    log::info!(
                        "Found JPEG SOI at offset {} in {} byte frame",
                        offset,
                        frame_size
                    );
                    let jpeg_frame: Vec<u8> = state.frame_buffer[offset..].to_vec();
                    if let Err(e) = context.frame_sender.send(jpeg_frame) {
                        log::warn!("Failed to send frame: {}", e);
                    }
                }
            }
            state.frame_buffer.clear();
        }
    }

    // Log summary for debugging
    if packets_with_data > 0 || total_bytes > 0 {
        log::debug!(
            "Transfer processed: {}/{} packets with data, {} total bytes, buffer now {} bytes",
            packets_with_data,
            num_packets,
            total_bytes,
            state.frame_buffer.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::validate_uvc_header;

    #[test]
    fn test_2_byte_header_minimal() {
        // Minimal header: len=2, EOH set (0x80), no PTS/SCR flags
        let data = [0x02, 0x80, 0xAB, 0xCD];
        assert_eq!(validate_uvc_header(&data), Some(2));
    }

    #[test]
    fn test_2_byte_header_with_fid_eof() {
        // 2-byte header with FID and EOF flags set: 0x83 = EOH + EOF + FID
        let data = [0x02, 0x83, 0xAB, 0xCD];
        assert_eq!(validate_uvc_header(&data), Some(2));
    }

    #[test]
    fn test_6_byte_header_pts_only() {
        // PTS header: len=6, EOH set, PTS flag set (0x84 = 0x80 | 0x04)
        let data = [0x06, 0x84, 0x00, 0x00, 0x00, 0x00, 0xAB, 0xCD];
        assert_eq!(validate_uvc_header(&data), Some(6));
    }

    #[test]
    fn test_8_byte_header_scr_only() {
        // SCR header: len=8, EOH set, SCR flag set (0x88 = 0x80 | 0x08)
        let data = [0x08, 0x88, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xAB];
        assert_eq!(validate_uvc_header(&data), Some(8));
    }

    #[test]
    fn test_12_byte_header_pts_and_scr() {
        // Full header: len=12, EOH set, PTS+SCR flags (0x8C = 0x80 | 0x04 | 0x08)
        let data = [0x0C, 0x8C, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xAB];
        assert_eq!(validate_uvc_header(&data), Some(12));
    }

    #[test]
    fn test_reject_no_eoh_bit() {
        // EOH bit not set - should reject
        let data = [0x02, 0x00, 0xAB, 0xCD];
        assert_eq!(validate_uvc_header(&data), None);
    }

    #[test]
    fn test_allow_length_mismatch_large() {
        // Says 12 bytes but only PTS flag set (should be 6)
        // With relaxed validation, we trust the declared length
        let data = [0x0C, 0x84, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xAB];
        assert_eq!(validate_uvc_header(&data), Some(12));
    }

    #[test]
    fn test_allow_length_mismatch_small() {
        // Says 2 bytes but PTS flag set (should be 6)
        // With relaxed validation, we trust the declared length
        let data = [0x02, 0x84, 0xAB, 0xCD, 0xEF, 0x12];
        assert_eq!(validate_uvc_header(&data), Some(2));
    }

    #[test]
    fn test_allow_reserved_bit_set() {
        // Reserved bit (0x10) set - with relaxed validation, we accept this
        let data = [0x02, 0x90, 0xAB, 0xCD]; // 0x90 = EOH + reserved
        assert_eq!(validate_uvc_header(&data), Some(2));
    }

    #[test]
    fn test_reject_too_short_data() {
        // Data too short to be a header
        let data = [0x02];
        assert_eq!(validate_uvc_header(&data), None);
    }

    #[test]
    fn test_reject_empty_data() {
        let data: [u8; 0] = [];
        assert_eq!(validate_uvc_header(&data), None);
    }

    #[test]
    fn test_reject_header_exceeds_packet() {
        // Header claims 6 bytes but packet only has 4
        let data = [0x06, 0x84, 0xAB, 0xCD];
        assert_eq!(validate_uvc_header(&data), None);
    }

    #[test]
    fn test_yuy2_false_positive_protection() {
        // YUY2 data that might look like a header:
        // Y=2 (could be header_len=2), U=128 (has EOH bit set)
        // But this should be rejected because 0x80 has no extra flags
        // and length matches, so it would actually be accepted as valid 2-byte header.
        // The key protection is the reserved bit check and length/flag consistency.
        let data = [0x02, 0x80, 0xAB, 0xCD];
        // This is actually a valid 2-byte header pattern
        assert_eq!(validate_uvc_header(&data), Some(2));

        // But with reserved bit set (0x90), it's rejected
        let data_with_reserved = [0x02, 0x90, 0xAB, 0xCD];
        assert_eq!(validate_uvc_header(&data_with_reserved), None);
    }
}
