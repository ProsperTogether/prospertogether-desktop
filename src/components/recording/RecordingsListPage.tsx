import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';

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
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)}m ago`;
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)}h ago`;
  return `${Math.floor(diffSec / 86400)}d ago`;
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

interface CardProps {
  recording: RecordingMeta;
  onOpen: (id: string) => void;
}

const RecordingCard = ({ recording, onOpen }: CardProps) => {
  const [thumbSrc, setThumbSrc] = useState<string | null>(null);

  useEffect(() => {
    if (!recording.thumbnail_file_name) return;
    let cancelled = false;
    (async () => {
      try {
        const path = await invoke<string>('get_recording_thumbnail_path', { id: recording.id });
        if (!cancelled) setThumbSrc(convertFileSrc(path));
      } catch {
        // no thumbnail — placeholder shows
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [recording.id, recording.thumbnail_file_name]);

  return (
    <button
      onClick={() => onOpen(recording.id)}
      className="flex flex-col rounded-xl border border-slate-200 overflow-hidden text-left bg-white hover:border-slate-300 hover:shadow-sm transition"
    >
      <div className="w-full aspect-video bg-slate-100 flex items-center justify-center">
        {thumbSrc ? (
          <img src={thumbSrc} alt="" className="w-full h-full object-cover" draggable={false} />
        ) : (
          <svg className="w-10 h-10 text-slate-300" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
            <rect x="3" y="5" width="18" height="14" rx="2" />
            <path d="M10 9l5 3-5 3V9z" fill="currentColor" />
          </svg>
        )}
      </div>
      <div className="p-3">
        <p className="text-[13px] font-medium text-slate-900 truncate mb-0.5">
          {captureTargetLabel(recording.capture_target)}
        </p>
        <p className="text-[11px] text-slate-500">
          {formatDuration(recording.duration_seconds)} · {formatFileSize(recording.file_size_bytes)} · {formatRelativeTime(recording.created_at)}
        </p>
      </div>
    </button>
  );
};

export const RecordingsListPage = () => {
  const navigate = useNavigate();
  const [recordings, setRecordings] = useState<RecordingMeta[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    setLoading(true);
    invoke<RecordingMeta[]>('list_pending_recordings')
      .then((list) => {
        setRecordings(list);
        setError(null);
      })
      .catch((err) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    load();
  }, []);

  const handleOpen = (id: string) => {
    navigate(`/review/${id}`);
  };

  return (
    <div className="flex flex-col py-8 px-6">
      <div className="w-full max-w-4xl mx-auto">
        <div className="flex items-center justify-between mb-5">
          <div>
            <h2 className="text-xl font-semibold text-slate-900 mb-1">Pending Recordings</h2>
            <p className="text-[13px] text-slate-500">
              Review, submit, or delete recordings you haven't sent in yet.
            </p>
          </div>
          <button
            onClick={() => navigate('/record')}
            className="px-4 py-2 bg-slate-900 text-white text-[13px] font-medium rounded-lg hover:bg-slate-800 transition"
          >
            New Recording
          </button>
        </div>

        {error && (
          <div className="rounded-lg bg-red-50 border border-red-100 px-3.5 py-2.5 mb-4">
            <p className="text-[13px] text-red-600">{error}</p>
          </div>
        )}

        {loading ? (
          <div className="flex items-center justify-center py-16">
            <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-brand-500 to-brand-700 animate-pulse" />
          </div>
        ) : recordings.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-16 text-center">
            <p className="text-slate-900 font-medium mb-1">No pending recordings</p>
            <p className="text-[13px] text-slate-500 mb-5">Start a new recording to see it here.</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-4">
            {recordings.map((r) => (
              <RecordingCard key={r.id} recording={r} onOpen={handleOpen} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
};
