// 席トークンの保存。ログインの代わりに、ブラウザがトークンを覚えておいて再接続に使う。
//
// - localStorage:このブラウザの席。作成者や、招待 URL を開いた人のトークンを保存する。
// - sessionStorage:このタブだけの席。すでに別の席のトークンを持っているブラウザで
//   招待 URL を開いた場合(1 つのブラウザの 2 つのタブで試す場合など)に使う。

const tokenKey = (id: string) => `igo.token.${id}`;
const inviteKey = (id: string) => `igo.invite.${id}`;
const RECENT_KEY = 'igo.recent';

function read(storage: Storage | undefined, key: string): string | null {
  try {
    return storage?.getItem(key) ?? null;
  } catch {
    return null;
  }
}

function write(storage: Storage | undefined, key: string, value: string): void {
  try {
    storage?.setItem(key, value);
  } catch {
    // 保存できない環境(プライベートブラウズなど)では、このページを開いている間だけ有効。
  }
}

const local = () => (typeof localStorage === 'undefined' ? undefined : localStorage);
const session = () => (typeof sessionStorage === 'undefined' ? undefined : sessionStorage);

/** この対局の席トークン。タブ固有のものを優先する。 */
export function loadToken(id: string): string | null {
  return read(session(), tokenKey(id)) ?? read(local(), tokenKey(id));
}

/** 対局を作成したときに、自分の席トークンを保存する。 */
export function saveOwnToken(id: string, token: string): void {
  write(local(), tokenKey(id), token);
}

/**
 * 招待 URL から受け取ったトークンを保存する。
 * このブラウザがまだ席を持っていなければ localStorage に、すでに別の席を持っていれば
 * このタブだけ(sessionStorage)に保存する。
 */
export function saveInvitedToken(id: string, token: string): void {
  const existing = read(local(), tokenKey(id));
  if (existing === null || existing === token) {
    write(local(), tokenKey(id), token);
  } else {
    write(session(), tokenKey(id), token);
  }
}

/** 相手に渡す招待トークン(対局の作成者だけが持つ)。 */
export function loadInviteToken(id: string): string | null {
  return read(local(), inviteKey(id));
}

export function saveInviteToken(id: string, token: string): void {
  write(local(), inviteKey(id), token);
}

/** このブラウザで作成・参加した対局の記録(トップ画面の一覧用)。 */
export interface RecentGame {
  id: string;
  size: number;
  black: string;
  white: string;
  at: number;
}

export function loadRecent(): RecentGame[] {
  try {
    const parsed: unknown = JSON.parse(read(local(), RECENT_KEY) ?? '[]');
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (g): g is RecentGame =>
        typeof g === 'object' && g !== null && typeof g.id === 'string' && typeof g.size === 'number',
    );
  } catch {
    return [];
  }
}

export function rememberGame(game: RecentGame): void {
  const others = loadRecent().filter((g) => g.id !== game.id);
  write(local(), RECENT_KEY, JSON.stringify([game, ...others].slice(0, 8)));
}

export type ThemeChoice = 'auto' | 'light' | 'dark';

export function loadTheme(): ThemeChoice {
  const value = read(local(), 'igo.theme');
  return value === 'light' || value === 'dark' ? value : 'auto';
}

export function saveTheme(theme: ThemeChoice): void {
  try {
    if (theme === 'auto') local()?.removeItem('igo.theme');
    else local()?.setItem('igo.theme', theme);
  } catch {
    // 保存できなくても表示は切り替わる。
  }
}
