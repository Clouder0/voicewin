import { useEffect, useMemo, useState } from 'react';
import { HistoryPage } from './HistoryPage';
import { ModelsPage } from './ModelsPage';
import { OverviewPage } from './OverviewPage';
import { ProfilesPage } from './ProfilesPage';
import { SettingsPage } from './SettingsPage';

type Page = 'overview' | 'profiles' | 'models' | 'history' | 'settings';


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
            if (dest === 'overview' || dest === 'profiles' || dest === 'models' || dest === 'history' || dest === 'settings') {
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
          ◎
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
          ◧
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
          ◼
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
          ≡
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
          ⚙
        </button>
      </nav>

      <main className="vw-content">{content}</main>
    </div>
  );
}
