//! Frame corruption detection for YUY2 video streams
//!
//! Detects common artifacts from cheap USB endoscopes:
//! - Horizontal banding (rows shifted or repeated)
//! - Diagonal shearing (stride misalignment)
//!
//! Configurable via `CLEANSCOPE_FRAME_VALIDATION` environment variable.

use serde::{Deserialize, Serialize};

/// Thresholds for different validation levels
const STRICT_ROW_DIFF_THRESHOLD: f32 = 40.0;
const MODERATE_SIZE_TOLERANCE: f32 = 1.1; // 10% tolerance
const MINIMAL_SIZE_TOLERANCE: f32 = 2.0; // 100% tolerance

/// Frame validation strictness levels
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum ValidationLevel {
    /// Full validation: row similarity + size + alignment
    #[default]
    Strict,
    /// Size checks only
    Moderate,
    /// Only massive size mismatches (>2x expected)
    Minimal,
    /// No validation
    Off,
}

impl ValidationLevel {
    /// Parse from environment variable string
    pub fn from_env_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "strict" => Self::Strict,
            "moderate" => Self::Moderate,
            "minimal" => Self::Minimal,
            "off" | "none" | "disabled" => Self::Off,
            _ => {
                log::warn!("Unknown validation level '{}', defaulting to 'strict'", s);
                Self::Strict
            }
        }
    }
}

/// Frame validation result with diagnostic metrics
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether the frame passed validation
    pub valid: bool,
    /// Average Y-channel difference between adjacent rows (Strict only)
    pub avg_row_diff: Option<f32>,
    /// Actual frame size in bytes
    pub actual_size: usize,
    /// Expected frame size in bytes
    pub expected_size: usize,
    /// Size ratio (actual / expected)
    pub size_ratio: f32,
    /// Whether stride alignment is correct
    pub stride_aligned: bool,
    /// Reason for validation failure (if any)
    pub failure_reason: Option<String>,
}

/// Validate a YUY2 frame for corruption artifacts
///
/// # Arguments
/// * `data` - Raw YUY2 frame data
/// * `width` - Expected frame width in pixels
/// * `height` - Expected frame height in pixels
/// * `expected_size` - Expected frame size in bytes
/// * `level` - Validation strictness level
///
/// # Returns
/// `ValidationResult` with metrics and pass/fail status
pub fn validate_yuy2_frame(
    data: &[u8],
    width: usize,
    height: usize,
    expected_size: usize,
    level: ValidationLevel,
) -> ValidationResult {
    let actual_size = data.len();
    let size_ratio = actual_size as f32 / expected_size.max(1) as f32;

    // Early exit for disabled validation
    if level == ValidationLevel::Off {
        return ValidationResult {
            valid: true,
            avg_row_diff: None,
            actual_size,
            expected_size,
            size_ratio,
            stride_aligned: true,
            failure_reason: None,
        };
    }

    let mut failure_reasons = Vec::new();

    // Size validation (all levels except Off)
    let size_valid = match level {
        ValidationLevel::Minimal => (0.5..=MINIMAL_SIZE_TOLERANCE).contains(&size_ratio),
        ValidationLevel::Moderate | ValidationLevel::Strict => {
            (0.9..=MODERATE_SIZE_TOLERANCE).contains(&size_ratio)
        }
        ValidationLevel::Off => true,
    };

    if !size_valid {
        failure_reasons.push(format!(
            "Size mismatch: {} bytes (expected {}, ratio {:.2})",
            actual_size, expected_size, size_ratio
        ));
    }

    // Stride alignment check (Moderate and Strict)
    let stride = width * 2; // YUY2 = 2 bytes per pixel
    let stride_aligned = if level == ValidationLevel::Strict || level == ValidationLevel::Moderate {
        // Allow small deviations (within one stride) from expected size
        actual_size.is_multiple_of(stride)
            || (actual_size as i32 - expected_size as i32).unsigned_abs() < stride as u32
    } else {
        true
    };

    if !stride_aligned && (level == ValidationLevel::Strict || level == ValidationLevel::Moderate) {
        failure_reasons.push(format!(
            "Stride misalignment: size {} not aligned to stride {}",
            actual_size, stride
        ));
    }

    // Row similarity check (Strict only)
    let avg_row_diff =
        if level == ValidationLevel::Strict && height >= 4 && data.len() >= stride * 4 {
            Some(compute_row_similarity(data, stride, height))
        } else {
            None
        };

    let row_diff_valid = match (level, avg_row_diff) {
        (ValidationLevel::Strict, Some(diff)) => {
            if diff > STRICT_ROW_DIFF_THRESHOLD {
                failure_reasons.push(format!(
                    "High row difference: {:.1} (threshold {})",
                    diff, STRICT_ROW_DIFF_THRESHOLD
                ));
                false
            } else {
                true
            }
        }
        _ => true,
    };

    let valid = size_valid && stride_aligned && row_diff_valid;
    let failure_reason = if failure_reasons.is_empty() {
        None
    } else {
        Some(failure_reasons.join("; "))
    };

    ValidationResult {
        valid,
        avg_row_diff,
        actual_size,
        expected_size,
        size_ratio,
        stride_aligned,
        failure_reason,
    }
}

/// Compute average Y-channel difference between adjacent rows
///
/// Samples the first 3-4 rows, checking every 16th pixel for performance.
/// High values (>40-80) indicate banding/corruption.
fn compute_row_similarity(data: &[u8], stride: usize, height: usize) -> f32 {
    let rows_to_check = 3.min(height - 1);
    let mut total_diff: u64 = 0;
    let mut samples: u64 = 0;

    for row in 0..rows_to_check {
        let row0_start = row * stride;
        let row1_start = (row + 1) * stride;

        // Sample every 16th pixel (every 32nd byte since YUY2 = 2 bytes/pixel)
        // Y values are at even indices (0, 2, 4, ...) in YUYV
        for x in (0..stride).step_by(32) {
            if row1_start + x >= data.len() {
                break;
            }

            let y0 = data[row0_start + x] as i16;
            let y1 = data[row1_start + x] as i16;
            total_diff += (y0 - y1).unsigned_abs() as u64;
            samples += 1;
        }
    }

    if samples == 0 {
        return 0.0;
    }

    total_diff as f32 / samples as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_frame_strict() {
        // Create a simple "valid" frame with consistent rows
        let width = 64;
        let height = 48;
        let stride = width * 2;
        let expected_size = stride * height;
        let data = vec![128u8; expected_size]; // Uniform gray

        let result =
            validate_yuy2_frame(&data, width, height, expected_size, ValidationLevel::Strict);

        assert!(result.valid);
        assert!(result.avg_row_diff.unwrap() < 1.0);
        assert!(result.stride_aligned);
        assert!(result.failure_reason.is_none());
    }

    #[test]
    fn test_corrupted_frame_high_row_diff() {
        // Create a frame with alternating bright/dark rows (simulates banding)
        let width = 64;
        let height = 48;
        let stride = width * 2;
        let expected_size = stride * height;
        let mut data = vec![0u8; expected_size];

        for row in 0..height {
            let val = if row % 2 == 0 { 16u8 } else { 235u8 };
            for x in 0..stride {
                data[row * stride + x] = val;
            }
        }

        let result =
            validate_yuy2_frame(&data, width, height, expected_size, ValidationLevel::Strict);

        assert!(!result.valid);
        assert!(result.avg_row_diff.unwrap() > 100.0); // High diff due to alternating rows
        assert!(result.failure_reason.is_some());
    }

    #[test]
    fn test_size_mismatch_minimal() {
        let width = 64;
        let height = 48;
        let expected_size = width * height * 2;
        let data = vec![128u8; expected_size / 2]; // Half the expected size

        // Minimal level: 50% is within tolerance
        let result = validate_yuy2_frame(
            &data,
            width,
            height,
            expected_size,
            ValidationLevel::Minimal,
        );
        assert!(result.valid);

        // Strict level: 50% is not acceptable
        let result =
            validate_yuy2_frame(&data, width, height, expected_size, ValidationLevel::Strict);
        assert!(!result.valid);
    }

    #[test]
    fn test_size_mismatch_too_small() {
        let width = 64;
        let height = 48;
        let expected_size = width * height * 2;
        let data = vec![128u8; expected_size / 4]; // 25% of expected - too small even for minimal

        let result = validate_yuy2_frame(
            &data,
            width,
            height,
            expected_size,
            ValidationLevel::Minimal,
        );
        assert!(!result.valid);
    }

    #[test]
    fn test_validation_off() {
        // Even with obviously wrong data, Off level should pass
        let data = vec![0u8; 100];
        let result = validate_yuy2_frame(&data, 640, 480, 614400, ValidationLevel::Off);

        assert!(result.valid);
        assert!(result.avg_row_diff.is_none());
        assert!(result.failure_reason.is_none());
    }

    #[test]
    fn test_moderate_level_skips_row_check() {
        // Create banded frame that would fail strict
        let width = 64;
        let height = 48;
        let stride = width * 2;
        let expected_size = stride * height;
        let mut data = vec![0u8; expected_size];

        for row in 0..height {
            let val = if row % 2 == 0 { 16u8 } else { 235u8 };
            for x in 0..stride {
                data[row * stride + x] = val;
            }
        }

        // Moderate should pass because it only checks size
        let result = validate_yuy2_frame(
            &data,
            width,
            height,
            expected_size,
            ValidationLevel::Moderate,
        );
        assert!(result.valid);
        assert!(result.avg_row_diff.is_none()); // No row diff computed for Moderate
    }

    #[test]
    fn test_from_env_str() {
        assert_eq!(
            ValidationLevel::from_env_str("strict"),
            ValidationLevel::Strict
        );
        assert_eq!(
            ValidationLevel::from_env_str("STRICT"),
            ValidationLevel::Strict
        );
        assert_eq!(
            ValidationLevel::from_env_str("moderate"),
            ValidationLevel::Moderate
        );
        assert_eq!(
            ValidationLevel::from_env_str("minimal"),
            ValidationLevel::Minimal
        );
        assert_eq!(ValidationLevel::from_env_str("off"), ValidationLevel::Off);
        assert_eq!(ValidationLevel::from_env_str("none"), ValidationLevel::Off);
        assert_eq!(
            ValidationLevel::from_env_str("disabled"),
            ValidationLevel::Off
        );
        // Unknown defaults to strict
        assert_eq!(
            ValidationLevel::from_env_str("invalid"),
            ValidationLevel::Strict
        );
    }
}
