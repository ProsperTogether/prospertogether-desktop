import { createContext, useEffect, type ReactNode } from 'react';
import { create } from 'zustand';

import api, { setAuthToken } from '../api/client';

interface AuthState {
  token: string | null;
  loading: boolean;
  email: string | null;
  setToken: (token: string | null) => void;
  setEmail: (email: string | null) => void;
  setLoading: (loading: boolean) => void;
  logout: () => void;
}

export const useAuthStore = create<AuthState>((set) => ({
  token: null,
  loading: true,
  email: null,
  setToken: (token) => {
    setAuthToken(token);
    set({ token });
  },
  setEmail: (email) => set({ email }),
  setLoading: (loading) => set({ loading }),
  logout: () => {
    setAuthToken(null);
    set({ token: null, email: null });
  }
}));

/**
 * Persist auth token + email to tauri-plugin-store, with localStorage fallback
 * for non-Tauri (dev) environments.
 */
export const persistAuth = async (token: string, email: string | null) => {
  try {
    const { load } = await import('@tauri-apps/plugin-store');
    const store = await load('settings.json');
    await store.set('auth_token', token);
    if (email) {
      await store.set('auth_email', email);
    } else {
      await store.delete('auth_email');
    }
    await store.save();
  } catch {
    localStorage.setItem('auth_token', token);
    if (email) {
      localStorage.setItem('auth_email', email);
    } else {
      localStorage.removeItem('auth_email');
    }
  }
};

/**
 * Clear persisted auth from tauri-plugin-store / localStorage.
 */
export const clearPersistedAuth = async () => {
  try {
    const { load } = await import('@tauri-apps/plugin-store');
    const store = await load('settings.json');
    await store.delete('auth_token');
    await store.delete('auth_email');
    await store.save();
  } catch {
    localStorage.removeItem('auth_token');
    localStorage.removeItem('auth_email');
  }
};

/**
 * Call POST /auth/refresh to validate the current token and rotate it forward.
 * On success, updates the in-memory store and persists the new token.
 * On 401/403 (token genuinely invalid/revoked), clears persisted auth and logs out.
 * On other errors (network down, server unreachable), leaves the cached session
 * intact so the user stays logged in across transient failures.
 */
const refreshSession = async (
  setToken: (t: string | null) => void,
  setEmail: (e: string | null) => void,
  logout: () => void
): Promise<boolean> => {
  try {
    const { data } = await api.post('/auth/refresh');
    const newToken: string | undefined = data?.token;
    const newEmail: string | null = data?.user?.email ?? null;
    if (newToken) {
      setToken(newToken);
      setEmail(newEmail);
      await persistAuth(newToken, newEmail);
      return true;
    }
    return false;
  } catch (err: any) {
    const status = err?.response?.status;
    if (status === 401 || status === 403) {
      await clearPersistedAuth();
      logout();
      return false;
    }
    // Transient error — keep the cached session, retry next interval/launch.
    return false;
  }
};

// Refresh the rolling JWT every 24 hours while the app is running so a
// long-running session never silently expires under the user.
const REFRESH_INTERVAL_MS = 24 * 60 * 60 * 1000;

const AuthContext = createContext<null>(null);

export const AuthProvider = ({ children }: { children: ReactNode }) => {
  const { token, setToken, setEmail, setLoading, logout } = useAuthStore();

  // Initial hydration from persistent storage + first refresh
  useEffect(() => {
    const loadToken = async () => {
      let savedToken: string | null = null;
      let savedEmail: string | null = null;

      try {
        // Try to load token + email from tauri-plugin-store
        const { load } = await import('@tauri-apps/plugin-store');
        const store = await load('settings.json');
        savedToken = (await store.get<string>('auth_token')) ?? null;
        savedEmail = (await store.get<string>('auth_email')) ?? null;
      } catch {
        // Not in Tauri environment (dev mode), check localStorage
        savedToken = localStorage.getItem('auth_token');
        savedEmail = localStorage.getItem('auth_email');
      }

      if (!savedToken) {
        setLoading(false);
        return;
      }

      // Hydrate immediately from cached values so the UI shows the user
      // without waiting for the network round-trip.
      setToken(savedToken);
      if (savedEmail) {
        setEmail(savedEmail);
      }

      // Validate + rotate the token with the server. This both refreshes
      // the email from the authoritative source and pushes the JWT expiry
      // forward, so any user who opens the app at least once per token
      // lifetime stays logged in indefinitely.
      await refreshSession(setToken, setEmail, logout);
      setLoading(false);
    };
    loadToken();
  }, [setToken, setEmail, setLoading, logout]);

  // Background refresh: while the app is open, rotate the token every 24h
  // so long-running sessions don't expire mid-use.
  useEffect(() => {
    if (!token) return;
    const interval = setInterval(() => {
      void refreshSession(setToken, setEmail, logout);
    }, REFRESH_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [token, setToken, setEmail, logout]);

  return <AuthContext.Provider value={null}>{children}</AuthContext.Provider>;
};
