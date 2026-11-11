// 盤の寸法と座標の計算。描画(Board.tsx)から切り離して、テストできるようにしてある。

/** 線と線の間隔(SVG の単位)。 */
export const CELL = 100;
/** 盤の端から最初の線までの余白。座標ラベルと、端の石のはみ出しぶん。 */
export const MARGIN = 100;
/** 石の半径。実物と同じく、線の間隔よりわずかに小さい。 */
export const STONE_R = 47.5;

/** GTP と同じ列の文字。I は使わない。 */
export const COLUMN_LETTERS = 'ABCDEFGHJKLMNOPQRSTUVWXYZ';

/** 盤全体の一辺(SVG の単位)。 */
export const extent = (size: number): number => (size - 1) * CELL + MARGIN * 2;

/** 交点の位置。 */
export const pos = (index: number): number => MARGIN + index * CELL;

/** 列ラベル(左から A, B, …。I を飛ばす)。 */
export const columnLabel = (x: number): string => COLUMN_LETTERS[x] ?? '?';

/** 行ラベル(下から 1, 2, …)。y は上が 0。 */
export const rowLabel = (y: number, size: number): string => String(size - y);

/** 着手の表記(D4 など)。 */
export const vertexLabel = (x: number, y: number, size: number): string => columnLabel(x) + rowLabel(y, size);

/** 星の位置。 */
export function starPoints(size: number): Array<[number, number]> {
  if (size < 7) return [];
  const edge = size >= 13 ? 3 : 2;
  const far = size - 1 - edge;
  const points: Array<[number, number]> = [
    [edge, edge],
    [far, edge],
    [edge, far],
    [far, far],
  ];
  if (size % 2 === 1) {
    const mid = (size - 1) / 2;
    points.push([mid, mid]);
    if (size >= 19) {
      points.push([mid, edge], [mid, far], [edge, mid], [far, mid]);
    }
  }
  return points;
}

/**
 * 盤(正方形)の左上からのピクセル位置を、いちばん近い交点に変換する。
 * 盤の外側なら null。
 */
export function pointFromOffset(
  size: number,
  offsetX: number,
  offsetY: number,
  widthPx: number,
): { x: number; y: number } | null {
  if (widthPx <= 0) return null;
  const scale = extent(size) / widthPx;
  const x = Math.round((offsetX * scale - MARGIN) / CELL);
  const y = Math.round((offsetY * scale - MARGIN) / CELL);
  if (x < 0 || y < 0 || x >= size || y >= size) return null;
  // Math.round は -0 を返すことがあるので、0 にそろえる。
  return { x: x + 0, y: y + 0 };
}

/** 文字列の 32bit ハッシュ(FNV-1a のあと、ビットをよく混ぜる)。 */
export function hash32(text: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  h ^= h >>> 16;
  h = Math.imul(h, 0x85ebca6b);
  h ^= h >>> 13;
  h = Math.imul(h, 0xc2b2ae35);
  h ^= h >>> 16;
  return h >>> 0;
}

/** 石ごとのわずかな個体差。 */
export interface StoneLook {
  /** 位置のずれ(SVG の単位)。 */
  dx: number;
  dy: number;
  /** 白石の縞模様の向き(度)。 */
  rotation: number;
  /** 白石の縞模様の種類。 */
  variant: number;
}

export const STONE_VARIANTS = 4;
/** 位置のずれの最大値(SVG の単位。線の間隔の約 2%)。 */
export const MAX_JITTER = 2.2;

/**
 * 石の個体差を決める。対局・位置・何手目に置かれた石かだけで決まるので、
 * 同じ着手の石は再描画しても動かない。
 */
export function stoneLook(gameId: string, x: number, y: number, moveIndex: number): StoneLook {
  const h = hash32(`${gameId}:${moveIndex}:${x}:${y}`);
  const unit = (bits: number) => (bits & 0xff) / 255;
  return {
    dx: (unit(h) * 2 - 1) * MAX_JITTER,
    dy: (unit(h >>> 8) * 2 - 1) * MAX_JITTER,
    rotation: Math.round(unit(h >>> 16) * 359),
    variant: (h >>> 24) % STONE_VARIANTS,
  };
}

/**
 * 盤上の各点について、その石が何手目(0 始まり)に置かれたかを求める。
 * いま盤上にある石は、その点への最後の着手で置かれたもの。
 */
export function placedAt(
  moves: ReadonlyArray<{ x: number | null; y: number | null }>,
): Map<string, number> {
  const map = new Map<string, number>();
  moves.forEach((move, index) => {
    if (move.x !== null && move.y !== null) map.set(`${move.x},${move.y}`, index);
  });
  return map;
}
