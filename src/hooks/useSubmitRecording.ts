import { useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';

export type SubmitStage =
  | 'idle'
  | 'transcribing'
  | 'keyframes'
  | 'uploading'
  | 'success'
  | 'error';

export interface SubmitResult {
  success: boolean;
  serverRecordingId: string | null;
  error?: string;
}

/**
 * Runs the transcribe → keyframes → upload pipeline for a single pending
 * recording. On success, deletes the local recording folder. On any
 * failure, leaves the recording in place so the user can retry. The
 * pipeline is not abortable mid-flight — the caller should disable the
 * Submit button while `stage` is not 'idle' / 'success' / 'error'.
 *
 * This hook replaces the inline pipeline that previously lived in
 * `RecordingControls.handleStop`. Extracting it keeps the review screen's
 * logic declarative and makes the stages individually visible to the UI.
 */
export function useSubmitRecording() {
  const [stage, setStage] = useState<SubmitStage>('idle');
  const [error, setError] = useState<string | null>(null);

  const submit = useCallback(async (recordingId: string, durationSeconds: number): Promise<SubmitResult> => {
    setError(null);
    setStage('transcribing');

    try {
      // 1. Get the absolute path of the video file.
      const videoPath = await invoke<string>('get_recording_video_path', { id: recordingId });

      // 2. Transcribe audio (non-blocking on failure — we still upload).
      let transcript = '';
      try {
        transcript = await invoke<string>('transcribe_audio', {
          audioPath: videoPath,
        });
      } catch (err) {
        console.error('[useSubmitRecording] transcription failed:', err);
        // Non-fatal — continue to frames + upload.
      }

      // 3. Extract keyframes (non-blocking on failure).
      setStage('keyframes');
      let frames: string[] | null = null;
      try {
        frames = await invoke<string[]>('extract_keyframes', {
          videoPath,
          durationSeconds: durationSeconds > 0 ? durationSeconds : null,
        });
      } catch (err) {
        console.warn('[useSubmitRecording] keyframe extraction failed:', err);
      }

      // 4. Upload to portal.
      setStage('uploading');

      const { load } = await import('@tauri-apps/plugin-store');
      const store = await load('settings.json');
      const token = (await store.get<string>('auth_token')) ?? '';
      const apiUrl =
        import.meta.env.VITE_API_BASE_URL ?? 'http://localhost:4000/api';

      const fs = await import('@tauri-apps/plugin-fs');
      const stat = await fs.stat(videoPath);
      const fileSize = stat.size;

      const initResult = await invoke<{
        id: string;
        uploadToken: string;
        chunkSize: number;
      }>('init_upload', {
        apiUrl,
        token,
        fileName: videoPath.split(/[\\/]/).pop() ?? 'recording.webm',
        fileSize,
      });

      const uploadResult = await invoke<{
        success: boolean;
        recordingId: string | null;
      }>('upload_file', {
        apiUrl,
        token,
        uploadToken: initResult.uploadToken,
        filePath: videoPath,
        durationSeconds,
        transcription: transcript || null,
        frames: frames && frames.length > 0 ? frames : null,
      });

      if (!uploadResult.success) {
        throw new Error('Upload returned success=false');
      }

      // 5. Delete the local recording folder now that the server has it.
      try {
        await invoke('delete_recording', { id: recordingId });
      } catch (err) {
        // Non-fatal — upload succeeded, just log the cleanup failure.
        console.warn('[useSubmitRecording] delete_recording failed:', err);
      }

      setStage('success');
      return { success: true, serverRecordingId: uploadResult.recordingId };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error('[useSubmitRecording] submit failed:', msg);
      setError(msg);
      setStage('error');
      return { success: false, serverRecordingId: null, error: msg };
    }
  }, []);

  const reset = useCallback(() => {
    setStage('idle');
    setError(null);
  }, []);

  return { stage, error, submit, reset };
}
