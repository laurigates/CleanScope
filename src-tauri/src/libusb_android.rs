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

use std::ptr;

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

/// USB transfer types
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum TransferType {
    Control = 0,
    Isochronous = 1,
    Bulk = 2,
    Interrupt = 3,
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

    /// Wrap an Android file descriptor as a libusb device handle
    pub fn wrap_fd(&self, fd: i32) -> Result<LibusbDeviceHandle, LibusbError> {
        unsafe {
            let mut dev_handle: *mut libusb1_sys::libusb_device_handle = ptr::null_mut();

            // libusb_wrap_sys_device expects a pointer to the file descriptor
            let mut fd_value: libc::c_int = fd;
            let ret = libusb1_sys::libusb_wrap_sys_device(
                self.ctx,
                &mut fd_value as *mut libc::c_int,
                &mut dev_handle,
            );

            if ret < 0 {
                log::error!("libusb_wrap_sys_device failed: {}", ret);
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
    /// Claim an interface on the device
    pub fn claim_interface(&self, interface_number: i32) -> Result<(), LibusbError> {
        unsafe {
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
