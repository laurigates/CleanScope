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

/// Context passed to the isochronous transfer callback
struct IsoCallbackContext {
    /// Channel to send received frame data
    frame_sender: std::sync::mpsc::Sender<Vec<u8>>,
    /// Flag to signal when streaming should stop
    stop_flag: Arc<AtomicBool>,
    /// Buffer to accumulate frame data across packets
    frame_buffer: Vec<u8>,
    /// Max packet size for this endpoint
    max_packet_size: u16,
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
    pub unsafe fn new(
        ctx: *mut libusb1_sys::libusb_context,
        handle: *mut libusb1_sys::libusb_device_handle,
        endpoint: u8,
        max_packet_size: u16,
    ) -> Result<Self, LibusbError> {
        let (frame_sender, frame_receiver) = std::sync::mpsc::channel();
        let stop_flag = Arc::new(AtomicBool::new(false));

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
                frame_buffer: Vec::with_capacity(1024 * 1024), // 1MB for frame accumulation
                max_packet_size,
            });

            transfers.push(transfer);
            buffers.push(buffer);
            contexts.push(context);
        }

        log::info!(
            "Allocated {} isochronous transfers, {} packets each, {} bytes per packet",
            NUM_TRANSFERS,
            ISO_PACKETS_PER_TRANSFER,
            max_packet_size
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
    let xfr = &mut *transfer;
    let context = &mut *(xfr.user_data as *mut IsoCallbackContext);

    // Check if we should stop
    if context.stop_flag.load(Ordering::Relaxed) {
        log::debug!("Transfer callback: stop flag set, not resubmitting");
        return;
    }

    let status = TransferStatus::from(xfr.status);

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

/// Process individual isochronous packets from a completed transfer
unsafe fn process_iso_packets(
    xfr: &mut libusb1_sys::libusb_transfer,
    context: &mut IsoCallbackContext,
) {
    let num_packets = xfr.num_iso_packets as usize;

    for i in 0..num_packets {
        // Get packet descriptor
        // The iso_packet_desc array is at the end of the transfer struct
        let pkt_desc_ptr = xfr.iso_packet_desc.as_ptr().add(i);
        let pkt_desc = &*pkt_desc_ptr;

        let pkt_status = TransferStatus::from(pkt_desc.status);
        let actual_length = pkt_desc.actual_length as usize;

        if pkt_status != TransferStatus::Completed {
            log::trace!("Packet {} status: {:?}", i, pkt_status);
            continue;
        }

        if actual_length == 0 {
            continue;
        }

        // Calculate packet buffer offset
        let offset = i * (context.max_packet_size as usize);
        let pkt_data = std::slice::from_raw_parts(xfr.buffer.add(offset), actual_length);

        // UVC payloads have a header (typically 2-12 bytes)
        // Header byte 0: header length
        // Header byte 1: bit flags (bit 1 = end of frame)
        if actual_length < 2 {
            continue;
        }

        let header_len = pkt_data[0] as usize;
        let header_flags = pkt_data[1];
        let end_of_frame = (header_flags & 0x02) != 0;

        // Extract payload (skip header)
        if header_len < actual_length {
            let payload = &pkt_data[header_len..];
            context.frame_buffer.extend_from_slice(payload);
        }

        // Check for end of frame
        if end_of_frame && !context.frame_buffer.is_empty() {
            // Check for JPEG SOI marker (0xFFD8)
            if context.frame_buffer.len() >= 2
                && context.frame_buffer[0] == 0xFF
                && context.frame_buffer[1] == 0xD8
            {
                log::debug!("Complete MJPEG frame: {} bytes", context.frame_buffer.len());

                // Send the frame to the receiver
                let frame = std::mem::take(&mut context.frame_buffer);
                if let Err(e) = context.frame_sender.send(frame) {
                    log::warn!("Failed to send frame: {}", e);
                }
            } else {
                log::trace!(
                    "Non-JPEG frame discarded: {} bytes, header: {:02x?}",
                    context.frame_buffer.len(),
                    &context.frame_buffer[..std::cmp::min(8, context.frame_buffer.len())]
                );
            }

            // Clear buffer for next frame
            context.frame_buffer.clear();
        }
    }
}
