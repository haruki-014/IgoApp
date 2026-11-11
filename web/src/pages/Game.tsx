// 対局画面:盤、手番、アゲハマ、パス・投了・待った、招待 URL、SGF、終局時の結果と内訳。

import { useEffect, useState } from 'react';
import { ApiError, fetchGame, sgfUrl } from '../api';
import { Board } from '../board/Board';
import { ConfirmDialog, CopyField, StoneIcon } from '../components/ui';
import { gamePath, onLinkClick } from '../router';
import { loadInviteToken, loadToken, rememberGame, saveInvitedToken } from '../storage';
import { colorName, describeKomi, describeMove, describeResult, seatName } from '../text';
import { HUMAN, opponent, type Color, type EngineInfo, type GameState, type ScoreView } from '../types';
import { useGame, type Connection, type GameError } from '../useGame';

/** 仕様で定められた説明文。 */
const RULE_NOTE =
  '両者が続けてパスすると終局し、盤上の石はすべて生きている石として数えます。死石は取り切ってからパスしてください';

/**
 * 招待 URL(`/g/<id>#t=<トークン>`)で開かれたら、トークンを保存して URL から消す。
 * トークンは `#` 以降に入れてあるので、サーバーには送られない。
 */
function takeSeatToken(id: string): string | null {
  const match = /^#t=([0-9a-f]{64})$/i.exec(window.location.hash);
  if (match) {
    saveInvitedToken(id, match[1]);
    window.history.replaceState(null, '', window.location.pathname + window.location.search);
  }
  return loadToken(id);
}

type Lookup = 'checking' | 'found' | 'missing' | 'unreachable';

export function GamePage({ id, engines }: { id: string; engines: EngineInfo[] }) {
  const [token] = useState(() => takeSeatToken(id));
  const [lookup, setLookup] = useState<Lookup>('checking');

  // まず対局があるか確かめる(存在しない対局に WebSocket をつなぎ続けないため)。
  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;
    const check = () => {
      fetchGame(id)
        .then(() => !cancelled && setLookup('found'))
        .catch((e) => {
          if (cancelled) return;
          if (e instanceof ApiError && e.status === 404) {
            setLookup('missing');
          } else {
            setLookup('unreachable');
            timer = window.setTimeout(check, 3000);
          }
        });
    };
    check();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [id]);

  const { state, connection, error, clearError, send } = useGame(id, token, lookup === 'found');

  if (lookup === 'missing') {
    return (
      <main className="message-page">
        <h1>対局が見つかりません</h1>
        <p>URL が正しいか確認してください。</p>
        <a className="button button-primary" href="/" onClick={onLinkClick}>
          トップへ戻る
        </a>
      </main>
    );
  }
  if (!state) {
    return (
      <main className="message-page">
        <p className="loading">
          {lookup === 'unreachable' ? 'サーバーに接続できません。再試行しています…' : '対局を読み込んでいます…'}
        </p>
      </main>
    );
  }
  return (
    <GameView
      state={state}
      token={token}
      engines={engines}
      connection={connection}
      error={error}
      clearError={clearError}
      send={send}
    />
  );
}

interface GameViewProps {
  state: GameState;
  /** この接続で使っている席トークン。 */
  token: string | null;
  engines: EngineInfo[];
  connection: Connection;
  error: GameError | null;
  clearError: () => void;
  send: ReturnType<typeof useGame>['send'];
}

function GameView({ state, token, engines, connection, error, clearError, send }: GameViewProps) {
  const you = state.you ?? null;
  const playing = state.status === 'playing';
  const myTurn = playing && you !== null && state.to_move === you;
  const online = connection === 'open';
  const [confirming, setConfirming] = useState<'pass' | 'resign' | null>(null);

  // 対局者として開いた対局を、トップ画面の「最近の対局」に残す。
  useEffect(() => {
    if (you) {
      rememberGame({
        id: state.id,
        size: state.board_size,
        black: state.players.black,
        white: state.players.white,
        at: Date.now(),
      });
    }
  }, [you, state.id, state.board_size, state.players.black, state.players.white]);

  // エラーは少しの間だけ表示する。
  useEffect(() => {
    if (!error) return;
    const timer = window.setTimeout(clearError, 5000);
    return () => window.clearTimeout(timer);
  }, [error, clearError]);

  // 終局したら、開いている確認ダイアログを閉じる。
  useEffect(() => {
    if (!playing) setConfirming(null);
  }, [playing]);

  const opponentPassed =
    state.last_move !== null && state.last_move.x === null && you !== null && state.last_move.color !== you;
  const humanVsAi = (state.players.black === HUMAN) !== (state.players.white === HUMAN);
  // 招待 URL は対局の作成者にだけ見せる(招待された側の席トークンと同じなら、本人なので出さない)。
  const storedInvite = loadInviteToken(state.id);
  const inviteToken = storedInvite !== token ? storedInvite : null;
  const origin = window.location.origin;

  const pass = () => {
    // 相手がパスした直後のパスは終局になるので、確認する。
    if (opponentPassed) setConfirming('pass');
    else send({ type: 'pass' });
  };

  return (
    <main className="game">
      <section className="game-status" aria-label="対局の状況">
        <div className="players">
          <PlayerCard state={state} seat="black" engines={engines} />
          <PlayerCard state={state} seat="white" engines={engines} />
        </div>
        <Headline state={state} />
      </section>

      <section className="game-board">
        <Board
          gameId={state.id}
          size={state.board_size}
          board={state.board}
          moves={state.moves}
          lastMove={state.last_move}
          ownership={state.score?.ownership ?? null}
          interactive={myTurn && online}
          myColor={you}
          onPlay={(x, y) => send({ type: 'play', x, y })}
        />
        <div className="toasts" aria-live="assertive">
          {error && (
            <p key={error.seq} className="toast toast-error" role="alert" onClick={clearError}>
              {error.message}
            </p>
          )}
          {connection === 'reconnecting' && <p className="toast">接続が切れました。再接続しています…</p>}
        </div>
      </section>

      <section className="game-panel">
        {!playing ? (
          <a className="button button-large" href="/" onClick={onLinkClick}>
            新しい対局を作成する
          </a>
        ) : you ? (
          <div className="actions">
            <button type="button" className="button button-large" onClick={pass} disabled={!myTurn || !online}>
              パス
            </button>
            {humanVsAi && (
              <button
                type="button"
                className="button button-large"
                onClick={() => send({ type: 'undo' })}
                disabled={!state.undo_available || !online}
                title="直前の自分の手まで戻します(AI の応手も取り消します)"
              >
                待った
              </button>
            )}
            <button
              type="button"
              className="button button-large button-danger-quiet"
              onClick={() => setConfirming('resign')}
              disabled={!playing || !online}
            >
              投了
            </button>
          </div>
        ) : (
          <p className="spectator-note">観戦中です。この画面からは着手できません。</p>
        )}

        {state.result && <ResultCard state={state} />}

        <p className="rule-note">{RULE_NOTE}。</p>

        <div className="card share">
          {inviteToken && playing && (
            <CopyField
              label="招待 URL(相手に送ってください)"
              value={`${origin}${gamePath(state.id)}#t=${inviteToken}`}
              hint="この URL を開いた人が相手の席に座ります。ほかの人には渡さないでください"
            />
          )}
          <CopyField
            label="観戦用 URL"
            value={`${origin}${gamePath(state.id)}`}
            hint="この URL を開いた人は、対局を見ることだけができます"
          />
          <a className="button" href={sgfUrl(state.id)} download={`igo-${state.id}.sgf`}>
            棋譜(SGF)をダウンロード
          </a>
        </div>

        <dl className="game-info">
          <div>
            <dt>盤</dt>
            <dd>{state.board_size} 路</dd>
          </div>
          <div>
            <dt>ルール</dt>
            <dd>中国ルール・{describeKomi(state.komi)}</dd>
          </div>
        </dl>
      </section>

      <ConfirmDialog
        open={confirming === 'pass'}
        title="パスして終局しますか?"
        confirmLabel="パスして終局する"
        onCancel={() => setConfirming(null)}
        onConfirm={() => {
          setConfirming(null);
          send({ type: 'pass' });
        }}
      >
        <p>相手はパスしています。あなたもパスすると終局し、いまの盤面のまま計算します。</p>
        <p>盤上の石はすべて生きている石として数えます。取れる石が残っていないか確かめてください。</p>
      </ConfirmDialog>
      <ConfirmDialog
        open={confirming === 'resign'}
        title="投了しますか?"
        confirmLabel="投了する"
        danger
        onCancel={() => setConfirming(null)}
        onConfirm={() => {
          setConfirming(null);
          send({ type: 'resign' });
        }}
      >
        <p>投了すると、あなたの負けで対局が終わります。取り消すことはできません。</p>
      </ConfirmDialog>
    </main>
  );
}

function PlayerCard({ state, seat, engines }: { state: GameState; seat: Color; engines: EngineInfo[] }) {
  const playing = state.status === 'playing';
  const toMove = playing && state.to_move === seat;
  const thinking = state.thinking === seat;
  const winner = state.result?.startsWith(seat === 'black' ? 'B+' : 'W+') ?? false;
  return (
    <div className={toMove ? 'player player-to-move' : 'player'}>
      <StoneIcon color={seat} size={26} />
      <div className="player-text">
        <span className="player-name">
          {colorName(seat)}・{seatName(state, seat, engines)}
        </span>
        <span className="player-sub">
          アゲハマ {state.captures[seat]}
          {seat === 'white' && <span className="player-komi"> ・{describeKomi(state.komi)}</span>}
        </span>
      </div>
      {thinking ? (
        <span className="badge badge-thinking">
          思考中<span className="dots" aria-hidden="true" />
        </span>
      ) : toMove ? (
        <span className="badge badge-turn">手番</span>
      ) : winner ? (
        <span className="badge badge-win">勝ち</span>
      ) : null}
    </div>
  );
}

function Headline({ state }: { state: GameState }) {
  const you = state.you ?? null;
  const last = state.last_move;
  let main: string;
  if (state.result) {
    main = `終局:${describeResult(state.result)}`;
  } else if (you === null) {
    main = `${colorName(state.to_move)}の手番です`;
  } else if (state.to_move === you) {
    main = `あなた(${colorName(you)})の手番です`;
  } else if (state.thinking) {
    main = 'AI が考えています';
  } else {
    main = `相手(${colorName(opponent(you))})の手番です`;
  }
  const passed = state.status === 'playing' && last !== null && last.x === null;
  return (
    <div className="headline">
      <p className="headline-main" aria-live="polite">
        {main}
      </p>
      <p className="headline-sub">
        {state.move_number === 0 ? (
          '対局開始'
        ) : (
          <>
            {state.move_number} 手目
            {last && <>:{describeMove(last, state.board_size)}</>}
          </>
        )}
        {you === null && <span className="badge">観戦中</span>}
      </p>
      {passed && last && (
        <p className="pass-banner">
          {colorName(last.color)}がパスしました。続けて{colorName(opponent(last.color))}もパスすると終局します。
        </p>
      )}
    </div>
  );
}

function ResultCard({ state }: { state: GameState }) {
  const result = state.result ?? '';
  return (
    <div className="card result" role="status">
      <p className="result-title">
        <span className="result-code">{result}</span>
        {describeResult(result)}
      </p>
      {state.score ? (
        <ScoreTable score={state.score} />
      ) : (
        <p className="result-note">投了による終局のため、計算は行いません。</p>
      )}
    </div>
  );
}

function ScoreTable({ score }: { score: ScoreView }) {
  return (
    <>
      <table className="score-table">
        <caption>計算の内訳(面積計算)</caption>
        <thead>
          <tr>
            <th scope="col" />
            <th scope="col">石</th>
            <th scope="col">地</th>
            <th scope="col">コミ</th>
            <th scope="col">合計</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <th scope="row">
              <StoneIcon color="black" size={14} /> 黒
            </th>
            <td>{score.black_stones}</td>
            <td>{score.black_territory}</td>
            <td>—</td>
            <td className="score-total">{score.black_total}</td>
          </tr>
          <tr>
            <th scope="row">
              <StoneIcon color="white" size={14} /> 白
            </th>
            <td>{score.white_stones}</td>
            <td>{score.white_territory}</td>
            <td>{score.komi}</td>
            <td className="score-total">{score.white_total}</td>
          </tr>
        </tbody>
      </table>
      <ul className="score-legend">
        <li>
          <span className="legend-swatch legend-black" aria-hidden="true" /> 黒地
        </li>
        <li>
          <span className="legend-swatch legend-white" aria-hidden="true" /> 白地
        </li>
        <li>
          どちらの地でもない空点:{score.neutral}(両方の石に接している点。得点になりません)
        </li>
      </ul>
      <p className="result-note">盤上の石は、すべて生きている石として数えています。</p>
    </>
  );
}
