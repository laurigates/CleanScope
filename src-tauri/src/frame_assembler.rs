//! Frame assembly from UVC payloads
//!
//! Extracts frame assembly logic from the USB isochronous transfer processing
//! to enable testing without USB hardware.
//!
//! # Usage
//!
//! ```rust,ignore
//! use clean_scope_lib::frame_assembler::{FrameAssembler, ProcessResult};
//!
//! let mut assembler = FrameAssembler::new(640 * 480 * 2); // YUY2 frame size
//!
//! for packet in usb_packets {
//!     if let ProcessResult::Frame(frame) = assembler.process_packet(&packet) {
//!         // Complete frame received
//!         process_frame(frame);
//!     }
//! }
//! ```

/// Common YUY2 frame sizes for auto-detection
const FRAME_SIZES: &[(usize, &str)] = &[
    (320 * 240 * 2, "320x240"),
    (640 * 480 * 2, "640x480"),
    (800 * 600 * 2, "800x600"),
    (1280 * 720 * 2, "1280x720"),
    (1920 * 1080 * 2, "1920x1080"),
    (1280 * 960 * 2, "1280x960"),
    (1600 * 1200 * 2, "1600x1200"),
    (960 * 480 * 2, "960x480"),
    (1920 * 480 * 2, "1920x480"),
];

/// Result of processing a single packet
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessResult {
    /// No complete frame yet, continue accumulating
    Accumulating,
    /// Complete frame ready
    Frame(Vec<u8>),
    /// Packet was skipped (not synced, error, etc.)
    Skipped,
}

/// Assembles complete frames from UVC payload packets
///
/// Handles both MJPEG (EOF-based) and YUY2 (size-based) frame detection.
#[derive(Debug)]
pub struct FrameAssembler {
    /// Buffer to accumulate frame data across packets
    frame_buffer: Vec<u8>,
    /// Last seen frame ID (FID bit) for detecting frame boundaries
    last_frame_id: Option<bool>,
    /// Whether we've synced to a frame boundary
    synced: bool,
    /// Detected format: true = MJPEG, false = uncompressed (YUY2)
    is_mjpeg: Option<bool>,
    /// Expected frame size for uncompressed video
    expected_frame_size: usize,
}

impl FrameAssembler {
    /// Create a new frame assembler
    ///
    /// # Arguments
    /// * `expected_frame_size` - Expected size for uncompressed frames (width * height * 2 for YUY2).
    ///   Set to 0 for MJPEG which uses EOF-based detection.
    pub fn new(expected_frame_size: usize) -> Self {
        Self {
            frame_buffer: Vec::with_capacity(expected_frame_size.max(1024 * 1024)),
            last_frame_id: None,
            synced: false,
            is_mjpeg: None,
            expected_frame_size,
        }
    }

    /// Create a new frame assembler for MJPEG format
    pub fn new_mjpeg() -> Self {
        let mut assembler = Self::new(0);
        assembler.is_mjpeg = Some(true);
        assembler
    }

    /// Create a new frame assembler for YUY2 format
    pub fn new_yuy2(width: u32, height: u32) -> Self {
        let expected_size = (width * height * 2) as usize;
        let mut assembler = Self::new(expected_size);
        assembler.is_mjpeg = Some(false);
        assembler
    }

    /// Reset the assembler state
    pub fn reset(&mut self) {
        self.frame_buffer.clear();
        self.last_frame_id = None;
        self.synced = false;
    }

    /// Force sync state (for testing with known-good packet streams)
    ///
    /// In production, sync is achieved by detecting FID toggle.
    /// For testing with synthetic packets, we can force sync immediately.
    #[cfg(test)]
    pub fn force_sync(&mut self) {
        self.synced = true;
    }

    /// Get current buffer size (for debugging)
    pub fn buffer_len(&self) -> usize {
        self.frame_buffer.len()
    }

    /// Check if assembler is synced to frame boundaries
    pub fn is_synced(&self) -> bool {
        self.synced
    }

    /// Get detected format
    pub fn detected_format(&self) -> Option<bool> {
        self.is_mjpeg
    }

    /// Process a single UVC payload packet
    ///
    /// Returns `ProcessResult::Frame(data)` when a complete frame is assembled.
    pub fn process_packet(&mut self, packet_data: &[u8]) -> ProcessResult {
        if packet_data.is_empty() {
            return ProcessResult::Skipped;
        }

        // Parse UVC header
        let validated_header = validate_uvc_header(packet_data);
        let header_len = validated_header.unwrap_or(0);

        // Extract flags from header (if present)
        let (end_of_frame, frame_id, error) = if validated_header.is_some() {
            let header_flags = packet_data[1];
            (
                (header_flags & 0x02) != 0, // EOF
                (header_flags & 0x01) != 0, // FID
                (header_flags & 0x40) != 0, // Error
            )
        } else {
            // No header - use last known FID
            (false, self.last_frame_id.unwrap_or(false), false)
        };

        // Handle UVC error flag
        if error {
            let is_mjpeg = self.is_mjpeg.unwrap_or(false);
            if is_mjpeg {
                log::warn!("UVC error in MJPEG packet - clearing buffer");
                self.frame_buffer.clear();
                self.synced = false;
                return ProcessResult::Skipped;
            }
            log::debug!("UVC error flag in YUY2 packet - skipping packet");
            return ProcessResult::Skipped;
        }

        // Detect format from first substantial data
        if self.is_mjpeg.is_none() && self.frame_buffer.len() >= 2 {
            let is_jpeg = is_jpeg_data(&self.frame_buffer);
            self.is_mjpeg = Some(is_jpeg);
            if is_jpeg {
                log::info!("Detected MJPEG format from JPEG SOI marker");
            } else {
                log::info!(
                    "Detected uncompressed (YUY2) format - using size-based frame detection"
                );
            }
        }

        let is_mjpeg = self.is_mjpeg.unwrap_or(false);
        let mut result = ProcessResult::Accumulating;

        // Handle FID toggle (frame boundary detection)
        if let Some(last_fid) = self.last_frame_id {
            if frame_id != last_fid {
                // FID toggled - new frame is starting
                if is_mjpeg {
                    result = self.handle_mjpeg_fid_toggle();
                } else {
                    result = self.handle_yuy2_fid_toggle();
                }
                self.synced = true;
            }
        }
        self.last_frame_id = Some(frame_id);

        // Skip accumulation if not synced
        if !self.synced {
            return ProcessResult::Skipped;
        }

        // Extract and accumulate payload
        self.accumulate_payload(packet_data, header_len, validated_header.is_some());

        // Check for complete frame (format-specific)
        if !is_mjpeg {
            // YUY2: Size-based frame detection
            if let Some(frame) = self.check_yuy2_frame_complete() {
                return ProcessResult::Frame(frame);
            }
        } else if end_of_frame && !self.frame_buffer.is_empty() {
            // MJPEG: EOF-based frame detection
            if let Some(frame) = self.extract_mjpeg_frame() {
                return ProcessResult::Frame(frame);
            }
        }

        result
    }

    /// Handle FID toggle for MJPEG format
    fn handle_mjpeg_fid_toggle(&mut self) -> ProcessResult {
        let frame_size = self.frame_buffer.len();
        if frame_size > 0 && self.synced {
            let has_jpeg_marker = is_jpeg_data(&self.frame_buffer);
            if has_jpeg_marker {
                log::info!(
                    "Complete MJPEG frame: {} bytes (trigger: FID toggle)",
                    frame_size
                );
                let frame = std::mem::take(&mut self.frame_buffer);
                return ProcessResult::Frame(frame);
            }
        }
        self.frame_buffer.clear();
        ProcessResult::Accumulating
    }

    /// Handle FID toggle for YUY2 format
    fn handle_yuy2_fid_toggle(&mut self) -> ProcessResult {
        let buffer_size = self.frame_buffer.len();
        if buffer_size > 0 && self.synced {
            log::debug!(
                "FID toggle frame boundary: buffer={} bytes, expected={} bytes",
                buffer_size,
                self.expected_frame_size
            );

            // Auto-correct expected_frame_size if significantly different
            let size_ratio = buffer_size as f32 / self.expected_frame_size as f32;
            if !(0.7..=1.5).contains(&size_ratio) {
                let corrected_size = round_to_yuy2_frame_size(buffer_size);
                if corrected_size != self.expected_frame_size {
                    log::warn!(
                        "Auto-correcting expected_frame_size: {} -> {}",
                        self.expected_frame_size,
                        corrected_size
                    );
                    self.expected_frame_size = corrected_size;
                }
            }

            let frame = std::mem::take(&mut self.frame_buffer);
            return ProcessResult::Frame(frame);
        }
        ProcessResult::Accumulating
    }

    /// Accumulate payload data into frame buffer
    fn accumulate_payload(&mut self, packet_data: &[u8], header_len: usize, has_header: bool) {
        if has_header {
            if header_len <= packet_data.len() {
                let payload = &packet_data[header_len..];
                // Skip zero-filled payloads
                if !(payload.len() > 8 && payload[0..8].iter().all(|&b| b == 0)) {
                    self.frame_buffer.extend_from_slice(payload);
                }
            }
        } else {
            // Pure payload data - skip zero-filled packets
            if !(packet_data.len() > 8 && packet_data[0..8].iter().all(|&b| b == 0)) {
                self.frame_buffer.extend_from_slice(packet_data);
            }
        }
    }

    /// Check if YUY2 frame is complete based on size
    fn check_yuy2_frame_complete(&mut self) -> Option<Vec<u8>> {
        let buffer_size = self.frame_buffer.len();
        let expected_size = self.expected_frame_size;

        if buffer_size >= expected_size && expected_size > 0 {
            log::debug!(
                "Complete YUY2 frame: {} bytes ({} overflow preserved)",
                expected_size,
                buffer_size - expected_size
            );
            let frame: Vec<u8> = self.frame_buffer.drain(..expected_size).collect();
            Some(frame)
        } else {
            None
        }
    }

    /// Extract complete MJPEG frame
    fn extract_mjpeg_frame(&mut self) -> Option<Vec<u8>> {
        let frame_size = self.frame_buffer.len();

        // Check for JPEG SOI marker (0xFFD8)
        let has_jpeg_marker = is_jpeg_data(&self.frame_buffer);

        if has_jpeg_marker {
            log::info!("Complete MJPEG frame: {} bytes (trigger: EOF)", frame_size);
            let frame = std::mem::take(&mut self.frame_buffer);
            return Some(frame);
        }

        // Scan for SOI marker in case it's offset
        for j in 0..frame_size.saturating_sub(1).min(100) {
            if is_jpeg_data(&self.frame_buffer[j..]) {
                log::info!(
                    "Found JPEG SOI at offset {} in {} byte frame",
                    j,
                    frame_size
                );
                let jpeg_frame = self.frame_buffer[j..].to_vec();
                self.frame_buffer.clear();
                return Some(jpeg_frame);
            }
        }

        self.frame_buffer.clear();
        None
    }
}

/// Validate UVC header and return header length if valid
///
/// UVC Header Format:
/// - Byte 0: Header length (2-12)
/// - Byte 1: BFH flags (bit 7 = EOH must be 1)
/// - Bytes 2-5: PTS (optional, present if bit 2 set)
/// - Bytes 6-11: SCR (optional, present if bit 3 set)
///
/// Uses relaxed validation - many cheap cameras don't strictly follow the spec.
#[inline]
pub fn validate_uvc_header(data: &[u8]) -> Option<usize> {
    if data.len() < 2 {
        return None;
    }

    let header_len = data[0] as usize;
    let header_flags = data[1];

    // EOH (End of Header) bit MUST be set for valid headers
    if (header_flags & 0x80) == 0 {
        return None;
    }

    // Basic sanity check on length
    if !(2..=12).contains(&header_len) || header_len > data.len() {
        return None;
    }

    Some(header_len)
}

/// Check if data starts with JPEG SOI marker (0xFFD8)
///
/// JPEG images always begin with the Start Of Image marker: 0xFF 0xD8.
/// This is used to distinguish MJPEG frames from uncompressed formats like YUY2.
#[inline]
pub fn is_jpeg_data(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8
}

/// Round a byte count to the nearest standard YUY2 frame size
pub fn round_to_yuy2_frame_size(actual_size: usize) -> usize {
    let mut best_match = actual_size;
    let mut best_diff = usize::MAX;

    for &(size, name) in FRAME_SIZES {
        let diff = size.abs_diff(actual_size);

        // Only match if within 5% tolerance
        if diff < best_diff && diff < size / 20 {
            best_diff = diff;
            best_match = size;
            log::debug!("Frame size {} matches {} ({})", actual_size, size, name);
        }
    }

    // If no close match, round to even
    if best_match == actual_size {
        (actual_size / 2) * 2
    } else {
        best_match
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // UVC Header Validation Tests (migrated from libusb_android.rs)
    // =========================================================================

    #[test]
    fn test_2_byte_header_minimal() {
        // Minimal valid header: length=2, EOH set
        let data = [0x02, 0x80, 0xAB, 0xCD];
        assert_eq!(validate_uvc_header(&data), Some(2));
    }

    #[test]
    fn test_2_byte_header_with_fid_eof() {
        // Header with FID and EOF flags
        let data = [0x02, 0x83, 0xAB, 0xCD]; // EOH | EOF | FID
        assert_eq!(validate_uvc_header(&data), Some(2));
    }

    #[test]
    fn test_6_byte_header_pts_only() {
        // 6-byte header with PTS (bit 2 set)
        let data = [0x06, 0x84, 0x11, 0x22, 0x33, 0x44, 0xAB, 0xCD];
        assert_eq!(validate_uvc_header(&data), Some(6));
    }

    #[test]
    fn test_8_byte_header_scr_only() {
        // 8-byte header with SCR (bit 3 set)
        let data = [0x08, 0x88, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0xAB];
        assert_eq!(validate_uvc_header(&data), Some(8));
    }

    #[test]
    fn test_12_byte_header_pts_and_scr() {
        // Full 12-byte header with PTS and SCR
        let data = [
            0x0C, 0x8C, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB,
        ];
        assert_eq!(validate_uvc_header(&data), Some(12));
    }

    #[test]
    fn test_reject_no_eoh_bit() {
        // EOH bit not set - should be rejected
        let data = [0x02, 0x00, 0xAB, 0xCD];
        assert_eq!(validate_uvc_header(&data), None);
    }

    #[test]
    fn test_allow_length_mismatch_large() {
        // Camera declares 12 bytes but flags suggest 2 - we trust the declared length
        let data = [
            0x0C, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xAB,
        ];
        assert_eq!(validate_uvc_header(&data), Some(12));
    }

    #[test]
    fn test_allow_length_mismatch_small() {
        // Camera declares 2 bytes but sets PTS/SCR flags - trust the length
        let data = [0x02, 0x8C, 0xAB, 0xCD];
        assert_eq!(validate_uvc_header(&data), Some(2));
    }

    #[test]
    fn test_allow_reserved_bit_set() {
        // Reserved bits set - accept anyway (relaxed validation)
        let data = [0x02, 0xB0, 0xAB, 0xCD]; // bit 5 and 4 set
        assert_eq!(validate_uvc_header(&data), Some(2));
    }

    #[test]
    fn test_reject_too_short_data() {
        // Only 1 byte - can't be valid header
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
        // Header claims 12 bytes but packet is only 6
        let data = [0x0C, 0x8C, 0x11, 0x22, 0x33, 0x44];
        assert_eq!(validate_uvc_header(&data), None);
    }

    #[test]
    fn test_yuy2_false_positive_protection() {
        // YUY2 data that might look like a header
        // 0x80 in position 1 with small byte 0 could be misinterpreted
        let data = [0x08, 0x80, 0x80, 0x08, 0x80, 0x80, 0x08, 0x80, 0x80];
        // This WILL be detected as a valid 8-byte header (EOH is set, length is valid)
        // This is expected behavior - callers must use context (format detection)
        assert_eq!(validate_uvc_header(&data), Some(8));
    }

    // =========================================================================
    // JPEG Detection Tests
    // =========================================================================

    #[test]
    fn test_is_jpeg_data_valid() {
        // Valid JPEG SOI marker
        assert!(is_jpeg_data(&[0xFF, 0xD8]));
        assert!(is_jpeg_data(&[0xFF, 0xD8, 0xFF, 0xE0])); // JPEG with JFIF marker
    }

    #[test]
    fn test_is_jpeg_data_invalid() {
        // Not JPEG
        assert!(!is_jpeg_data(&[])); // Empty
        assert!(!is_jpeg_data(&[0xFF])); // Too short
        assert!(!is_jpeg_data(&[0xFF, 0xD9])); // EOI marker, not SOI
        assert!(!is_jpeg_data(&[0x00, 0x00])); // YUY2 data
        assert!(!is_jpeg_data(&[0x80, 0x80])); // Random data
    }

    // =========================================================================
    // FrameAssembler Tests
    // =========================================================================

    #[test]
    fn test_assembler_creation() {
        let assembler = FrameAssembler::new(640 * 480 * 2);
        assert_eq!(assembler.buffer_len(), 0);
        assert!(!assembler.is_synced());
        assert_eq!(assembler.detected_format(), None);
    }

    #[test]
    fn test_assembler_mjpeg_mode() {
        let assembler = FrameAssembler::new_mjpeg();
        assert_eq!(assembler.detected_format(), Some(true));
    }

    #[test]
    fn test_assembler_yuy2_mode() {
        let assembler = FrameAssembler::new_yuy2(640, 480);
        assert_eq!(assembler.detected_format(), Some(false));
        assert_eq!(assembler.expected_frame_size, 640 * 480 * 2);
    }

    #[test]
    fn test_empty_packet_skipped() {
        let mut assembler = FrameAssembler::new(1024);
        assert_eq!(assembler.process_packet(&[]), ProcessResult::Skipped);
    }

    #[test]
    fn test_reset_clears_state() {
        let mut assembler = FrameAssembler::new(1024);
        assembler.synced = true;
        assembler.frame_buffer.push(0x42);
        assembler.last_frame_id = Some(true);

        assembler.reset();

        assert!(!assembler.is_synced());
        assert_eq!(assembler.buffer_len(), 0);
        assert_eq!(assembler.last_frame_id, None);
    }

    #[test]
    fn test_round_to_yuy2_frame_size_exact() {
        assert_eq!(round_to_yuy2_frame_size(640 * 480 * 2), 640 * 480 * 2);
    }

    #[test]
    fn test_round_to_yuy2_frame_size_close() {
        // Slightly off should still match
        let expected = 640 * 480 * 2;
        let close = expected + 100;
        assert_eq!(round_to_yuy2_frame_size(close), expected);
    }

    #[test]
    fn test_round_to_yuy2_frame_size_unknown() {
        // Very different size should return rounded even value
        let weird_size = 12345;
        assert_eq!(round_to_yuy2_frame_size(weird_size), 12344); // rounded to even
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::test_utils::{PacketGenerator, Rgb};

    #[test]
    fn test_yuy2_frame_assembly_from_synthetic_packets() {
        let mut gen = PacketGenerator::new(1024);
        let mut assembler = FrameAssembler::new_yuy2(640, 480);
        assembler.force_sync(); // Start synced for testing

        // Generate packets for a solid red frame
        let packets = gen.yuy2_solid_frame(640, 480, Rgb::RED);

        // Process all packets
        let mut frames = Vec::new();
        for packet in &packets {
            if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
                frames.push(frame);
            }
        }

        // Should produce exactly one complete frame
        assert_eq!(frames.len(), 1, "Expected exactly 1 frame");

        // Frame should be correct size
        let expected_size = 640 * 480 * 2;
        assert_eq!(frames[0].len(), expected_size);

        // Verify frame content (YUY2 pattern for red)
        let (y, u, v) = Rgb::RED.to_yuv();
        // Check first macropixel
        assert_eq!(frames[0][0], y, "Y0 mismatch");
        assert_eq!(frames[0][1], u, "U mismatch");
        assert_eq!(frames[0][2], y, "Y1 mismatch");
        assert_eq!(frames[0][3], v, "V mismatch");
    }

    #[test]
    fn test_multiple_yuy2_frames() {
        let mut gen = PacketGenerator::new(1024);
        let mut assembler = FrameAssembler::new_yuy2(64, 64);
        assembler.force_sync(); // Start synced for testing

        // Generate 3 frames with different colors
        let colors = [Rgb::RED, Rgb::GREEN, Rgb::BLUE];
        let mut all_packets = Vec::new();

        for color in &colors {
            let packets = gen.yuy2_solid_frame(64, 64, *color);
            all_packets.extend(packets);
        }

        // Process all packets
        let mut frames = Vec::new();
        for packet in &all_packets {
            if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
                frames.push(frame);
            }
        }

        // Should produce 3 complete frames
        assert_eq!(frames.len(), 3, "Expected 3 frames");

        // Verify each frame has correct color
        for (i, (frame, color)) in frames.iter().zip(colors.iter()).enumerate() {
            let (y, u, _v) = color.to_yuv();
            assert_eq!(
                frame[0], y,
                "Frame {} Y0 mismatch: expected {}, got {}",
                i, y, frame[0]
            );
            assert_eq!(
                frame[1], u,
                "Frame {} U mismatch: expected {}, got {}",
                i, u, frame[1]
            );
        }
    }

    #[test]
    fn test_mjpeg_frame_assembly() {
        let mut gen = PacketGenerator::new(512);
        let mut assembler = FrameAssembler::new_mjpeg();
        assembler.force_sync(); // Start synced for testing

        // Generate MJPEG packets
        let packets = gen.mjpeg_solid_frame(8, 8, Rgb::BLUE);

        // Process all packets
        let mut frames = Vec::new();
        for packet in &packets {
            if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
                frames.push(frame);
            }
        }

        // Should produce one frame
        assert_eq!(frames.len(), 1, "Expected 1 MJPEG frame");

        // Frame should start with JPEG SOI marker
        assert!(
            frames[0].len() >= 2,
            "Frame too short: {} bytes",
            frames[0].len()
        );
        assert_eq!(frames[0][0], 0xFF, "Missing JPEG SOI marker (FF)");
        assert_eq!(frames[0][1], 0xD8, "Missing JPEG SOI marker (D8)");

        // Frame should end with JPEG EOI marker
        let len = frames[0].len();
        assert_eq!(frames[0][len - 2], 0xFF, "Missing JPEG EOI marker (FF)");
        assert_eq!(frames[0][len - 1], 0xD9, "Missing JPEG EOI marker (D9)");
    }

    #[test]
    fn test_gradient_frame_pixel_verification() {
        let mut gen = PacketGenerator::new(2048);
        let mut assembler = FrameAssembler::new_yuy2(16, 8);
        assembler.force_sync(); // Start synced for testing

        // Generate gradient frame
        let packets = gen.yuy2_gradient_frame(16, 8);

        // Process packets
        let mut frames = Vec::new();
        for packet in &packets {
            if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
                frames.push(frame);
            }
        }

        assert_eq!(frames.len(), 1);
        let frame = &frames[0];

        // Verify gradient: Y values should increase from left to right
        // Each macropixel is 4 bytes (Y0, U, Y1, V)
        let y_left = frame[0]; // First pixel Y
        let y_right = frame[frame.len() - 4]; // Last macropixel Y0

        // Left should be darker (lower Y) than right
        assert!(
            y_left < y_right,
            "Gradient check failed: left Y={} should be < right Y={}",
            y_left,
            y_right
        );
    }

    #[test]
    fn test_frame_assembly_with_small_packets() {
        // Test with very small packets (simulates fragmented USB transfers)
        let mut gen = PacketGenerator::new(64); // Small packets
        let mut assembler = FrameAssembler::new_yuy2(32, 32);
        assembler.force_sync(); // Start synced for testing

        let packets = gen.yuy2_solid_frame(32, 32, Rgb::WHITE);

        // 32x32 YUY2 = 2048 bytes, with 64-byte packets = 32 packets
        assert!(packets.len() >= 30, "Expected many small packets");

        let mut frames = Vec::new();
        for packet in &packets {
            if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
                frames.push(frame);
            }
        }

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].len(), 32 * 32 * 2);
    }

    #[test]
    fn test_fid_synchronization() {
        // Test that the assembler properly syncs on FID toggle
        let mut gen = PacketGenerator::new(1024);
        let mut assembler = FrameAssembler::new_yuy2(32, 32);
        // Note: NOT calling force_sync() - testing natural sync

        // Generate two frames - the assembler should sync on the FID toggle
        // between frame 1 and frame 2
        let frame1_packets = gen.yuy2_solid_frame(32, 32, Rgb::RED);
        let frame2_packets = gen.yuy2_solid_frame(32, 32, Rgb::GREEN);

        let mut frames = Vec::new();

        // Process first frame - assembler will learn FID but not produce frame
        for packet in &frame1_packets {
            if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
                frames.push(frame);
            }
        }

        // First frame is lost because we weren't synced yet
        assert_eq!(frames.len(), 0, "Should not have synced on first frame");

        // Process second frame - FID will toggle, triggering sync and frame output
        for packet in &frame2_packets {
            if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
                frames.push(frame);
            }
        }

        // We should now have 1 frame (the second one, after sync)
        // Note: The first packet of frame 2 triggers sync via FID toggle,
        // and we accumulate from there
        assert!(assembler.is_synced(), "Should be synced after FID toggle");
    }

    #[test]
    fn test_error_packet_handling() {
        let mut assembler = FrameAssembler::new_yuy2(64, 64);
        assembler.force_sync();

        // Create a packet with error flag set (bit 6 of byte 1)
        let error_packet = vec![
            0x02, // Header length = 2
            0xC0, // EOH (0x80) + Error (0x40)
            0xAB, 0xCD, // Payload (should be skipped)
        ];

        let result = assembler.process_packet(&error_packet);
        assert_eq!(result, ProcessResult::Skipped);
    }
}
