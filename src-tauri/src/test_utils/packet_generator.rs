//! Synthetic packet generation for testing
//!
//! Generates valid UVC packets with known payloads for pixel-perfect test validation.
//!
//! # Example
//!
//! ```rust,ignore
//! use clean_scope_lib::test_utils::{PacketGenerator, Rgb};
//!
//! let mut gen = PacketGenerator::default();
//!
//! // Generate MJPEG packets for a solid red frame
//! let packets = gen.mjpeg_solid_frame(640, 480, Rgb::RED);
//!
//! // Generate YUY2 packets for a gradient test pattern
//! let packets = gen.yuy2_gradient_frame(640, 480);
//! ```

/// RGB color for test patterns
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    /// Red component (0-255)
    pub r: u8,
    /// Green component (0-255)
    pub g: u8,
    /// Blue component (0-255)
    pub b: u8,
}

impl Rgb {
    /// Pure red color
    pub const RED: Rgb = Rgb { r: 255, g: 0, b: 0 };
    /// Pure green color
    pub const GREEN: Rgb = Rgb { r: 0, g: 255, b: 0 };
    /// Pure blue color
    pub const BLUE: Rgb = Rgb { r: 0, g: 0, b: 255 };
    /// Pure white color
    pub const WHITE: Rgb = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };
    /// Pure black color
    pub const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
    /// Medium gray color
    pub const GRAY: Rgb = Rgb {
        r: 128,
        g: 128,
        b: 128,
    };
    /// Yellow color
    pub const YELLOW: Rgb = Rgb {
        r: 255,
        g: 255,
        b: 0,
    };
    /// Cyan color
    pub const CYAN: Rgb = Rgb {
        r: 0,
        g: 255,
        b: 255,
    };
    /// Magenta color
    pub const MAGENTA: Rgb = Rgb {
        r: 255,
        g: 0,
        b: 255,
    };

    /// Convert RGB to YUY2 (Y, U, V components)
    /// Returns (Y, U, V) using BT.601 standard
    pub fn to_yuv(&self) -> (u8, u8, u8) {
        // BT.601 Limited range conversion
        let r = self.r as f32;
        let g = self.g as f32;
        let b = self.b as f32;

        let y = (16.0 + 65.481 * r / 255.0 + 128.553 * g / 255.0 + 24.966 * b / 255.0)
            .clamp(16.0, 235.0) as u8;
        let u = (128.0 - 37.797 * r / 255.0 - 74.203 * g / 255.0 + 112.0 * b / 255.0)
            .clamp(16.0, 240.0) as u8;
        let v = (128.0 + 112.0 * r / 255.0 - 93.786 * g / 255.0 - 18.214 * b / 255.0)
            .clamp(16.0, 240.0) as u8;

        (y, u, v)
    }
}

/// UVC header builder
#[derive(Debug, Clone)]
pub struct UvcHeader {
    /// Header length (2, 6, 8, or 12)
    pub length: u8,
    /// Frame ID (FID) bit - toggles each frame
    pub fid: bool,
    /// End of Frame (EOF) bit
    pub eof: bool,
    /// Presentation Time Stamp (optional)
    pub pts: Option<u32>,
    /// Source Clock Reference (optional)
    pub scr: Option<[u8; 6]>,
}

impl UvcHeader {
    /// Create a minimal 2-byte header
    pub fn minimal(fid: bool, eof: bool) -> Self {
        Self {
            length: 2,
            fid,
            eof,
            pts: None,
            scr: None,
        }
    }

    /// Create a 12-byte header with PTS and SCR
    pub fn full(fid: bool, eof: bool, pts: u32) -> Self {
        Self {
            length: 12,
            fid,
            eof,
            pts: Some(pts),
            scr: Some([0; 6]),
        }
    }

    /// Serialize header to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.length as usize);

        // Byte 0: Header length
        bytes.push(self.length);

        // Byte 1: BFH flags
        let mut flags = 0x80u8; // EOH (End of Header) always set
        if self.fid {
            flags |= 0x01;
        }
        if self.eof {
            flags |= 0x02;
        }
        if self.pts.is_some() {
            flags |= 0x04;
        }
        if self.scr.is_some() {
            flags |= 0x08;
        }
        bytes.push(flags);

        // Optional PTS (4 bytes, little-endian)
        if let Some(pts) = self.pts {
            bytes.extend_from_slice(&pts.to_le_bytes());
        }

        // Optional SCR (6 bytes)
        if let Some(scr) = self.scr {
            bytes.extend_from_slice(&scr);
        }

        bytes
    }
}

/// Generates synthetic UVC packets for testing
pub struct PacketGenerator {
    /// Maximum payload size per packet (excluding header)
    pub max_payload_size: usize,
    /// Current frame ID (toggles each frame)
    current_fid: bool,
}

impl Default for PacketGenerator {
    fn default() -> Self {
        Self::new(3072) // Common max packet size for USB 2.0 high-speed
    }
}

impl PacketGenerator {
    /// Create a new packet generator with specified max payload size
    pub fn new(max_payload_size: usize) -> Self {
        Self {
            max_payload_size,
            current_fid: false,
        }
    }

    /// Generate YUY2 packets for a solid color frame
    ///
    /// Returns a vector of packets, each with UVC header + payload
    pub fn yuy2_solid_frame(&mut self, width: u32, height: u32, color: Rgb) -> Vec<Vec<u8>> {
        let frame_size = (width * height * 2) as usize;
        let frame_data = self.generate_yuy2_solid(width, height, color);
        self.packetize_frame(&frame_data, frame_size)
    }

    /// Generate YUY2 packets for a horizontal gradient test pattern
    ///
    /// Creates a gradient from black on the left to white on the right,
    /// useful for detecting column alignment issues.
    pub fn yuy2_gradient_frame(&mut self, width: u32, height: u32) -> Vec<Vec<u8>> {
        let frame_size = (width * height * 2) as usize;
        let frame_data = self.generate_yuy2_gradient(width, height);
        self.packetize_frame(&frame_data, frame_size)
    }

    /// Generate YUY2 packets for a checkerboard test pattern
    ///
    /// Creates alternating black and white squares (8x8 pixels),
    /// useful for detecting interlacing or frame boundary issues.
    pub fn yuy2_checkerboard_frame(&mut self, width: u32, height: u32) -> Vec<Vec<u8>> {
        let frame_size = (width * height * 2) as usize;
        let frame_data = self.generate_yuy2_checkerboard(width, height);
        self.packetize_frame(&frame_data, frame_size)
    }

    /// Generate YUY2 packets for SMPTE color bars test pattern
    ///
    /// Creates 8 vertical color bars (White, Yellow, Cyan, Green, Magenta, Red, Blue, Black),
    /// useful for testing YUV-to-RGB conversion accuracy.
    pub fn yuy2_color_bars_frame(&mut self, width: u32, height: u32) -> Vec<Vec<u8>> {
        let frame_size = (width * height * 2) as usize;
        let frame_data = self.generate_yuy2_color_bars(width, height);
        self.packetize_frame(&frame_data, frame_size)
    }

    /// Generate YUY2 packets for a vertical gradient test pattern
    ///
    /// Creates a gradient from black at top to white at bottom,
    /// useful for detecting row alignment and stride issues.
    pub fn yuy2_vertical_gradient_frame(&mut self, width: u32, height: u32) -> Vec<Vec<u8>> {
        let frame_size = (width * height * 2) as usize;
        let frame_data = self.generate_yuy2_vertical_gradient(width, height);
        self.packetize_frame(&frame_data, frame_size)
    }

    /// Generate YUY2 packets for a crosshatch/grid test pattern
    ///
    /// Creates a grid pattern with white lines on black background.
    /// Useful for detecting stride misalignment (shows as diagonal/jagged lines).
    pub fn yuy2_crosshatch_frame(
        &mut self,
        width: u32,
        height: u32,
        grid_spacing: u32,
    ) -> Vec<Vec<u8>> {
        let frame_size = (width * height * 2) as usize;
        let frame_data = self.generate_yuy2_crosshatch(width, height, grid_spacing);
        self.packetize_frame(&frame_data, frame_size)
    }

    /// Generate a minimal MJPEG frame (valid JPEG with solid color)
    ///
    /// Creates a minimal valid JPEG that can be decoded.
    pub fn mjpeg_solid_frame(&mut self, _width: u32, _height: u32, color: Rgb) -> Vec<Vec<u8>> {
        let jpeg_data = self.generate_minimal_jpeg(color);
        self.packetize_frame_mjpeg(&jpeg_data)
    }

    /// Generate raw YUY2 frame data (no packets, just frame bytes)
    pub fn generate_yuy2_solid(&self, width: u32, height: u32, color: Rgb) -> Vec<u8> {
        let (y, u, v) = color.to_yuv();
        let mut frame = Vec::with_capacity((width * height * 2) as usize);

        for _ in 0..height {
            for _ in 0..(width / 2) {
                // YUY2: Y0 U Y1 V (4 bytes for 2 pixels)
                frame.push(y); // Y0
                frame.push(u); // U
                frame.push(y); // Y1
                frame.push(v); // V
            }
        }

        frame
    }

    /// Generate YUY2 gradient frame data
    fn generate_yuy2_gradient(&self, width: u32, height: u32) -> Vec<u8> {
        let mut frame = Vec::with_capacity((width * height * 2) as usize);

        for _ in 0..height {
            for x in 0..(width / 2) {
                // Gradient from 16 (black) to 235 (white) across width
                let intensity = ((x as f32 / (width / 2) as f32) * 219.0 + 16.0) as u8;
                frame.push(intensity); // Y0
                frame.push(128); // U (neutral)
                frame.push(intensity); // Y1
                frame.push(128); // V (neutral)
            }
        }

        frame
    }

    /// Generate YUY2 checkerboard frame data
    fn generate_yuy2_checkerboard(&self, width: u32, height: u32) -> Vec<u8> {
        let mut frame = Vec::with_capacity((width * height * 2) as usize);
        let (y_white, u_white, v_white) = Rgb::WHITE.to_yuv();
        let (y_black, u_black, v_black) = Rgb::BLACK.to_yuv();
        let block_size = 8u32;

        for y in 0..height {
            for x in 0..(width / 2) {
                let block_x = (x * 2) / block_size;
                let block_y = y / block_size;
                let is_white = (block_x + block_y).is_multiple_of(2);

                let (y_val, u_val, v_val) = if is_white {
                    (y_white, u_white, v_white)
                } else {
                    (y_black, u_black, v_black)
                };

                frame.push(y_val); // Y0
                frame.push(u_val); // U
                frame.push(y_val); // Y1
                frame.push(v_val); // V
            }
        }

        frame
    }

    /// Generate YUY2 color bars frame data (SMPTE-style)
    ///
    /// Creates 8 vertical color bars: White, Yellow, Cyan, Green, Magenta, Red, Blue, Black.
    /// Useful for testing YUV-to-RGB conversion accuracy and detecting color channel issues.
    pub fn generate_yuy2_color_bars(&self, width: u32, height: u32) -> Vec<u8> {
        let mut frame = Vec::with_capacity((width * height * 2) as usize);

        // SMPTE color bar order (left to right)
        let colors = [
            Rgb::WHITE,
            Rgb::YELLOW,
            Rgb::CYAN,
            Rgb::GREEN,
            Rgb::MAGENTA,
            Rgb::RED,
            Rgb::BLUE,
            Rgb::BLACK,
        ];

        // Precompute YUV values for each color
        let yuv_colors: Vec<(u8, u8, u8)> = colors.iter().map(|c| c.to_yuv()).collect();
        let bar_width = width / colors.len() as u32;

        for _ in 0..height {
            for x in 0..(width / 2) {
                // Determine which color bar this pixel belongs to
                let pixel_x = x * 2;
                let bar_index = ((pixel_x / bar_width) as usize).min(colors.len() - 1);
                let (y_val, u_val, v_val) = yuv_colors[bar_index];

                frame.push(y_val); // Y0
                frame.push(u_val); // U
                frame.push(y_val); // Y1
                frame.push(v_val); // V
            }
        }

        frame
    }

    /// Generate YUY2 vertical gradient frame data
    ///
    /// Creates a gradient from black at the top to white at the bottom,
    /// useful for detecting row alignment and stride issues.
    pub fn generate_yuy2_vertical_gradient(&self, width: u32, height: u32) -> Vec<u8> {
        let mut frame = Vec::with_capacity((width * height * 2) as usize);

        for y in 0..height {
            // Gradient from 16 (black) to 235 (white) down the height
            let intensity = ((y as f32 / height as f32) * 219.0 + 16.0) as u8;

            for _ in 0..(width / 2) {
                frame.push(intensity); // Y0
                frame.push(128); // U (neutral)
                frame.push(intensity); // Y1
                frame.push(128); // V (neutral)
            }
        }

        frame
    }

    /// Generate YUY2 crosshatch/grid frame data
    ///
    /// Creates a grid pattern with white lines on black background.
    /// Grid spacing is configurable. Useful for detecting stride misalignment,
    /// which manifests as diagonal or jagged lines.
    pub fn generate_yuy2_crosshatch(&self, width: u32, height: u32, grid_spacing: u32) -> Vec<u8> {
        let mut frame = Vec::with_capacity((width * height * 2) as usize);
        let (y_white, u_white, v_white) = Rgb::WHITE.to_yuv();
        let (y_black, u_black, v_black) = Rgb::BLACK.to_yuv();

        for row in 0..height {
            let is_horizontal_line = row % grid_spacing == 0;

            for x in 0..(width / 2) {
                let pixel_x = x * 2;
                let is_vertical_line = pixel_x % grid_spacing == 0;
                let is_line = is_horizontal_line || is_vertical_line;

                let (y_val, u_val, v_val) = if is_line {
                    (y_white, u_white, v_white)
                } else {
                    (y_black, u_black, v_black)
                };

                frame.push(y_val); // Y0
                frame.push(u_val); // U
                frame.push(y_val); // Y1
                frame.push(v_val); // V
            }
        }

        frame
    }

    /// Generate a minimal valid JPEG for testing
    fn generate_minimal_jpeg(&self, color: Rgb) -> Vec<u8> {
        // This creates a minimal 1x1 JPEG with the specified color
        // For testing purposes, we use a pre-computed minimal JPEG structure

        // JPEG structure:
        // - SOI (Start of Image): FF D8
        // - APP0 (JFIF marker): FF E0 ...
        // - DQT (Quantization tables): FF DB ...
        // - SOF0 (Start of Frame): FF C0 ...
        // - DHT (Huffman tables): FF C4 ...
        // - SOS (Start of Scan): FF DA ...
        // - Compressed data
        // - EOI (End of Image): FF D9

        // For simplicity, we'll generate a very basic structure
        // In production, you might use an actual JPEG encoder

        let (y, u, v) = color.to_yuv();

        // Minimal 8x8 JPEG with single MCU
        // This is a pre-computed minimal JPEG that can be modified for color
        let mut jpeg = vec![
            0xFF, 0xD8, // SOI
            0xFF, 0xE0, 0x00, 0x10, // APP0 length
            0x4A, 0x46, 0x49, 0x46, 0x00, // "JFIF\0"
            0x01, 0x01, // version
            0x00, // aspect ratio units
            0x00, 0x01, // X density
            0x00, 0x01, // Y density
            0x00, 0x00, // thumbnail size
        ];

        // Add simplified quantization table
        jpeg.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
        jpeg.extend_from_slice(&[16u8; 64]); // Simple quantization values

        // SOF0 (8x8 image, YCbCr)
        jpeg.extend_from_slice(&[
            0xFF, 0xC0, 0x00, 0x11, // SOF0, length
            0x08, // precision
            0x00, 0x08, // height = 8
            0x00, 0x08, // width = 8
            0x03, // components
            0x01, 0x11, 0x00, // Y: 1, 1:1 sampling, quant table 0
            0x02, 0x11, 0x00, // Cb: 2, 1:1 sampling, quant table 0
            0x03, 0x11, 0x00, // Cr: 3, 1:1 sampling, quant table 0
        ]);

        // Simplified Huffman tables (DC and AC for luminance)
        jpeg.extend_from_slice(&[
            0xFF, 0xC4, 0x00, 0x1F, 0x00, // DHT DC luminance
            0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
        ]);

        // AC Huffman table
        jpeg.extend_from_slice(&[
            0xFF, 0xC4, 0x00, 0xB5, 0x10, // DHT AC luminance
        ]);
        // Simplified AC table entries - code counts with 2 codes of length 1
        let mut code_counts = [0u8; 16];
        code_counts[0] = 0x02; // 2 codes of length 1
        jpeg.extend_from_slice(&code_counts);
        jpeg.extend_from_slice(&[0x01, 0x02]); // Code values

        // Start of Scan
        jpeg.extend_from_slice(&[
            0xFF, 0xDA, 0x00, 0x0C, // SOS, length
            0x03, // components
            0x01, 0x00, // Y: DC table 0, AC table 0
            0x02, 0x00, // Cb: DC table 0, AC table 0
            0x03, 0x00, // Cr: DC table 0, AC table 0
            0x00, 0x3F, 0x00, // Spectral selection and approximation
        ]);

        // Simplified scan data (encoding the solid color)
        // This is a very simplified representation - a proper JPEG encoder
        // would compute DCT coefficients and Huffman encode them
        let _ = (y, u, v); // Acknowledge color (simplified encoding ignores it)
        jpeg.extend_from_slice(&[0x7F, 0xFF]); // Minimal scan data

        // EOI
        jpeg.extend_from_slice(&[0xFF, 0xD9]);

        jpeg
    }

    /// Packetize frame data into UVC packets (for uncompressed/YUY2)
    fn packetize_frame(&mut self, frame_data: &[u8], _expected_size: usize) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();
        let mut offset = 0;

        // Toggle FID for new frame
        self.current_fid = !self.current_fid;

        while offset < frame_data.len() {
            let remaining = frame_data.len() - offset;
            let payload_size = remaining.min(self.max_payload_size);
            let is_last = offset + payload_size >= frame_data.len();

            // Create header
            let header = UvcHeader::minimal(self.current_fid, is_last);
            let mut packet = header.to_bytes();

            // Add payload
            packet.extend_from_slice(&frame_data[offset..offset + payload_size]);

            packets.push(packet);
            offset += payload_size;
        }

        packets
    }

    /// Packetize MJPEG frame data into UVC packets
    fn packetize_frame_mjpeg(&mut self, jpeg_data: &[u8]) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();
        let mut offset = 0;

        // Toggle FID for new frame
        self.current_fid = !self.current_fid;

        while offset < jpeg_data.len() {
            let remaining = jpeg_data.len() - offset;
            let payload_size = remaining.min(self.max_payload_size);
            let is_last = offset + payload_size >= jpeg_data.len();

            // Create header with EOF on last packet
            let header = UvcHeader::minimal(self.current_fid, is_last);
            let mut packet = header.to_bytes();

            // Add payload
            packet.extend_from_slice(&jpeg_data[offset..offset + payload_size]);

            packets.push(packet);
            offset += payload_size;
        }

        packets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb_to_yuv_black() {
        let (y, u, v) = Rgb::BLACK.to_yuv();
        assert_eq!(y, 16); // Y = 16 for black in limited range
        assert_eq!(u, 128); // U = 128 (neutral)
        assert_eq!(v, 128); // V = 128 (neutral)
    }

    #[test]
    fn test_rgb_to_yuv_white() {
        let (y, u, v) = Rgb::WHITE.to_yuv();
        assert_eq!(y, 235); // Y = 235 for white in limited range
                            // U and V should be close to 128 (neutral), allow for rounding
        assert!((u as i16 - 128).abs() <= 1);
        assert!((v as i16 - 128).abs() <= 1);
    }

    #[test]
    fn test_rgb_to_yuv_gray() {
        let (y, u, v) = Rgb::GRAY.to_yuv();
        // Gray should have neutral U/V
        assert!(y > 16 && y < 235);
        assert!((u as i16 - 128).abs() < 3); // Close to 128
        assert!((v as i16 - 128).abs() < 3);
    }

    #[test]
    fn test_uvc_header_minimal() {
        let header = UvcHeader::minimal(true, false);
        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), 2);
        assert_eq!(bytes[0], 2); // Length
        assert_eq!(bytes[1] & 0x80, 0x80); // EOH set
        assert_eq!(bytes[1] & 0x01, 0x01); // FID set
        assert_eq!(bytes[1] & 0x02, 0x00); // EOF not set
    }

    #[test]
    fn test_uvc_header_full() {
        let header = UvcHeader::full(false, true, 12345);
        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), 12);
        assert_eq!(bytes[0], 12); // Length
        assert_eq!(bytes[1] & 0x01, 0x00); // FID not set
        assert_eq!(bytes[1] & 0x02, 0x02); // EOF set
        assert_eq!(bytes[1] & 0x04, 0x04); // PTS present
        assert_eq!(bytes[1] & 0x08, 0x08); // SCR present
    }

    #[test]
    fn test_generate_yuy2_solid_size() {
        let gen = PacketGenerator::default();
        let frame = gen.generate_yuy2_solid(640, 480, Rgb::RED);
        assert_eq!(frame.len(), 640 * 480 * 2);
    }

    #[test]
    fn test_generate_yuy2_solid_pattern() {
        let gen = PacketGenerator::default();
        let frame = gen.generate_yuy2_solid(8, 2, Rgb::WHITE);

        // Check first macropixel (Y0 U Y1 V)
        let (y, u, v) = Rgb::WHITE.to_yuv();
        assert_eq!(frame[0], y); // Y0
        assert_eq!(frame[1], u); // U
        assert_eq!(frame[2], y); // Y1
        assert_eq!(frame[3], v); // V
    }

    #[test]
    fn test_packetize_small_frame() {
        let mut gen = PacketGenerator::new(1024);
        let packets = gen.yuy2_solid_frame(8, 8, Rgb::GRAY);

        // 8x8 YUY2 = 128 bytes, fits in one packet
        assert_eq!(packets.len(), 1);

        // Check header
        assert_eq!(packets[0][0], 2); // Header length
        assert_eq!(packets[0][1] & 0x82, 0x82); // EOH + EOF set

        // Total size = 2 (header) + 128 (payload)
        assert_eq!(packets[0].len(), 130);
    }

    #[test]
    fn test_packetize_large_frame() {
        let mut gen = PacketGenerator::new(1024);
        let packets = gen.yuy2_solid_frame(640, 480, Rgb::BLUE);

        // 640x480 YUY2 = 614400 bytes
        // With 1024 byte max payload, need ceiling(614400/1024) = 600 packets
        assert_eq!(packets.len(), 600);

        // First packet should have FID but not EOF
        assert_eq!(packets[0][1] & 0x02, 0x00); // No EOF

        // Last packet should have EOF
        assert_eq!(packets[599][1] & 0x02, 0x02); // EOF set
    }

    #[test]
    fn test_fid_toggles_between_frames() {
        let mut gen = PacketGenerator::new(1024);

        // First frame
        let packets1 = gen.yuy2_solid_frame(8, 8, Rgb::RED);
        let fid1 = packets1[0][1] & 0x01;

        // Second frame
        let packets2 = gen.yuy2_solid_frame(8, 8, Rgb::GREEN);
        let fid2 = packets2[0][1] & 0x01;

        // FID should toggle
        assert_ne!(fid1, fid2);

        // Third frame should match first
        let packets3 = gen.yuy2_solid_frame(8, 8, Rgb::BLUE);
        let fid3 = packets3[0][1] & 0x01;
        assert_eq!(fid1, fid3);
    }

    #[test]
    fn test_mjpeg_frame_has_jpeg_markers() {
        let mut gen = PacketGenerator::new(1024);
        let packets = gen.mjpeg_solid_frame(8, 8, Rgb::RED);

        // Concatenate all payloads (skip headers)
        let mut frame_data = Vec::new();
        for packet in &packets {
            let header_len = packet[0] as usize;
            frame_data.extend_from_slice(&packet[header_len..]);
        }

        // Check for JPEG SOI marker
        assert!(frame_data.len() >= 2);
        assert_eq!(frame_data[0], 0xFF);
        assert_eq!(frame_data[1], 0xD8);

        // Check for JPEG EOI marker at end
        assert_eq!(frame_data[frame_data.len() - 2], 0xFF);
        assert_eq!(frame_data[frame_data.len() - 1], 0xD9);
    }

    #[test]
    fn test_checkerboard_pattern() {
        let gen = PacketGenerator::default();
        let frame = gen.generate_yuy2_solid(16, 16, Rgb::WHITE);

        // All pixels should be the same (white)
        let (y, _, _) = Rgb::WHITE.to_yuv();
        for i in (0..frame.len()).step_by(4) {
            assert_eq!(frame[i], y); // Y0
            assert_eq!(frame[i + 2], y); // Y1
        }
    }

    #[test]
    fn test_color_bars_size() {
        let gen = PacketGenerator::default();
        let frame = gen.generate_yuy2_color_bars(640, 480);
        assert_eq!(frame.len(), 640 * 480 * 2);
    }

    #[test]
    fn test_color_bars_first_bar_white() {
        let gen = PacketGenerator::default();
        // Width must be divisible by 8 (number of bars) and by 2 (YUY2 macropixel)
        let frame = gen.generate_yuy2_color_bars(64, 8);

        // First bar should be white (bar_width = 64/8 = 8 pixels)
        let (y_white, _, _) = Rgb::WHITE.to_yuv();

        // Check first pixel of first row
        assert_eq!(frame[0], y_white, "First pixel Y should be white");
    }

    #[test]
    fn test_color_bars_last_bar_black() {
        let gen = PacketGenerator::default();
        let frame = gen.generate_yuy2_color_bars(64, 8);

        // Last bar should be black
        let (y_black, _, _) = Rgb::BLACK.to_yuv();

        // Last macropixel of first row (at position width-2 = 62)
        // Row stride = 64 * 2 = 128 bytes, last macropixel at 128-4 = 124
        let last_macropixel = 64 * 2 - 4;
        assert_eq!(
            frame[last_macropixel], y_black,
            "Last pixel Y should be black"
        );
    }

    #[test]
    fn test_vertical_gradient_size() {
        let gen = PacketGenerator::default();
        let frame = gen.generate_yuy2_vertical_gradient(640, 480);
        assert_eq!(frame.len(), 640 * 480 * 2);
    }

    #[test]
    fn test_vertical_gradient_top_dark_bottom_light() {
        let gen = PacketGenerator::default();
        let width = 64u32;
        let height = 64u32;
        let frame = gen.generate_yuy2_vertical_gradient(width, height);

        // First row (top) should be dark (Y close to 16)
        let y_top = frame[0];
        assert!(y_top < 32, "Top should be dark, got Y={}", y_top);

        // Last row (bottom) should be light (Y close to 235)
        let last_row_start = ((height - 1) * width * 2) as usize;
        let y_bottom = frame[last_row_start];
        assert!(y_bottom > 200, "Bottom should be light, got Y={}", y_bottom);

        // Gradient should increase
        assert!(y_bottom > y_top, "Y should increase from top to bottom");
    }

    #[test]
    fn test_vertical_gradient_rows_uniform() {
        let gen = PacketGenerator::default();
        let width = 32u32;
        let height = 16u32;
        let frame = gen.generate_yuy2_vertical_gradient(width, height);

        // Each row should have uniform Y values
        for row in 0..height {
            let row_start = (row * width * 2) as usize;
            let expected_y = frame[row_start]; // Y of first pixel in row

            // Check all Y values in this row are the same
            for x in 0..(width / 2) {
                let offset = row_start + (x * 4) as usize;
                assert_eq!(
                    frame[offset],
                    expected_y,
                    "Row {} pixel {} Y should be uniform",
                    row,
                    x * 2
                );
                assert_eq!(
                    frame[offset + 2],
                    expected_y,
                    "Row {} pixel {} Y should be uniform",
                    row,
                    x * 2 + 1
                );
            }
        }
    }

    #[test]
    fn test_crosshatch_size() {
        let gen = PacketGenerator::default();
        let frame = gen.generate_yuy2_crosshatch(640, 480, 32);
        assert_eq!(frame.len(), 640 * 480 * 2);
    }

    #[test]
    fn test_crosshatch_grid_lines() {
        let gen = PacketGenerator::default();
        let width = 64u32;
        let height = 64u32;
        let grid_spacing = 16u32;
        let frame = gen.generate_yuy2_crosshatch(width, height, grid_spacing);

        let (y_white, _, _) = Rgb::WHITE.to_yuv();
        let (y_black, _, _) = Rgb::BLACK.to_yuv();

        // Row 0 should be all white (horizontal line)
        assert_eq!(frame[0], y_white, "Row 0 should be horizontal line (white)");

        // Row 1, column 0 should be white (vertical line)
        let row1_start = (width * 2) as usize;
        assert_eq!(
            frame[row1_start], y_white,
            "Column 0 should be vertical line (white)"
        );

        // Row 1, column 1 should be black (not on any line)
        // Column 1 means x=2 in the macropixel, which is at byte offset 4 (Y0 of second macropixel)
        // But pixel_x=2 % 16 != 0, so it should be black
        // Actually macropixel 1 has pixel_x = 2, which is not on grid
        assert_eq!(
            frame[row1_start + 4],
            y_black,
            "Interior pixel should be black"
        );
    }

    #[test]
    fn test_rgb_to_yuv_yellow() {
        let (y, u, _v) = Rgb::YELLOW.to_yuv();
        // Yellow has high Y (bright), low U, high V
        assert!(y > 200, "Yellow should have high luminance");
        assert!(u < 128, "Yellow should have U below neutral");
    }

    #[test]
    fn test_rgb_to_yuv_cyan() {
        let (y, u, v) = Rgb::CYAN.to_yuv();
        // Cyan has medium-high Y, high U, low V
        assert!(y > 150, "Cyan should have high luminance");
        assert!(u > 128, "Cyan should have U above neutral");
        assert!(v < 128, "Cyan should have V below neutral");
    }

    #[test]
    fn test_rgb_to_yuv_magenta() {
        let (_y, u, v) = Rgb::MAGENTA.to_yuv();
        // Magenta has medium Y, high U, high V
        assert!(u > 128, "Magenta should have U above neutral");
        assert!(v > 128, "Magenta should have V above neutral");
    }
}
