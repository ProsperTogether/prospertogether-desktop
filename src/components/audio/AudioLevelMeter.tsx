import { useEffect, useRef, useState, useCallback } from 'react';

interface AudioLevelMeterProps {
  /** Start/stop the meter */
  active: boolean;
  /** Called once when audio signal is first detected */
  onAudioDetected?: () => void;
}

const BAR_COUNT = 20;

export const AudioLevelMeter = ({ active, onAudioDetected }: AudioLevelMeterProps) => {
  const [level, setLevel] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [detected, setDetected] = useState(false);
  const streamRef = useRef<MediaStream | null>(null);
  const rafRef = useRef<number>(0);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const detectedRef = useRef(false);

  const stop = useCallback(() => {
    if (rafRef.current) cancelAnimationFrame(rafRef.current);
    rafRef.current = 0;
    if (streamRef.current) {
      streamRef.current.getTracks().forEach(t => t.stop());
      streamRef.current = null;
    }
    analyserRef.current = null;
  }, []);

  useEffect(() => {
    if (!active) {
      stop();
      return;
    }

    let cancelled = false;

    (async () => {
      try {
        const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        if (cancelled) { stream.getTracks().forEach(t => t.stop()); return; }

        streamRef.current = stream;
        const ctx = new AudioContext();
        const source = ctx.createMediaStreamSource(stream);
        const analyser = ctx.createAnalyser();
        analyser.fftSize = 256;
        analyser.smoothingTimeConstant = 0.5;
        source.connect(analyser);
        analyserRef.current = analyser;

        const data = new Uint8Array(analyser.frequencyBinCount);

        const tick = () => {
          if (cancelled || !analyserRef.current) return;
          analyserRef.current.getByteFrequencyData(data);
          // RMS-ish: average of frequency bins, normalized 0-1
          let sum = 0;
          for (let i = 0; i < data.length; i++) sum += data[i];
          const avg = sum / data.length / 255;
          // Boost low levels for visibility
          const boosted = Math.min(1, avg * 3);
          setLevel(boosted);

          if (boosted > 0.05 && !detectedRef.current) {
            detectedRef.current = true;
            setDetected(true);
            onAudioDetected?.();
          }

          rafRef.current = requestAnimationFrame(tick);
        };
        tick();
      } catch (err) {
        if (!cancelled) {
          setError('Microphone access denied. Allow microphone permissions and try again.');
        }
      }
    })();

    return () => {
      cancelled = true;
      stop();
    };
  }, [active, stop, onAudioDetected]);

  // Reset detection state when deactivated
  useEffect(() => {
    if (!active) {
      detectedRef.current = false;
      setDetected(false);
      setLevel(0);
      setError(null);
    }
  }, [active]);

  if (error) {
    return (
      <div className="rounded-lg bg-red-50 border border-red-100 px-3 py-2">
        <p className="text-[12px] text-red-600">{error}</p>
      </div>
    );
  }

  if (!active) return null;

  return (
    <div className="space-y-2">
      {/* Bar meter */}
      <div className="flex items-center gap-[3px] h-6">
        {Array.from({ length: BAR_COUNT }, (_, i) => {
          const threshold = i / BAR_COUNT;
          const isLit = level > threshold;
          // Color: green for 0-60%, yellow for 60-80%, red for 80-100%
          const pct = i / BAR_COUNT;
          let color = 'bg-slate-200';
          if (isLit) {
            if (pct < 0.6) color = 'bg-emerald-500';
            else if (pct < 0.8) color = 'bg-yellow-400';
            else color = 'bg-red-500';
          }
          return (
            <div
              key={i}
              className={`flex-1 h-full rounded-sm transition-colors duration-75 ${color}`}
            />
          );
        })}
      </div>

      {/* Status */}
      <div className={`text-[12px] font-medium flex items-center gap-1.5 ${
        detected ? 'text-emerald-600' : 'text-slate-400'
      }`}>
        <div className={`w-1.5 h-1.5 rounded-full ${
          detected ? 'bg-emerald-500' : level > 0.02 ? 'bg-yellow-400 animate-pulse' : 'bg-slate-300'
        }`} />
        {detected
          ? 'Audio is working'
          : 'Speak or make a sound to test...'}
      </div>
    </div>
  );
};
