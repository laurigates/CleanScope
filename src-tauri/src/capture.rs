//! USB packet capture module for testing and debugging.
//!
//! This module provides thread-safe packet capture functionality for recording
//! USB data streams from UVC devices. Captured packets can be saved to disk
//! for offline analysis and replay testing.
//!
//! # File Format
//!
//! Packets are stored in a binary format:
//! - `packets.bin`: Sequence of `[u32 LE: length][bytes: data]...`
//! - `metadata.json`: Device and capture information
//!
//! # Example
//!
//! ```ignore
//! let capture = CaptureState::new();
//! capture.start_capture(CaptureMetadata {
//!     vendor_id: 0x1234,
//!     product_id: 0x5678,
//!     ..Default::default()
//! });
//!
//! // In USB callback:
//! capture.record_packet(&packet_data);
//!
//! // When done:
//! let result = capture.stop_capture(Path::new("/output"))?;
//! ```

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use thiserror::Error;

/// Errors that can occur during packet capture operations.
#[derive(Error, Debug)]
pub enum CaptureError {
    /// Capture is not currently active when trying to record or stop.
    #[error("capture is not active")]
    NotActive,

    /// Capture is already active when trying to start.
    #[error("capture is already active")]
    AlreadyActive,

    /// Failed to acquire lock on internal state.
    #[error("failed to acquire lock: {0}")]
    LockError(String),

    /// I/O error during file operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization error.
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    /// Output directory does not exist.
    #[error("output directory does not exist: {0}")]
    DirectoryNotFound(String),
}

/// Result type alias for capture operations.
pub type Result<T> = std::result::Result<T, CaptureError>;

/// Metadata about the capture session and device.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaptureMetadata {
    /// USB vendor ID of the device.
    pub vendor_id: u16,
    /// USB product ID of the device.
    pub product_id: u16,
    /// Video format type (e.g., "mjpeg", "yuy2", "unknown").
    pub format_type: String,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Total number of packets captured.
    #[serde(default)]
    pub total_packets: u64,
    /// Total number of complete frames captured.
    #[serde(default)]
    pub total_frames: u64,
    /// Capture duration in milliseconds.
    #[serde(default)]
    pub duration_ms: u64,
    /// Total bytes captured.
    #[serde(default)]
    pub total_bytes: u64,
    /// Optional description or notes about the capture.
    #[serde(default)]
    pub description: String,
}

/// Result returned when capture stops successfully.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureResult {
    /// Path to the saved packets.bin file.
    pub packets_path: String,
    /// Path to the saved metadata.json file.
    pub metadata_path: String,
    /// Summary of the capture session.
    pub metadata: CaptureMetadata,
}

/// Thread-safe state for recording USB packets.
///
/// This struct manages the capture lifecycle and provides thread-safe
/// access for recording packets from USB callback threads.
pub struct CaptureState {
    /// Whether capture is currently active.
    is_capturing: AtomicBool,
    /// Captured packet data (each packet is a `Vec<u8>`).
    packets: Mutex<Vec<Vec<u8>>>,
    /// When the capture started.
    start_time: Mutex<Option<Instant>>,
    /// Metadata about the capture session.
    metadata: Mutex<CaptureMetadata>,
    /// Atomic counter for total packets (fast path for USB callback).
    packet_count: AtomicU64,
    /// Atomic counter for total bytes (fast path for USB callback).
    byte_count: AtomicU64,
}

impl CaptureState {
    /// Creates a new capture state with no active capture.
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_capturing: AtomicBool::new(false),
            packets: Mutex::new(Vec::new()),
            start_time: Mutex::new(None),
            metadata: Mutex::new(CaptureMetadata::default()),
            packet_count: AtomicU64::new(0),
            byte_count: AtomicU64::new(0),
        }
    }

    /// Returns whether capture is currently active.
    #[must_use]
    pub fn is_capturing(&self) -> bool {
        self.is_capturing.load(Ordering::Acquire)
    }

    /// Returns the current packet count (thread-safe, lock-free).
    #[must_use]
    pub fn packet_count(&self) -> u64 {
        self.packet_count.load(Ordering::Relaxed)
    }

    /// Returns the current byte count (thread-safe, lock-free).
    #[must_use]
    pub fn byte_count(&self) -> u64 {
        self.byte_count.load(Ordering::Relaxed)
    }

    /// Starts a new capture session.
    ///
    /// # Arguments
    ///
    /// * `metadata` - Initial metadata about the device and format.
    ///
    /// # Errors
    ///
    /// Returns `CaptureError::AlreadyActive` if a capture is already in progress.
    /// Returns `CaptureError::LockError` if the internal mutex cannot be acquired.
    pub fn start_capture(&self, metadata: CaptureMetadata) -> Result<()> {
        // Check if already capturing (compare_exchange for atomicity)
        if self
            .is_capturing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(CaptureError::AlreadyActive);
        }

        // Clear previous capture data
        {
            let mut packets = self
                .packets
                .lock()
                .map_err(|e| CaptureError::LockError(e.to_string()))?;
            packets.clear();
        }

        // Reset counters
        self.packet_count.store(0, Ordering::Release);
        self.byte_count.store(0, Ordering::Release);

        // Set start time
        {
            let mut start_time = self
                .start_time
                .lock()
                .map_err(|e| CaptureError::LockError(e.to_string()))?;
            *start_time = Some(Instant::now());
        }

        // Store metadata
        {
            let mut meta = self
                .metadata
                .lock()
                .map_err(|e| CaptureError::LockError(e.to_string()))?;
            *meta = metadata;
        }

        log::info!("Packet capture started");
        Ok(())
    }

    /// Records a packet during capture.
    ///
    /// This method is designed to be called from USB callback threads and
    /// is optimized for minimal blocking. If capture is not active, the
    /// packet is silently ignored.
    ///
    /// # Arguments
    ///
    /// * `packet` - Raw packet data to record.
    pub fn record_packet(&self, packet: &[u8]) {
        // Fast path: check if capturing without locking
        if !self.is_capturing.load(Ordering::Acquire) {
            return;
        }

        // Update atomic counters (lock-free)
        self.packet_count.fetch_add(1, Ordering::Relaxed);
        self.byte_count
            .fetch_add(packet.len() as u64, Ordering::Relaxed);

        // Store packet data (requires lock)
        if let Ok(mut packets) = self.packets.lock() {
            packets.push(packet.to_vec());
        } else {
            log::warn!("Failed to acquire lock for packet recording");
        }
    }

    /// Increments the frame counter in metadata.
    ///
    /// Call this when a complete frame has been assembled.
    pub fn record_frame(&self) {
        if !self.is_capturing.load(Ordering::Acquire) {
            return;
        }

        if let Ok(mut meta) = self.metadata.lock() {
            meta.total_frames += 1;
        }
    }

    /// Stops the capture and saves data to disk.
    ///
    /// # Arguments
    ///
    /// * `output_dir` - Directory where `packets.bin` and `metadata.json` will be saved.
    ///
    /// # Errors
    ///
    /// Returns `CaptureError::NotActive` if no capture is in progress.
    /// Returns `CaptureError::DirectoryNotFound` if the output directory doesn't exist.
    /// Returns `CaptureError::Io` if file operations fail.
    /// Returns `CaptureError::Json` if metadata serialization fails.
    pub fn stop_capture(&self, output_dir: &Path) -> Result<CaptureResult> {
        // Check if capturing
        if self
            .is_capturing
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(CaptureError::NotActive);
        }

        // Verify output directory exists
        if !output_dir.exists() {
            return Err(CaptureError::DirectoryNotFound(
                output_dir.display().to_string(),
            ));
        }

        // Calculate duration
        let duration_ms = {
            let start_time = self
                .start_time
                .lock()
                .map_err(|e| CaptureError::LockError(e.to_string()))?;
            start_time
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0)
        };

        // Get final counts
        let total_packets = self.packet_count.load(Ordering::Acquire);
        let total_bytes = self.byte_count.load(Ordering::Acquire);

        // Update metadata with final stats
        let metadata = {
            let mut meta = self
                .metadata
                .lock()
                .map_err(|e| CaptureError::LockError(e.to_string()))?;
            meta.duration_ms = duration_ms;
            meta.total_packets = total_packets;
            meta.total_bytes = total_bytes;
            meta.clone()
        };

        // Generate timestamp for filenames
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Save packets to binary file
        let packets_filename = format!("packets_{}.bin", timestamp);
        let packets_path = output_dir.join(&packets_filename);
        self.save_packets(&packets_path)?;

        // Save metadata to JSON file
        let metadata_filename = format!("metadata_{}.json", timestamp);
        let metadata_path = output_dir.join(&metadata_filename);
        self.save_metadata(&metadata_path, &metadata)?;

        log::info!(
            "Capture stopped: {} packets, {} bytes, {} ms",
            total_packets,
            total_bytes,
            duration_ms
        );

        Ok(CaptureResult {
            packets_path: packets_path.display().to_string(),
            metadata_path: metadata_path.display().to_string(),
            metadata,
        })
    }

    /// Cancels the current capture without saving.
    ///
    /// This is useful for aborting a capture due to errors.
    pub fn cancel_capture(&self) {
        self.is_capturing.store(false, Ordering::Release);
        if let Ok(mut packets) = self.packets.lock() {
            packets.clear();
        }
        log::info!("Capture cancelled");
    }

    /// Saves packets to a binary file.
    ///
    /// Format: `[u32 LE: packet_length][bytes: packet_data]...`
    fn save_packets(&self, path: &Path) -> Result<()> {
        let packets = self
            .packets
            .lock()
            .map_err(|e| CaptureError::LockError(e.to_string()))?;

        let mut file = std::fs::File::create(path)?;

        for packet in packets.iter() {
            // Write packet length as u32 little-endian
            let len = packet.len() as u32;
            file.write_all(&len.to_le_bytes())?;

            // Write packet data
            file.write_all(packet)?;
        }

        file.flush()?;
        log::debug!("Saved {} packets to {}", packets.len(), path.display());

        Ok(())
    }

    /// Saves metadata to a JSON file.
    fn save_metadata(&self, path: &Path, metadata: &CaptureMetadata) -> Result<()> {
        let json = serde_json::to_string_pretty(metadata)?;
        std::fs::write(path, json)?;
        log::debug!("Saved metadata to {}", path.display());
        Ok(())
    }
}

impl Default for CaptureState {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Legacy API Compatibility
// =============================================================================
// The following types and methods maintain backward compatibility with the
// existing lib.rs integration. New code should prefer the `start_capture`,
// `record_packet`, and `stop_capture` API.

/// Current status of the capture system (legacy API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureStatus {
    /// Whether capture is currently active.
    pub is_capturing: bool,
    /// Number of packets captured so far.
    pub packet_count: u64,
    /// Duration since capture started (milliseconds).
    pub duration_ms: u64,
    /// Total bytes captured.
    pub total_bytes: u64,
}

/// A single captured packet with timestamp (legacy API).
#[derive(Debug, Clone)]
pub struct CapturedPacket {
    /// Timestamp relative to capture start (microseconds).
    pub timestamp_us: u64,
    /// Raw packet data.
    pub data: Vec<u8>,
    /// Packet type/endpoint info.
    pub endpoint: u8,
}

impl CaptureState {
    /// Start capturing packets (legacy API).
    ///
    /// This is a simplified start that doesn't require metadata.
    /// Use `start_capture` for the new API with device metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if capture is already in progress.
    pub fn start(&self) -> std::result::Result<(), String> {
        self.start_capture(CaptureMetadata::default())
            .map_err(|e| e.to_string())
    }

    /// Stop capturing and return captured packets (legacy API).
    ///
    /// This returns packets directly instead of saving to disk.
    /// Use `stop_capture` for the new API that saves to files.
    pub fn stop(&self) -> Vec<CapturedPacket> {
        // Set capturing to false
        self.is_capturing.store(false, Ordering::Release);

        // Get duration for timestamps
        let start_time = self.start_time.lock().ok().and_then(|g| *g);

        // Extract packets with timestamps
        let packets = if let Ok(mut p) = self.packets.lock() {
            std::mem::take(&mut *p)
        } else {
            Vec::new()
        };

        log::info!(
            "Packet capture stopped: {} packets, {} bytes",
            packets.len(),
            self.byte_count.load(Ordering::Acquire)
        );

        // Convert to CapturedPacket format
        // Note: Since we don't store timestamps per-packet in the new format,
        // we estimate based on packet index
        let duration_us = start_time
            .map(|t| t.elapsed().as_micros() as u64)
            .unwrap_or(0);
        let packet_count = packets.len() as u64;

        packets
            .into_iter()
            .enumerate()
            .map(|(i, data)| {
                let timestamp_us = if packet_count > 1 {
                    (duration_us * i as u64) / (packet_count - 1).max(1)
                } else {
                    0
                };
                CapturedPacket {
                    timestamp_us,
                    data,
                    endpoint: 0, // Endpoint info not captured in new format
                }
            })
            .collect()
    }

    /// Get current capture status (legacy API).
    #[must_use]
    pub fn status(&self) -> CaptureStatus {
        let duration_ms = if let Ok(start) = self.start_time.lock() {
            start.map(|s| s.elapsed().as_millis() as u64).unwrap_or(0)
        } else {
            0
        };

        CaptureStatus {
            is_capturing: self.is_capturing.load(Ordering::Acquire),
            packet_count: self.packet_count.load(Ordering::Relaxed),
            duration_ms,
            total_bytes: self.byte_count.load(Ordering::Relaxed),
        }
    }

    /// Add a packet to the capture buffer with endpoint info (legacy API).
    ///
    /// Called during streaming. Use `record_packet` for the new API.
    pub fn add_packet(&self, data: &[u8], _endpoint: u8) {
        // Delegate to new API (endpoint info is not preserved)
        self.record_packet(data);
    }
}

/// Write captured packets to files (legacy API).
///
/// Creates two files in the specified directory:
/// - `capture_<timestamp>.bin` - Raw packet data with headers
/// - `capture_<timestamp>.json` - Metadata about the capture
///
/// # Errors
///
/// Returns an error string if file operations fail.
pub fn write_capture_files(
    cache_dir: &std::path::Path,
    packets: &[CapturedPacket],
    duration_ms: u64,
) -> std::result::Result<CaptureResult, String> {
    use std::io::Write as _;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Calculate totals
    let packet_count = packets.len() as u64;
    let total_bytes: u64 = packets.iter().map(|p| p.data.len() as u64).sum();

    // Write binary packet file (legacy format with timestamps)
    let packets_filename = format!("capture_{}.bin", timestamp);
    let packets_path = cache_dir.join(&packets_filename);

    let mut file = std::fs::File::create(&packets_path)
        .map_err(|e| format!("Could not create file: {}", e))?;

    // Write packet data with simple header format:
    // [8 bytes: timestamp_us][4 bytes: length][1 byte: endpoint][data...]
    for packet in packets {
        file.write_all(&packet.timestamp_us.to_le_bytes())
            .map_err(|e| format!("Write error: {}", e))?;
        file.write_all(&(packet.data.len() as u32).to_le_bytes())
            .map_err(|e| format!("Write error: {}", e))?;
        file.write_all(&[packet.endpoint])
            .map_err(|e| format!("Write error: {}", e))?;
        file.write_all(&packet.data)
            .map_err(|e| format!("Write error: {}", e))?;
    }

    // Write metadata JSON
    let metadata_filename = format!("capture_{}.json", timestamp);
    let metadata_path = cache_dir.join(&metadata_filename);

    let metadata = CaptureMetadata {
        total_packets: packet_count,
        total_bytes,
        duration_ms,
        ..Default::default()
    };

    let json = serde_json::to_string_pretty(&metadata).map_err(|e| format!("JSON error: {}", e))?;
    std::fs::write(&metadata_path, json).map_err(|e| format!("Could not write metadata: {}", e))?;

    log::info!(
        "Capture saved: {} packets, {} bytes to {}",
        packet_count,
        total_bytes,
        packets_path.display()
    );

    Ok(CaptureResult {
        packets_path: packets_path.to_string_lossy().to_string(),
        metadata_path: metadata_path.to_string_lossy().to_string(),
        metadata,
    })
}

// =============================================================================
// File Reading Utilities
// =============================================================================

/// Reads packets from a binary capture file.
///
/// # Arguments
///
/// * `path` - Path to the `packets.bin` file.
///
/// # Returns
///
/// A vector of packets, where each packet is a `Vec<u8>`.
///
/// # Errors
///
/// Returns `CaptureError::Io` if file operations fail.
pub fn read_packets(path: &Path) -> Result<Vec<Vec<u8>>> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut packets = Vec::new();

    loop {
        // Read packet length (u32 little-endian)
        let mut len_bytes = [0u8; 4];
        match file.read_exact(&mut len_bytes) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(CaptureError::Io(e)),
        }

        let len = u32::from_le_bytes(len_bytes) as usize;

        // Read packet data
        let mut packet = vec![0u8; len];
        file.read_exact(&mut packet)?;

        packets.push(packet);
    }

    Ok(packets)
}

/// Reads capture metadata from a JSON file.
///
/// # Arguments
///
/// * `path` - Path to the `metadata.json` file.
///
/// # Errors
///
/// Returns `CaptureError::Io` if the file cannot be read.
/// Returns `CaptureError::Json` if the JSON is invalid.
pub fn read_metadata(path: &Path) -> Result<CaptureMetadata> {
    let json = std::fs::read_to_string(path)?;
    let metadata: CaptureMetadata = serde_json::from_str(&json)?;
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_capture_state_new() {
        let state = CaptureState::new();
        assert!(!state.is_capturing());
        assert_eq!(state.packet_count(), 0);
        assert_eq!(state.byte_count(), 0);
    }

    #[test]
    fn test_start_capture() {
        let state = CaptureState::new();
        let metadata = CaptureMetadata {
            vendor_id: 0x1234,
            product_id: 0x5678,
            format_type: "mjpeg".to_string(),
            width: 1280,
            height: 720,
            ..Default::default()
        };

        state.start_capture(metadata).unwrap();
        assert!(state.is_capturing());
    }

    #[test]
    fn test_start_capture_already_active() {
        let state = CaptureState::new();
        let metadata = CaptureMetadata::default();

        state.start_capture(metadata.clone()).unwrap();
        let result = state.start_capture(metadata);

        assert!(matches!(result, Err(CaptureError::AlreadyActive)));
    }

    #[test]
    fn test_record_packet() {
        let state = CaptureState::new();
        state.start_capture(CaptureMetadata::default()).unwrap();

        let packet1 = vec![0xFFu8, 0xD8, 0xFF, 0xE0];
        let packet2 = vec![0x00u8, 0x01, 0x02, 0x03, 0x04];

        state.record_packet(&packet1);
        state.record_packet(&packet2);

        assert_eq!(state.packet_count(), 2);
        assert_eq!(state.byte_count(), 9);
    }

    #[test]
    fn test_record_packet_when_not_capturing() {
        let state = CaptureState::new();

        // Should silently ignore packets when not capturing
        state.record_packet(&[0x00, 0x01, 0x02]);

        assert_eq!(state.packet_count(), 0);
        assert_eq!(state.byte_count(), 0);
    }

    #[test]
    fn test_cancel_capture() {
        let state = CaptureState::new();
        state.start_capture(CaptureMetadata::default()).unwrap();
        state.record_packet(&[0x00, 0x01]);

        state.cancel_capture();

        assert!(!state.is_capturing());
        // Can start a new capture after cancel
        state.start_capture(CaptureMetadata::default()).unwrap();
        assert!(state.is_capturing());
    }

    #[test]
    fn test_stop_capture_not_active() {
        let state = CaptureState::new();
        let result = state.stop_capture(Path::new("/tmp"));

        assert!(matches!(result, Err(CaptureError::NotActive)));
    }

    #[test]
    fn test_concurrent_packet_recording() {
        let state = Arc::new(CaptureState::new());
        state.start_capture(CaptureMetadata::default()).unwrap();

        let mut handles = vec![];

        for i in 0..10 {
            let state_clone = Arc::clone(&state);
            handles.push(thread::spawn(move || {
                for j in 0..100 {
                    let packet = vec![(i * 100 + j) as u8; 10];
                    state_clone.record_packet(&packet);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(state.packet_count(), 1000);
        assert_eq!(state.byte_count(), 10000);
    }

    #[test]
    fn test_save_and_read_packets() {
        let temp_dir = std::env::temp_dir();
        let packets_path = temp_dir.join("test_packets.bin");

        // Create test packets
        let packets = vec![
            vec![0xFFu8, 0xD8, 0xFF, 0xE0],
            vec![0x00u8, 0x01, 0x02],
            vec![0xAAu8; 1000],
        ];

        // Write packets manually for testing read function
        {
            let mut file = std::fs::File::create(&packets_path).unwrap();
            for packet in &packets {
                let len = packet.len() as u32;
                file.write_all(&len.to_le_bytes()).unwrap();
                file.write_all(packet).unwrap();
            }
        }

        // Read packets back
        let read_packets = read_packets(&packets_path).unwrap();

        assert_eq!(read_packets.len(), 3);
        assert_eq!(read_packets[0], packets[0]);
        assert_eq!(read_packets[1], packets[1]);
        assert_eq!(read_packets[2], packets[2]);

        // Cleanup
        std::fs::remove_file(&packets_path).ok();
    }

    #[test]
    fn test_save_and_read_metadata() {
        let temp_dir = std::env::temp_dir();
        let metadata_path = temp_dir.join("test_metadata.json");

        let metadata = CaptureMetadata {
            vendor_id: 0x1234,
            product_id: 0x5678,
            format_type: "yuy2".to_string(),
            width: 1920,
            height: 1080,
            total_packets: 500,
            total_frames: 30,
            duration_ms: 1000,
            total_bytes: 50000,
            description: "Test capture".to_string(),
        };

        // Write metadata
        let json = serde_json::to_string_pretty(&metadata).unwrap();
        std::fs::write(&metadata_path, json).unwrap();

        // Read metadata back
        let read_metadata = read_metadata(&metadata_path).unwrap();

        assert_eq!(read_metadata.vendor_id, 0x1234);
        assert_eq!(read_metadata.product_id, 0x5678);
        assert_eq!(read_metadata.format_type, "yuy2");
        assert_eq!(read_metadata.width, 1920);
        assert_eq!(read_metadata.height, 1080);
        assert_eq!(read_metadata.total_packets, 500);
        assert_eq!(read_metadata.total_frames, 30);
        assert_eq!(read_metadata.duration_ms, 1000);
        assert_eq!(read_metadata.total_bytes, 50000);
        assert_eq!(read_metadata.description, "Test capture");

        // Cleanup
        std::fs::remove_file(&metadata_path).ok();
    }

    #[test]
    fn test_full_capture_workflow() {
        let temp_dir = std::env::temp_dir();
        let state = CaptureState::new();

        let metadata = CaptureMetadata {
            vendor_id: 0xABCD,
            product_id: 0xEF01,
            format_type: "mjpeg".to_string(),
            width: 640,
            height: 480,
            ..Default::default()
        };

        // Start capture
        state.start_capture(metadata).unwrap();

        // Record some packets
        for i in 0..10 {
            let packet = vec![i as u8; (i + 1) * 10];
            state.record_packet(&packet);
        }

        // Record some frames
        state.record_frame();
        state.record_frame();

        // Stop capture
        let result = state.stop_capture(&temp_dir).unwrap();

        // Verify result
        assert_eq!(result.metadata.vendor_id, 0xABCD);
        assert_eq!(result.metadata.product_id, 0xEF01);
        assert_eq!(result.metadata.total_packets, 10);
        assert_eq!(result.metadata.total_frames, 2);
        // duration_ms is u64, always >= 0

        // Verify files exist
        assert!(Path::new(&result.packets_path).exists());
        assert!(Path::new(&result.metadata_path).exists());

        // Read back and verify
        let packets = read_packets(Path::new(&result.packets_path)).unwrap();
        assert_eq!(packets.len(), 10);

        let read_meta = read_metadata(Path::new(&result.metadata_path)).unwrap();
        assert_eq!(read_meta.vendor_id, 0xABCD);

        // Cleanup
        std::fs::remove_file(&result.packets_path).ok();
        std::fs::remove_file(&result.metadata_path).ok();
    }
}
