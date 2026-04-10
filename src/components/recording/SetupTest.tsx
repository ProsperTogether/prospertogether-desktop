import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { AudioLevelMeter } from '../audio/AudioLevelMeter';
import { useRecordingStore } from '../../store/recordingStore';

interface AudioDevice {
  id: string;
  name: string;
}

type TestStatus = 'idle' | 'capturing_screen' | 'test_recording' | 'playback';

export const SetupTest = () => {
  const navigate = useNavigate();
  const { selectedAudio, setSelectedAudio } = useRecordingStore();

  const [audioDevices, setAudioDevices] = useState<AudioDevice[]>([]);
  const [testStatus, setTestStatus] = useState<TestStatus>('idle');
  const [meterActive, setMeterActive] = useState(false);
  const [screenshotUrl, setScreenshotUrl] = useState<string | null>(null);
  const [testVideoUrl, setTestVideoUrl] = useState<string | null>(null);
  const [testingRecording, setTestingRecording] = useState(false);
  const [recordCountdown, setRecordCountdown] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    import('@tauri-apps/api/core')
      .then(core => core.invoke<AudioDevice[]>('list_audio_devices'))
      .then(devices => {
        setAudioDevices(devices);
        import('@tauri-apps/plugin-store')
          .then(({ load }) => load('settings.json'))
          .then(store => store.get<string>('preferred_audio_device'))
          .then(saved => {
            if (saved && devices.some(d => d.id === saved)) {
              setSelectedAudio(saved);
            } else if (devices.length > 0 && !selectedAudio) {
              setSelectedAudio(devices[0].id);
            }
          })
          .catch(() => {
            if (devices.length > 0 && !selectedAudio) {
              setSelectedAudio(devices[0].id);
            }
          });
      })
      .catch(() => {});
  }, [setSelectedAudio, selectedAudio]);

  const handleAudioDeviceChange = async (deviceId: string) => {
    setSelectedAudio(deviceId || null);
    setMeterActive(false);
    try {
      const { load } = await import('@tauri-apps/plugin-store');
      const store = await load('settings.json');
      await store.set('preferred_audio_device', deviceId);
      await store.save();
    } catch {
      // non-critical
    }
  };

  const handleCaptureScreenshot = async () => {
    setTestStatus('capturing_screen');
    setError(null);
    setScreenshotUrl(null);
    try {
      const core = await import('@tauri-apps/api/core');
      const dataUrl = await core.invoke<string>('capture_screenshot');
      setScreenshotUrl(dataUrl);
    } catch (err) {
      setError(`Screenshot failed: ${err}`);
    } finally {
      setTestStatus('idle');
    }
  };

  const handleTestRecording = async () => {
    setTestingRecording(true);
    setTestStatus('test_recording');
    setError(null);
    setTestVideoUrl(null);
    setRecordCountdown(5);
    try {
      const core = await import('@tauri-apps/api/core');
      const filePath = await core.invoke<string>('start_recording', {
        screen: '',
        audio: selectedAudio ?? '',
      });
      for (let i = 4; i >= 0; i--) {
        await new Promise(r => setTimeout(r, 1000));
        setRecordCountdown(i);
      }
      await core.invoke<string>('stop_recording');
      const fs = await import('@tauri-apps/plugin-fs');
      const bytes = await fs.readFile(filePath);
      const blob = new Blob([bytes], { type: 'video/webm' });
      setTestVideoUrl(URL.createObjectURL(blob));
      setTestStatus('playback');
    } catch (err) {
      setError(`Test recording failed: ${err}`);
      setTestStatus('idle');
    } finally {
      setTestingRecording(false);
      setRecordCountdown(0);
    }
  };

  const Section = ({ title, children: content }: { title: string; children: React.ReactNode }) => (
    <div className="bg-white rounded-2xl shadow-sm ring-1 ring-slate-900/5 p-5 space-y-3.5">
      <h3 className="text-[14px] font-semibold text-slate-900">{title}</h3>
      {content}
    </div>
  );

  return (
    <div className="p-5 max-w-lg mx-auto">
      <div className="mb-5">
        <h1 className="text-lg font-semibold text-slate-900">Setup & Test</h1>
        <p className="text-[13px] text-slate-500 mt-0.5">Verify your audio and screen capture</p>
      </div>

      <div className="space-y-3">
        {/* Audio Device */}
        <Section title="Audio Device">
          <div className="space-y-2">
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
                  onChange={() => handleAudioDeviceChange(d.id)}
                  className="sr-only"
                />
              </label>
            ))}
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
                onChange={() => handleAudioDeviceChange('none')}
                className="sr-only"
              />
            </label>
          </div>

          {selectedAudio && selectedAudio !== 'none' && (
            <div className="pt-1">
              <AudioLevelMeter active={meterActive} />
              {!meterActive ? (
                <button
                  onClick={() => setMeterActive(true)}
                  className="px-4 py-2 text-[13px] font-medium bg-slate-100 text-slate-700 rounded-lg hover:bg-slate-200 transition"
                >
                  Test Microphone
                </button>
              ) : (
                <button
                  onClick={() => setMeterActive(false)}
                  className="px-3 py-1.5 text-[12px] text-slate-500 hover:text-slate-700 transition"
                >
                  Stop test
                </button>
              )}
            </div>
          )}
        </Section>

        {/* Screen Capture */}
        <Section title="Screen Capture">
          <p className="text-[13px] text-slate-500">Take a test screenshot to verify screen capture works.</p>
          <button
            onClick={handleCaptureScreenshot}
            disabled={testStatus === 'capturing_screen'}
            className="px-4 py-2 text-[13px] font-medium bg-slate-100 text-slate-700 rounded-lg hover:bg-slate-200 disabled:opacity-50 transition"
          >
            {testStatus === 'capturing_screen' ? (
              <span className="flex items-center gap-2">
                <div className="w-3.5 h-3.5 border-2 border-slate-400 border-t-slate-700 rounded-full animate-spin" />
                Capturing...
              </span>
            ) : 'Capture Screenshot'}
          </button>
          {screenshotUrl && (
            <div>
              <img
                src={screenshotUrl}
                alt="Screen capture preview"
                className="w-full rounded-lg border border-slate-200"
              />
              <p className="text-[12px] text-emerald-600 font-medium mt-2 flex items-center gap-1.5">
                <div className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                Screen capture is working
              </p>
            </div>
          )}
        </Section>

        {/* Test Recording */}
        <Section title="Test Recording">
          <p className="text-[13px] text-slate-500">Record 5 seconds of video + audio to verify everything works.</p>

          {testingRecording ? (
            <div className="rounded-xl bg-slate-900 p-5 text-center space-y-3">
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
              disabled={testStatus === 'capturing_screen'}
              className="px-4 py-2 text-[13px] font-medium bg-slate-100 text-slate-700 rounded-lg hover:bg-slate-200 disabled:opacity-50 transition"
            >
              Record 5-Second Test
            </button>
          )}

          {testVideoUrl && !testingRecording && (
            <div>
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
        </Section>

        {error && (
          <div className="rounded-lg bg-red-50 border border-red-100 px-3.5 py-2.5">
            <p className="text-[13px] text-red-600">{error}</p>
          </div>
        )}

        <button
          onClick={() => navigate('/')}
          className="w-full px-4 py-2.5 text-[13px] text-slate-500 hover:text-slate-700 transition"
        >
          Back to Dashboard
        </button>
      </div>
    </div>
  );
};
