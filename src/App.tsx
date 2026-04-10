import { useEffect, useState } from 'react';
import { Navigate, Route, Routes } from 'react-router-dom';

import { LoginPage } from './components/auth/LoginPage';
import { AppLayout } from './components/layout/AppLayout';
import { DashboardPage } from './components/dashboard/DashboardPage';
import { OnboardingPage } from './components/onboarding/OnboardingPage';
import { RecordingControls } from './components/recording/RecordingControls';
import { SetupTest } from './components/recording/SetupTest';
import { OverlayPage } from './components/overlay/OverlayPage';
import { BorderPage } from './components/overlay/BorderPage';
import { RegionPickerPage } from './components/recording/RegionPickerPage';
import { DockPickerPage } from './components/recording/DockPickerPage';
import { SubmitPage } from './components/submission/SubmitPage';
import { ReviewPage } from './components/recording/ReviewPage';
import { RecordingsListPage } from './components/recording/RecordingsListPage';
import { useAuthStore } from './store/authStore';

function useOnboardingCheck() {
  const { token } = useAuthStore();
  const [checked, setChecked] = useState(false);
  const [complete, setComplete] = useState(false);

  useEffect(() => {
    if (!token) {
      setChecked(true);
      return;
    }
    (async () => {
      try {
        const { load } = await import('@tauri-apps/plugin-store');
        const store = await load('settings.json');
        const done = await store.get<boolean>('onboarding_complete');
        setComplete(!!done);
      } catch {
        setComplete(localStorage.getItem('onboarding_complete') === 'true');
      }
      setChecked(true);
    })();
  }, [token]);

  return { checked, complete };
}

function AuthedLayout({ children }: { children: React.ReactNode }) {
  const { token } = useAuthStore();
  const { checked, complete } = useOnboardingCheck();

  if (!token) return <Navigate to="/login" />;
  if (!checked) {
    return (
      <div className="flex items-center justify-center h-screen bg-slate-50">
        <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-brand-500 to-brand-700 animate-pulse" />
      </div>
    );
  }
  if (!complete) return <Navigate to="/onboarding" />;
  return <AppLayout>{children}</AppLayout>;
}

export default function App() {
  const { token, loading } = useAuthStore();
  const { checked, complete } = useOnboardingCheck();

  if (loading) {
    return (
      <div className="flex items-center justify-center h-screen bg-slate-50">
        <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-brand-500 to-brand-700 animate-pulse" />
      </div>
    );
  }

  return (
    <Routes>
      <Route path="/login" element={token ? <Navigate to="/" /> : <LoginPage />} />
      <Route
        path="/onboarding"
        element={
          !token ? <Navigate to="/login" /> :
          (checked && complete) ? <Navigate to="/" /> :
          <OnboardingPage />
        }
      />
      <Route path="/" element={<AuthedLayout><DashboardPage /></AuthedLayout>} />
      <Route path="/record" element={<AuthedLayout><RecordingControls /></AuthedLayout>} />
      <Route path="/submit" element={<AuthedLayout><SubmitPage /></AuthedLayout>} />
      <Route path="/review/:id" element={<AuthedLayout><ReviewPage /></AuthedLayout>} />
      <Route path="/recordings" element={<AuthedLayout><RecordingsListPage /></AuthedLayout>} />
      <Route path="/setup" element={<AuthedLayout><SetupTest /></AuthedLayout>} />
      <Route path="/overlay" element={<OverlayPage />} />
      <Route path="/region-picker" element={<RegionPickerPage />} />
      <Route path="/dock-picker" element={<DockPickerPage />} />
      <Route path="/border" element={<BorderPage />} />
      <Route path="*" element={<Navigate to="/" />} />
    </Routes>
  );
}
