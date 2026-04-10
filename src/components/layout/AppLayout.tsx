import { useState, useRef, useEffect } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';

import { clearPersistedAuth, useAuthStore } from '../../store/authStore';
import { useRecordingStore } from '../../store/recordingStore';
import { UpdateIndicator } from './UpdateIndicator';

export const AppLayout = ({ children }: { children: React.ReactNode }) => {
  const navigate = useNavigate();
  const location = useLocation();
  const { email, logout } = useAuthStore();
  const recordingStatus = useRecordingStore(s => s.status);

  // On first mount: ask the backend whether a recording is currently in
  // progress (orphaned from a previous Tauri process, or carried over from
  // a hot reload). If yes, navigate the user to /record so RecordingControls
  // mounts and resumes the dock UI. Without this, the user could be sitting
  // on the dashboard while ffmpeg silently records to disk in the background.
  const recoveryAttemptedRef = useRef(false);
  useEffect(() => {
    if (recoveryAttemptedRef.current) return;
    recoveryAttemptedRef.current = true;

    let cancelled = false;
    (async () => {
      try {
        const core = await import('@tauri-apps/api/core');
        const rec = await core.invoke<{
          active: boolean;
          file_path: string | null;
          started_at_ms: number | null;
          duration_seconds: number;
        }>('get_recording_state');
        if (cancelled) return;
        if (rec.active && location.pathname !== '/record') {
          console.log('[AppLayout] recording in progress, navigating to /record', rec);
          navigate('/record', { replace: true });
        }
      } catch (err) {
        console.warn('[AppLayout] recording recovery check failed:', err);
      }
    })();

    return () => { cancelled = true; };
    // We deliberately only run this once on mount; the in-flight ref prevents
    // re-runs from React StrictMode's double-effect-invocation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  // While recording, the main window is collapsed into a top dock by
  // RecordingControls. Hide the AppLayout chrome (top header + page padding)
  // so the dock has the entire 960×84 window to itself instead of fighting
  // a 50px-tall sticky header for vertical space.
  const isRecordingDock = recordingStatus === 'recording';
  const [profileOpen, setProfileOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  // Close dropdown on outside click
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setProfileOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, []);

  // Close dropdown on navigation
  useEffect(() => {
    setProfileOpen(false);
  }, [location.pathname]);

  const handleLogout = async () => {
    await clearPersistedAuth();
    logout();
  };

  const initials = email
    ? email.split('@')[0].slice(0, 2).toUpperCase()
    : '??';

  // CRITICAL: the tree shape returned from this component MUST be stable
  // across recording-status changes. If we early-return `<>{children}</>`
  // for the dock case and a `<div><header/><main>{children}</main></div>`
  // shape otherwise, React sees `children` in a different tree position
  // when the status flips and UNMOUNTS+REMOUNTS the entire RecordingControls
  // subtree on every recording start/stop. That cascades catastrophically:
  // the unmount cleanup runs `restoreWindow()` (undoing the dock collapse
  // that just happened), the new mount runs its recovery effect (which
  // resets the store to idle), and the user sees a "flash and back to
  // Ready to Record" instead of a stable dock UI. So: ALWAYS render the
  // same `<div><main>{children}</main></div>` shell, and conditionally
  // hide the header inside it. The main element stays at the same
  // position in the tree, so React preserves RecordingControls' identity
  // across status changes.
  return (
    <div className={isRecordingDock ? '' : 'min-h-screen bg-slate-50 flex flex-col'}>
      {/* Top bar — hidden in dock mode so the 960×84 window has zero
          leftover layout pressure, but kept in the JSX above an early
          return so the conditional doesn't break tree-shape stability. */}
      {!isRecordingDock && (
        <header className="bg-white border-b border-slate-200 px-5 py-3 flex items-center justify-between sticky top-0 z-30">
        <button
          onClick={() => navigate('/')}
          className="flex items-center gap-2.5 hover:opacity-80 transition"
        >
          <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-brand-500 to-brand-700 flex items-center justify-center">
            <svg className="w-4.5 h-4.5 text-white" viewBox="0 0 24 24" fill="none" strokeWidth={2.2} stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" d="m15.75 10.5 4.72-4.72a.75.75 0 0 1 1.28.53v11.38a.75.75 0 0 1-1.28.53l-4.72-4.72M4.5 18.75h9a2.25 2.25 0 0 0 2.25-2.25v-9a2.25 2.25 0 0 0-2.25-2.25h-9A2.25 2.25 0 0 0 2.25 7.5v9a2.25 2.25 0 0 0 2.25 2.25Z" />
            </svg>
          </div>
          <span className="text-[15px] font-semibold text-slate-900 tracking-tight">Prosper Together</span>
        </button>

        {/* Profile */}
        <div className="relative" ref={dropdownRef}>
          <button
            onClick={() => setProfileOpen(!profileOpen)}
            className="flex items-center gap-2 rounded-full pl-1 pr-2.5 py-1 hover:bg-slate-100 transition"
          >
            <div className="w-7 h-7 rounded-full bg-gradient-to-br from-brand-400 to-brand-600 flex items-center justify-center text-[11px] font-bold text-white">
              {initials}
            </div>
            <svg className={`w-3.5 h-3.5 text-slate-400 transition ${profileOpen ? 'rotate-180' : ''}`} viewBox="0 0 20 20" fill="currentColor">
              <path fillRule="evenodd" d="M5.23 7.21a.75.75 0 011.06.02L10 11.168l3.71-3.938a.75.75 0 111.08 1.04l-4.25 4.5a.75.75 0 01-1.08 0l-4.25-4.5a.75.75 0 01.02-1.06z" clipRule="evenodd" />
            </svg>
          </button>

          {profileOpen && (
            <div className="absolute right-0 top-full mt-1.5 w-56 bg-white rounded-xl shadow-lg ring-1 ring-black/8 py-1.5 animate-in fade-in slide-in-from-top-1 duration-150">
              <div className="px-3.5 py-2.5 border-b border-slate-100">
                <p className="text-[13px] font-medium text-slate-900 truncate">{email}</p>
                <p className="text-[11px] text-slate-400 mt-0.5">Prosper Together</p>
              </div>
              <div className="py-1">
                <button
                  onClick={() => navigate('/setup')}
                  className="w-full text-left px-3.5 py-2 text-[13px] text-slate-700 hover:bg-slate-50 flex items-center gap-2.5 transition"
                >
                  <svg className="w-4 h-4 text-slate-400" fill="none" viewBox="0 0 24 24" strokeWidth={1.8} stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.325.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 0 1 1.37.49l1.296 2.247a1.125 1.125 0 0 1-.26 1.431l-1.003.827c-.293.241-.438.613-.43.992a7.723 7.723 0 0 1 0 .255c-.008.378.137.75.43.991l1.004.827c.424.35.534.955.26 1.43l-1.298 2.247a1.125 1.125 0 0 1-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.47 6.47 0 0 1-.22.128c-.331.183-.581.495-.644.869l-.213 1.281c-.09.543-.56.94-1.11.94h-2.594c-.55 0-1.019-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 0 1-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 0 1-1.369-.49l-1.297-2.247a1.125 1.125 0 0 1 .26-1.431l1.004-.827c.292-.24.437-.613.43-.991a6.932 6.932 0 0 1 0-.255c.007-.38-.138-.751-.43-.992l-1.004-.827a1.125 1.125 0 0 1-.26-1.43l1.297-2.247a1.125 1.125 0 0 1 1.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.086.22-.128.332-.183.582-.495.644-.869l.214-1.28Z" />
                    <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z" />
                  </svg>
                  Setup & Test
                </button>
                <button
                  onClick={handleLogout}
                  className="w-full text-left px-3.5 py-2 text-[13px] text-slate-700 hover:bg-slate-50 flex items-center gap-2.5 transition"
                >
                  <svg className="w-4 h-4 text-slate-400" fill="none" viewBox="0 0 24 24" strokeWidth={1.8} stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 9V5.25A2.25 2.25 0 0 0 13.5 3h-6a2.25 2.25 0 0 0-2.25 2.25v13.5A2.25 2.25 0 0 0 7.5 21h6a2.25 2.25 0 0 0 2.25-2.25V15m3 0 3-3m0 0-3-3m3 3H9" />
                  </svg>
                  Log out
                </button>
              </div>
            </div>
          )}
        </div>
      </header>
      )}

      {/* Content — main is ALWAYS at the same tree position so React
          preserves RecordingControls' identity across status changes. */}
      <main className={isRecordingDock ? '' : 'flex-1'}>
        {children}
      </main>

      {/* Auto-update indicator — hidden while the dock is visible so it
          doesn't overlap the recording controls. Mounted once here so it
          manages its own state across every route. */}
      {!isRecordingDock && <UpdateIndicator />}
    </div>
  );
};
