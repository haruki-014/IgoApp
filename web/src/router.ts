// 2 画面しかないので、History API だけの小さなルーターで足りる。

import { useEffect, useState } from 'react';

export type Route = { page: 'home' } | { page: 'game'; id: string } | { page: 'not-found' };

const GAME_PATH = /^\/g\/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\/?$/i;

export function parseRoute(pathname: string): Route {
  if (pathname === '/' || pathname === '') return { page: 'home' };
  const match = GAME_PATH.exec(pathname);
  if (match) return { page: 'game', id: match[1].toLowerCase() };
  return { page: 'not-found' };
}

export const gamePath = (id: string) => `/g/${id}`;

const NAVIGATE_EVENT = 'igo:navigate';

export function navigate(path: string): void {
  window.history.pushState(null, '', path);
  window.dispatchEvent(new Event(NAVIGATE_EVENT));
  window.scrollTo(0, 0);
}

export function useRoute(): Route {
  const [pathname, setPathname] = useState(() => window.location.pathname);
  useEffect(() => {
    const update = () => setPathname(window.location.pathname);
    window.addEventListener('popstate', update);
    window.addEventListener(NAVIGATE_EVENT, update);
    return () => {
      window.removeEventListener('popstate', update);
      window.removeEventListener(NAVIGATE_EVENT, update);
    };
  }, []);
  return parseRoute(pathname);
}

/** 同じサイト内のリンク。通常のクリックだけを横取りし、新しいタブで開く操作はそのまま通す。 */
export function onLinkClick(event: React.MouseEvent<HTMLAnchorElement>): void {
  if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
    return;
  }
  const href = event.currentTarget.getAttribute('href');
  if (!href || !href.startsWith('/')) return;
  event.preventDefault();
  navigate(href);
}
