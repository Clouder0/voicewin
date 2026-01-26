import { useEffect, useMemo, useState } from 'react';
import { HistoryPage } from './HistoryPage';
import { ModelsPage } from './ModelsPage';
import { OverviewPage } from './OverviewPage';
import { ProfilesPage } from './ProfilesPage';

type Page = 'overview' | 'profiles' | 'models' | 'history';


function PageContainer({ children }: { children: React.ReactNode }) {
  return <div className="vw-page vw-pageEnter">{children}</div>;
}

export function App() {
  const [page, setPage] = useState<Page>('overview');

  useEffect(() => {
    let unlisten: null | (() => void) = null;

    async function start() {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        unlisten = await listen<Page>('voicewin://navigate', (e) => {
          const dest = e.payload;
          if (dest === 'overview' || dest === 'profiles' || dest === 'models' || dest === 'history') {
            setPage(dest);
          }
        });
      } catch {
        // Not running inside Tauri.
      }
    }

    void start();

    return () => {
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
    }
  }, [page]);

  return (
    <div className="vw-shell">
      <nav className="vw-navRail" aria-label="Navigation">
        <button
          type="button"
          className="vw-navItem"
          data-active={page === 'overview'}
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
          onClick={() => setPage('history')}
          aria-label="History"
          title="History"
        >
          ≡
        </button>
      </nav>

      <main className="vw-content">{content}</main>
    </div>
  );
}
