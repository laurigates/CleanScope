export interface ResolutionInfo {
  width: number;
  height: number;
  frame_index: number;
  available_count: number;
}

export interface UsbError {
  error_type: "normal" | "device_unplugged" | "transfer_error" | "timeout" | "unknown";
  message: string;
  recoverable: boolean;
}

export interface UsbStatusExtended {
  connected: boolean;
  info?: string;
  disconnect_reason?: "normal" | "device_unplugged" | "transfer_error" | "timeout" | "unknown";
}

export interface ReconnectStatus {
  attempt: number;
  max_attempts: number;
  reconnecting: boolean;
  message?: string;
}

export interface CaptureResult {
  path: string;
  raw_path: string | null;
  size: number;
  raw_size: number;
  header_hex: string;
  format_hint: string;
  width: number;
  height: number;
}

export interface BuildInfo {
  version: string;
  git_hash: string;
  build_time: string;
}

export type ConnectionStatus = "disconnected" | "connecting" | "connected" | "reconnecting";
