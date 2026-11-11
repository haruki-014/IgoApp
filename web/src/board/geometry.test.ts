import { describe, expect, it } from 'vitest';
import {
  CELL,
  MARGIN,
  MAX_JITTER,
  STONE_VARIANTS,
  columnLabel,
  extent,
  placedAt,
  pointFromOffset,
  pos,
  rowLabel,
  starPoints,
  stoneLook,
  vertexLabel,
} from './geometry';

describe('座標ラベル', () => {
  it('列は I を飛ばす', () => {
    expect(columnLabel(0)).toBe('A');
    expect(columnLabel(7)).toBe('H');
    expect(columnLabel(8)).toBe('J');
    expect(columnLabel(18)).toBe('T');
    expect(columnLabel(24)).toBe('Z');
    expect(Array.from({ length: 25 }, (_, x) => columnLabel(x)).join('')).not.toContain('I');
  });

  it('行は下から数える', () => {
    expect(rowLabel(0, 19)).toBe('19');
    expect(rowLabel(18, 19)).toBe('1');
    expect(vertexLabel(3, 15, 19)).toBe('D4');
    expect(vertexLabel(15, 3, 19)).toBe('Q16');
    expect(vertexLabel(4, 4, 9)).toBe('E5');
  });
});

describe('星', () => {
  it('9・13・19 路', () => {
    expect(starPoints(9)).toEqual([
      [2, 2],
      [6, 2],
      [2, 6],
      [6, 6],
      [4, 4],
    ]);
    expect(starPoints(13)).toHaveLength(5);
    expect(starPoints(13)).toContainEqual([3, 3]);
    expect(starPoints(13)).toContainEqual([6, 6]);
    expect(starPoints(19)).toHaveLength(9);
    expect(starPoints(19)).toContainEqual([9, 9]);
    expect(starPoints(19)).toContainEqual([15, 3]);
    expect(starPoints(19)).toContainEqual([9, 15]);
  });

  it('小さい盤には星を描かない', () => {
    expect(starPoints(2)).toEqual([]);
    expect(starPoints(5)).toEqual([]);
  });
});

describe('ポインタ位置から交点への変換', () => {
  it('交点の真上は、その交点になる', () => {
    for (const size of [2, 9, 13, 19, 25]) {
      const width = 640;
      const scale = width / extent(size);
      for (const [x, y] of [
        [0, 0],
        [size - 1, 0],
        [0, size - 1],
        [size - 1, size - 1],
        [Math.floor(size / 2), 1],
      ]) {
        expect(pointFromOffset(size, pos(x) * scale, pos(y) * scale, width)).toEqual({ x, y });
      }
    }
  });

  it('いちばん近い交点に丸める', () => {
    const size = 9;
    const width = extent(size); // 1 単位 = 1px
    expect(pointFromOffset(size, MARGIN + CELL * 2 + 49, MARGIN + CELL * 3 - 49, width)).toEqual({ x: 2, y: 3 });
    expect(pointFromOffset(size, MARGIN + CELL * 2 + 51, MARGIN + CELL * 3 - 51, width)).toEqual({ x: 3, y: 2 });
    // 端の石は線の外側にはみ出すので、余白でも半マスぶんは端の交点として扱う。
    expect(pointFromOffset(size, MARGIN - 40, MARGIN - 40, width)).toEqual({ x: 0, y: 0 });
  });

  it('盤の外は null', () => {
    const size = 9;
    const width = extent(size);
    expect(pointFromOffset(size, MARGIN - 60, MARGIN, width)).toBeNull();
    expect(pointFromOffset(size, MARGIN, width - 20, width)).toBeNull();
    expect(pointFromOffset(size, 10, 10, 0)).toBeNull();
  });
});

describe('石の個体差', () => {
  it('同じ着手の石は、何度計算しても同じ(再描画しても動かない)', () => {
    const a = stoneLook('game-1', 3, 4, 10);
    expect(stoneLook('game-1', 3, 4, 10)).toEqual(a);
  });

  it('着手が違えば変わる', () => {
    const looks = new Set<string>();
    for (let move = 0; move < 50; move++) {
      looks.add(JSON.stringify(stoneLook('game-1', 3, 4, move)));
    }
    expect(looks.size).toBeGreaterThan(40);
    expect(stoneLook('game-1', 3, 4, 10)).not.toEqual(stoneLook('game-2', 3, 4, 10));
  });

  it('ずれはごくわずか', () => {
    let maxSeen = 0;
    const variants = new Set<number>();
    for (let i = 0; i < 500; i++) {
      const look = stoneLook('g', i % 19, Math.floor(i / 19), i);
      expect(Math.abs(look.dx)).toBeLessThanOrEqual(MAX_JITTER);
      expect(Math.abs(look.dy)).toBeLessThanOrEqual(MAX_JITTER);
      expect(look.rotation).toBeGreaterThanOrEqual(0);
      expect(look.rotation).toBeLessThan(360);
      expect(look.variant).toBeLessThan(STONE_VARIANTS);
      maxSeen = Math.max(maxSeen, Math.abs(look.dx), Math.abs(look.dy));
      variants.add(look.variant);
    }
    expect(maxSeen).toBeGreaterThan(MAX_JITTER * 0.8);
    expect(MAX_JITTER).toBeLessThan(CELL * 0.03);
    expect(variants.size).toBe(STONE_VARIANTS);
  });
});

describe('石が置かれた手数', () => {
  it('同じ点に打ち直された場合は、最後の着手を採る', () => {
    const map = placedAt([
      { x: 2, y: 2 },
      { x: null, y: null },
      { x: 3, y: 3 },
      { x: 2, y: 2 },
    ]);
    expect(map.get('2,2')).toBe(3);
    expect(map.get('3,3')).toBe(2);
    expect(map.size).toBe(2);
  });
});
