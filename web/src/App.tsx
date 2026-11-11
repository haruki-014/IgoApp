import { useEffect, useState } from 'react';
import { fetchEngines } from './api';
import { ThemeToggle } from './components/ui';
import { GamePage } from './pages/Game';
import { HomePage } from './pages/Home';
import { onLinkClick, useRoute } from './router';
import type { EngineInfo } from './types';

export function App() {
  const route = useRoute();
  const [engines, setEngines] = useState<EngineInfo[]>([]);
  const [enginesStatus, setEnginesStatus] = useState<'loading' | 'ready' | 'failed'>('loading');

  useEffect(() => {
    let cancelled = false;
    fetchEngines()
      .then((list) => {
        if (cancelled) return;
        setEngines(list);
        setEnginesStatus('ready');
      })
      .catch(() => !cancelled && setEnginesStatus('failed'));
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    document.title = route.page === 'game' ? '対局 | 囲碁対局' : '囲碁対局';
  }, [route.page]);

  return (
    <>
      <header className="site-header">
        <a className="brand" href="/" onClick={onLinkClick}>
          <span className="brand-mark" aria-hidden="true" />
          囲碁対局
        </a>
        <ThemeToggle />
      </header>
      {route.page === 'home' && <HomePage engines={engines} enginesStatus={enginesStatus} />}
      {route.page === 'game' && <GamePage key={route.id} id={route.id} engines={engines} />}
      {route.page === 'not-found' && (
        <main className="message-page">
          <h1>ページが見つかりません</h1>
          <a className="button button-primary" href="/" onClick={onLinkClick}>
            トップへ戻る
          </a>
        </main>
      )}
    </>
  );
}
