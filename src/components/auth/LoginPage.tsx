import { useState } from 'react';

import api from '../../api/client';
import { persistAuth, useAuthStore } from '../../store/authStore';

export const LoginPage = () => {
  const [email, setEmail] = useState('');
  const [sent, setSent] = useState(false);
  const [error, setError] = useState('');
  const [manualToken, setManualToken] = useState('');
  const [loading, setLoading] = useState(false);
  const { setToken, setEmail: setStoreEmail } = useAuthStore();

  const handleRequestLink = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    setLoading(true);
    try {
      await api.post('/auth/magic-link/request', { email, platform: 'desktop' });
      setSent(true);
    } catch (err: any) {
      setError(err.response?.data?.message ?? 'Failed to send login code');
    } finally {
      setLoading(false);
    }
  };

  const handleManualToken = async () => {
    if (!manualToken.trim()) return;
    setError('');
    setLoading(true);
    try {
      const { data } = await api.post('/auth/magic-link/consume', { token: manualToken.trim() });
      const userEmail: string | null = data.user?.email ?? null;
      setToken(data.token);
      setStoreEmail(userEmail);
      await persistAuth(data.token, userEmail);
    } catch (err: any) {
      setError(err.response?.data?.message ?? 'Invalid or expired code');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex items-center justify-center h-screen bg-gradient-to-br from-slate-50 to-slate-100">
      <div className="w-full max-w-sm">
        {/* Logo */}
        <div className="flex items-center justify-center gap-2.5 mb-8">
          <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-brand-500 to-brand-700 flex items-center justify-center shadow-lg shadow-brand-500/20">
            <svg className="w-5 h-5 text-white" viewBox="0 0 24 24" fill="none" strokeWidth={2.2} stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" d="m15.75 10.5 4.72-4.72a.75.75 0 0 1 1.28.53v11.38a.75.75 0 0 1-1.28.53l-4.72-4.72M4.5 18.75h9a2.25 2.25 0 0 0 2.25-2.25v-9a2.25 2.25 0 0 0-2.25-2.25h-9A2.25 2.25 0 0 0 2.25 7.5v9a2.25 2.25 0 0 0 2.25 2.25Z" />
            </svg>
          </div>
          <span className="text-xl font-bold text-slate-900 tracking-tight">Prosper Together</span>
        </div>

        <div className="bg-white rounded-2xl shadow-xl shadow-slate-900/5 ring-1 ring-slate-900/5 p-7">
          {!sent ? (
            <>
              <div className="mb-6">
                <h1 className="text-lg font-semibold text-slate-900">Sign in</h1>
                <p className="text-[13px] text-slate-500 mt-1">Enter your email to receive a login code</p>
              </div>
              <form onSubmit={handleRequestLink} className="space-y-4">
                <div>
                  <label className="block text-[13px] font-medium text-slate-700 mb-1.5">Email address</label>
                  <input
                    type="email"
                    value={email}
                    onChange={(e) => setEmail(e.target.value)}
                    required
                    autoFocus
                    className="block w-full rounded-lg border border-slate-200 bg-slate-50/50 px-3.5 py-2.5 text-sm text-slate-900 placeholder:text-slate-400 focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 focus:bg-white transition outline-none"
                    placeholder="you@company.com"
                  />
                </div>
                {error && (
                  <div className="rounded-lg bg-red-50 border border-red-100 px-3.5 py-2.5">
                    <p className="text-[13px] text-red-600">{error}</p>
                  </div>
                )}
                <button
                  type="submit"
                  disabled={loading}
                  className="w-full px-4 py-2.5 bg-gradient-to-b from-brand-500 to-brand-600 text-white text-sm font-medium rounded-lg hover:from-brand-600 hover:to-brand-700 disabled:opacity-60 shadow-sm shadow-brand-600/25 transition"
                >
                  {loading ? 'Sending...' : 'Continue'}
                </button>
              </form>
            </>
          ) : (
            <>
              <div className="mb-6">
                <h1 className="text-lg font-semibold text-slate-900">Check your email</h1>
                <p className="text-[13px] text-slate-500 mt-1">
                  We sent a login code to <span className="font-medium text-slate-700">{email}</span>
                </p>
              </div>
              <div className="space-y-4">
                <div>
                  <label className="block text-[13px] font-medium text-slate-700 mb-1.5">Login code</label>
                  <input
                    type="text"
                    value={manualToken}
                    onChange={(e) => setManualToken(e.target.value)}
                    autoFocus
                    onKeyDown={(e) => e.key === 'Enter' && handleManualToken()}
                    className="block w-full rounded-lg border border-slate-200 bg-slate-50/50 px-3.5 py-2.5 text-sm font-mono text-slate-900 placeholder:text-slate-400 focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 focus:bg-white transition outline-none"
                    placeholder="Paste code from email"
                  />
                </div>
                {error && (
                  <div className="rounded-lg bg-red-50 border border-red-100 px-3.5 py-2.5">
                    <p className="text-[13px] text-red-600">{error}</p>
                  </div>
                )}
                <button
                  onClick={handleManualToken}
                  disabled={loading || !manualToken.trim()}
                  className="w-full px-4 py-2.5 bg-gradient-to-b from-brand-500 to-brand-600 text-white text-sm font-medium rounded-lg hover:from-brand-600 hover:to-brand-700 disabled:opacity-60 shadow-sm shadow-brand-600/25 transition"
                >
                  {loading ? 'Verifying...' : 'Sign in'}
                </button>
                <button
                  onClick={() => { setSent(false); setError(''); setManualToken(''); }}
                  className="w-full text-[13px] text-slate-500 hover:text-slate-700 transition"
                >
                  Use a different email
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
};
