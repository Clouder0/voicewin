import { useEffect, useMemo, useState } from 'react';
import { HistoryPage } from './HistoryPage';
import { ModelsPage } from './ModelsPage';
import { OverviewPage } from './OverviewPage';
import { ProfilesPage } from './ProfilesPage';
import { PromptsPage } from './PromptsPage';
import { SettingsPage } from './SettingsPage';

type Page = 'overview' | 'profiles' | 'prompts' | 'models' | 'history' | 'settings';

function NavIcon({ children }: { children: React.ReactNode }) {
  return (
    <svg className="vw-navIcon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      {children}
    </svg>
  );
}

function OverviewIcon() {
  return (
    <NavIcon>
      <circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" strokeWidth="2" />
      <circle cx="12" cy="12" r="2" fill="currentColor" />
    </NavIcon>
  );
}

function ProfilesIcon() {
  return (
    <NavIcon>
      <rect x="4" y="4" width="7" height="7" rx="1.2" fill="none" stroke="currentColor" strokeWidth="2" />
      <rect x="13" y="4" width="7" height="7" rx="1.2" fill="none" stroke="currentColor" strokeWidth="2" />
      <rect x="4" y="13" width="7" height="7" rx="1.2" fill="none" stroke="currentColor" strokeWidth="2" />
      <rect x="13" y="13" width="7" height="7" rx="1.2" fill="currentColor" />
    </NavIcon>
  );
}

function PromptsIcon() {
  return (
    <NavIcon>
      <path d="M6 7h12M6 12h12M6 17h8" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
      <circle cx="16.5" cy="17" r="1.5" fill="currentColor" />
    </NavIcon>
  );
}

function ModelsIcon() {
  return (
    <NavIcon>
      <path d="M12 3l8 4.5v9L12 21l-8-4.5v-9L12 3z" fill="none" stroke="currentColor" strokeWidth="2" />
      <path d="M12 8.2l3.8 2.1v3.4L12 15.8l-3.8-2.1v-3.4L12 8.2z" fill="currentColor" />
    </NavIcon>
  );
}

function HistoryIcon() {
  return (
    <NavIcon>
      <circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" strokeWidth="2" />
      <path d="M12 7.5v5l3.5 2" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    </NavIcon>
  );
}

function SettingsIcon() {
  return (
    <NavIcon>
      <circle cx="12" cy="12" r="3" fill="none" stroke="currentColor" strokeWidth="2" />
      <path
        d="M12 3.8v2.2M12 18v2.2M20.2 12H18M6 12H3.8M17.8 6.2l-1.6 1.6M7.8 16.2l-1.6 1.6M6.2 6.2l1.6 1.6M16.2 16.2l1.6 1.6"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
    </NavIcon>
  );
}


function PageContainer({ children }: { children: React.ReactNode }) {
  return <div className="vw-page vw-pageEnter">{children}</div>;
}

export function App() {
  const [page, setPage] = useState<Page>('overview');

  useEffect(() => {
    let disposed = false;
    let unlisten: null | (() => void) = null;

    async function start() {
        try {
          const { listen } = await import('@tauri-apps/api/event');
          const nextUnlisten = await listen<Page>('voicewin://navigate', (e) => {
            const dest = e.payload;
            if (dest === 'overview' || dest === 'profiles' || dest === 'prompts' || dest === 'models' || dest === 'history' || dest === 'settings') {
              setPage(dest);
            }
          });

          if (disposed) {
            nextUnlisten();
            return;
          }

          unlisten = nextUnlisten;
        } catch {
          // Not running inside Tauri.
        }
    }

    void start();

    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, []);

  const content = useMemo(() => {
    switch (page) {
      case 'overview':
        return (
          <PageContainer>
            <OverviewPage />
          </PageContainer>
        );
      case 'profiles':
        return (
          <PageContainer>
            <ProfilesPage />
          </PageContainer>
        );
      case 'prompts':
        return (
          <PageContainer>
            <PromptsPage />
          </PageContainer>
        );
      case 'models':
        return (
          <PageContainer>
            <ModelsPage />
          </PageContainer>
        );
      case 'history':
        return (
          <PageContainer>
            <HistoryPage />
          </PageContainer>
        );
      case 'settings':
        return (
          <PageContainer>
            <SettingsPage />
          </PageContainer>
        );
    }
  }, [page]);

  return (
    <div className="vw-shell">
      <nav className="vw-navRail" aria-label="Navigation">
        <button
          type="button"
          className="vw-navItem"
          data-active={page === 'overview'}
          aria-current={page === 'overview' ? 'page' : undefined}
          onClick={() => setPage('overview')}
          aria-label="Overview"
          title="Overview"
        >
          <OverviewIcon />
        </button>
        <button
          type="button"
          className="vw-navItem"
          data-active={page === 'profiles'}
          aria-current={page === 'profiles' ? 'page' : undefined}
          onClick={() => setPage('profiles')}
          aria-label="Profiles"
          title="Profiles"
        >
          <ProfilesIcon />
        </button>
        <button
          type="button"
          className="vw-navItem"
          data-active={page === 'prompts'}
          aria-current={page === 'prompts' ? 'page' : undefined}
          onClick={() => setPage('prompts')}
          aria-label="Prompts"
          title="Prompts"
        >
          <PromptsIcon />
        </button>
        <button
          type="button"
          className="vw-navItem"
          data-active={page === 'models'}
          aria-current={page === 'models' ? 'page' : undefined}
          onClick={() => setPage('models')}
          aria-label="Models"
          title="Models"
        >
          <ModelsIcon />
        </button>
        <button
          type="button"
          className="vw-navItem"
          data-active={page === 'history'}
          aria-current={page === 'history' ? 'page' : undefined}
          onClick={() => setPage('history')}
          aria-label="History"
          title="History"
        >
          <HistoryIcon />
        </button>
        <button
          type="button"
          className="vw-navItem"
          data-active={page === 'settings'}
          aria-current={page === 'settings' ? 'page' : undefined}
          onClick={() => setPage('settings')}
          aria-label="Settings"
          title="Settings"
        >
          <SettingsIcon />
        </button>
      </nav>

      <main className="vw-content">{content}</main>
    </div>
  );
}
