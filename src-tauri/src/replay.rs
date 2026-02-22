//! USB packet replay module for desktop testing without physical hardware.
//!
//! This module provides functionality to replay captured USB packets from binary
//! files, simulating a real USB camera device for testing and development.
//!
//! # File Format
//!
//! Supports the legacy capture format from `capture::write_capture_files`:
//! ```text
//! [u64 LE: timestamp_us][u32 LE: length][u8: endpoint][data bytes]...
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use std::path::Path;
//! use clean_scope_lib::replay::PacketReplay;
//!
//! let mut replay = PacketReplay::load(Path::new("capture_12345.bin"))?;
//!
//! // Start replay and receive frames via channel
//! let receiver = replay.start()?;
//!
//! while let Ok(frame) = receiver.recv() {
//!     // Process assembled frame
//!     process_frame(&frame);
//! }
//! ```

use std::io::Read;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use thiserror::Error;

use crate::capture::{read_metadata, CaptureMetadata};
use crate::frame_assembler::{FrameAssembler, ProcessResult};

/// Errors that can occur during packet replay operations.
#[derive(Error, Debug)]
pub enum ReplayError {
    /// File not found or cannot be opened.
    #[error("failed to open file: {0}")]
    FileOpen(#[from] std::io::Error),

    /// Invalid or corrupted packet data in the capture file.
    #[error("invalid packet data at offset {offset}: {message}")]
    InvalidPacket {
        /// Byte offset in the capture file where the error occurred.
        offset: u64,
        /// Description of the error.
        message: String,
    },

    /// Metadata file is missing or invalid.
    #[error("metadata error: {0}")]
    Metadata(String),

    /// Replay is already running.
    #[error("replay is already running")]
    AlreadyRunning,

    /// Replay is not running.
    #[error("replay is not running")]
    NotRunning,

    /// Channel send error.
    #[error("channel closed")]
    ChannelClosed,
}

/// Result type alias for replay operations.
pub type Result<T> = std::result::Result<T, ReplayError>;

/// A single captured packet with timing information.
#[derive(Debug, Clone)]
pub struct ReplayPacket {
    /// Timestamp relative to capture start (microseconds).
    pub timestamp_us: u64,
    /// USB endpoint this packet was received on.
    pub endpoint: u8,
    /// Raw packet data.
    pub data: Vec<u8>,
}

/// Configuration for packet replay.
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    /// Playback speed multiplier (1.0 = realtime, 2.0 = 2x speed, 0.0 = as fast as possible).
    pub speed: f64,
    /// Whether to loop the replay when reaching the end.
    pub loop_playback: bool,
    /// Expected frame size for YUY2 (0 = auto-detect or MJPEG).
    pub expected_frame_size: usize,
    /// Force MJPEG mode (overrides auto-detection).
    pub force_mjpeg: bool,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            speed: 1.0,
            loop_playback: false,
            expected_frame_size: 0,
            force_mjpeg: false,
        }
    }
}

/// Replays captured USB packets for desktop testing.
///
/// Loads packets from a binary capture file and replays them through the
/// [`FrameAssembler`] to produce complete frames for testing.
pub struct PacketReplay {
    /// Loaded packets ready for replay.
    packets: Vec<ReplayPacket>,
    /// Optional metadata from the capture session.
    metadata: Option<CaptureMetadata>,
    /// Replay configuration.
    config: ReplayConfig,
    /// Handle to the replay thread (if running).
    thread_handle: Option<JoinHandle<()>>,
    /// Sender to stop the replay.
    stop_sender: Option<Sender<()>>,
}

impl PacketReplay {
    /// Load captured packets from a binary file.
    ///
    /// Expects the legacy capture format:
    /// `[u64 LE: timestamp_us][u32 LE: length][u8: endpoint][data bytes]...`
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the binary capture file (e.g., `capture_12345.bin`).
    ///
    /// # Errors
    ///
    /// Returns `ReplayError::FileOpen` if the file cannot be opened.
    /// Returns `ReplayError::InvalidPacket` if the file contains corrupted data.
    pub fn load(path: &Path) -> Result<Self> {
        let packets = Self::read_packets_with_timestamps(path)?;

        // Try to load metadata from a companion .json file
        let metadata = Self::try_load_metadata(path);

        log::info!("Loaded {} packets from {}", packets.len(), path.display());

        if let Some(ref meta) = metadata {
            log::info!(
                "Metadata: {}x{} {}, {} frames, {} ms",
                meta.width,
                meta.height,
                meta.format_type,
                meta.total_frames,
                meta.duration_ms
            );
        }

        Ok(Self {
            packets,
            metadata,
            config: ReplayConfig::default(),
            thread_handle: None,
            stop_sender: None,
        })
    }

    /// Load packets with a custom configuration.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the binary capture file.
    /// * `config` - Replay configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or contains invalid packet data.
    pub fn load_with_config(path: &Path, config: ReplayConfig) -> Result<Self> {
        let mut replay = Self::load(path)?;
        replay.config = config;
        Ok(replay)
    }

    /// Read packets with timestamp information from a binary file.
    ///
    /// Format: `[u64 LE: timestamp_us][u32 LE: length][u8: endpoint][data bytes]...`
    fn read_packets_with_timestamps(path: &Path) -> Result<Vec<ReplayPacket>> {
        let mut file = std::fs::File::open(path)?;
        let file_size = file.metadata()?.len();
        let mut packets = Vec::new();
        let mut offset = 0u64;

        loop {
            // Read timestamp (8 bytes)
            let mut timestamp_bytes = [0u8; 8];
            match file.read_exact(&mut timestamp_bytes) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(ReplayError::FileOpen(e)),
            }
            let timestamp_us = u64::from_le_bytes(timestamp_bytes);

            // Read packet length (4 bytes)
            let mut len_bytes = [0u8; 4];
            file.read_exact(&mut len_bytes)
                .map_err(|_| ReplayError::InvalidPacket {
                    offset,
                    message: "unexpected EOF reading packet length".to_string(),
                })?;
            let len = u32::from_le_bytes(len_bytes) as usize;

            // Sanity check on length
            if len > 1024 * 1024 {
                return Err(ReplayError::InvalidPacket {
                    offset,
                    message: format!("packet length {} exceeds 1MB limit", len),
                });
            }

            // Read endpoint (1 byte)
            let mut endpoint_byte = [0u8; 1];
            file.read_exact(&mut endpoint_byte)
                .map_err(|_| ReplayError::InvalidPacket {
                    offset,
                    message: "unexpected EOF reading endpoint".to_string(),
                })?;
            let endpoint = endpoint_byte[0];

            // Read packet data
            let mut data = vec![0u8; len];
            file.read_exact(&mut data)
                .map_err(|_| ReplayError::InvalidPacket {
                    offset,
                    message: format!("unexpected EOF reading {} bytes of data", len),
                })?;

            packets.push(ReplayPacket {
                timestamp_us,
                endpoint,
                data,
            });

            // Update offset for error reporting
            offset += 8 + 4 + 1 + len as u64;

            // Safety check
            if offset > file_size {
                break;
            }
        }

        Ok(packets)
    }

    /// Try to load metadata from a companion JSON file.
    ///
    /// Looks for a file with the same base name but `.json` extension.
    fn try_load_metadata(path: &Path) -> Option<CaptureMetadata> {
        // Try same directory with .json extension
        let json_path = path.with_extension("json");
        if json_path.exists() {
            if let Ok(meta) = read_metadata(&json_path) {
                return Some(meta);
            }
        }

        // Try replacing _capture_ with _metadata_ pattern
        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
            if file_name.starts_with("capture_") {
                let json_name = file_name
                    .replace("capture_", "metadata_")
                    .replace(".bin", ".json");
                let json_path = path.with_file_name(&json_name);
                if json_path.exists() {
                    if let Ok(meta) = read_metadata(&json_path) {
                        return Some(meta);
                    }
                }
            }
        }

        None
    }

    /// Get the loaded metadata, if available.
    #[must_use]
    pub fn metadata(&self) -> Option<&CaptureMetadata> {
        self.metadata.as_ref()
    }

    /// Get the number of loaded packets.
    #[must_use]
    pub fn packet_count(&self) -> usize {
        self.packets.len()
    }

    /// Get the total duration of the capture in milliseconds.
    #[must_use]
    pub fn duration_ms(&self) -> u64 {
        self.packets
            .last()
            .map(|p| p.timestamp_us / 1000)
            .unwrap_or(0)
    }

    /// Set the replay configuration.
    pub fn set_config(&mut self, config: ReplayConfig) {
        self.config = config;
    }

    /// Check if replay is currently running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.thread_handle.is_some()
    }

    /// Start replaying packets in a background thread.
    ///
    /// Returns a receiver that yields assembled frames.
    ///
    /// # Errors
    ///
    /// Returns `ReplayError::AlreadyRunning` if replay is already in progress.
    pub fn start(&mut self) -> Result<Receiver<Vec<u8>>> {
        if self.is_running() {
            return Err(ReplayError::AlreadyRunning);
        }

        let (frame_tx, frame_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();

        // Clone data for the thread
        let packets = self.packets.clone();
        let config = self.config.clone();
        let metadata = self.metadata.clone();

        let handle = thread::spawn(move || {
            Self::replay_thread(packets, config, metadata, frame_tx, stop_rx);
        });

        self.thread_handle = Some(handle);
        self.stop_sender = Some(stop_tx);

        log::info!("Packet replay started");
        Ok(frame_rx)
    }

    /// Stop the replay thread.
    ///
    /// Blocks until the thread has finished.
    ///
    /// # Errors
    ///
    /// Returns `ReplayError::NotRunning` if replay is not in progress.
    pub fn stop(&mut self) -> Result<()> {
        let stop_tx = self.stop_sender.take().ok_or(ReplayError::NotRunning)?;
        let handle = self.thread_handle.take().ok_or(ReplayError::NotRunning)?;

        // Signal the thread to stop
        let _ = stop_tx.send(());

        // Wait for the thread to finish
        handle.join().map_err(|_| ReplayError::NotRunning)?;

        log::info!("Packet replay stopped");
        Ok(())
    }

    /// The main replay thread function.
    fn replay_thread(
        packets: Vec<ReplayPacket>,
        config: ReplayConfig,
        metadata: Option<CaptureMetadata>,
        frame_tx: Sender<Vec<u8>>,
        stop_rx: Receiver<()>,
    ) {
        // Create frame assembler based on metadata or config
        let mut assembler = Self::create_assembler(&config, &metadata);

        loop {
            let replay_start = Instant::now();
            let mut last_timestamp_us = 0u64;

            for packet in &packets {
                // Check for stop signal
                if stop_rx.try_recv().is_ok() {
                    log::debug!("Replay thread received stop signal");
                    return;
                }

                // Calculate delay if speed > 0
                if config.speed > 0.0 {
                    let elapsed_us = packet.timestamp_us.saturating_sub(last_timestamp_us);
                    let delay_us = (elapsed_us as f64 / config.speed) as u64;

                    if delay_us > 0 {
                        // Check actual elapsed time vs expected
                        let expected_elapsed = Duration::from_micros(
                            (packet.timestamp_us as f64 / config.speed) as u64,
                        );
                        let actual_elapsed = replay_start.elapsed();

                        if expected_elapsed > actual_elapsed {
                            let sleep_time = expected_elapsed - actual_elapsed;
                            // Sleep in small chunks to check for stop signals
                            let chunk = Duration::from_millis(10);
                            let mut remaining = sleep_time;
                            while remaining > Duration::ZERO {
                                if stop_rx.try_recv().is_ok() {
                                    return;
                                }
                                let sleep = remaining.min(chunk);
                                thread::sleep(sleep);
                                remaining = remaining.saturating_sub(sleep);
                            }
                        }
                    }
                }

                last_timestamp_us = packet.timestamp_us;

                // Process packet through frame assembler
                match assembler.process_packet(&packet.data) {
                    ProcessResult::Frame(frame) => {
                        if frame_tx.send(frame).is_err() {
                            log::debug!("Frame receiver dropped, stopping replay");
                            return;
                        }
                    }
                    ProcessResult::Accumulating | ProcessResult::Skipped => {}
                }
            }

            // Loop or exit
            if config.loop_playback {
                log::debug!("Replay loop completed, restarting");
                assembler.reset();
            } else {
                log::debug!("Replay completed");
                break;
            }
        }
    }

    /// Create a frame assembler based on configuration and metadata.
    fn create_assembler(
        config: &ReplayConfig,
        metadata: &Option<CaptureMetadata>,
    ) -> FrameAssembler {
        if config.force_mjpeg {
            return FrameAssembler::new_mjpeg();
        }

        if config.expected_frame_size > 0 {
            return FrameAssembler::new(config.expected_frame_size);
        }

        // Auto-detect from metadata
        if let Some(meta) = metadata {
            if meta.format_type.to_lowercase().contains("mjpeg")
                || meta.format_type.to_lowercase().contains("jpeg")
            {
                return FrameAssembler::new_mjpeg();
            }

            if meta.width > 0 && meta.height > 0 {
                return FrameAssembler::new_yuy2(meta.width, meta.height);
            }
        }

        // Default: unknown format, will auto-detect
        FrameAssembler::new(0)
    }
}

impl Drop for PacketReplay {
    fn drop(&mut self) {
        if self.is_running() {
            let _ = self.stop();
        }
    }
}

/// Synchronous packet replay for simple use cases.
///
/// Replays all packets without timing and returns all assembled frames.
///
/// # Arguments
///
/// * `path` - Path to the binary capture file.
///
/// # Errors
///
/// Returns `ReplayError` if the file cannot be loaded or contains invalid data.
///
/// # Example
///
/// ```rust,ignore
/// let frames = replay_all_frames(Path::new("capture.bin"))?;
/// println!("Replayed {} frames", frames.len());
/// ```
pub fn replay_all_frames(path: &Path) -> Result<Vec<Vec<u8>>> {
    let replay = PacketReplay::load(path)?;
    let config = ReplayConfig {
        speed: 0.0, // As fast as possible
        ..Default::default()
    };

    let mut assembler = PacketReplay::create_assembler(&config, &replay.metadata);
    let mut frames = Vec::new();

    for packet in &replay.packets {
        if let ProcessResult::Frame(frame) = assembler.process_packet(&packet.data) {
            frames.push(frame);
        }
    }

    Ok(frames)
}

/// Replay packets and return frames via an iterator.
///
/// This is a lazy iterator that processes packets on-demand.
pub struct FrameIterator {
    packets: std::vec::IntoIter<ReplayPacket>,
    assembler: FrameAssembler,
}

impl FrameIterator {
    /// Create a new frame iterator from a capture file.
    ///
    /// # Errors
    ///
    /// Returns `ReplayError` if the file cannot be loaded.
    pub fn new(path: &Path) -> Result<Self> {
        Self::with_config(path, ReplayConfig::default())
    }

    /// Create with custom configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or contains invalid packet data.
    pub fn with_config(path: &Path, config: ReplayConfig) -> Result<Self> {
        let packets = PacketReplay::read_packets_with_timestamps(path)?;
        let metadata = PacketReplay::try_load_metadata(path);
        let assembler = PacketReplay::create_assembler(&config, &metadata);

        Ok(Self {
            packets: packets.into_iter(),
            assembler,
        })
    }
}

impl Iterator for FrameIterator {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let packet = self.packets.next()?;
            if let ProcessResult::Frame(frame) = self.assembler.process_packet(&packet.data) {
                return Some(frame);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    /// Create a test capture file with synthetic packets.
    fn create_test_capture(packets: &[ReplayPacket]) -> std::path::PathBuf {
        let dir = tempdir().unwrap();
        let path = dir.keep().join("test_capture.bin");

        let mut file = std::fs::File::create(&path).unwrap();
        for packet in packets {
            file.write_all(&packet.timestamp_us.to_le_bytes()).unwrap();
            file.write_all(&(packet.data.len() as u32).to_le_bytes())
                .unwrap();
            file.write_all(&[packet.endpoint]).unwrap();
            file.write_all(&packet.data).unwrap();
        }

        path
    }

    /// Create a minimal UVC packet with header.
    fn create_uvc_packet(fid: bool, eof: bool, payload: &[u8]) -> Vec<u8> {
        let mut packet = Vec::with_capacity(2 + payload.len());
        packet.push(0x02); // Header length
        let mut flags = 0x80u8; // EOH
        if fid {
            flags |= 0x01;
        }
        if eof {
            flags |= 0x02;
        }
        packet.push(flags);
        packet.extend_from_slice(payload);
        packet
    }

    #[test]
    fn test_load_empty_capture() {
        let path = create_test_capture(&[]);
        let replay = PacketReplay::load(&path).unwrap();
        assert_eq!(replay.packet_count(), 0);
        assert_eq!(replay.duration_ms(), 0);
    }

    #[test]
    fn test_load_single_packet() {
        let packets = vec![ReplayPacket {
            timestamp_us: 1000,
            endpoint: 0x81,
            data: vec![0x02, 0x80, 0xAB, 0xCD],
        }];

        let path = create_test_capture(&packets);
        let replay = PacketReplay::load(&path).unwrap();

        assert_eq!(replay.packet_count(), 1);
        assert_eq!(replay.duration_ms(), 1);
        assert_eq!(replay.packets[0].timestamp_us, 1000);
        assert_eq!(replay.packets[0].endpoint, 0x81);
    }

    #[test]
    fn test_load_multiple_packets() {
        let packets = vec![
            ReplayPacket {
                timestamp_us: 0,
                endpoint: 0x81,
                data: vec![0x02, 0x81, 0x11, 0x22], // FID=1
            },
            ReplayPacket {
                timestamp_us: 16667, // ~60fps
                endpoint: 0x81,
                data: vec![0x02, 0x80, 0x33, 0x44], // FID=0
            },
            ReplayPacket {
                timestamp_us: 33333,
                endpoint: 0x81,
                data: vec![0x02, 0x81, 0x55, 0x66], // FID=1
            },
        ];

        let path = create_test_capture(&packets);
        let replay = PacketReplay::load(&path).unwrap();

        assert_eq!(replay.packet_count(), 3);
        assert_eq!(replay.duration_ms(), 33);
    }

    #[test]
    fn test_replay_config_default() {
        let config = ReplayConfig::default();
        assert!((config.speed - 1.0).abs() < f64::EPSILON);
        assert!(!config.loop_playback);
        assert_eq!(config.expected_frame_size, 0);
        assert!(!config.force_mjpeg);
    }

    #[test]
    fn test_replay_all_frames_empty() {
        let path = create_test_capture(&[]);
        let frames = replay_all_frames(&path).unwrap();
        assert!(frames.is_empty());
    }

    #[test]
    fn test_replay_yuy2_frame() {
        // Create a simple YUY2 "frame" (just enough data to test assembly)
        // Frame: 4x2 pixels = 16 bytes (YUY2: 2 bytes per pixel)
        let frame_data: Vec<u8> = (0..16).collect();

        // The assembler needs to sync first by detecting FID toggle.
        // Sequence: First frame (FID=0), then second frame (FID=1) triggers sync,
        // then third frame (FID=0) produces the second frame.
        let packets = vec![
            // First frame (FID=0) - will be lost during sync
            ReplayPacket {
                timestamp_us: 0,
                endpoint: 0x81,
                data: create_uvc_packet(false, false, &frame_data[0..8]),
            },
            ReplayPacket {
                timestamp_us: 1000,
                endpoint: 0x81,
                data: create_uvc_packet(false, true, &frame_data[8..16]),
            },
            // Second frame (FID=1) - triggers sync, starts accumulating
            ReplayPacket {
                timestamp_us: 16667,
                endpoint: 0x81,
                data: create_uvc_packet(true, false, &frame_data[0..8]),
            },
            ReplayPacket {
                timestamp_us: 17667,
                endpoint: 0x81,
                data: create_uvc_packet(true, true, &frame_data[8..16]),
            },
            // Third frame (FID=0) - triggers FID toggle, outputs second frame
            ReplayPacket {
                timestamp_us: 33333,
                endpoint: 0x81,
                data: create_uvc_packet(false, false, &[0xAA, 0xBB]),
            },
        ];

        let path = create_test_capture(&packets);

        // Use config with expected frame size
        let config = ReplayConfig {
            speed: 0.0,
            expected_frame_size: 16,
            ..Default::default()
        };

        let mut replay = PacketReplay::load_with_config(&path, config).unwrap();
        let receiver = replay.start().unwrap();

        // Collect frames with timeout
        let mut frames = Vec::new();
        while let Ok(frame) = receiver.recv_timeout(Duration::from_millis(100)) {
            frames.push(frame);
        }

        replay.stop().unwrap();

        // Should have assembled at least one frame
        assert!(!frames.is_empty(), "Expected at least one frame");
        assert_eq!(frames[0].len(), 16, "Frame should be 16 bytes");
    }

    #[test]
    fn test_frame_iterator() {
        let packets = vec![
            ReplayPacket {
                timestamp_us: 0,
                endpoint: 0x81,
                data: create_uvc_packet(true, false, &[0x11, 0x22]),
            },
            ReplayPacket {
                timestamp_us: 1000,
                endpoint: 0x81,
                data: create_uvc_packet(false, false, &[0x33, 0x44]),
            },
        ];

        let path = create_test_capture(&packets);
        let iterator = FrameIterator::new(&path).unwrap();

        // Iterator should process packets (may not produce frames without proper data)
        let frames: Vec<_> = iterator.collect();
        assert!(frames.len() <= 2); // At most one frame per FID toggle
    }

    #[test]
    fn test_metadata_loading() {
        let dir = tempdir().unwrap();
        let bin_path = dir.path().join("capture_12345.bin");
        let json_path = dir.path().join("capture_12345.json");

        // Create empty capture file
        std::fs::File::create(&bin_path).unwrap();

        // Create metadata file
        let metadata = CaptureMetadata {
            vendor_id: 0x1234,
            product_id: 0x5678,
            format_type: "mjpeg".to_string(),
            width: 1280,
            height: 720,
            total_packets: 100,
            total_frames: 30,
            duration_ms: 1000,
            total_bytes: 50000,
            description: "Test capture".to_string(),
        };
        let json = serde_json::to_string(&metadata).unwrap();
        std::fs::write(&json_path, json).unwrap();

        // Load and verify metadata
        let replay = PacketReplay::load(&bin_path).unwrap();
        let loaded_meta = replay.metadata().unwrap();

        assert_eq!(loaded_meta.vendor_id, 0x1234);
        assert_eq!(loaded_meta.format_type, "mjpeg");
        assert_eq!(loaded_meta.width, 1280);
    }

    #[test]
    fn test_already_running_error() {
        let path = create_test_capture(&[]);
        let mut replay = PacketReplay::load(&path).unwrap();

        let _rx = replay.start().unwrap();
        let result = replay.start();

        assert!(matches!(result, Err(ReplayError::AlreadyRunning)));

        replay.stop().unwrap();
    }

    #[test]
    fn test_not_running_error() {
        let path = create_test_capture(&[]);
        let mut replay = PacketReplay::load(&path).unwrap();

        let result = replay.stop();
        assert!(matches!(result, Err(ReplayError::NotRunning)));
    }

    #[test]
    fn test_drop_stops_replay() {
        let path = create_test_capture(&[]);
        let mut replay = PacketReplay::load(&path).unwrap();

        let _rx = replay.start().unwrap();
        assert!(replay.is_running());

        // Drop should stop the replay thread
        drop(replay);
        // If this doesn't hang, the thread was properly stopped
    }

    #[test]
    fn test_create_assembler_mjpeg() {
        let config = ReplayConfig {
            force_mjpeg: true,
            ..Default::default()
        };

        let assembler = PacketReplay::create_assembler(&config, &None);
        assert_eq!(assembler.detected_format(), Some(true));
    }

    #[test]
    fn test_create_assembler_from_metadata() {
        let metadata = Some(CaptureMetadata {
            format_type: "yuy2".to_string(),
            width: 640,
            height: 480,
            ..Default::default()
        });

        let assembler = PacketReplay::create_assembler(&ReplayConfig::default(), &metadata);
        assert_eq!(assembler.detected_format(), Some(false));
    }

    #[test]
    fn test_invalid_packet_length() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad_capture.bin");

        // Write a packet with invalid length (> 1MB)
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(&0u64.to_le_bytes()).unwrap(); // timestamp
        file.write_all(&(2 * 1024 * 1024u32).to_le_bytes()).unwrap(); // 2MB length

        let result = PacketReplay::load(&path);
        assert!(matches!(
            result,
            Err(ReplayError::InvalidPacket { message, .. }) if message.contains("exceeds")
        ));
    }

    #[test]
    fn test_truncated_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("truncated.bin");

        // Write incomplete packet (just timestamp and length, no data)
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(&0u64.to_le_bytes()).unwrap();
        file.write_all(&100u32.to_le_bytes()).unwrap();
        file.write_all(&[0x81]).unwrap(); // endpoint

        let result = PacketReplay::load(&path);
        assert!(matches!(result, Err(ReplayError::InvalidPacket { .. })));
    }
}
