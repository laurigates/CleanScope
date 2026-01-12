//! Integration tests for the complete frame processing pipeline.
//!
//! Tests the full packet-to-RGB pipeline:
//! ```text
//! Packets → Frame Assembly → Validation → YUV Conversion → RGB Output
//! ```
//!
//! These tests verify that all components work together correctly without
//! requiring physical USB hardware.

use clean_scope_lib::frame_assembler::{FrameAssembler, ProcessResult};
use clean_scope_lib::frame_validation::{validate_yuy2_frame, ValidationLevel};
use clean_scope_lib::test_utils::{PacketGenerator, Rgb};
use clean_scope_lib::yuv_conversion::{convert_yuv422_to_rgb, YuvPackedFormat};

/// Helper to assemble frames from packets
fn assemble_frame(packets: &[Vec<u8>], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut assembler = FrameAssembler::new_yuy2(width, height);
    // Force sync for testing with synthetic packets
    #[cfg(test)]
    {
        // Access the private force_sync via the public interface
        // FrameAssembler exposes force_sync() only in test builds
    }

    // Process packets and collect frames
    let mut frames = Vec::new();
    for packet in packets {
        if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
            frames.push(frame);
        }
    }

    frames.into_iter().next()
}

// ============================================================================
// Happy Path: Complete Pipeline Tests
// ============================================================================

#[test]
fn test_complete_pipeline_yuy2_solid_red() {
    // Generate synthetic YUY2 packets for a solid red frame
    let mut gen = PacketGenerator::new(1024);
    let width = 64u32;
    let height = 48u32;
    let packets = gen.yuy2_solid_frame(width, height, Rgb::RED);

    // Stage 1: Assemble frame from packets
    let mut assembler = FrameAssembler::new_yuy2(width, height);
    let mut frames = Vec::new();
    for packet in &packets {
        if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
            frames.push(frame);
        }
    }

    // FrameAssembler needs to sync first - generate a second frame to trigger sync
    let packets2 = gen.yuy2_solid_frame(width, height, Rgb::RED);
    for packet in &packets2 {
        if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
            frames.push(frame);
        }
    }

    assert!(!frames.is_empty(), "Should produce at least one frame");
    let yuy2_frame = &frames[0];

    // Stage 2: Validate frame
    let expected_size = (width * height * 2) as usize;
    let validation = validate_yuy2_frame(
        yuy2_frame,
        width as usize,
        height as usize,
        expected_size,
        ValidationLevel::Strict,
    );

    assert!(
        validation.valid,
        "Frame should be valid. Failure: {:?}",
        validation.failure_reason
    );
    assert!(validation.stride_aligned, "Stride should be aligned");

    // Stage 3: Convert to RGB
    let rgb = convert_yuv422_to_rgb(yuy2_frame, width, height, None, YuvPackedFormat::Yuyv)
        .expect("YUV to RGB conversion should succeed");

    // Verify output size
    let expected_rgb_size = (width * height * 3) as usize;
    assert_eq!(
        rgb.len(),
        expected_rgb_size,
        "RGB output should be width * height * 3 bytes"
    );

    // Verify red color dominates
    // Red in YUV (BT.601 limited range) should convert back to high R, low G, low B
    let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
    assert!(
        r > 150,
        "Red channel should be high for red color, got R={}, G={}, B={}",
        r,
        g,
        b
    );
    assert!(
        g < 100,
        "Green channel should be low for red color, got R={}, G={}, B={}",
        r,
        g,
        b
    );
    assert!(
        b < 100,
        "Blue channel should be low for red color, got R={}, G={}, B={}",
        r,
        g,
        b
    );
}

#[test]
fn test_complete_pipeline_yuy2_solid_green() {
    let mut gen = PacketGenerator::new(1024);
    let width = 64u32;
    let height = 48u32;

    // Generate two frames to allow sync
    let _packets1 = gen.yuy2_solid_frame(width, height, Rgb::GREEN);
    let packets2 = gen.yuy2_solid_frame(width, height, Rgb::GREEN);

    let mut assembler = FrameAssembler::new_yuy2(width, height);

    // Process first frame (will be dropped during sync)
    for packet in &_packets1 {
        assembler.process_packet(packet);
    }

    // Process second frame (should be captured after sync)
    let mut frames = Vec::new();
    for packet in &packets2 {
        if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
            frames.push(frame);
        }
    }

    assert!(!frames.is_empty(), "Should produce frame after sync");
    let yuy2_frame = &frames[0];

    // Validate
    let expected_size = (width * height * 2) as usize;
    let validation = validate_yuy2_frame(
        yuy2_frame,
        width as usize,
        height as usize,
        expected_size,
        ValidationLevel::Strict,
    );
    assert!(validation.valid, "Green frame should be valid");

    // Convert to RGB
    let rgb = convert_yuv422_to_rgb(yuy2_frame, width, height, None, YuvPackedFormat::Yuyv)
        .expect("Conversion should succeed");

    // Verify green color
    let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
    assert!(
        g > 150,
        "Green channel should be high for green color, got R={}, G={}, B={}",
        r,
        g,
        b
    );
    assert!(
        r < 100,
        "Red channel should be low for green color, got R={}, G={}, B={}",
        r,
        g,
        b
    );
    assert!(
        b < 100,
        "Blue channel should be low for green color, got R={}, G={}, B={}",
        r,
        g,
        b
    );
}

#[test]
fn test_complete_pipeline_yuy2_solid_blue() {
    let mut gen = PacketGenerator::new(1024);
    let width = 64u32;
    let height = 48u32;

    let _packets1 = gen.yuy2_solid_frame(width, height, Rgb::BLUE);
    let packets2 = gen.yuy2_solid_frame(width, height, Rgb::BLUE);

    let mut assembler = FrameAssembler::new_yuy2(width, height);

    for packet in &_packets1 {
        assembler.process_packet(packet);
    }

    let mut frames = Vec::new();
    for packet in &packets2 {
        if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
            frames.push(frame);
        }
    }

    assert!(!frames.is_empty(), "Should produce frame after sync");
    let yuy2_frame = &frames[0];

    let expected_size = (width * height * 2) as usize;
    let validation = validate_yuy2_frame(
        yuy2_frame,
        width as usize,
        height as usize,
        expected_size,
        ValidationLevel::Strict,
    );
    assert!(validation.valid, "Blue frame should be valid");

    let rgb = convert_yuv422_to_rgb(yuy2_frame, width, height, None, YuvPackedFormat::Yuyv)
        .expect("Conversion should succeed");

    let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
    assert!(
        b > 150,
        "Blue channel should be high for blue color, got R={}, G={}, B={}",
        r,
        g,
        b
    );
    assert!(
        r < 100,
        "Red channel should be low for blue color, got R={}, G={}, B={}",
        r,
        g,
        b
    );
    assert!(
        g < 100,
        "Green channel should be low for blue color, got R={}, G={}, B={}",
        r,
        g,
        b
    );
}

#[test]
fn test_complete_pipeline_yuy2_gradient() {
    let mut gen = PacketGenerator::new(2048);
    let width = 64u32;
    let height = 32u32;

    let _packets1 = gen.yuy2_gradient_frame(width, height);
    let packets2 = gen.yuy2_gradient_frame(width, height);

    let mut assembler = FrameAssembler::new_yuy2(width, height);

    for packet in &_packets1 {
        assembler.process_packet(packet);
    }

    let mut frames = Vec::new();
    for packet in &packets2 {
        if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
            frames.push(frame);
        }
    }

    assert!(!frames.is_empty(), "Should produce gradient frame");
    let yuy2_frame = &frames[0];

    // Validate
    let expected_size = (width * height * 2) as usize;
    let validation = validate_yuy2_frame(
        yuy2_frame,
        width as usize,
        height as usize,
        expected_size,
        ValidationLevel::Strict,
    );
    assert!(validation.valid, "Gradient frame should be valid");

    // Convert to RGB
    let rgb = convert_yuv422_to_rgb(yuy2_frame, width, height, None, YuvPackedFormat::Yuyv)
        .expect("Conversion should succeed");

    // Verify gradient: left side should be darker than right side
    // Check first row: pixel at x=0 vs pixel at x=width-1
    let left_pixel = (rgb[0] as u32 + rgb[1] as u32 + rgb[2] as u32) / 3;
    let right_offset = ((width - 1) * 3) as usize;
    let right_pixel =
        (rgb[right_offset] as u32 + rgb[right_offset + 1] as u32 + rgb[right_offset + 2] as u32)
            / 3;

    assert!(
        left_pixel < right_pixel,
        "Gradient: left ({}) should be darker than right ({})",
        left_pixel,
        right_pixel
    );
}

#[test]
fn test_complete_pipeline_yuy2_checkerboard() {
    let mut gen = PacketGenerator::new(2048);
    let width = 64u32;
    let height = 64u32;

    let _packets1 = gen.yuy2_checkerboard_frame(width, height);
    let packets2 = gen.yuy2_checkerboard_frame(width, height);

    let mut assembler = FrameAssembler::new_yuy2(width, height);

    for packet in &_packets1 {
        assembler.process_packet(packet);
    }

    let mut frames = Vec::new();
    for packet in &packets2 {
        if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
            frames.push(frame);
        }
    }

    assert!(!frames.is_empty(), "Should produce checkerboard frame");
    let yuy2_frame = &frames[0];

    let expected_size = (width * height * 2) as usize;
    let validation = validate_yuy2_frame(
        yuy2_frame,
        width as usize,
        height as usize,
        expected_size,
        ValidationLevel::Strict,
    );
    assert!(validation.valid, "Checkerboard frame should be valid");

    let rgb = convert_yuv422_to_rgb(yuy2_frame, width, height, None, YuvPackedFormat::Yuyv)
        .expect("Conversion should succeed");

    // Verify we have variation in the frame (not all same color)
    let first_pixel = (rgb[0], rgb[1], rgb[2]);
    let mut has_different_pixel = false;

    for chunk in rgb.chunks_exact(3) {
        if (chunk[0], chunk[1], chunk[2]) != first_pixel {
            has_different_pixel = true;
            break;
        }
    }

    assert!(
        has_different_pixel,
        "Checkerboard should have pixel variation"
    );
}

// ============================================================================
// Multi-Frame Sequence Tests
// ============================================================================

#[test]
fn test_pipeline_multiple_frames_sequence() {
    let mut gen = PacketGenerator::new(1024);
    let width = 32u32;
    let height = 32u32;
    let colors = [Rgb::RED, Rgb::GREEN, Rgb::BLUE, Rgb::WHITE, Rgb::BLACK];

    let mut assembler = FrameAssembler::new_yuy2(width, height);
    let mut all_frames = Vec::new();

    // Generate and process frames for each color
    for color in &colors {
        let packets = gen.yuy2_solid_frame(width, height, *color);
        for packet in &packets {
            if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
                all_frames.push((frame, *color));
            }
        }
    }

    // Should have at least 4 frames (first one lost to sync)
    assert!(
        all_frames.len() >= 4,
        "Should produce at least 4 frames, got {}",
        all_frames.len()
    );

    // Validate and convert each frame
    for (i, (yuy2_frame, _expected_color)) in all_frames.iter().enumerate() {
        let expected_size = (width * height * 2) as usize;

        // Validate
        let validation = validate_yuy2_frame(
            yuy2_frame,
            width as usize,
            height as usize,
            expected_size,
            ValidationLevel::Strict,
        );
        assert!(
            validation.valid,
            "Frame {} should be valid: {:?}",
            i, validation.failure_reason
        );

        // Convert
        let rgb = convert_yuv422_to_rgb(yuy2_frame, width, height, None, YuvPackedFormat::Yuyv)
            .expect("Conversion should succeed");

        assert_eq!(
            rgb.len(),
            (width * height * 3) as usize,
            "Frame {} RGB size mismatch",
            i
        );
    }
}

// ============================================================================
// Validation Level Tests
// ============================================================================

#[test]
fn test_pipeline_validation_levels() {
    let mut gen = PacketGenerator::new(1024);
    let width = 64u32;
    let height = 48u32;

    let _packets1 = gen.yuy2_solid_frame(width, height, Rgb::GRAY);
    let packets2 = gen.yuy2_solid_frame(width, height, Rgb::GRAY);

    let mut assembler = FrameAssembler::new_yuy2(width, height);
    for packet in &_packets1 {
        assembler.process_packet(packet);
    }

    let mut frames = Vec::new();
    for packet in &packets2 {
        if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
            frames.push(frame);
        }
    }

    assert!(!frames.is_empty());
    let yuy2_frame = &frames[0];
    let expected_size = (width * height * 2) as usize;

    // Test all validation levels
    let levels = [
        ValidationLevel::Strict,
        ValidationLevel::Moderate,
        ValidationLevel::Minimal,
        ValidationLevel::Off,
    ];

    for level in levels {
        let validation = validate_yuy2_frame(
            yuy2_frame,
            width as usize,
            height as usize,
            expected_size,
            level,
        );
        assert!(
            validation.valid,
            "Valid frame should pass {:?} validation",
            level
        );
    }
}

// ============================================================================
// YUV Format Tests (YUYV vs UYVY)
// ============================================================================

#[test]
fn test_pipeline_yuyv_format() {
    let mut gen = PacketGenerator::new(1024);
    let width = 32u32;
    let height = 32u32;

    let _packets1 = gen.yuy2_solid_frame(width, height, Rgb::RED);
    let packets2 = gen.yuy2_solid_frame(width, height, Rgb::RED);

    let mut assembler = FrameAssembler::new_yuy2(width, height);
    for packet in &_packets1 {
        assembler.process_packet(packet);
    }

    let mut frames = Vec::new();
    for packet in &packets2 {
        if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
            frames.push(frame);
        }
    }

    assert!(!frames.is_empty());
    let yuy2_frame = &frames[0];

    // Convert with YUYV format (correct for our generator)
    let rgb_yuyv = convert_yuv422_to_rgb(yuy2_frame, width, height, None, YuvPackedFormat::Yuyv)
        .expect("YUYV conversion should succeed");

    // Convert with UYVY format (incorrect - should produce different colors)
    let rgb_uyvy = convert_yuv422_to_rgb(yuy2_frame, width, height, None, YuvPackedFormat::Uyvy)
        .expect("UYVY conversion should succeed");

    // Both should produce output of same size
    assert_eq!(rgb_yuyv.len(), rgb_uyvy.len());

    // But the colors should be different (wrong format produces wrong colors)
    // YUYV should produce correct red, UYVY should produce incorrect colors
    let (r_yuyv, g_yuyv, b_yuyv) = (rgb_yuyv[0], rgb_yuyv[1], rgb_yuyv[2]);
    let (r_uyvy, g_uyvy, b_uyvy) = (rgb_uyvy[0], rgb_uyvy[1], rgb_uyvy[2]);

    // YUYV should have high red (correct format)
    assert!(
        r_yuyv > 150,
        "YUYV (correct) should have high red: R={}",
        r_yuyv
    );

    // UYVY result should be different (wrong format swaps byte interpretation)
    assert!(
        (r_yuyv as i32 - r_uyvy as i32).abs() > 20
            || (g_yuyv as i32 - g_uyvy as i32).abs() > 20
            || (b_yuyv as i32 - b_uyvy as i32).abs() > 20,
        "YUYV and UYVY should produce different colors. YUYV: R={},G={},B={} UYVY: R={},G={},B={}",
        r_yuyv,
        g_yuyv,
        b_yuyv,
        r_uyvy,
        g_uyvy,
        b_uyvy
    );
}

// ============================================================================
// Stride Detection Tests
// ============================================================================

#[test]
fn test_pipeline_auto_stride_detection() {
    let mut gen = PacketGenerator::new(2048);
    let width = 64u32;
    let height = 32u32;

    let _packets1 = gen.yuy2_solid_frame(width, height, Rgb::WHITE);
    let packets2 = gen.yuy2_solid_frame(width, height, Rgb::WHITE);

    let mut assembler = FrameAssembler::new_yuy2(width, height);
    for packet in &_packets1 {
        assembler.process_packet(packet);
    }

    let mut frames = Vec::new();
    for packet in &packets2 {
        if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
            frames.push(frame);
        }
    }

    assert!(!frames.is_empty());
    let yuy2_frame = &frames[0];

    // Convert with no stride override (auto-detect)
    let rgb_auto = convert_yuv422_to_rgb(yuy2_frame, width, height, None, YuvPackedFormat::Yuyv)
        .expect("Auto stride should work");

    // Convert with explicit stride
    let explicit_stride = width * 2;
    let rgb_explicit = convert_yuv422_to_rgb(
        yuy2_frame,
        width,
        height,
        Some(explicit_stride),
        YuvPackedFormat::Yuyv,
    )
    .expect("Explicit stride should work");

    // Both should produce identical output for correctly-sized frames
    assert_eq!(
        rgb_auto, rgb_explicit,
        "Auto and explicit stride should match"
    );
}

// ============================================================================
// Frame Size Variation Tests
// ============================================================================

#[test]
fn test_pipeline_various_resolutions() {
    let resolutions = [
        (32u32, 32u32),   // Tiny
        (64u32, 48u32),   // Small
        (160u32, 120u32), // QSIF
        (320u32, 240u32), // QVGA
    ];

    for (width, height) in resolutions {
        let mut gen = PacketGenerator::new(4096);

        let _packets1 = gen.yuy2_solid_frame(width, height, Rgb::GRAY);
        let packets2 = gen.yuy2_solid_frame(width, height, Rgb::GRAY);

        let mut assembler = FrameAssembler::new_yuy2(width, height);
        for packet in &_packets1 {
            assembler.process_packet(packet);
        }

        let mut frames = Vec::new();
        for packet in &packets2 {
            if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
                frames.push(frame);
            }
        }

        assert!(
            !frames.is_empty(),
            "Should produce frame for {}x{}",
            width,
            height
        );
        let yuy2_frame = &frames[0];

        let expected_size = (width * height * 2) as usize;
        assert_eq!(
            yuy2_frame.len(),
            expected_size,
            "YUY2 frame size for {}x{}",
            width,
            height
        );

        let validation = validate_yuy2_frame(
            yuy2_frame,
            width as usize,
            height as usize,
            expected_size,
            ValidationLevel::Strict,
        );
        assert!(
            validation.valid,
            "{}x{} should be valid: {:?}",
            width, height, validation.failure_reason
        );

        let rgb = convert_yuv422_to_rgb(yuy2_frame, width, height, None, YuvPackedFormat::Yuyv)
            .unwrap_or_else(|_| panic!("{}x{} conversion should succeed", width, height));

        let expected_rgb_size = (width * height * 3) as usize;
        assert_eq!(
            rgb.len(),
            expected_rgb_size,
            "RGB size for {}x{}",
            width,
            height
        );
    }
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_pipeline_undersized_frame_validation() {
    // Create an intentionally small frame
    let width = 64usize;
    let height = 48usize;
    let expected_size = width * height * 2;
    let undersized_data = vec![128u8; expected_size / 2]; // Half the expected size

    // Strict validation should fail
    let validation = validate_yuy2_frame(
        &undersized_data,
        width,
        height,
        expected_size,
        ValidationLevel::Strict,
    );
    assert!(
        !validation.valid,
        "Undersized frame should fail strict validation"
    );
    assert!(validation.failure_reason.is_some());

    // Minimal validation might pass (depending on ratio)
    let validation_minimal = validate_yuy2_frame(
        &undersized_data,
        width,
        height,
        expected_size,
        ValidationLevel::Minimal,
    );
    // 50% ratio should pass minimal (0.5..=2.0 range)
    assert!(
        validation_minimal.valid,
        "50% size should pass minimal validation"
    );
}

#[test]
fn test_pipeline_conversion_error_on_small_data() {
    let width = 640u32;
    let height = 480u32;
    let small_data = vec![0u8; 100]; // Way too small

    let result = convert_yuv422_to_rgb(&small_data, width, height, None, YuvPackedFormat::Yuyv);
    assert!(result.is_err(), "Conversion should fail on small data");

    let err = result.unwrap_err();
    assert!(
        err.0.contains("too small"),
        "Error should mention data is too small: {}",
        err.0
    );
}

// ============================================================================
// Packet Fragmentation Tests
// ============================================================================

#[test]
fn test_pipeline_small_packet_fragmentation() {
    // Test with very small packets (simulates USB packet fragmentation)
    let mut gen = PacketGenerator::new(64); // Very small packets
    let width = 32u32;
    let height = 32u32;

    let _packets1 = gen.yuy2_solid_frame(width, height, Rgb::RED);
    let packets2 = gen.yuy2_solid_frame(width, height, Rgb::RED);

    // 32x32 YUY2 = 2048 bytes, with 64-byte max payload = many packets
    assert!(
        packets2.len() > 20,
        "Should have many small packets: {}",
        packets2.len()
    );

    let mut assembler = FrameAssembler::new_yuy2(width, height);
    for packet in &_packets1 {
        assembler.process_packet(packet);
    }

    let mut frames = Vec::new();
    for packet in &packets2 {
        if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
            frames.push(frame);
        }
    }

    assert!(!frames.is_empty(), "Should assemble from small packets");
    let yuy2_frame = &frames[0];

    // Full pipeline should still work
    let expected_size = (width * height * 2) as usize;
    let validation = validate_yuy2_frame(
        yuy2_frame,
        width as usize,
        height as usize,
        expected_size,
        ValidationLevel::Strict,
    );
    assert!(validation.valid, "Small-packet frame should be valid");

    let rgb = convert_yuv422_to_rgb(yuy2_frame, width, height, None, YuvPackedFormat::Yuyv)
        .expect("Conversion should succeed");
    assert_eq!(rgb.len(), (width * height * 3) as usize);
}

#[test]
fn test_pipeline_large_packet_single_frame() {
    // Test with packets large enough to hold entire frame
    let width = 32u32;
    let height = 16u32;
    let frame_size = (width * height * 2) as usize; // 1024 bytes

    let mut gen = PacketGenerator::new(frame_size + 100); // Larger than frame
    let _packets1 = gen.yuy2_solid_frame(width, height, Rgb::BLUE);
    let packets2 = gen.yuy2_solid_frame(width, height, Rgb::BLUE);

    // Should be just 1 packet per frame
    assert_eq!(packets2.len(), 1, "Frame should fit in single packet");

    let mut assembler = FrameAssembler::new_yuy2(width, height);
    for packet in &_packets1 {
        assembler.process_packet(packet);
    }

    let mut frames = Vec::new();
    for packet in &packets2 {
        if let ProcessResult::Frame(frame) = assembler.process_packet(packet) {
            frames.push(frame);
        }
    }

    assert!(!frames.is_empty(), "Should assemble single-packet frame");

    let expected_size = (width * height * 2) as usize;
    let validation = validate_yuy2_frame(
        &frames[0],
        width as usize,
        height as usize,
        expected_size,
        ValidationLevel::Strict,
    );
    assert!(validation.valid, "Single-packet frame should be valid");
}
