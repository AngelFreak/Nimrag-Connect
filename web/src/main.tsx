import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import { QueryClientProvider } from '@tanstack/react-query';
import { registerSW } from 'virtual:pwa-register';

// Fonts are bundled (self-hosted); nothing is fetched from Google Fonts or any CDN at runtime.
import '@fontsource/inter/latin-400.css';
import '@fontsource/inter/latin-500.css';
import '@fontsource/inter/latin-600.css';
import '@fontsource/space-grotesk/latin-500.css';
import '@fontsource/space-grotesk/latin-600.css';
import './styles/tokens.css';
import './styles/global.css';
import './styles/components.css';
import './styles/account.css';

import { App } from './App';
import { queryClient } from './api/queryClient';
import { listenForFreshData } from './dataRefresh';

const root = document.getElementById('root');
if (!root) throw new Error('missing #root');

createRoot(root).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
);

// The page stays open for days in the Android app: refetch after the app says new data arrived and
// when the page comes back to the foreground.
listenForFreshData(queryClient);

// Offline shell. The worker is only generated in production builds. The browser checks for a new
// worker only on navigation, and the Android app keeps one page open for days, so also check when
// the page comes back to the foreground and every hour; a new worker reloads the page (autoUpdate).
if (import.meta.env.PROD) {
  registerSW({
    immediate: true,
    onRegisteredSW(_url, registration) {
      if (!registration) return;
      const check = () => {
        if (navigator.onLine) void registration.update().catch(() => undefined);
      };
      document.addEventListener('visibilitychange', () => {
        if (document.visibilityState === 'visible') check();
      });
      setInterval(check, 60 * 60 * 1000);
    },
  });
}
