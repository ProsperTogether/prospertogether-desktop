import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';

import { useSubmitRecording, type SubmitStage } from '../../hooks/useSubmitRecording';
import type { RecordingMeta } from '../../types/recording';

function formatDuration(seconds: number): string {
  const m = Math.floor(seconds / 60).toString().padStart(2, '0');
  const s = Math.floor(seconds % 60).toString().padStart(2, '0');
  return `${m}:${s}`;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatRelativeTime(isoString: string): string {
  const then = new Date(isoString).getTime();
  const now = Date.now();
  const diffSec = Math.floor((now - then) / 1000);
  if (diffSec < 60) return 'just now';
  if (diffSec < 3600) {
    const m = Math.floor(diffSec / 60);
    return `${m} minute${m === 1 ? '' : 's'} ago`;
  }
  if (diffSec < 86400) {
    const h = Math.floor(diffSec / 3600);
    return `${h} hour${h === 1 ? '' : 's'} ago`;
  }
  const d = Math.floor(diffSec / 86400);
  return `${d} day${d === 1 ? '' : 's'} ago`;
}

function captureTargetLabel(target: RecordingMeta['capture_target']): string {
  switch (target.mode) {
    case 'screen':
      return 'Entire Screen';
    case 'monitor':
      return target.label;
    case 'window':
      return target.title;
    case 'region':
      return `Region ${target.width}×${target.height}`;
  }
}

function stageLabel(stage: SubmitStage): string {
  switch (stage) {
    case 'transcribing':
      return 'Transcribing audio…';
    case 'keyframes':
      return 'Extracting keyframes…';
    case 'uploading':
      return 'Uploading to server…';
    case 'success':
      return 'Upload complete';
    case 'error':
      return 'Upload failed';
    default:
      return '';
  }
}

export const ReviewPage = () => {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();

  const [meta, setMeta] = useState<RecordingMeta | null>(null);
  const [videoSrc, setVideoSrc] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const { stage, error: submitError, submit } = useSubmitRecording();
  const busy = stage !== 'idle' && stage !== 'error' && stage !== 'success';

  // Load metadata + video src on mount or when id changes.
  useEffect(() => {
    if (!id) return;
    let cancelled = false;
    (async () => {
      try {
        const m = await invoke<RecordingMeta>('get_recording', { id });
        if (cancelled) return;
        setMeta(m);
        const path = await invoke<string>('get_recording_video_path', { id });
        if (cancelled) return;
        setVideoSrc(convertFileSrc(path));
      } catch (err) {
        if (cancelled) return;
        setLoadError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [id]);

  const handleSubmit = useCallback(async () => {
    if (!meta) return;
    const result = await submit(meta.id, meta.duration_seconds);
    if (result.success) {
      if (result.serverRecordingId) {
        navigate(`/submit?recordingId=${result.serverRecordingId}`);
      } else {
        navigate('/submit');
      }
    }
    // On failure, stay on this page — error banner is rendered below.
  }, [meta, submit, navigate]);

  const handleRecordAgain = useCallback(async () => {
    if (!meta) return;
    const ok = window.confirm('Delete this recording and start a new one?');
    if (!ok) return;
    try {
      await invoke('delete_recording', { id: meta.id });
    } catch (err) {
      console.warn('[ReviewPage] delete failed:', err);
    }
    navigate('/record');
  }, [meta, navigate]);

  const handleDelete = useCallback(async () => {
    if (!meta) return;
    const ok = window.confirm('Delete this recording? This cannot be undone.');
    if (!ok) return;
    try {
      await invoke('delete_recording', { id: meta.id });
    } catch (err) {
      console.warn('[ReviewPage] delete failed:', err);
    }
    // Go to recordings list if there may be other pending; dashboard if empty.
    try {
      const remaining = await invoke<RecordingMeta[]>('list_pending_recordings');
      navigate(remaining.length > 0 ? '/recordings' : '/');
    } catch {
      navigate('/');
    }
  }, [meta, navigate]);

  if (loadError) {
    return (
      <div className="flex flex-col items-center justify-center py-16 px-6">
        <p className="text-lg font-medium text-slate-900 mb-2">Recording not found</p>
        <p className="text-sm text-slate-500 mb-6">{loadError}</p>
        <button
          onClick={() => navigate('/recordings')}
          className="px-4 py-2 bg-slate-900 text-white text-sm font-medium rounded-lg"
        >
          Back to recordings
        </button>
      </div>
    );
  }

  if (!meta || !videoSrc) {
    return (
      <div className="flex items-center justify-center py-16">
        <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-brand-500 to-brand-700 animate-pulse" />
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center py-8 px-6">
      <div className="w-full max-w-2xl">
        <h2 className="text-xl font-semibold text-slate-900 mb-1">Review Recording</h2>
        <p className="text-[13px] text-slate-500 mb-5">
          Watch your capture before sending it in.
        </p>

        {submitError && (
          <div className="rounded-lg bg-red-50 border border-red-100 px-3.5 py-2.5 mb-4">
            <p className="text-[13px] text-red-600">Upload failed: {submitError}</p>
          </div>
        )}

        {/* Video player */}
        <div className="rounded-xl overflow-hidden bg-black shadow-lg mb-4">
          <video
            src={videoSrc}
            controls
            className="w-full h-auto block"
            style={{ maxHeight: '60vh' }}
            onLoadedMetadata={() => console.log('[ReviewPage] video metadata loaded, src=', videoSrc)}
            onError={(e) => {
              const el = e.currentTarget;
              console.error('[ReviewPage] video error', {
                src: videoSrc,
                errorCode: el.error?.code,
                errorMessage: el.error?.message,
                networkState: el.networkState,
                readyState: el.readyState,
              });
            }}
          />
        </div>

        {/* Metadata */}
        <div className="grid grid-cols-2 gap-3 mb-5 text-[13px]">
          <div className="rounded-lg border border-slate-200 px-3 py-2">
            <div className="text-[11px] text-slate-400 uppercase tracking-wide">Duration</div>
            <div className="text-slate-900 font-medium">{formatDuration(meta.duration_seconds)}</div>
          </div>
          <div className="rounded-lg border border-slate-200 px-3 py-2">
            <div className="text-[11px] text-slate-400 uppercase tracking-wide">Size</div>
            <div className="text-slate-900 font-medium">{formatFileSize(meta.file_size_bytes)}</div>
          </div>
          <div className="rounded-lg border border-slate-200 px-3 py-2">
            <div className="text-[11px] text-slate-400 uppercase tracking-wide">Recorded</div>
            <div className="text-slate-900 font-medium">{formatRelativeTime(meta.created_at)}</div>
          </div>
          <div className="rounded-lg border border-slate-200 px-3 py-2">
            <div className="text-[11px] text-slate-400 uppercase tracking-wide">Target</div>
            <div className="text-slate-900 font-medium truncate">{captureTargetLabel(meta.capture_target)}</div>
          </div>
        </div>

        {/* Submission stage indicator */}
        {busy && (
          <div className="rounded-lg bg-blue-50 border border-blue-100 px-3.5 py-2.5 mb-4 flex items-center gap-3">
            <div className="w-4 h-4 border-2 border-blue-300 border-t-blue-600 rounded-full animate-spin" />
            <p className="text-[13px] text-blue-700">{stageLabel(stage)}</p>
          </div>
        )}

        {/* Actions */}
        <div className="flex flex-col gap-2">
          <button
            onClick={handleSubmit}
            disabled={busy}
            className="w-full px-6 py-3 bg-gradient-to-b from-red-500 to-red-600 text-white text-[15px] font-semibold rounded-xl hover:from-red-600 hover:to-red-700 shadow-lg shadow-red-500/25 transition disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Submit
          </button>
          <div className="grid grid-cols-2 gap-2">
            <button
              onClick={handleRecordAgain}
              disabled={busy}
              className="px-4 py-2.5 bg-white text-slate-700 text-[13px] font-medium rounded-lg border border-slate-200 hover:bg-slate-50 transition disabled:opacity-50 disabled:cursor-not-allowed"
            >
              Record Again
            </button>
            <button
              onClick={handleDelete}
              disabled={busy}
              className="px-4 py-2.5 bg-white text-red-600 text-[13px] font-medium rounded-lg border border-red-200 hover:bg-red-50 transition disabled:opacity-50 disabled:cursor-not-allowed"
            >
              Delete
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
