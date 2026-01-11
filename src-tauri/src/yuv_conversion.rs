//! YUV to RGB conversion utilities
//!
//! Platform-independent color space conversion functions for video processing.
//! These functions convert various YUV formats to RGB for display.
//!
//! # Supported Formats
//!
//! - **YUV 4:2:2 Packed**: YUYV and UYVY byte orders
//! - **YUV 4:2:0 Planar**: I420 (Y/U/V planes)
//! - **YUV 4:2:0 Semi-Planar**: NV12 (Y plane + interleaved UV)
//! - **RGB Passthrough**: RGB888 and BGR888
//!
//! # Architecture
//!
//! On Android, this module uses `yuvutils_rs` for hardware-optimized conversions.
//! On other platforms, pure Rust implementations are provided for testing.

/// Error type for conversion failures
#[derive(Debug, Clone)]
pub struct ConversionError(pub String);

impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ConversionError {}

impl From<String> for ConversionError {
    fn from(s: String) -> Self {
        ConversionError(s)
    }
}

/// YUV 4:2:2 packed format variant
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum YuvPackedFormat {
    /// YUYV format: Y0-U-Y1-V byte order (luminance first)
    #[default]
    Yuyv,
    /// UYVY format: U-Y0-V-Y1 byte order (chrominance first)
    /// This is what macOS reports for many USB endoscopes
    Uyvy,
}

/// Calculate YUY2 stride from frame size when dimensions don't match exactly
///
/// Some cameras add padding bytes to each row for alignment. This function
/// detects the actual stride from the frame size.
///
/// # Arguments
///
/// * `frame_size` - Total frame size in bytes
/// * `width` - Frame width in pixels
/// * `height` - Frame height in pixels
///
/// # Returns
///
/// The detected stride in bytes per row
pub fn calculate_yuy2_stride(frame_size: usize, width: u32, height: u32) -> u32 {
    let expected_stride = width * 2; // Standard: 2 bytes per pixel
    let expected_size = (expected_stride * height) as usize;

    // If frame matches expected size exactly, use standard stride
    if frame_size == expected_size {
        return expected_stride;
    }

    // Calculate actual stride from frame size
    // actual_stride = frame_size / height (rounded)
    let actual_stride = (frame_size as u32) / height;

    // Validate the calculated stride is reasonable:
    // - Must be at least width * 2 (minimum for YUY2)
    // - Should not be more than 20% larger (typical alignment padding is small)
    let max_reasonable_stride = expected_stride * 12 / 10; // 120% of expected

    if actual_stride >= expected_stride && actual_stride <= max_reasonable_stride {
        log::info!(
            "Detected YUY2 stride: {} bytes/row (expected {}, frame_size={}, height={})",
            actual_stride,
            expected_stride,
            frame_size,
            height
        );
        actual_stride
    } else if actual_stride > max_reasonable_stride {
        // Frame is much larger than expected - might be a different resolution
        log::warn!(
            "Calculated stride {} is too large (expected ~{}), using expected stride",
            actual_stride,
            expected_stride
        );
        expected_stride
    } else {
        // Frame is smaller than expected - use expected stride and truncate
        log::warn!(
            "Calculated stride {} is too small (expected {}), using expected stride",
            actual_stride,
            expected_stride
        );
        expected_stride
    }
}

// ============================================================================
// Android implementation using yuvutils_rs (hardware-optimized)
// ============================================================================

#[cfg(target_os = "android")]
mod android_impl {
    use super::*;
    use yuvutils_rs::{
        uyvy422_to_rgb, yuv420_to_rgb, yuv_nv12_to_rgb, yuyv422_to_rgb, YuvBiPlanarImage,
        YuvConversionMode, YuvPackedImage, YuvPlanarImage, YuvRange, YuvStandardMatrix,
    };

    /// Convert YUV 4:2:2 packed frame to RGB with automatic stride detection
    ///
    /// This function handles cameras that use row padding for alignment.
    /// Supports both YUYV and UYVY byte orders.
    ///
    /// # Arguments
    ///
    /// * `yuv_data` - Raw YUV 4:2:2 packed data
    /// * `width` - Frame width in pixels
    /// * `height` - Frame height in pixels
    /// * `stride_override` - If Some, use this as the YUV stride instead of auto-detecting
    /// * `format` - YUYV or UYVY byte order
    ///
    /// # Returns
    ///
    /// RGB24 data (3 bytes per pixel, R-G-B order)
    pub fn convert_yuv422_to_rgb(
        yuv_data: &[u8],
        width: u32,
        height: u32,
        stride_override: Option<u32>,
        format: YuvPackedFormat,
    ) -> Result<Vec<u8>, ConversionError> {
        let frame_size = yuv_data.len();
        let expected_stride = width * 2;

        // Use override stride if provided, otherwise auto-detect
        let actual_stride =
            stride_override.unwrap_or_else(|| calculate_yuy2_stride(frame_size, width, height));

        // Log conversion parameters when they change
        static LAST_PARAMS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let format_bit = if format == YuvPackedFormat::Uyvy {
            1u64
        } else {
            0u64
        };
        let params_hash = ((width as u64) << 48)
            | ((height as u64) << 32)
            | ((actual_stride as u64) << 16)
            | format_bit;
        let last = LAST_PARAMS.swap(params_hash, std::sync::atomic::Ordering::Relaxed);
        if last != params_hash {
            log::info!(
                "{:?} conversion: input={} bytes, width={}, height={}, stride={}",
                format,
                frame_size,
                width,
                height,
                actual_stride
            );
        }

        // Validate we have enough data
        let min_required = (expected_stride * height) as usize;
        if frame_size < min_required {
            return Err(ConversionError(format!(
                "YUV data too small: {} bytes, expected at least {} bytes",
                frame_size, min_required
            )));
        }

        // Calculate how much data we need with the given stride
        let actual_frame_size = (actual_stride * height) as usize;
        let data_to_use = actual_frame_size.min(frame_size);

        let packed_image = YuvPackedImage {
            yuy: &yuv_data[..data_to_use],
            yuy_stride: actual_stride,
            width,
            height,
        };

        // RGB output: 3 bytes per pixel
        let rgb_stride = width * 3;
        let mut rgb_buffer = vec![0u8; (rgb_stride * height) as usize];

        // Convert based on format - BT.601 for standard cameras, Limited range is common
        match format {
            YuvPackedFormat::Yuyv => {
                yuyv422_to_rgb(
                    &packed_image,
                    &mut rgb_buffer,
                    rgb_stride,
                    YuvRange::Limited,
                    YuvStandardMatrix::Bt601,
                )
                .map_err(|e| ConversionError(format!("YUYV conversion error: {:?}", e)))?;
            }
            YuvPackedFormat::Uyvy => {
                uyvy422_to_rgb(
                    &packed_image,
                    &mut rgb_buffer,
                    rgb_stride,
                    YuvRange::Limited,
                    YuvStandardMatrix::Bt601,
                )
                .map_err(|e| ConversionError(format!("UYVY conversion error: {:?}", e)))?;
            }
        }

        Ok(rgb_buffer)
    }

    /// Convert I420 (planar YUV420) frame to RGB
    ///
    /// I420 layout: Y plane (width*height), U plane (width/2 * height/2), V plane (width/2 * height/2)
    /// Total size: width * height * 1.5 bytes
    ///
    /// # Arguments
    ///
    /// * `yuv_data` - Raw I420 planar data
    /// * `width` - Frame width in pixels
    /// * `height` - Frame height in pixels
    ///
    /// # Returns
    ///
    /// RGB24 data (3 bytes per pixel, R-G-B order)
    pub fn convert_i420_to_rgb(
        yuv_data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, ConversionError> {
        let y_size = (width * height) as usize;
        let uv_size = y_size / 4; // Each U and V plane is 1/4 the size of Y
        let expected_size = y_size + uv_size * 2;

        if yuv_data.len() < expected_size {
            return Err(ConversionError(format!(
                "I420 data too small: {} bytes, expected {} bytes for {}x{}",
                yuv_data.len(),
                expected_size,
                width,
                height
            )));
        }

        // Split into Y, U, V planes
        let y_plane = &yuv_data[0..y_size];
        let u_plane = &yuv_data[y_size..y_size + uv_size];
        let v_plane = &yuv_data[y_size + uv_size..y_size + uv_size * 2];

        let planar_image = YuvPlanarImage {
            y_plane,
            y_stride: width,
            u_plane,
            u_stride: width / 2,
            v_plane,
            v_stride: width / 2,
            width,
            height,
        };

        // RGB output: 3 bytes per pixel
        let rgb_stride = width * 3;
        let mut rgb_buffer = vec![0u8; (rgb_stride * height) as usize];

        yuv420_to_rgb(
            &planar_image,
            &mut rgb_buffer,
            rgb_stride,
            YuvRange::Limited,
            YuvStandardMatrix::Bt601,
        )
        .map_err(|e| ConversionError(format!("I420 conversion error: {:?}", e)))?;

        // Log first conversion
        static I420_LOGGED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !I420_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log::info!(
                "I420 conversion: {}x{}, Y={}bytes, U={}bytes, V={}bytes -> RGB={}bytes",
                width,
                height,
                y_size,
                uv_size,
                uv_size,
                rgb_buffer.len()
            );
        }

        Ok(rgb_buffer)
    }

    /// Convert NV12 (semi-planar YUV420) frame to RGB
    ///
    /// NV12 layout: Y plane (width*height), interleaved UV plane (width * height/2)
    /// Total size: width * height * 1.5 bytes
    ///
    /// # Arguments
    ///
    /// * `yuv_data` - Raw NV12 semi-planar data
    /// * `width` - Frame width in pixels
    /// * `height` - Frame height in pixels
    ///
    /// # Returns
    ///
    /// RGB24 data (3 bytes per pixel, R-G-B order)
    pub fn convert_nv12_to_rgb(
        yuv_data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, ConversionError> {
        let y_size = (width * height) as usize;
        let uv_size = y_size / 2; // UV plane is half the size of Y (interleaved)
        let expected_size = y_size + uv_size;

        if yuv_data.len() < expected_size {
            return Err(ConversionError(format!(
                "NV12 data too small: {} bytes, expected {} bytes for {}x{}",
                yuv_data.len(),
                expected_size,
                width,
                height
            )));
        }

        // Split into Y and UV planes
        let y_plane = &yuv_data[0..y_size];
        let uv_plane = &yuv_data[y_size..y_size + uv_size];

        let bi_planar_image = YuvBiPlanarImage {
            y_plane,
            y_stride: width,
            uv_plane,
            uv_stride: width, // UV stride is same as width for NV12
            width,
            height,
        };

        // RGB output: 3 bytes per pixel
        let rgb_stride = width * 3;
        let mut rgb_buffer = vec![0u8; (rgb_stride * height) as usize];

        yuv_nv12_to_rgb(
            &bi_planar_image,
            &mut rgb_buffer,
            rgb_stride,
            YuvRange::Limited,
            YuvStandardMatrix::Bt601,
            YuvConversionMode::Balanced,
        )
        .map_err(|e| ConversionError(format!("NV12 conversion error: {:?}", e)))?;

        // Log first conversion
        static NV12_LOGGED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !NV12_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log::info!(
                "NV12 conversion: {}x{}, Y={}bytes, UV={}bytes -> RGB={}bytes",
                width,
                height,
                y_size,
                uv_size,
                rgb_buffer.len()
            );
        }

        Ok(rgb_buffer)
    }
}

// ============================================================================
// Pure Rust implementation for desktop testing
// ============================================================================

#[cfg(not(target_os = "android"))]
mod desktop_impl {
    use super::*;

    /// Clamp a value to the 0-255 range
    #[inline]
    fn clamp_u8(val: i32) -> u8 {
        val.clamp(0, 255) as u8
    }

    /// Convert YUV to RGB using BT.601 limited range coefficients
    ///
    /// BT.601 limited range:
    /// - Y: 16-235 (scaled to 0-255)
    /// - U, V: 16-240, centered at 128
    #[inline]
    fn yuv_to_rgb_bt601(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
        // Expand limited range Y to full range
        let y = y as i32 - 16;
        let u = u as i32 - 128;
        let v = v as i32 - 128;

        // BT.601 coefficients (scaled by 256 for integer math)
        // R = 1.164 * Y + 1.596 * V
        // G = 1.164 * Y - 0.392 * U - 0.813 * V
        // B = 1.164 * Y + 2.017 * U
        let r = (298 * y + 409 * v + 128) >> 8;
        let g = (298 * y - 100 * u - 208 * v + 128) >> 8;
        let b = (298 * y + 516 * u + 128) >> 8;

        (clamp_u8(r), clamp_u8(g), clamp_u8(b))
    }

    /// Convert YUV 4:2:2 packed frame to RGB
    ///
    /// # Errors
    /// Returns `ConversionError` if the input data is too small for the specified dimensions.
    pub fn convert_yuv422_to_rgb(
        yuv_data: &[u8],
        width: u32,
        height: u32,
        stride_override: Option<u32>,
        format: YuvPackedFormat,
    ) -> Result<Vec<u8>, ConversionError> {
        let frame_size = yuv_data.len();
        let expected_stride = width * 2;

        // Use override stride if provided, otherwise auto-detect
        let actual_stride =
            stride_override.unwrap_or_else(|| calculate_yuy2_stride(frame_size, width, height));

        // Validate we have enough data
        let min_required = (expected_stride * height) as usize;
        if frame_size < min_required {
            return Err(ConversionError(format!(
                "YUV data too small: {} bytes, expected at least {} bytes",
                frame_size, min_required
            )));
        }

        // RGB output: 3 bytes per pixel
        let rgb_stride = (width * 3) as usize;
        let mut rgb_buffer = vec![0u8; rgb_stride * height as usize];

        for row in 0..height {
            let yuv_row_start = (row * actual_stride) as usize;
            let rgb_row_start = row as usize * rgb_stride;

            // Process 2 pixels at a time (4 bytes YUV -> 6 bytes RGB)
            for col in (0..width).step_by(2) {
                let yuv_offset = yuv_row_start + (col * 2) as usize;

                if yuv_offset + 4 > yuv_data.len() {
                    break;
                }

                // Extract Y, U, V based on format
                let (y0, u, y1, v) = match format {
                    YuvPackedFormat::Yuyv => (
                        yuv_data[yuv_offset],
                        yuv_data[yuv_offset + 1],
                        yuv_data[yuv_offset + 2],
                        yuv_data[yuv_offset + 3],
                    ),
                    YuvPackedFormat::Uyvy => (
                        yuv_data[yuv_offset + 1],
                        yuv_data[yuv_offset],
                        yuv_data[yuv_offset + 3],
                        yuv_data[yuv_offset + 2],
                    ),
                };

                // Convert first pixel
                let (r0, g0, b0) = yuv_to_rgb_bt601(y0, u, v);
                let rgb_offset = rgb_row_start + (col * 3) as usize;
                rgb_buffer[rgb_offset] = r0;
                rgb_buffer[rgb_offset + 1] = g0;
                rgb_buffer[rgb_offset + 2] = b0;

                // Convert second pixel (if within bounds)
                if col + 1 < width {
                    let (r1, g1, b1) = yuv_to_rgb_bt601(y1, u, v);
                    rgb_buffer[rgb_offset + 3] = r1;
                    rgb_buffer[rgb_offset + 4] = g1;
                    rgb_buffer[rgb_offset + 5] = b1;
                }
            }
        }

        Ok(rgb_buffer)
    }

    /// Convert I420 (planar YUV420) frame to RGB
    ///
    /// # Errors
    /// Returns `ConversionError` if the input data is too small for the specified dimensions.
    pub fn convert_i420_to_rgb(
        yuv_data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, ConversionError> {
        let y_size = (width * height) as usize;
        let uv_size = y_size / 4;
        let expected_size = y_size + uv_size * 2;

        if yuv_data.len() < expected_size {
            return Err(ConversionError(format!(
                "I420 data too small: {} bytes, expected {} bytes for {}x{}",
                yuv_data.len(),
                expected_size,
                width,
                height
            )));
        }

        let y_plane = &yuv_data[0..y_size];
        let u_plane = &yuv_data[y_size..y_size + uv_size];
        let v_plane = &yuv_data[y_size + uv_size..];

        let rgb_stride = (width * 3) as usize;
        let mut rgb_buffer = vec![0u8; rgb_stride * height as usize];

        let uv_width = (width / 2) as usize;

        for row in 0..height as usize {
            let y_row_start = row * width as usize;
            let uv_row = row / 2;
            let rgb_row_start = row * rgb_stride;

            for col in 0..width as usize {
                let y = y_plane[y_row_start + col];
                let uv_col = col / 2;
                let uv_idx = uv_row * uv_width + uv_col;
                let u = u_plane[uv_idx];
                let v = v_plane[uv_idx];

                let (r, g, b) = yuv_to_rgb_bt601(y, u, v);
                let rgb_offset = rgb_row_start + col * 3;
                rgb_buffer[rgb_offset] = r;
                rgb_buffer[rgb_offset + 1] = g;
                rgb_buffer[rgb_offset + 2] = b;
            }
        }

        Ok(rgb_buffer)
    }

    /// Convert NV12 (semi-planar YUV420) frame to RGB
    ///
    /// # Errors
    /// Returns `ConversionError` if the input data is too small for the specified dimensions.
    pub fn convert_nv12_to_rgb(
        yuv_data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, ConversionError> {
        let y_size = (width * height) as usize;
        let uv_size = y_size / 2;
        let expected_size = y_size + uv_size;

        if yuv_data.len() < expected_size {
            return Err(ConversionError(format!(
                "NV12 data too small: {} bytes, expected {} bytes for {}x{}",
                yuv_data.len(),
                expected_size,
                width,
                height
            )));
        }

        let y_plane = &yuv_data[0..y_size];
        let uv_plane = &yuv_data[y_size..];

        let rgb_stride = (width * 3) as usize;
        let mut rgb_buffer = vec![0u8; rgb_stride * height as usize];

        for row in 0..height as usize {
            let y_row_start = row * width as usize;
            let uv_row = row / 2;
            let uv_row_start = uv_row * width as usize;
            let rgb_row_start = row * rgb_stride;

            for col in 0..width as usize {
                let y = y_plane[y_row_start + col];
                let uv_col = (col / 2) * 2; // UV pairs are interleaved
                let uv_idx = uv_row_start + uv_col;
                let u = uv_plane[uv_idx];
                let v = uv_plane[uv_idx + 1];

                let (r, g, b) = yuv_to_rgb_bt601(y, u, v);
                let rgb_offset = rgb_row_start + col * 3;
                rgb_buffer[rgb_offset] = r;
                rgb_buffer[rgb_offset + 1] = g;
                rgb_buffer[rgb_offset + 2] = b;
            }
        }

        Ok(rgb_buffer)
    }
}

// ============================================================================
// Platform-independent functions (pure Rust, no external dependencies)
// ============================================================================

/// Pass through RGB888 data directly (no conversion needed)
///
/// RGB888 is already in the correct format for display (3 bytes per pixel, R-G-B order)
///
/// # Arguments
///
/// * `data` - Raw RGB888 data
/// * `width` - Frame width in pixels
/// * `height` - Frame height in pixels
///
/// # Returns
///
/// A copy of the input data (validated for size)
///
/// # Errors
/// Returns `ConversionError` if the input data is too small for the specified dimensions.
pub fn pass_through_rgb888(
    data: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, ConversionError> {
    let expected = (width * height * 3) as usize;
    if data.len() < expected {
        return Err(ConversionError(format!(
            "RGB888 data too small: {} bytes, expected {} for {}x{}",
            data.len(),
            expected,
            width,
            height
        )));
    }

    // Log once
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        log::info!(
            "RGB888 pass-through: {}x{}, {} bytes (no conversion)",
            width,
            height,
            expected
        );
    }

    Ok(data[..expected].to_vec())
}

/// Convert BGR888 to RGB888 by swapping R and B channels
///
/// BGR888 is B-G-R byte order, we need R-G-B for display
///
/// # Arguments
///
/// * `data` - Raw BGR888 data
/// * `width` - Frame width in pixels
/// * `height` - Frame height in pixels
///
/// # Returns
///
/// RGB888 data with R and B channels swapped
///
/// # Errors
/// Returns `ConversionError` if the input data is too small for the specified dimensions.
pub fn convert_bgr888_to_rgb(
    data: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, ConversionError> {
    let expected = (width * height * 3) as usize;
    if data.len() < expected {
        return Err(ConversionError(format!(
            "BGR888 data too small: {} bytes, expected {} for {}x{}",
            data.len(),
            expected,
            width,
            height
        )));
    }

    // Log once
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        log::info!(
            "BGR888 -> RGB888 conversion: {}x{}, {} bytes",
            width,
            height,
            expected
        );
    }

    // Swap B and R channels: BGR -> RGB
    let mut rgb = Vec::with_capacity(expected);
    for chunk in data[..expected].chunks_exact(3) {
        rgb.push(chunk[2]); // R (was at position 2 in BGR)
        rgb.push(chunk[1]); // G (stays in middle)
        rgb.push(chunk[0]); // B (was at position 0 in BGR)
    }

    Ok(rgb)
}

// ============================================================================
// Re-export the platform-specific implementations
// ============================================================================

#[cfg(target_os = "android")]
pub use android_impl::{convert_i420_to_rgb, convert_nv12_to_rgb, convert_yuv422_to_rgb};

#[cfg(not(target_os = "android"))]
pub use desktop_impl::{convert_i420_to_rgb, convert_nv12_to_rgb, convert_yuv422_to_rgb};

/// Legacy wrapper for backward compatibility
/// Defaults to YUYV format
///
/// # Errors
/// Returns `ConversionError` if the input data is too small for the specified dimensions.
pub fn convert_yuy2_to_rgb(
    yuy2_data: &[u8],
    width: u32,
    height: u32,
    stride_override: Option<u32>,
) -> Result<Vec<u8>, ConversionError> {
    convert_yuv422_to_rgb(
        yuy2_data,
        width,
        height,
        stride_override,
        YuvPackedFormat::Yuyv,
    )
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test YUYV frame with known values
    ///
    /// Creates a frame where Y increases left-to-right and U/V are centered (128)
    /// This produces a grayscale gradient.
    fn create_test_yuyv_frame(width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::with_capacity((width * height * 2) as usize);

        for _row in 0..height {
            for col in (0..width).step_by(2) {
                // Y increases with column position (grayscale gradient)
                let y0 = ((col * 255) / width) as u8;
                let y1 = (((col + 1) * 255) / width) as u8;
                // U and V at neutral (128) for grayscale
                let u = 128u8;
                let v = 128u8;

                // YUYV byte order
                data.push(y0);
                data.push(u);
                data.push(y1);
                data.push(v);
            }
        }

        data
    }

    /// Create a test UYVY frame with known values
    fn create_test_uyvy_frame(width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::with_capacity((width * height * 2) as usize);

        for _row in 0..height {
            for col in (0..width).step_by(2) {
                let y0 = ((col * 255) / width) as u8;
                let y1 = (((col + 1) * 255) / width) as u8;
                let u = 128u8;
                let v = 128u8;

                // UYVY byte order
                data.push(u);
                data.push(y0);
                data.push(v);
                data.push(y1);
            }
        }

        data
    }

    /// Create a test I420 frame
    fn create_test_i420_frame(width: u32, height: u32) -> Vec<u8> {
        let y_size = (width * height) as usize;
        let uv_size = y_size / 4;
        let mut data = vec![0u8; y_size + uv_size * 2];

        // Y plane: grayscale gradient
        for row in 0..height {
            for col in 0..width {
                let idx = (row * width + col) as usize;
                data[idx] = ((col * 255) / width) as u8;
            }
        }

        // U and V planes: neutral (128)
        for i in 0..uv_size {
            data[y_size + i] = 128; // U
            data[y_size + uv_size + i] = 128; // V
        }

        data
    }

    /// Create a test NV12 frame
    fn create_test_nv12_frame(width: u32, height: u32) -> Vec<u8> {
        let y_size = (width * height) as usize;
        let uv_size = y_size / 2;
        let mut data = vec![0u8; y_size + uv_size];

        // Y plane: grayscale gradient
        for row in 0..height {
            for col in 0..width {
                let idx = (row * width + col) as usize;
                data[idx] = ((col * 255) / width) as u8;
            }
        }

        // UV plane: interleaved, neutral (128)
        for i in (0..uv_size).step_by(2) {
            data[y_size + i] = 128; // U
            data[y_size + i + 1] = 128; // V
        }

        data
    }

    #[test]
    fn test_yuv422_yuyv_basic() {
        let width = 4u32;
        let height = 2u32;
        let yuv_data = create_test_yuyv_frame(width, height);

        let result = convert_yuv422_to_rgb(&yuv_data, width, height, None, YuvPackedFormat::Yuyv);
        assert!(result.is_ok(), "Conversion should succeed");

        let rgb = result.unwrap();
        assert_eq!(
            rgb.len(),
            (width * height * 3) as usize,
            "RGB output should be width * height * 3 bytes"
        );

        // First pixel should be dark (Y=0 with neutral U/V)
        // Note: due to BT.601 limited range, Y=0 maps to black
        assert!(rgb[0] < 50, "First pixel R should be dark");
        assert!(rgb[1] < 50, "First pixel G should be dark");
        assert!(rgb[2] < 50, "First pixel B should be dark");
    }

    #[test]
    fn test_yuv422_uyvy_basic() {
        let width = 4u32;
        let height = 2u32;
        let yuv_data = create_test_uyvy_frame(width, height);

        let result = convert_yuv422_to_rgb(&yuv_data, width, height, None, YuvPackedFormat::Uyvy);
        assert!(result.is_ok(), "Conversion should succeed");

        let rgb = result.unwrap();
        assert_eq!(rgb.len(), (width * height * 3) as usize);
    }

    #[test]
    fn test_yuv422_handles_stride() {
        let width = 4u32;
        let height = 2u32;
        let standard_stride = width * 2;

        // Create frame with padding (stride = width * 2 + 4 extra bytes per row)
        let padded_stride = standard_stride + 4;
        let mut yuv_data = Vec::new();

        for _row in 0..height {
            // Add actual pixel data
            for col in (0..width).step_by(2) {
                let y0 = ((col * 255) / width) as u8;
                let y1 = (((col + 1) * 255) / width) as u8;
                yuv_data.push(y0);
                yuv_data.push(128); // U
                yuv_data.push(y1);
                yuv_data.push(128); // V
            }
            // Add padding bytes
            yuv_data.extend_from_slice(&[0, 0, 0, 0]);
        }

        let result = convert_yuv422_to_rgb(
            &yuv_data,
            width,
            height,
            Some(padded_stride),
            YuvPackedFormat::Yuyv,
        );
        assert!(
            result.is_ok(),
            "Conversion with stride override should succeed"
        );

        let rgb = result.unwrap();
        assert_eq!(rgb.len(), (width * height * 3) as usize);
    }

    #[test]
    fn test_yuv422_rejects_too_small_data() {
        let width = 640u32;
        let height = 480u32;
        let yuv_data = vec![0u8; 100]; // Much too small

        let result = convert_yuv422_to_rgb(&yuv_data, width, height, None, YuvPackedFormat::Yuyv);
        assert!(result.is_err(), "Should reject data that is too small");

        let err = result.unwrap_err();
        assert!(
            err.0.contains("too small"),
            "Error should mention data is too small"
        );
    }

    #[test]
    fn test_i420_basic() {
        let width = 4u32;
        let height = 4u32; // Must be even for I420
        let yuv_data = create_test_i420_frame(width, height);

        let result = convert_i420_to_rgb(&yuv_data, width, height);
        assert!(result.is_ok(), "I420 conversion should succeed");

        let rgb = result.unwrap();
        assert_eq!(rgb.len(), (width * height * 3) as usize);
    }

    #[test]
    fn test_i420_rejects_too_small_data() {
        let width = 640u32;
        let height = 480u32;
        let yuv_data = vec![0u8; 100]; // Much too small

        let result = convert_i420_to_rgb(&yuv_data, width, height);
        assert!(result.is_err(), "Should reject data that is too small");

        let err = result.unwrap_err();
        assert!(err.0.contains("too small"));
    }

    #[test]
    fn test_nv12_basic() {
        let width = 4u32;
        let height = 4u32; // Must be even for NV12
        let yuv_data = create_test_nv12_frame(width, height);

        let result = convert_nv12_to_rgb(&yuv_data, width, height);
        assert!(result.is_ok(), "NV12 conversion should succeed");

        let rgb = result.unwrap();
        assert_eq!(rgb.len(), (width * height * 3) as usize);
    }

    #[test]
    fn test_nv12_rejects_too_small_data() {
        let width = 640u32;
        let height = 480u32;
        let yuv_data = vec![0u8; 100]; // Much too small

        let result = convert_nv12_to_rgb(&yuv_data, width, height);
        assert!(result.is_err(), "Should reject data that is too small");
    }

    #[test]
    fn test_rgb888_passthrough() {
        let width = 4u32;
        let height = 2u32;
        let expected_size = (width * height * 3) as usize;

        // Create test RGB data
        let rgb_data: Vec<u8> = (0..expected_size as u8).collect();

        let result = pass_through_rgb888(&rgb_data, width, height);
        assert!(result.is_ok(), "RGB888 passthrough should succeed");

        let output = result.unwrap();
        assert_eq!(output.len(), expected_size);
        assert_eq!(output, rgb_data, "Output should match input exactly");
    }

    #[test]
    fn test_rgb888_rejects_too_small() {
        let width = 640u32;
        let height = 480u32;
        let rgb_data = vec![0u8; 100]; // Much too small

        let result = pass_through_rgb888(&rgb_data, width, height);
        assert!(result.is_err());
    }

    #[test]
    fn test_bgr888_to_rgb_swaps_channels() {
        let width = 2u32;
        let height = 1u32;

        // Create BGR data: [B0, G0, R0, B1, G1, R1]
        let bgr_data = vec![
            10u8, 20u8, 30u8, // Pixel 0: B=10, G=20, R=30
            40u8, 50u8, 60u8, // Pixel 1: B=40, G=50, R=60
        ];

        let result = convert_bgr888_to_rgb(&bgr_data, width, height);
        assert!(result.is_ok(), "BGR to RGB conversion should succeed");

        let rgb = result.unwrap();
        assert_eq!(rgb.len(), 6);

        // Check that R and B are swapped
        assert_eq!(rgb[0], 30, "Pixel 0 R should be 30 (was B in BGR)");
        assert_eq!(rgb[1], 20, "Pixel 0 G should be 20 (unchanged)");
        assert_eq!(rgb[2], 10, "Pixel 0 B should be 10 (was R in BGR)");

        assert_eq!(rgb[3], 60, "Pixel 1 R should be 60 (was B in BGR)");
        assert_eq!(rgb[4], 50, "Pixel 1 G should be 50 (unchanged)");
        assert_eq!(rgb[5], 40, "Pixel 1 B should be 40 (was R in BGR)");
    }

    #[test]
    fn test_bgr888_rejects_too_small() {
        let width = 640u32;
        let height = 480u32;
        let bgr_data = vec![0u8; 100];

        let result = convert_bgr888_to_rgb(&bgr_data, width, height);
        assert!(result.is_err());
    }

    #[test]
    fn test_calculate_yuy2_stride_exact_match() {
        let width = 640u32;
        let height = 480u32;
        let expected_stride = width * 2;
        let frame_size = (expected_stride * height) as usize;

        let stride = calculate_yuy2_stride(frame_size, width, height);
        assert_eq!(
            stride, expected_stride,
            "Should use standard stride when frame size matches exactly"
        );
    }

    #[test]
    fn test_calculate_yuy2_stride_with_padding() {
        let width = 640u32;
        let height = 480u32;
        let padded_stride = width * 2 + 64; // 64 bytes padding per row
        let frame_size = (padded_stride * height) as usize;

        let stride = calculate_yuy2_stride(frame_size, width, height);
        assert_eq!(stride, padded_stride, "Should detect padded stride");
    }

    #[test]
    fn test_calculate_yuy2_stride_excessive_padding() {
        let width = 640u32;
        let height = 480u32;
        let expected_stride = width * 2;
        // 50% larger than expected - too much to be reasonable padding
        let excessive_stride = expected_stride * 3 / 2;
        let frame_size = (excessive_stride * height) as usize;

        let stride = calculate_yuy2_stride(frame_size, width, height);
        assert_eq!(
            stride, expected_stride,
            "Should fall back to expected stride for excessive padding"
        );
    }

    #[test]
    fn test_yuy2_legacy_wrapper() {
        let width = 4u32;
        let height = 2u32;
        let yuv_data = create_test_yuyv_frame(width, height);

        let result = convert_yuy2_to_rgb(&yuv_data, width, height, None);
        assert!(
            result.is_ok(),
            "Legacy wrapper should work with YUYV format"
        );

        let rgb = result.unwrap();
        assert_eq!(rgb.len(), (width * height * 3) as usize);
    }

    #[test]
    fn test_conversion_error_display() {
        let err = ConversionError("test error message".to_string());
        assert_eq!(format!("{}", err), "test error message");
    }

    #[test]
    fn test_yuv_packed_format_default() {
        let format = YuvPackedFormat::default();
        assert_eq!(
            format,
            YuvPackedFormat::Yuyv,
            "Default format should be YUYV"
        );
    }

    /// Test that grayscale conversion produces similar R, G, B values
    #[test]
    fn test_grayscale_conversion_produces_neutral_colors() {
        let width = 4u32;
        let height = 2u32;

        // Create a frame where Y=128 (mid-gray) and U=V=128 (neutral)
        let mut yuv_data = Vec::new();
        for _ in 0..height {
            for _ in (0..width).step_by(2) {
                yuv_data.push(128); // Y0 = mid-gray
                yuv_data.push(128); // U = neutral
                yuv_data.push(128); // Y1 = mid-gray
                yuv_data.push(128); // V = neutral
            }
        }

        let result = convert_yuv422_to_rgb(&yuv_data, width, height, None, YuvPackedFormat::Yuyv);
        assert!(result.is_ok());

        let rgb = result.unwrap();

        // Check that each pixel has similar R, G, B values (grayscale)
        for pixel in rgb.chunks_exact(3) {
            let r = pixel[0] as i32;
            let g = pixel[1] as i32;
            let b = pixel[2] as i32;

            // Allow some tolerance for rounding differences between implementations
            let max_diff = 10;
            assert!(
                (r - g).abs() <= max_diff && (g - b).abs() <= max_diff,
                "Grayscale pixel should have similar R, G, B values: R={}, G={}, B={}",
                r,
                g,
                b
            );
        }
    }
}
