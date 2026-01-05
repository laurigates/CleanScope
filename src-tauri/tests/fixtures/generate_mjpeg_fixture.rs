//! Generates a test fixture binary file for MJPEG 640x480.
//!
//! Run with: `cargo run --bin generate_mjpeg_fixture`
//!
//! This creates capture.bin with synthetic MJPEG packets that form a valid frame.
//!
//! The frame assembler requires a FID (Frame ID) toggle to sync and emit frames.
//! This fixture generates:
//! - Frame 1 (FID=0): Used for sync, discarded
//! - Frame 2 (FID=1): Accumulated, emitted when Frame 3 starts
//! - Frame 3 start (FID=0): Single packet to trigger FID toggle and emit Frame 2

use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Create a minimal UVC header.
/// - length: header length (2 for minimal)
/// - fid: frame ID toggle bit
/// - eof: end of frame bit
fn create_uvc_header(fid: bool, eof: bool) -> Vec<u8> {
    let mut header = Vec::with_capacity(2);
    header.push(0x02); // Header length = 2

    // BFH (Bit Field Header) byte:
    // Bit 0: FID (Frame ID) - toggles between frames
    // Bit 1: EOF (End of Frame)
    // Bit 7: EOH (End of Header) - always 1
    let mut flags = 0x80u8; // EOH
    if fid {
        flags |= 0x01;
    }
    if eof {
        flags |= 0x02;
    }
    header.push(flags);

    header
}

/// Create a minimal valid JPEG for testing.
/// This generates an 8x8 pixel JPEG with a solid color.
fn create_minimal_jpeg() -> Vec<u8> {
    // A minimal valid JPEG structure that most decoders can handle.
    // This is a simplified JFIF JPEG with minimal quantization tables.

    let mut jpeg = Vec::new();

    // SOI (Start of Image)
    jpeg.extend_from_slice(&[0xFF, 0xD8]);

    // APP0 (JFIF marker)
    jpeg.extend_from_slice(&[
        0xFF, 0xE0, // APP0 marker
        0x00, 0x10, // Length (16 bytes)
        0x4A, 0x46, 0x49, 0x46, 0x00, // "JFIF\0"
        0x01, 0x01, // Version 1.1
        0x00, // Aspect ratio units (0 = no units)
        0x00, 0x01, // X density
        0x00, 0x01, // Y density
        0x00, 0x00, // No thumbnail
    ]);

    // DQT (Define Quantization Table) - luminance
    jpeg.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    // Simple quantization table (all 16s for simplicity)
    jpeg.extend_from_slice(&[16u8; 64]);

    // SOF0 (Start of Frame - Baseline DCT)
    jpeg.extend_from_slice(&[
        0xFF, 0xC0, // SOF0 marker
        0x00, 0x0B, // Length (11 bytes)
        0x08, // Precision (8 bits)
        0x00, 0x08, // Height = 8
        0x00, 0x08, // Width = 8
        0x01, // Number of components = 1 (grayscale)
        0x01, 0x11, 0x00, // Component 1: ID=1, sampling=1x1, quant table=0
    ]);

    // DHT (Define Huffman Table) - DC luminance
    jpeg.extend_from_slice(&[
        0xFF, 0xC4, // DHT marker
        0x00, 0x1F, // Length
        0x00, // DC table, ID 0
        // Code counts for lengths 1-16
        0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, // Values
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
    ]);

    // DHT (Define Huffman Table) - AC luminance
    jpeg.extend_from_slice(&[
        0xFF, 0xC4, // DHT marker
        0x00, 0xB5, // Length
        0x10, // AC table, ID 0
        // Code counts for lengths 1-16
        0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00, 0x00, 0x01,
        0x7D, // Values (162 bytes of standard AC luminance Huffman values)
        0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61,
        0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52,
        0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25,
        0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45,
        0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64,
        0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83,
        0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99,
        0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6,
        0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3,
        0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8,
        0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA,
    ]);

    // SOS (Start of Scan)
    jpeg.extend_from_slice(&[
        0xFF, 0xDA, // SOS marker
        0x00, 0x08, // Length
        0x01, // Number of components
        0x01, 0x00, // Component 1: DC table 0, AC table 0
        0x00, 0x3F, 0x00, // Spectral selection, approximation
    ]);

    // Compressed image data (minimal - represents a gray 8x8 block)
    // This is a simplified representation of DC coefficient followed by EOB
    jpeg.extend_from_slice(&[0xFB, 0xD3, 0x28, 0xA2, 0x80, 0x00]);

    // EOI (End of Image)
    jpeg.extend_from_slice(&[0xFF, 0xD9]);

    jpeg
}

/// Write a packet to the capture file in the replay format.
fn write_packet(
    file: &mut File,
    timestamp_us: u64,
    endpoint: u8,
    data: &[u8],
) -> std::io::Result<()> {
    // Format: [u64 LE: timestamp_us][u32 LE: length][u8: endpoint][data bytes]
    file.write_all(&timestamp_us.to_le_bytes())?;
    file.write_all(&(data.len() as u32).to_le_bytes())?;
    file.write_all(&[endpoint])?;
    file.write_all(data)?;
    Ok(())
}

/// Write a complete frame as packets
fn write_frame_packets(
    file: &mut File,
    jpeg_data: &[u8],
    fid: bool,
    endpoint: u8,
    start_timestamp_us: &mut u64,
) -> std::io::Result<usize> {
    let max_payload = 512; // Smaller for test fixture
    let mut offset = 0;
    let mut packet_count = 0;

    while offset < jpeg_data.len() {
        let remaining = jpeg_data.len() - offset;
        let payload_size = remaining.min(max_payload);
        let is_last = offset + payload_size >= jpeg_data.len();

        // Create UVC header
        let header = create_uvc_header(fid, is_last);

        // Combine header + payload
        let mut packet_data = header;
        packet_data.extend_from_slice(&jpeg_data[offset..offset + payload_size]);

        // Write packet to file
        write_packet(file, *start_timestamp_us, endpoint, &packet_data)?;

        offset += payload_size;
        *start_timestamp_us += 1000; // 1ms between packets
        packet_count += 1;
    }

    Ok(packet_count)
}

fn main() -> std::io::Result<()> {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mjpeg_640x480");

    let output_path = fixture_dir.join("capture.bin");
    let mut file = File::create(&output_path)?;

    // Generate a minimal JPEG
    let jpeg_data = create_minimal_jpeg();
    println!("Generated JPEG: {} bytes", jpeg_data.len());

    let endpoint = 0x81; // Video streaming endpoint
    let mut timestamp_us = 0u64;
    let mut total_packets = 0;

    // Frame 1 (FID=0): Sync frame - will be discarded by assembler
    println!("Writing Frame 1 (FID=0) - sync frame");
    let count = write_frame_packets(&mut file, &jpeg_data, false, endpoint, &mut timestamp_us)?;
    total_packets += count;
    println!("  {} packets", count);

    // Gap between frames (16.67ms for ~60fps)
    timestamp_us += 16667 - 1000;

    // Frame 2 (FID=1): This frame will be emitted
    println!("Writing Frame 2 (FID=1) - emitted frame");
    let count = write_frame_packets(&mut file, &jpeg_data, true, endpoint, &mut timestamp_us)?;
    total_packets += count;
    println!("  {} packets", count);

    // Gap between frames
    timestamp_us += 16667 - 1000;

    // Frame 3 start (FID=0): Single packet to trigger FID toggle
    println!("Writing Frame 3 start (FID=0) - trigger packet");
    let header = create_uvc_header(false, false);
    let mut trigger_packet = header;
    // Add just SOI marker to start a new frame
    trigger_packet.extend_from_slice(&[0xFF, 0xD8]);
    write_packet(&mut file, timestamp_us, endpoint, &trigger_packet)?;
    total_packets += 1;
    println!("  1 packet (trigger only)");

    println!(
        "\nCreated capture.bin with {} total packets at {}",
        total_packets,
        output_path.display()
    );

    // Verify the file
    let file_size = std::fs::metadata(&output_path)?;
    println!("File size: {} bytes", file_size.len());

    // Update metadata
    let metadata_path = fixture_dir.join("capture.json");
    let metadata = format!(
        r#"{{
  "vendor_id": 4660,
  "product_id": 22136,
  "format_type": "mjpeg",
  "width": 640,
  "height": 480,
  "total_packets": {},
  "total_frames": 1,
  "duration_ms": {},
  "total_bytes": {},
  "description": "Synthetic test fixture with minimal 8x8 MJPEG frame for E2E testing"
}}"#,
        total_packets,
        timestamp_us / 1000,
        file_size.len()
    );
    std::fs::write(&metadata_path, metadata)?;
    println!("Updated capture.json");

    Ok(())
}
