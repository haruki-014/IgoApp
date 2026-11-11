// 碁盤と碁石。画像素材は使わず、SVG のフィルタとグラデーションだけで描く。
//
// 木目(feTurbulence)は描き直しに時間がかかるので、石を描く SVG とは別の SVG に分け、
// 石が動いても木目が再計算されないようにしてある。

import { memo, useEffect, useMemo, useRef, useState } from 'react';
import type { Color, MoveView } from '../types';
import {
  CELL,
  STONE_R,
  STONE_VARIANTS,
  columnLabel,
  extent,
  placedAt,
  pointFromOffset,
  pos,
  rowLabel,
  starPoints,
  stoneLook,
  type StoneLook,
} from './geometry';

interface BoardProps {
  gameId: string;
  size: number;
  /** 盤面。1 行が 1 段で、. = 空点、X = 黒、O = 白。 */
  board: string[];
  moves: MoveView[];
  lastMove: MoveView | null;
  /** 終局後の各点の帰属(b / w / .)。対局中は null。 */
  ownership: string[] | null;
  /** 自分の手番で、着手できる状態か。 */
  interactive: boolean;
  myColor: Color | null;
  onPlay: (x: number, y: number) => void;
}

interface Point {
  x: number;
  y: number;
}

/** 榧(かや)の盤面。盤サイズによらず同じ木目になるよう、固定の座標系で描く。 */
const Wood = memo(function Wood() {
  return (
    <svg className="board-wood" viewBox="0 0 1000 1000" preserveAspectRatio="none" aria-hidden="true">
      <defs>
        <linearGradient id="kaya-base" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#ecc77e" />
          <stop offset="0.5" stopColor="#e2b566" />
          <stop offset="1" stopColor="#d8a857" />
        </linearGradient>
        {/* 柾目:縦に走る、ゆるやかに濃淡のある筋。 */}
        <filter id="kaya-grain" x="0" y="0" width="100%" height="100%">
          <feTurbulence type="fractalNoise" baseFrequency="0.028 0.0011" numOctaves="4" seed="23" />
          <feColorMatrix
            type="matrix"
            values="0 0 0 0 0.52
                    0 0 0 0 0.31
                    0 0 0 0 0.10
                    2.6 0 0 0 -1.05"
          />
          <feComposite in2="SourceAlpha" operator="in" />
        </filter>
        {/* 細い繊維。 */}
        <filter id="kaya-fiber" x="0" y="0" width="100%" height="100%">
          <feTurbulence type="fractalNoise" baseFrequency="0.42 0.006" numOctaves="2" seed="5" />
          <feColorMatrix
            type="matrix"
            values="0 0 0 0 0.36
                    0 0 0 0 0.21
                    0 0 0 0 0.06
                    0 1.5 0 0 -0.62"
          />
          <feComposite in2="SourceAlpha" operator="in" />
        </filter>
        {/* 明るい照り。 */}
        <filter id="kaya-sheen" x="0" y="0" width="100%" height="100%">
          <feTurbulence type="fractalNoise" baseFrequency="0.011 0.0006" numOctaves="2" seed="71" />
          <feColorMatrix
            type="matrix"
            values="0 0 0 0 1
                    0 0 0 0 0.93
                    0 0 0 0 0.72
                    0 0 1.8 0 -0.78"
          />
          <feComposite in2="SourceAlpha" operator="in" />
        </filter>
        <radialGradient id="kaya-vignette" cx="0.5" cy="0.45" r="0.75">
          <stop offset="0.55" stopColor="#5a3408" stopOpacity="0" />
          <stop offset="1" stopColor="#5a3408" stopOpacity="0.2" />
        </radialGradient>
      </defs>
      <rect width="1000" height="1000" fill="url(#kaya-base)" />
      <rect width="1000" height="1000" filter="url(#kaya-sheen)" opacity="0.55" />
      <rect width="1000" height="1000" filter="url(#kaya-grain)" opacity="0.62" />
      <rect width="1000" height="1000" filter="url(#kaya-fiber)" opacity="0.5" />
      <rect width="1000" height="1000" fill="url(#kaya-vignette)" />
    </svg>
  );
});

/** 蛤碁石の縞。大きな同心円の一部を、石の円で切り抜く。 */
function shellStripes(variant: number) {
  const settings = [
    { cy: 150, first: 104, step: 8.5, count: 12 },
    { cy: 230, first: 184, step: 7.2, count: 14 },
    { cy: 112, first: 66, step: 10.5, count: 10 },
    { cy: 520, first: 474, step: 6.4, count: 16 },
  ][variant % STONE_VARIANTS];
  return Array.from({ length: settings.count }, (_, i) => (
    <circle
      key={i}
      cx={i % 3 === 0 ? 1.5 : 0}
      cy={settings.cy}
      r={settings.first + i * settings.step}
      fill="none"
      stroke="#8f8876"
      strokeWidth={i % 2 === 0 ? 1.5 : 1}
      strokeOpacity={0.16 + (i % 3) * 0.05}
    />
  ));
}

const Defs = memo(function Defs() {
  return (
    <defs>
      {/* 那智黒:つやを抑えた黒に、わずかな光沢。 */}
      <radialGradient id="stone-black" cx="0.36" cy="0.3" r="0.78">
        <stop offset="0" stopColor="#5c5e66" />
        <stop offset="0.2" stopColor="#33343a" />
        <stop offset="0.58" stopColor="#17171a" />
        <stop offset="1" stopColor="#040405" />
      </radialGradient>
      <radialGradient id="stone-black-gloss" cx="0.5" cy="0.5" r="0.5">
        <stop offset="0" stopColor="#ffffff" stopOpacity="0.4" />
        <stop offset="1" stopColor="#ffffff" stopOpacity="0" />
      </radialGradient>
      {/* 蛤:乳白色。縁に向かってわずかに影が落ちる。 */}
      <radialGradient id="stone-white" cx="0.38" cy="0.3" r="0.82">
        <stop offset="0" stopColor="#ffffff" />
        <stop offset="0.48" stopColor="#f7f4ed" />
        <stop offset="0.86" stopColor="#e4dfd2" />
        <stop offset="1" stopColor="#c6c0b0" />
      </radialGradient>
      <radialGradient id="stone-white-rim" cx="0.42" cy="0.36" r="0.66">
        <stop offset="0.62" stopColor="#6b6453" stopOpacity="0" />
        <stop offset="1" stopColor="#6b6453" stopOpacity="0.3" />
      </radialGradient>
      <radialGradient id="stone-white-gloss" cx="0.5" cy="0.5" r="0.5">
        <stop offset="0" stopColor="#ffffff" stopOpacity="0.95" />
        <stop offset="1" stopColor="#ffffff" stopOpacity="0" />
      </radialGradient>
      {/* 石の下の、ごく小さな影。 */}
      <radialGradient id="stone-shadow" cx="0.5" cy="0.5" r="0.5">
        <stop offset="0" stopColor="#1c1004" stopOpacity="0.55" />
        <stop offset="0.72" stopColor="#1c1004" stopOpacity="0.5" />
        <stop offset="1" stopColor="#1c1004" stopOpacity="0" />
      </radialGradient>
      <clipPath id="stone-clip">
        <circle r={STONE_R} />
      </clipPath>
      {Array.from({ length: STONE_VARIANTS }, (_, variant) => (
        <g key={variant} id={`shell-${variant}`} clipPath="url(#stone-clip)">
          {shellStripes(variant)}
        </g>
      ))}
    </defs>
  );
});

interface StoneProps {
  cx: number;
  cy: number;
  color: Color;
  look: StoneLook;
  /** 仮置き(半透明)。 */
  ghost?: boolean;
  /** 仮置きの濃さ。 */
  opacity?: number;
  /** いま打たれたばかりの石(置くときに短いアニメーションを付ける)。 */
  fresh?: boolean;
}

const Stone = memo(function Stone({ cx, cy, color, look, ghost = false, opacity = 1, fresh = false }: StoneProps) {
  const x = cx + look.dx;
  const y = cy + look.dy;
  return (
    <g
      className={ghost ? 'stone stone-ghost' : fresh ? 'stone stone-fresh' : 'stone'}
      opacity={opacity}
      style={fresh ? { transformOrigin: `${x}px ${y}px` } : undefined}
    >
      {!ghost && <circle cx={x + 2.5} cy={y + 5} r={STONE_R + 3.5} fill="url(#stone-shadow)" />}
      {color === 'black' ? (
        <>
          <circle cx={x} cy={y} r={STONE_R} fill="url(#stone-black)" />
          <ellipse
            cx={x - 15}
            cy={y - 19}
            rx={19}
            ry={12}
            fill="url(#stone-black-gloss)"
            transform={`rotate(-32 ${x - 15} ${y - 19})`}
          />
        </>
      ) : (
        <>
          <circle cx={x} cy={y} r={STONE_R} fill="url(#stone-white)" />
          <use href={`#shell-${look.variant}`} transform={`translate(${x} ${y}) rotate(${look.rotation})`} />
          <circle cx={x} cy={y} r={STONE_R} fill="url(#stone-white-rim)" />
          <ellipse
            cx={x - 15}
            cy={y - 19}
            rx={17}
            ry={10}
            fill="url(#stone-white-gloss)"
            transform={`rotate(-32 ${x - 15} ${y - 19})`}
          />
        </>
      )}
    </g>
  );
});

const GHOST_LOOK: StoneLook = { dx: 0, dy: 0, rotation: 20, variant: 0 };

/** 線・星・座標ラベル。盤サイズが同じなら描き直さない。 */
const Grid = memo(function Grid({ size }: { size: number }) {
  const first = pos(0);
  const last = pos(size - 1);
  const lines: string[] = [];
  for (let i = 1; i < size - 1; i++) {
    lines.push(`M${pos(i)} ${first}V${last}`, `M${first} ${pos(i)}H${last}`);
  }
  const fontSize = size <= 9 ? 27 : size <= 13 ? 31 : 37;
  const labelOffset = 71;
  return (
    <g>
      <path className="board-line" d={lines.join('')} />
      <rect className="board-line board-frame" x={first} y={first} width={last - first} height={last - first} />
      {starPoints(size).map(([x, y]) => (
        <circle key={`${x},${y}`} className="board-star" cx={pos(x)} cy={pos(y)} r={size <= 9 ? 8 : 9.5} />
      ))}
      <g className="board-label" fontSize={fontSize} textAnchor="middle" dominantBaseline="central">
        {Array.from({ length: size }, (_, i) => (
          <g key={i}>
            <text x={pos(i)} y={first - labelOffset}>
              {columnLabel(i)}
            </text>
            <text x={pos(i)} y={last + labelOffset}>
              {columnLabel(i)}
            </text>
            <text x={first - labelOffset} y={pos(i)}>
              {rowLabel(i, size)}
            </text>
            <text x={last + labelOffset} y={pos(i)}>
              {rowLabel(i, size)}
            </text>
          </g>
        ))}
      </g>
    </g>
  );
});

export function Board({ gameId, size, board, moves, lastMove, ownership, interactive, myColor, onPlay }: BoardProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const pointerType = useRef('mouse');
  /** マウスが乗っている点。 */
  const [hover, setHover] = useState<Point | null>(null);
  /** タッチ操作で仮置きした点。もう一度タップすると着手する。 */
  const [pending, setPending] = useState<Point | null>(null);

  // 盤面が変わったら(着手・待った)、仮置きを取り消す。
  const moveCount = moves.length;
  useEffect(() => {
    setPending(null);
  }, [moveCount, interactive]);

  // 画面を開いた時点ですでに盤上にあった石には、置くときのアニメーションを付けない。
  const initialMoves = useRef(moves.length);
  const placed = useMemo(() => placedAt(moves), [moves]);
  const isEmpty = (p: Point) => board[p.y]?.[p.x] === '.';

  const pointOf = (event: { clientX: number; clientY: number }): Point | null => {
    const rect = svgRef.current?.getBoundingClientRect();
    if (!rect) return null;
    return pointFromOffset(size, event.clientX - rect.left, event.clientY - rect.top, rect.width);
  };

  const stones = [];
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const cell = board[y]?.[x];
      if (cell !== 'X' && cell !== 'O') continue;
      const moveIndex = placed.get(`${x},${y}`) ?? -1;
      stones.push(
        <Stone
          key={`${x},${y},${moveIndex}`}
          cx={pos(x)}
          cy={pos(y)}
          color={cell === 'X' ? 'black' : 'white'}
          look={stoneLook(gameId, x, y, moveIndex)}
          fresh={moveIndex === moves.length - 1 && moveIndex > initialMoves.current - 1}
        />,
      );
    }
  }

  // 終局後:各陣地を薄く色付けする(石のない点だけ。石はそれ自体が得点)。
  const territory = [];
  if (ownership) {
    for (let y = 0; y < size; y++) {
      for (let x = 0; x < size; x++) {
        const owner = ownership[y]?.[x];
        if (board[y]?.[x] !== '.' || (owner !== 'b' && owner !== 'w')) continue;
        territory.push(
          <g key={`${x},${y}`} className={owner === 'b' ? 'territory territory-black' : 'territory territory-white'}>
            <rect className="territory-area" x={pos(x) - CELL / 2} y={pos(y) - CELL / 2} width={CELL} height={CELL} />
            <rect className="territory-mark" x={pos(x) - 15} y={pos(y) - 15} width={30} height={30} rx={3} />
          </g>,
        );
      }
    }
  }

  let marker = null;
  if (lastMove && lastMove.x !== null && lastMove.y !== null && board[lastMove.y]?.[lastMove.x] !== '.') {
    const look = stoneLook(gameId, lastMove.x, lastMove.y, placed.get(`${lastMove.x},${lastMove.y}`) ?? -1);
    marker = (
      <circle
        className={lastMove.color === 'black' ? 'last-move on-black' : 'last-move on-white'}
        cx={pos(lastMove.x) + look.dx}
        cy={pos(lastMove.y) + look.dy}
        r={17}
      />
    );
  }

  const ghostPoint = interactive && myColor ? (pending ?? hover) : null;
  const ghost =
    ghostPoint && myColor && isEmpty(ghostPoint) ? (
      <Stone
        cx={pos(ghostPoint.x)}
        cy={pos(ghostPoint.y)}
        color={myColor}
        look={GHOST_LOOK}
        ghost
        opacity={pending ? 0.72 : 0.5}
      />
    ) : null;

  const total = extent(size);
  return (
    <div className="board-wrap">
      <div className={interactive ? 'board board-interactive' : 'board'}>
        <Wood />
        <svg
          ref={svgRef}
          className="board-main"
          viewBox={`0 0 ${total} ${total}`}
          role="img"
          aria-label={`${size} 路盤。${moves.length} 手目まで進んでいます`}
          onPointerDown={(event) => {
            pointerType.current = event.pointerType;
          }}
          onPointerMove={(event) => {
            if (event.pointerType !== 'mouse' || !interactive) return;
            const p = pointOf(event);
            setHover((current) => (current?.x === p?.x && current?.y === p?.y ? current : p));
          }}
          onPointerLeave={() => setHover(null)}
          onClick={(event) => {
            if (!interactive) return;
            const p = pointOf(event);
            if (!p || !isEmpty(p)) {
              setPending(null);
              // 石のある点などは、サーバーに送って理由を表示してもらう。
              if (p && pointerType.current === 'mouse') onPlay(p.x, p.y);
              return;
            }
            if (pointerType.current === 'mouse') {
              onPlay(p.x, p.y);
            } else if (pending && pending.x === p.x && pending.y === p.y) {
              // タッチ操作は打ち間違えやすいので、同じ点を 2 回タップして着手する。
              setPending(null);
              onPlay(p.x, p.y);
            } else {
              setPending(p);
            }
          }}
        >
          <Defs />
          <Grid size={size} />
          <g>{territory}</g>
          <g>{stones}</g>
          {marker}
          {ghost}
        </svg>
      </div>
      <p className={pending ? 'tap-hint tap-hint-visible' : 'tap-hint'} aria-live="polite">
        {pending ? 'もう一度同じ場所をタップすると着手します' : ' '}
      </p>
    </div>
  );
}
