// 画面に出す文言の組み立て。

import { vertexLabel } from './board/geometry';
import { HUMAN, type Color, type EngineInfo, type GameState, type MoveView } from './types';

export const colorName = (color: Color): string => (color === 'black' ? '黒' : '白');

/** `B+3.5`、`W+R` を文章にする。 */
export function describeResult(result: string): string {
  const match = /^([BW])\+(R|\d+(?:\.\d+)?)$/.exec(result);
  if (!match) return result;
  const winner = match[1] === 'B' ? '黒' : '白';
  const loser = match[1] === 'B' ? '白' : '黒';
  return match[2] === 'R' ? `${winner}の勝ち(${loser}の投了)` : `${winner}の ${match[2]} 目勝ち`;
}

/** エンジン名を表示名にする。一覧にないエンジン(設定から外されたもの)は名前のまま。 */
export function engineLabel(name: string, engines: EngineInfo[]): string {
  return engines.find((engine) => engine.name === name)?.label ?? name;
}

/** 席に座っているプレイヤーの表示名。 */
export function seatName(state: GameState, seat: Color, engines: EngineInfo[]): string {
  const player = state.players[seat];
  if (player !== HUMAN) return engineLabel(player, engines);
  if (state.you === seat) return 'あなた';
  return state.you ? '相手' : '人間';
}

/** 最終手の表記(「黒 D4」「白 パス」)。 */
export function describeMove(move: MoveView, size: number): string {
  const where = move.x === null || move.y === null ? 'パス' : vertexLabel(move.x, move.y, size);
  return `${colorName(move.color)} ${where}`;
}

/** コミの表記(7.5 → 「7.5 目」、-5.5 → 「逆コミ 5.5 目」)。 */
export function describeKomi(komi: number): string {
  return komi < 0 ? `逆コミ ${(-komi).toFixed(1)} 目` : `コミ ${komi.toFixed(1)} 目`;
}
