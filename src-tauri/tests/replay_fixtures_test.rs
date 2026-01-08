//! Integration tests for replay fixtures.
//!
//! These tests verify that the test fixture files in `tests/fixtures/` can be
//! loaded and replayed correctly.

use clean_scope_lib::replay::{PacketReplay, ReplayConfig};
use std::path::Path;

/// Path to the MJPEG 640x480 test fixture directory.
fn mjpeg_fixture_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mjpeg_640x480")
}

#[test]
fn test_load_mjpeg_fixture() {
    let capture_path = mjpeg_fixture_dir().join("capture.bin");
    assert!(capture_path.exists(), "Fixture file should exist");

    let replay = PacketReplay::load(&capture_path).expect("Should load fixture");

    // Verify packet count (3 packets: sync frame, emitted frame, trigger packet)
    assert_eq!(replay.packet_count(), 3, "Should have 3 packets");

    // Verify metadata was loaded
    let metadata = replay.metadata().expect("Should have metadata");
    assert_eq!(metadata.vendor_id, 0x1234);
    assert_eq!(metadata.product_id, 0x5678);
    assert_eq!(metadata.format_type, "mjpeg");
    assert_eq!(metadata.width, 640);
    assert_eq!(metadata.height, 480);
    assert_eq!(metadata.total_frames, 1);
}

#[test]
fn test_replay_mjpeg_fixture_produces_frame() {
    let capture_path = mjpeg_fixture_dir().join("capture.bin");

    let config = ReplayConfig {
        speed: 0.0,        // As fast as possible
        force_mjpeg: true, // Force MJPEG mode
        ..Default::default()
    };

    let mut replay =
        PacketReplay::load_with_config(&capture_path, config).expect("Should load fixture");

    let receiver = replay.start().expect("Should start replay");

    // Collect frames with a timeout
    let frame = receiver
        .recv_timeout(std::time::Duration::from_millis(100))
        .expect("Should receive at least one frame");

    // Verify frame is a valid JPEG (starts with SOI marker)
    assert!(frame.len() >= 2, "Frame should have at least 2 bytes");
    assert_eq!(frame[0], 0xFF, "Frame should start with JPEG SOI marker");
    assert_eq!(frame[1], 0xD8, "Frame should start with JPEG SOI marker");

    // Verify frame ends with EOI marker
    let len = frame.len();
    assert_eq!(
        frame[len - 2],
        0xFF,
        "Frame should end with JPEG EOI marker"
    );
    assert_eq!(
        frame[len - 1],
        0xD9,
        "Frame should end with JPEG EOI marker"
    );

    replay.stop().expect("Should stop replay");
}

#[test]
fn test_fixture_metadata_matches_capture() {
    let fixture_dir = mjpeg_fixture_dir();
    let capture_path = fixture_dir.join("capture.bin");
    // Metadata file uses same basename as capture with .json extension
    let metadata_path = fixture_dir.join("capture.json");

    assert!(capture_path.exists(), "capture.bin should exist");
    assert!(metadata_path.exists(), "capture.json should exist");

    // Load metadata directly
    let metadata_json = std::fs::read_to_string(&metadata_path).expect("Should read metadata");
    let metadata: serde_json::Value =
        serde_json::from_str(&metadata_json).expect("Should parse JSON");

    // Verify required fields exist
    assert!(
        metadata.get("format_type").is_some(),
        "Should have format_type"
    );
    assert!(metadata.get("width").is_some(), "Should have width");
    assert!(metadata.get("height").is_some(), "Should have height");
    assert!(
        metadata.get("total_packets").is_some(),
        "Should have total_packets"
    );
    assert!(
        metadata.get("total_frames").is_some(),
        "Should have total_frames"
    );
}
