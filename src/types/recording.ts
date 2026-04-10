/**
 * Minimal capture target label used when persisting a recording. Does not
 * include HWND or rect — those would be stale by the time the user opens
 * the review screen.
 */
export type PersistedCaptureTarget =
  | { mode: 'screen' }
  | { mode: 'monitor'; label: string }
  | { mode: 'window'; title: string }
  | { mode: 'region'; width: number; height: number };

/**
 * Metadata for a single pending recording. Mirrors the Rust `RecordingMeta`
 * struct in `src-tauri/src/commands/recordings.rs`.
 *
 * IMPORTANT: serde uses snake_case field names when serializing. Keep the
 * property names below in snake_case to match (do NOT convert to camelCase
 * here — the wire format is snake_case). The UI code that consumes these
 * objects accesses `created_at`, `duration_seconds`, etc.
 */
export interface RecordingMeta {
  id: string;
  created_at: string; // ISO-8601
  duration_seconds: number;
  file_size_bytes: number;
  video_file_name: string;
  thumbnail_file_name: string | null;
  capture_target: PersistedCaptureTarget;
}
