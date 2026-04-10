import { create } from 'zustand';
import type { CaptureTarget } from '../types/capture';

export type RecordingStatus = 'idle' | 'countdown' | 'recording' | 'paused' | 'processing' | 'uploading';

interface RecordingState {
  status: RecordingStatus;
  duration: number;
  /** Unix ms when the recording started. Used to compute elapsed duration
   *  after a recovery from a previous Tauri process death. Null when no
   *  recording is in progress. */
  startedAtMs: number | null;
  captureTarget: CaptureTarget;
  selectedAudio: string | null;
  setStatus: (status: RecordingStatus) => void;
  setDuration: (duration: number) => void;
  setStartedAtMs: (ms: number | null) => void;
  setCaptureTarget: (target: CaptureTarget) => void;
  setSelectedAudio: (audio: string | null) => void;
  reset: () => void;
}

export const useRecordingStore = create<RecordingState>((set) => ({
  status: 'idle',
  duration: 0,
  startedAtMs: null,
  captureTarget: { mode: 'screen' },
  selectedAudio: null,
  setStatus: (status) => set({ status }),
  setDuration: (duration) => set({ duration }),
  setStartedAtMs: (ms) => set({ startedAtMs: ms }),
  setCaptureTarget: (target) => set({ captureTarget: target }),
  setSelectedAudio: (audio) => set({ selectedAudio: audio }),
  reset: () => set({ status: 'idle', duration: 0, startedAtMs: null })
}));
