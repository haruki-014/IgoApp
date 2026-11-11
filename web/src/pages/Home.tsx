// トップ画面:対局を作成する。

import { useEffect, useState, type FormEvent } from 'react';
import { ApiError, createGame, type CreateGameRequest } from '../api';
import { StoneIcon } from '../components/ui';
import { gamePath, navigate, onLinkClick } from '../router';
import { loadRecent, rememberGame, saveInviteToken, saveOwnToken, type RecentGame } from '../storage';
import { engineLabel } from '../text';
import { HUMAN, type Color, type EngineInfo } from '../types';

type Mode = 'online' | 'ai' | 'watch';
type ColorChoice = Color | 'random';

const SIZES = [9, 13, 19] as const;
const KOMI_MIN = -20.5;
const KOMI_MAX = 20.5;

const MODES: Array<{ value: Mode; title: string; text: string }> = [
  { value: 'online', title: 'オンライン対人戦', text: '招待 URL を相手に送って対局します' },
  { value: 'ai', title: 'AI と対局', text: '思考エンジンと対局します。待ったができます' },
  { value: 'watch', title: 'AI 同士を観戦', text: '思考エンジン同士の対局を眺めます' },
];

interface HomeProps {
  engines: EngineInfo[];
  /** エンジン一覧の取得状況。 */
  enginesStatus: 'loading' | 'ready' | 'failed';
}

function playerLabel(player: string, engines: EngineInfo[]): string {
  return player === HUMAN ? '人間' : engineLabel(player, engines);
}

export function HomePage({ engines, enginesStatus }: HomeProps) {
  const [mode, setMode] = useState<Mode>('online');
  const [size, setSize] = useState<number>(9);
  const [komi, setKomi] = useState(7.5);
  const [myColor, setMyColor] = useState<ColorChoice>('black');
  const [opponent, setOpponent] = useState('');
  const [blackEngine, setBlackEngine] = useState('');
  const [whiteEngine, setWhiteEngine] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [recent] = useState<RecentGame[]>(loadRecent);

  // エンジン一覧が届いたら、選択の初期値を入れる。
  useEffect(() => {
    if (engines.length === 0) return;
    const first = engines[0].name;
    const second = engines[1]?.name ?? first;
    setOpponent((current) => current || first);
    setBlackEngine((current) => current || first);
    setWhiteEngine((current) => current || second);
  }, [engines]);

  const noEngines = enginesStatus !== 'loading' && engines.length === 0;
  const needsEngine = mode !== 'online';

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (submitting) return;
    setError(null);

    const color: Color = myColor === 'random' ? (Math.random() < 0.5 ? 'black' : 'white') : myColor;
    let body: CreateGameRequest;
    if (mode === 'online') {
      body = { board_size: size, komi, black: HUMAN, white: HUMAN, creator_color: color };
    } else if (mode === 'ai') {
      body =
        color === 'black'
          ? { board_size: size, komi, black: HUMAN, white: opponent }
          : { board_size: size, komi, black: opponent, white: HUMAN };
    } else {
      body = { board_size: size, komi, black: blackEngine, white: whiteEngine };
    }

    setSubmitting(true);
    try {
      const created = await createGame(body);
      if (created.token) saveOwnToken(created.id, created.token);
      if (created.invite_token) saveInviteToken(created.id, created.invite_token);
      rememberGame({ id: created.id, size, black: body.black, white: body.white, at: Date.now() });
      navigate(gamePath(created.id));
    } catch (e) {
      setError(e instanceof ApiError ? e.message : '対局を作成できませんでした');
      setSubmitting(false);
    }
  };

  const engineSelect = (id: string, label: string, value: string, onChange: (value: string) => void) => (
    <label className="field" htmlFor={id}>
      <span className="field-label">{label}</span>
      <select id={id} value={value} onChange={(event) => onChange(event.target.value)} disabled={engines.length === 0}>
        {engines.map((engine) => (
          <option key={engine.name} value={engine.name}>
            {engine.label}
          </option>
        ))}
      </select>
    </label>
  );

  return (
    <main className="home">
      <header className="home-hero">
        <h1>囲碁対局</h1>
        <p>オンラインでの対人戦、AI との対局、AI 同士の観戦ができます。ルールは中国ルール(面積計算)です。</p>
      </header>

      <form className="card create-form" onSubmit={submit}>
        <h2>対局を作成する</h2>

        <fieldset className="field">
          <legend className="field-label">対局の種類</legend>
          <div className="mode-list">
            {MODES.map((option) => (
              <label key={option.value} className={mode === option.value ? 'mode mode-selected' : 'mode'}>
                <input
                  type="radio"
                  name="mode"
                  value={option.value}
                  checked={mode === option.value}
                  onChange={() => setMode(option.value)}
                />
                <span className="mode-title">{option.title}</span>
                <span className="mode-text">{option.text}</span>
              </label>
            ))}
          </div>
        </fieldset>

        <div className="form-grid">
          <fieldset className="field">
            <legend className="field-label">盤の大きさ</legend>
            <div className="segmented">
              {SIZES.map((value) => (
                <label key={value} className={size === value ? 'segment segment-selected' : 'segment'}>
                  <input type="radio" name="size" checked={size === value} onChange={() => setSize(value)} />
                  {value} 路
                </label>
              ))}
            </div>
          </fieldset>

          <div className="field" role="group" aria-labelledby="komi-label">
            <span className="field-label" id="komi-label">
              コミ(白に加える得点)
            </span>
            <div className="stepper">
              <button
                type="button"
                className="button"
                onClick={() => setKomi((k) => Math.max(KOMI_MIN, k - 1))}
                disabled={komi <= KOMI_MIN}
                aria-label="コミを 1 目減らす"
              >
                −
              </button>
              <output aria-live="polite">{komi.toFixed(1)} 目</output>
              <button
                type="button"
                className="button"
                onClick={() => setKomi((k) => Math.min(KOMI_MAX, k + 1))}
                disabled={komi >= KOMI_MAX}
                aria-label="コミを 1 目増やす"
              >
                +
              </button>
              {komi !== 7.5 && (
                <button type="button" className="button button-quiet" onClick={() => setKomi(7.5)}>
                  7.5 に戻す
                </button>
              )}
            </div>
          </div>
        </div>

        {mode !== 'watch' && (
          <fieldset className="field">
            <legend className="field-label">あなたの石</legend>
            <div className="segmented">
              {(
                [
                  ['black', '黒(先手)'],
                  ['white', '白(後手)'],
                  ['random', 'おまかせ'],
                ] as const
              ).map(([value, label]) => (
                <label key={value} className={myColor === value ? 'segment segment-selected' : 'segment'}>
                  <input type="radio" name="color" checked={myColor === value} onChange={() => setMyColor(value)} />
                  {value !== 'random' && <StoneIcon color={value} size={14} />}
                  {label}
                </label>
              ))}
            </div>
          </fieldset>
        )}

        {mode === 'ai' && engineSelect('opponent', '相手の AI', opponent, setOpponent)}
        {mode === 'watch' && (
          <div className="form-grid">
            {engineSelect('black-engine', '黒の AI', blackEngine, setBlackEngine)}
            {engineSelect('white-engine', '白の AI', whiteEngine, setWhiteEngine)}
          </div>
        )}

        {needsEngine && enginesStatus === 'failed' && (
          <p className="notice notice-error">AI の一覧を取得できませんでした。ページを読み込み直してください。</p>
        )}
        {needsEngine && enginesStatus === 'ready' && noEngines && (
          <p className="notice">このサーバーには AI(思考エンジン)が設定されていません。</p>
        )}
        {error && (
          <p className="notice notice-error" role="alert">
            {error}
          </p>
        )}

        <div className="form-actions">
          <button
            type="submit"
            className="button button-primary button-large"
            disabled={submitting || (needsEngine && engines.length === 0)}
          >
            {submitting ? '作成しています…' : '対局を始める'}
          </button>
        </div>
      </form>

      {recent.length > 0 && (
        <section className="card recent">
          <h2>最近の対局</h2>
          <ul>
            {recent.map((game) => (
              <li key={game.id}>
                <a href={gamePath(game.id)} onClick={onLinkClick}>
                  <span className="recent-title">
                    {game.size} 路 ・ {playerLabel(game.black, engines)} 対 {playerLabel(game.white, engines)}
                  </span>
                  <time dateTime={new Date(game.at).toISOString()}>
                    {new Date(game.at).toLocaleString('ja-JP', { dateStyle: 'short', timeStyle: 'short' })}
                  </time>
                </a>
              </li>
            ))}
          </ul>
        </section>
      )}
    </main>
  );
}
