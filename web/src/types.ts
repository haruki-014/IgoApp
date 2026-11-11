// サーバーとやり取りするデータの型。サーバー側の定義は crates/server/src/model.rs。

export type Color = 'black' | 'white';

export const opponent = (color: Color): Color => (color === 'black' ? 'white' : 'black');

/** 1 手。パスは x・y が null。 */
export interface MoveView {
  color: Color;
  x: number | null;
  y: number | null;
}

/** 終局時の計算の内訳。 */
export interface ScoreView {
  black_stones: number;
  black_territory: number;
  white_stones: number;
  white_territory: number;
  neutral: number;
  komi: number;
  black_total: number;
  white_total: number;
  /** 各点の帰属。1 行が盤の 1 段で、b = 黒、w = 白、. = 中立。 */
  ownership: string[];
}

/** 対局の状態(WebSocket の state メッセージ)。 */
export interface GameState {
  id: string;
  board_size: number;
  komi: number;
  /** 各席のプレイヤー。human またはエンジン名。 */
  players: { black: string; white: string };
  /** 盤面。1 行が盤の 1 段(上から)で、. = 空点、X = 黒、O = 白。 */
  board: string[];
  to_move: Color;
  move_number: number;
  last_move: MoveView | null;
  moves: MoveView[];
  /** アゲハマ(それぞれが取った石の数)。 */
  captures: { black: number; white: number };
  status: 'playing' | 'finished';
  /** B+3.5、W+R など。 */
  result: string | null;
  score: ScoreView | null;
  undo_available: boolean;
  /** 思考中の AI の色。 */
  thinking: Color | null;
  /** この接続の席。観戦者は null。 */
  you?: Color | null;
}

export interface ErrorMessage {
  code: string;
  message: string;
}

export interface EngineInfo {
  name: string;
  label: string;
  max_board_size: number;
}

export const HUMAN = 'human';

export type ClientMessage =
  | { type: 'join'; token?: string }
  | { type: 'play'; x: number; y: number }
  | { type: 'pass' }
  | { type: 'resign' }
  | { type: 'undo' };
