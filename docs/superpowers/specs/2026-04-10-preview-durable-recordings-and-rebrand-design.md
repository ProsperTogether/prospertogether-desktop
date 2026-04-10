# Preview, Durable Recordings, and Rebrand Design

**Date**: 2026-04-10
**Status**: Approved design, pending implementation plan

## Context

Two related but independent changes to the desktop capture app:

1. **Preview + durable recordings** — After a user stops recording, the current code immediately runs transcription, keyframe extraction, and upload with no way to preview the result, cancel, or retry. If any step fails, the user has no way to recover the recording file. A user can spend an hour on a recording and lose it all to a transient error. We need to (a) save recordings durably so nothing is ever lost, (b) show a preview before committing, and (c) let users manage pending recordings across app restarts.

2. **Rebrand from "UserFirst Agent" to "Prosper Together"** — Every place the current code references "UserFirst" / `userfirst` / `com.userfirst.agent` / `userfirst-agent` needs to become "Prosper Together" / `prospertogether` / `com.prospertogether.desktop` / `prospertogether-desktop`. This is mechanical but touches both Rust and frontend code and has a non-obvious consequence: changing the Tauri bundle identifier moves the app data directory, which would orphan existing users' stored data without a migration step.

These are bundled in one plan because they both touch `stop_recording`, app data paths, and configuration — doing them sequentially in one plan avoids rework (the new preview code writes paths using the new bundle identifier from the start).

## Goals

- Zero risk of losing a recording once the user has clicked Stop
- Preview before commit — the user can watch what they captured and decide Submit vs. Record Again vs. Delete
- Pending recordings survive app restarts and crashes; a list shows them on next launch
- Rebrand is complete across package names, bundle identifiers, display strings, paths, and user-visible text
- Existing dev/production installations don't silently lose their settings or any in-flight recording when the bundle identifier changes

## Non-goals

- Automatic cleanup policies for old pending recordings (user decides)
- Video editing / trimming
- Transcript displayed inside the preview screen (transcription runs on Submit only)
- Multi-select bulk delete on the pending list
- Cross-device sync of pending recordings

---

## Track A: Rebrand to Prosper Together

### Target strings

| Context | Old | New |
|---|---|---|
| Display name (user-facing) | UserFirst Agent | Prosper Together |
| Cargo package name | `userfirst-agent` | `prospertogether-desktop` |
| Cargo lib name | `userfirst_agent_lib` | `prospertogether_desktop_lib` |
| Bundle identifier | `com.userfirst.agent` | `com.prospertogether.desktop` |
| Temp dir segment | `userfirst` | `prospertogether` |
| Window title | UserFirst Agent | Prosper Together |
| Tray tooltip | UserFirst Agent | Prosper Together |
| package.json name | (current) | `prospertogether-desktop` |

### Files to modify

| File | What changes |
|---|---|
| `src-tauri/Cargo.toml` | `[package] name`, `[lib] name` |
| `src-tauri/tauri.conf.json` | `productName`, `identifier`, `windows[0].title` |
| `src-tauri/src/lib.rs` | Tray tooltip, any hardcoded strings |
| `src-tauri/src/commands/recording.rs` | `%TEMP%/userfirst/` → `%TEMP%/prospertogether/`, state file directory |
| `src-tauri/src/commands/window_capture.rs` | Same temp path references if present |
| `src-tauri/src/commands/setup.rs` | Same temp path references if present |
| `src-tauri/capabilities/default.json` | Any `com.userfirst.agent` references |
| `package.json` | `name` field |
| `src/components/layout/AppLayout.tsx` | Any "UserFirst" strings in nav/sidebar |
| `src/components/auth/LoginPage.tsx` | "UserFirst" in login heading / body |
| `src/components/onboarding/OnboardingPage.tsx` | "UserFirst" in welcome text |

A full-repo grep for case-insensitive `userfirst` must return zero matches in `src/`, `src-tauri/src/`, and top-level config files after the rename. Matches in `target/` and `node_modules/` are expected (they regenerate on next build / install).

### Bundle identifier migration

Tauri derives `app.path().app_data_dir()` from the bundle identifier. Changing `com.userfirst.agent` → `com.prospertogether.desktop` means the app starts looking at a new folder on next launch:

- Old: `%APPDATA%/com.userfirst.agent/`
- New: `%APPDATA%/com.prospertogether.desktop/`

Stored data that would be orphaned without migration:
- `settings.json` (tauri-plugin-store) — contains `auth_token`, `preferred_audio_device`, `onboarding_complete`
- Any recordings the new durable-storage feature places in the new app data dir (which won't exist yet on the first migrated launch, so this only matters going forward)

Also orphaned: any in-flight orphaned recording state file at `%TEMP%/userfirst/recording-state.json`.

**Migration strategy**: on app startup, before any other path-using code runs, check if the old app data dir exists and the new one does not. If so, copy the contents over (specifically `settings.json`, and any `recordings/` subdirectory if it already exists). Also check the old temp dir (`%TEMP%/userfirst/`) for `recording-state.json` and move it to the new temp dir (`%TEMP%/prospertogether/`) so orphaned-recording recovery still works across the rename.

The migration runs at most once per install: if the new dir exists, assume migration already happened and skip. Log migration actions to stderr for diagnostics.

### cargo clean requirement

Renaming the Cargo package invalidates the `target/` directory's incremental build cache. The plan will include an explicit `cargo clean` step before the first rebuild, otherwise link errors are likely.

---

## Track B: Preview + durable recordings

### Data layout

Each pending recording is a self-contained folder under `{app_data_dir}/recordings/`. Since `app_data_dir()` is derived from the new bundle identifier (set in Track A), the effective Windows path is:

```
%APPDATA%/com.prospertogether.desktop/recordings/
├── {uuid}/
│   ├── recording.webm     # the actual capture (VP8 + Opus)
│   ├── thumbnail.jpg      # single frame, ~640x360, for preview cards
│   └── metadata.json      # duration, target, size, createdAt, captureTarget
└── {uuid}/
    └── ...
```

One folder per recording keeps the model simple:
- List = `readdir` the parent + parse each `metadata.json`
- Delete = recursive `remove_dir_all` of one folder
- No central index file to corrupt or keep in sync

### metadata.json schema

```json
{
  "id": "uuid-v4",
  "createdAt": "2026-04-10T14:23:45.123Z",
  "durationSeconds": 45,
  "fileSizeBytes": 1234567,
  "videoFileName": "recording.webm",
  "thumbnailFileName": "thumbnail.jpg",
  "captureTarget": {
    "mode": "window",
    "title": "Notepad — untitled.txt"
  }
}
```

`captureTarget` stores just enough to display in the review screen (mode + human label). The original `hwnd` and `rect` are intentionally not persisted — they're stale by the time the user opens the review anyway.

### stop_recording flow changes

**Current**:
1. ffmpeg finalizes to `%TEMP%/userfirst/recording-{uuid}.webm`
2. Return path to frontend
3. Frontend immediately runs transcribe → keyframes → upload → delete

**New**:
1. ffmpeg finalizes to `%TEMP%/prospertogether/recording-{uuid}.webm` (same as before, using new temp dir from Track A)
2. Rust creates `{app_data_dir}/recordings/{uuid}/` and moves the finalized webm into it as `recording.webm`
3. Rust runs a fast one-off ffmpeg invocation to grab a frame into `{uuid}/thumbnail.jpg`:
   `ffmpeg -ss 00:00:02 -i recording.webm -vframes 1 -vf scale=640:-2 -y thumbnail.jpg`
   (If the recording is shorter than 2s, use `-ss 00:00:00`.)
4. Rust writes `{uuid}/metadata.json` with the structure above
5. Rust returns the recording ID (uuid string) to the frontend
6. Frontend navigates to `/review/{id}`

The current two-segment concat logic in `stop_recording` still runs before the move (multiple segments get stitched first, then the final file moves). Window-capture and gdigrab paths both end up at the same move-and-finalize step.

### Review screen `/review/:id`

**Component**: `src/components/recording/ReviewPage.tsx`

Layout:
- Header: "Review Recording"
- HTML5 `<video>` player with `src={convertFileSrc(absoluteVideoPath)}`, `controls`, no autoplay. Native browser scrub bar and playback controls.
- Metadata panel: duration (`mm:ss`), file size (`X.X MB`), recorded time (relative: "just now", "2 minutes ago", or absolute if older), capture target label
- Action row, in order: **Submit** (primary, filled red to match Start Recording), **Record Again** (secondary), **Delete** (destructive, ghost red)
- Submission progress overlay (hidden unless a submit is in flight): shows current stage ("Transcribing…", "Extracting keyframes…", "Uploading…") + a progress bar
- Error banner at the top if the previous submit attempt failed

**Loading**: On mount, fetch metadata via `invoke('get_recording', { id })`. If 404, show "Recording not found" + button to pending list.

**Action handlers**:

- **Submit**: Disables all buttons, shows progress overlay, runs the existing pipeline (`transcribe_audio` → `extract_keyframes` → `init_upload` → `upload_file`). On success, calls `delete_recording` to remove the folder, then navigates to `/submit?recordingId={serverRecordingId}` (the existing confirmation page). On failure, re-enables buttons, shows error banner, recording stays in the pending list.
- **Record Again**: Confirmation dialog ("This will delete the current recording. Are you sure?"). On confirm: `delete_recording` → navigate to `/record`.
- **Delete**: Confirmation dialog ("Delete this recording? This cannot be undone."). On confirm: `delete_recording` → if there are other pending recordings, navigate to `/recordings`; else navigate to `/`.

### Pending list `/recordings`

**Component**: `src/components/recording/RecordingsListPage.tsx`

- Fetches list via `invoke('list_pending_recordings')` on mount, sorted newest first
- Grid of cards (3 per row on desktop, responsive), each showing:
  - Thumbnail image (from `{uuid}/thumbnail.jpg` via `convertFileSrc`)
  - Truncated capture target title
  - Duration + file size
  - Relative timestamp ("2 hours ago")
  - Click anywhere on the card → navigates to `/review/{id}`
- Empty state: "No pending recordings" + button to `/record`
- Optional refresh button (just re-invokes list_pending_recordings)

### Dashboard badge

The existing `DashboardPage` gets a new card or badge: "Pending Recordings (N)" where N > 0 routes to `/recordings`. When N = 0, show the card but with "No pending recordings" subtitle. The badge count is fetched via `invoke('list_pending_recordings')` on dashboard mount. Zero auto-navigation — the user always clicks through.

### Rust commands (new)

| Command | Signature | Purpose |
|---|---|---|
| `list_pending_recordings` | `() -> Result<Vec<RecordingMeta>, String>` | readdir `{app_data_dir}/recordings/`, parse each metadata.json, sort by createdAt desc |
| `get_recording` | `(id: String) -> Result<RecordingMeta, String>` | Parse one recording's metadata.json, return error if missing |
| `get_recording_video_path` | `(id: String) -> Result<String, String>` | Absolute path to `{uuid}/recording.webm` for the frontend to `convertFileSrc` |
| `get_recording_thumbnail_path` | `(id: String) -> Result<String, String>` | Absolute path to `{uuid}/thumbnail.jpg` |
| `delete_recording` | `(id: String) -> Result<(), String>` | `remove_dir_all({uuid}/)` |

`RecordingMeta` is serde-serializable matching the `metadata.json` schema plus the computed `id` (folder name).

### Rust commands (changed)

- `stop_recording` — adds the move-to-app-data-dir + thumbnail-generation + metadata-write steps. Returns the recording ID string instead of the temp file path.
- `recover_orphaned_recording` (startup) — if it finds an in-flight ffmpeg, on completion the recording is finalized into the durable location like any other stop. Also handles the migration case where the state file was at the old temp path (per Track A).
- `init_upload` / `upload_file` — signatures unchanged; the caller (now `ReviewPage` instead of `RecordingControls`) passes the file path obtained from `get_recording_video_path`.

### Frontend: submission pipeline relocation

The transcribe → keyframes → upload logic currently in `RecordingControls.handleStop` (lines ~469–556) moves into a new hook:

```tsx
// src/hooks/useSubmitRecording.ts
export function useSubmitRecording() {
  const [stage, setStage] = useState<'idle' | 'transcribing' | 'keyframes' | 'uploading' | 'error'>('idle');
  const [error, setError] = useState<string | null>(null);

  const submit = async (recordingId: string) => { /* ... */ };

  return { stage, error, submit };
}
```

The hook:
1. `get_recording_video_path(id)` → absolute path
2. `invoke('transcribe_audio', ...)` — sets stage, catches errors
3. `invoke('extract_keyframes', ...)` — same
4. `invoke('init_upload', ...)` → `invoke('upload_file', ...)` — same
5. On success: `invoke('delete_recording', { id })` then return `{ success: true, serverRecordingId }`
6. On any failure: set error, return `{ success: false, error }`

The ReviewPage consumes this hook and renders progress states based on `stage`.

### What gets deleted from RecordingControls.tsx

- The `processing` and `uploading` status values (kept in `RecordingStatus` type only if still used elsewhere — otherwise removed)
- The `processing` and `uploading` render branches (entire JSX blocks with spinners)
- The transcribe + keyframes + upload inline calls in `handleStop`
- Net: ~150 lines removed

`handleStop` shrinks to roughly:
```tsx
const handleStop = useCallback(async () => {
  // existing cleanup: destroy border overlay, drawing overlay, restore window
  // ...
  try {
    const core = await import('@tauri-apps/api/core');
    const recordingId = await core.invoke<string>('stop_recording');
    navigate(`/review/${recordingId}`);
  } catch (err) {
    setError(`Stop failed: ${err}`);
  }
}, [...]);
```

### Routes added

```tsx
<Route path="/review/:id" element={<AuthedLayout><ReviewPage /></AuthedLayout>} />
<Route path="/recordings" element={<AuthedLayout><RecordingsListPage /></AuthedLayout>} />
```

### State management

No new Zustand store. Recordings are fetched on demand from Rust — straight `useEffect` with local state, matching the existing pattern (WindowPicker, ScreenPicker, etc. use the same approach).

### Tauri asset protocol for video playback

The HTML5 `<video>` element in the review screen needs to load a local file from `recordings/{uuid}/recording.webm`. Tauri sandbox-blocks direct file URLs by default. We enable the asset protocol in `tauri.conf.json`:

```jsonc
{
  "app": {
    "security": {
      "assetProtocol": {
        "enable": true,
        "scope": [
          "$APPDATA/com.prospertogether.desktop/recordings/**"
        ]
      }
    }
  }
}
```

The frontend then uses `convertFileSrc(absolutePath)` from `@tauri-apps/api/core` to turn the absolute path returned by `get_recording_video_path` into an `http://asset.localhost/...` URL the `<video>` element can load. Same approach for `thumbnail.jpg` in the pending list cards.

If this is the first use of the asset protocol in the app, the plan includes a verification step that plays a recording end-to-end to confirm the scope is correct.

### Error handling

| Scenario | Behavior |
|---|---|
| Stop → move to app data dir fails (e.g., disk full) | Rust returns error, frontend shows error, ffmpeg output stays in temp dir, not lost |
| Stop → thumbnail generation fails | Write metadata without thumbnail, review screen shows placeholder, not blocking |
| Stop → metadata.json write fails | Rust returns error, temp file stays, not lost |
| Submit → transcribe fails | Error banner on review, recording stays, Submit button re-enabled |
| Submit → keyframes fail | Non-blocking; continue to upload with no frames (current behavior) |
| Submit → upload fails | Error banner, recording stays, Submit button re-enabled |
| App crashes during Submit before delete | Recording still in `recordings/`, shows up in pending list on next launch |
| Corrupted metadata.json in one folder | `list_pending_recordings` logs error + skips that folder (doesn't fail whole list); `delete_recording` still works to remove it |
| User navigates away from `/review/:id` during submit | Submit continues in background — the hook doesn't abort on unmount. After completion, if the user is still in the app, show a toast; if they navigated to `/review/:sameId`, they see the completed state |

Actually, **simpler**: if the user navigates away mid-submit, abort the submit. Keep the recording, let them retry later. Avoids the complexity of background work outlasting the component. Document this as the chosen behavior.

### Concurrency and edge cases

- Multiple review tabs open for the same recording: not possible (Tauri single-window)
- User clicks Submit twice: disable the button immediately on first click
- Stop called twice concurrently: existing `recording_active` guard prevents this
- Recording folder partially written (e.g., `recording.webm` exists but `metadata.json` does not): `list_pending_recordings` treats missing metadata as corrupt, skips it; the folder stays until manually cleaned. Add a small repair pass: if a folder has a webm but no metadata, generate minimal metadata from file stat (duration unknown, label "Recovered recording")

### Recovery on startup (interacting with Track A migration)

Startup sequence:
1. Run bundle-identifier migration (Track A): copy old app data + move old temp state file
2. Run `recover_orphaned_recording` as today: if an ffmpeg is still alive from a previous process, restore state so stop_recording can finalize it into the durable location
3. Run a new `recover_incomplete_recordings` pass: look for folders in `recordings/` that have a webm but no metadata, generate minimal metadata for them so they appear in the pending list
4. Frontend on dashboard mount fetches pending count and surfaces the link

---

## Verification plan

End-to-end testing requires a running Windows machine. The verification below runs manually via `npm run tauri dev`.

### Track A verification

1. **Cold rebuild compiles**: after `cargo clean` and rename, `npm run tauri dev` starts without errors
2. **App title correct**: the main window title bar shows "Prosper Together"
3. **Tray tooltip correct**: hover the tray icon → shows "Prosper Together"
4. **App data path migrated**: if an old `%APPDATA%/com.userfirst.agent/settings.json` exists, after first launch of the new build, `%APPDATA%/com.prospertogether.desktop/settings.json` exists with the same contents, user doesn't need to re-login
5. **Zero references to old branding**: grep `-irn userfirst src/ src-tauri/src/` returns nothing (ignoring target/ and node_modules/)
6. **Bundle identifier in built installer** (if building release): `target/release/bundle/...` has `com.prospertogether.desktop`

### Track B verification

1. **Stop → review flow**: start a recording, stop → the agent shows a `/review/:id` screen with the video player loaded
2. **Video plays inline**: clicking play on the `<video>` element plays the recording
3. **Metadata correct**: duration, size, capture target title, timestamp all accurate
4. **Submit success path**: click Submit → see progress stages → success → confirmation page → verify `recordings/{id}/` folder is gone
5. **Submit failure path**: simulate an upload failure (e.g., bad auth token) → see error banner, recording folder still present → retry Submit → success
6. **Record Again**: click Record Again → confirm → folder deleted → land on `/record`
7. **Delete**: click Delete → confirm → folder deleted → land on `/recordings` (or `/` if empty)
8. **Close app mid-review**: stop recording, land on review, close the app. Relaunch → dashboard shows pending count badge → click → see the recording in the list → click → review screen loads with video
9. **Close app mid-submit**: click Submit, wait for transcribing, kill the app process → relaunch → recording still in pending list → retry Submit → success
10. **Multiple pending recordings**: record 3 in a row (each click Record Again instead of Submit) → see all 3 in the list, newest first, each with correct thumbnail
11. **Corrupt metadata recovery**: manually delete one `metadata.json` → restart the app → the folder still appears in the pending list with "Recovered recording" label → Delete works

---

## Out of scope

- Transcript displayed in preview screen (deferred per earlier question)
- Auto-cleanup of old pending recordings (user manages)
- Multi-select bulk delete
- Editing / trimming the video
- Cross-device sync
- Background submission that outlives navigation (simpler abort-on-navigate chosen instead)
- Renaming anywhere OUTSIDE `portal/agent/` (e.g., the `api/` or `web/` directories — this rename is scoped to the desktop app)
