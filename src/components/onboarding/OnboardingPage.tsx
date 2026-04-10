import { useEffect, useState, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';

import { AudioLevelMeter } from '../audio/AudioLevelMeter';
import { useRecordingStore } from '../../store/recordingStore';

interface AudioDevice {
  id: string;
  name: string;
}

type Step = 'welcome' | 'audio' | 'screen' | 'test' | 'done';

export const OnboardingPage = () => {
  const navigate = useNavigate();
  const { selectedAudio, setSelectedAudio } = useRecordingStore();

  const [step, setStep] = useState<Step>('welcome');
  const [audioDevices, setAudioDevices] = useState<AudioDevice[]>([]);
  const [meterActive, setMeterActive] = useState(false);
  const [screenshotUrl, setScreenshotUrl] = useState<string | null>(null);
  const [capturingScreen, setCapturingScreen] = useState(false);
  const [testVideoUrl, setTestVideoUrl] = useState<string | null>(null);
  const [testingRecording, setTestingRecording] = useState(false);
  const [recordCountdown, setRecordCountdown] = useState(0);
  const [error, setError] = useState<string | null>(null);

  // Fetch audio devices
  useEffect(() => {
    import('@tauri-apps/api/core')
      .then(core => core.invoke<AudioDevice[]>('list_audio_devices'))
      .then(devices => {
        setAudioDevices(devices);
        if (devices.length > 0 && !selectedAudio) {
          setSelectedAudio(devices[0].id);
        }
      })
      .catch(() => {});
  }, [setSelectedAudio, selectedAudio]);

  const handleSelectAudio = async (deviceId: string) => {
    setSelectedAudio(deviceId || null);
    setMeterActive(false);
    try {
      const { load } = await import('@tauri-apps/plugin-store');
      const store = await load('settings.json');
      await store.set('preferred_audio_device', deviceId);
      await store.save();
    } catch { /* non-critical */ }
  };

  const handleCaptureScreen = async () => {
    setCapturingScreen(true);
    setError(null);
    setScreenshotUrl(null);
    try {
      const core = await import('@tauri-apps/api/core');
      const dataUrl = await core.invoke<string>('capture_screenshot');
      setScreenshotUrl(dataUrl);
    } catch (err) {
      setError(String(err));
    } finally {
      setCapturingScreen(false);
    }
  };

  const handleTestRecording = async () => {
    setTestingRecording(true);
    setError(null);
    setTestVideoUrl(null);
    setRecordCountdown(5);
    try {
      const core = await import('@tauri-apps/api/core');
      const filePath = await core.invoke<string>('start_recording', {
        screen: '',
        audio: selectedAudio ?? '',
      });
      // Tick countdown each second
      for (let i = 4; i >= 0; i--) {
        await new Promise(r => setTimeout(r, 1000));
        setRecordCountdown(i);
      }
      await core.invoke<string>('stop_recording');
      const fs = await import('@tauri-apps/plugin-fs');
      const bytes = await fs.readFile(filePath);
      const blob = new Blob([bytes], { type: 'video/webm' });
      setTestVideoUrl(URL.createObjectURL(blob));
    } catch (err) {
      setError(String(err));
    } finally {
      setTestingRecording(false);
      setRecordCountdown(0);
    }
  };

  const handleFinish = useCallback(async () => {
    try {
      const { load } = await import('@tauri-apps/plugin-store');
      const store = await load('settings.json');
      await store.set('onboarding_complete', true);
      await store.save();
    } catch {
      localStorage.setItem('onboarding_complete', 'true');
    }
    navigate('/');
  }, [navigate]);

  const steps: { key: Step; label: string }[] = [
    { key: 'welcome', label: 'Welcome' },
    { key: 'audio', label: 'Audio' },
    { key: 'screen', label: 'Screen' },
    { key: 'test', label: 'Test' },
    { key: 'done', label: 'Ready' },
  ];
  const currentIdx = steps.findIndex(s => s.key === step);

  return (
    <div className="min-h-screen bg-gradient-to-br from-slate-50 to-slate-100 flex flex-col items-center justify-center p-6">
      <div className="w-full max-w-md">

        {/* Progress dots */}
        <div className="flex items-center justify-center gap-2 mb-8">
          {steps.map((s, i) => (
            <div
              key={s.key}
              className={`h-1.5 rounded-full transition-all duration-300 ${
                i <= currentIdx
                  ? 'bg-brand-500 w-8'
                  : 'bg-slate-200 w-4'
              }`}
            />
          ))}
        </div>

        <div className="bg-white rounded-2xl shadow-xl shadow-slate-900/5 ring-1 ring-slate-900/5 p-7">

          {/* ── Welcome ─────────────────────────────────── */}
          {step === 'welcome' && (
            <div className="text-center">
              <div className="w-14 h-14 rounded-2xl bg-gradient-to-br from-brand-500 to-brand-700 flex items-center justify-center mx-auto mb-5 shadow-lg shadow-brand-500/20">
                <svg className="w-7 h-7 text-white" viewBox="0 0 24 24" fill="none" strokeWidth={2} stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" d="m15.75 10.5 4.72-4.72a.75.75 0 0 1 1.28.53v11.38a.75.75 0 0 1-1.28.53l-4.72-4.72M4.5 18.75h9a2.25 2.25 0 0 0 2.25-2.25v-9a2.25 2.25 0 0 0-2.25-2.25h-9A2.25 2.25 0 0 0 2.25 7.5v9a2.25 2.25 0 0 0 2.25 2.25Z" />
                </svg>
              </div>
              <h1 className="text-xl font-bold text-slate-900 mb-2">Welcome to Prosper Together</h1>
              <p className="text-[14px] text-slate-500 leading-relaxed mb-8">
                Let's set up your microphone and screen capture so you're ready to record.
              </p>
              <button
                onClick={() => setStep('audio')}
                className="w-full px-4 py-2.5 bg-gradient-to-b from-brand-500 to-brand-600 text-white text-sm font-medium rounded-lg hover:from-brand-600 hover:to-brand-700 shadow-sm shadow-brand-600/25 transition"
              >
                Get Started
              </button>
            </div>
          )}

          {/* ── Audio Device ────────────────────────────── */}
          {step === 'audio' && (
            <div>
              <h2 className="text-lg font-semibold text-slate-900 mb-1">Select your microphone</h2>
              <p className="text-[13px] text-slate-500 mb-5">Choose which audio device to use for recordings.</p>

              <div className="space-y-2 mb-5">
                {audioDevices.map(d => (
                  <label
                    key={d.id}
                    className={`flex items-center gap-3 p-3.5 rounded-xl border cursor-pointer transition ${
                      selectedAudio === d.id
                        ? 'border-brand-500 bg-brand-50/50 ring-1 ring-brand-500/20'
                        : 'border-slate-200 hover:border-slate-300'
                    }`}
                  >
                    <div className={`w-4 h-4 rounded-full border-2 flex items-center justify-center flex-shrink-0 ${
                      selectedAudio === d.id ? 'border-brand-500' : 'border-slate-300'
                    }`}>
                      {selectedAudio === d.id && (
                        <div className="w-2 h-2 rounded-full bg-brand-500" />
                      )}
                    </div>
                    <span className="text-[13px] font-medium text-slate-800">{d.name}</span>
                    <input
                      type="radio"
                      name="audio"
                      value={d.id}
                      checked={selectedAudio === d.id}
                      onChange={() => handleSelectAudio(d.id)}
                      className="sr-only"
                    />
                  </label>
                ))}
                {audioDevices.length === 0 && (
                  <label
                    className={`flex items-center gap-3 p-3.5 rounded-xl border cursor-pointer transition ${
                      selectedAudio === 'none'
                        ? 'border-brand-500 bg-brand-50/50 ring-1 ring-brand-500/20'
                        : 'border-slate-200 hover:border-slate-300'
                    }`}
                  >
                    <div className={`w-4 h-4 rounded-full border-2 flex items-center justify-center flex-shrink-0 ${
                      selectedAudio === 'none' ? 'border-brand-500' : 'border-slate-300'
                    }`}>
                      {selectedAudio === 'none' && (
                        <div className="w-2 h-2 rounded-full bg-brand-500" />
                      )}
                    </div>
                    <span className="text-[13px] font-medium text-slate-500">No audio (screen only)</span>
                    <input
                      type="radio"
                      name="audio"
                      value="none"
                      checked={selectedAudio === 'none'}
                      onChange={() => handleSelectAudio('none')}
                      className="sr-only"
                    />
                  </label>
                )}
              </div>

              {/* Live audio level meter */}
              {selectedAudio && selectedAudio !== 'none' && (
                <div className="mb-5">
                  <AudioLevelMeter active={meterActive} />
                  {!meterActive && (
                    <button
                      onClick={() => setMeterActive(true)}
                      className="px-4 py-2 text-[13px] font-medium bg-slate-100 text-slate-700 rounded-lg hover:bg-slate-200 transition"
                    >
                      Test Microphone
                    </button>
                  )}
                </div>
              )}

              {error && (
                <div className="rounded-lg bg-red-50 border border-red-100 px-3 py-2 mb-4">
                  <p className="text-[12px] text-red-600">{error}</p>
                </div>
              )}

              <div className="flex gap-2.5">
                <button
                  onClick={() => { setStep('welcome'); setError(null); setMeterActive(false); }}
                  className="px-4 py-2.5 text-sm text-slate-500 border border-slate-200 rounded-lg hover:bg-slate-50 transition"
                >
                  Back
                </button>
                <button
                  onClick={() => { setStep('screen'); setError(null); setMeterActive(false); }}
                  className="flex-1 px-4 py-2.5 bg-gradient-to-b from-brand-500 to-brand-600 text-white text-sm font-medium rounded-lg hover:from-brand-600 hover:to-brand-700 shadow-sm shadow-brand-600/25 transition"
                >
                  Continue
                </button>
              </div>
            </div>
          )}

          {/* ── Screen Capture ──────────────────────────── */}
          {step === 'screen' && (
            <div>
              <h2 className="text-lg font-semibold text-slate-900 mb-1">Screen capture</h2>
              <p className="text-[13px] text-slate-500 mb-5">Take a test screenshot to verify screen capture works.</p>

              <button
                onClick={handleCaptureScreen}
                disabled={capturingScreen}
                className="px-4 py-2 text-[13px] font-medium bg-slate-100 text-slate-700 rounded-lg hover:bg-slate-200 disabled:opacity-50 transition mb-4"
              >
                {capturingScreen ? (
                  <span className="flex items-center gap-2">
                    <div className="w-3.5 h-3.5 border-2 border-slate-400 border-t-slate-700 rounded-full animate-spin" />
                    Capturing...
                  </span>
                ) : 'Capture Screenshot'}
              </button>

              {screenshotUrl && (
                <div className="mb-4">
                  <img
                    src={screenshotUrl}
                    alt="Screen preview"
                    className="w-full rounded-lg border border-slate-200"
                  />
                  <p className="text-[12px] text-emerald-600 font-medium mt-2 flex items-center gap-1.5">
                    <div className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                    Screen capture is working
                  </p>
                </div>
              )}

              {error && (
                <div className="rounded-lg bg-red-50 border border-red-100 px-3 py-2 mb-4">
                  <p className="text-[12px] text-red-600">{error}</p>
                </div>
              )}

              <div className="flex gap-2.5">
                <button
                  onClick={() => { setStep('audio'); setError(null); }}
                  className="px-4 py-2.5 text-sm text-slate-500 border border-slate-200 rounded-lg hover:bg-slate-50 transition"
                >
                  Back
                </button>
                <button
                  onClick={() => { setStep('test'); setError(null); }}
                  className="flex-1 px-4 py-2.5 bg-gradient-to-b from-brand-500 to-brand-600 text-white text-sm font-medium rounded-lg hover:from-brand-600 hover:to-brand-700 shadow-sm shadow-brand-600/25 transition"
                >
                  Continue
                </button>
              </div>
            </div>
          )}

          {/* ── Test Recording ──────────────────────────── */}
          {step === 'test' && (
            <div>
              <h2 className="text-lg font-semibold text-slate-900 mb-1">Test recording</h2>
              <p className="text-[13px] text-slate-500 mb-5">Record a quick 5-second clip to make sure everything works together.</p>

              {testingRecording ? (
                <div className="mb-5 rounded-xl bg-slate-900 p-5 text-center space-y-3">
                  <div className="flex items-center justify-center gap-2">
                    <div className="w-2.5 h-2.5 rounded-full bg-red-500 animate-pulse shadow-lg shadow-red-500/50" />
                    <span className="text-xs uppercase tracking-widest font-semibold text-red-400">Recording</span>
                  </div>
                  <p className="text-4xl font-light font-mono text-white tabular-nums">
                    0:{recordCountdown.toString().padStart(2, '0')}
                  </p>
                  <div className="w-full bg-slate-700 rounded-full h-1.5 overflow-hidden">
                    <div
                      className="h-full bg-red-500 rounded-full transition-all duration-1000 ease-linear"
                      style={{ width: `${((5 - recordCountdown) / 5) * 100}%` }}
                    />
                  </div>
                  <p className="text-[12px] text-slate-500">Speak and move your mouse to test...</p>
                </div>
              ) : (
                <button
                  onClick={handleTestRecording}
                  className="px-4 py-2 text-[13px] font-medium bg-slate-100 text-slate-700 rounded-lg hover:bg-slate-200 transition mb-4"
                >
                  Record 5-Second Test
                </button>
              )}

              {testVideoUrl && !testingRecording && (
                <div className="mb-4">
                  <video
                    src={testVideoUrl}
                    controls
                    autoPlay
                    className="w-full rounded-lg border border-slate-200"
                  />
                  <p className="text-[12px] text-emerald-600 font-medium mt-2 flex items-center gap-1.5">
                    <div className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                    Recording complete — check video and audio playback
                  </p>
                </div>
              )}

              {error && (
                <div className="rounded-lg bg-red-50 border border-red-100 px-3 py-2 mb-4">
                  <p className="text-[12px] text-red-600">{error}</p>
                </div>
              )}

              <div className="flex gap-2.5">
                <button
                  onClick={() => { setStep('screen'); setError(null); }}
                  className="px-4 py-2.5 text-sm text-slate-500 border border-slate-200 rounded-lg hover:bg-slate-50 transition"
                >
                  Back
                </button>
                <button
                  onClick={() => setStep('done')}
                  className="flex-1 px-4 py-2.5 bg-gradient-to-b from-brand-500 to-brand-600 text-white text-sm font-medium rounded-lg hover:from-brand-600 hover:to-brand-700 shadow-sm shadow-brand-600/25 transition"
                >
                  Continue
                </button>
              </div>
            </div>
          )}

          {/* ── Done ────────────────────────────────────── */}
          {step === 'done' && (
            <div className="text-center">
              <div className="w-14 h-14 rounded-full bg-emerald-100 flex items-center justify-center mx-auto mb-5">
                <svg className="w-7 h-7 text-emerald-600" fill="none" viewBox="0 0 24 24" strokeWidth={2.2} stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" d="m4.5 12.75 6 6 9-13.5" />
                </svg>
              </div>
              <h1 className="text-xl font-bold text-slate-900 mb-2">You're all set!</h1>
              <p className="text-[14px] text-slate-500 leading-relaxed mb-8">
                Your microphone and screen capture are configured. You can change these anytime from the profile menu.
              </p>
              <div className="flex gap-2.5">
                <button
                  onClick={() => setStep('test')}
                  className="px-4 py-2.5 text-sm text-slate-500 border border-slate-200 rounded-lg hover:bg-slate-50 transition"
                >
                  Back
                </button>
                <button
                  onClick={handleFinish}
                  className="flex-1 px-4 py-2.5 bg-gradient-to-b from-brand-500 to-brand-600 text-white text-sm font-medium rounded-lg hover:from-brand-600 hover:to-brand-700 shadow-sm shadow-brand-600/25 transition"
                >
                  Start Using Prosper Together
                </button>
              </div>
            </div>
          )}
        </div>

        {/* Skip link */}
        {step !== 'done' && (
          <button
            onClick={handleFinish}
            className="block mx-auto mt-4 text-[13px] text-slate-400 hover:text-slate-600 transition"
          >
            Skip setup
          </button>
        )}
      </div>
    </div>
  );
};
