// 小さな部品:石のアイコン、確認ダイアログ、コピー用ボタン、テーマ切り替え。

import { useEffect, useRef, useState, type ReactNode } from 'react';
import { loadTheme, saveTheme, type ThemeChoice } from '../storage';
import type { Color } from '../types';

/** 文中に置く小さな石。 */
export function StoneIcon({ color, size = 18 }: { color: Color; size?: number }) {
  return <span className={`stone-icon stone-icon-${color}`} style={{ width: size, height: size }} aria-hidden="true" />;
}

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  children: ReactNode;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({ open, title, children, confirmLabel, danger, onConfirm, onCancel }: ConfirmDialogProps) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);
  return (
    <dialog
      ref={ref}
      className="dialog"
      onCancel={(event) => {
        event.preventDefault();
        onCancel();
      }}
      onClick={(event) => {
        // 枠の外(背景)をクリックしたら閉じる。
        if (event.target === ref.current) onCancel();
      }}
    >
      <div className="dialog-body">
        <h2>{title}</h2>
        <div className="dialog-text">{children}</div>
        <div className="dialog-actions">
          <button type="button" className="button" onClick={onCancel} autoFocus>
            やめる
          </button>
          <button type="button" className={danger ? 'button button-danger' : 'button button-primary'} onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </dialog>
  );
}

async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // クリップボード API が使えない環境(http での配信など)では、選択してコピーする。
    const area = document.createElement('textarea');
    area.value = text;
    area.setAttribute('readonly', '');
    area.style.position = 'fixed';
    area.style.opacity = '0';
    document.body.appendChild(area);
    area.select();
    let ok = false;
    try {
      ok = document.execCommand('copy');
    } catch {
      ok = false;
    }
    area.remove();
    return ok;
  }
}

/** URL を表示し、ボタンでコピーできるようにする。 */
export function CopyField({ label, value, hint }: { label: string; value: string; hint?: string }) {
  const [status, setStatus] = useState<'idle' | 'copied' | 'failed'>('idle');
  useEffect(() => {
    if (status === 'idle') return;
    const timer = window.setTimeout(() => setStatus('idle'), 2200);
    return () => window.clearTimeout(timer);
  }, [status]);
  return (
    <div className="copy-field">
      <label>
        <span className="field-label">{label}</span>
        <span className="copy-row">
          <input type="text" readOnly value={value} onFocus={(event) => event.currentTarget.select()} />
          <button
            type="button"
            className="button"
            onClick={async () => setStatus((await copyText(value)) ? 'copied' : 'failed')}
          >
            {status === 'copied' ? 'コピーしました' : 'コピー'}
          </button>
        </span>
      </label>
      <p className="field-hint" aria-live="polite">
        {status === 'failed' ? 'コピーできませんでした。URL を選択してコピーしてください' : hint}
      </p>
    </div>
  );
}

const THEME_LABEL: Record<ThemeChoice, string> = { auto: '自動', light: 'ライト', dark: 'ダーク' };
const NEXT_THEME: Record<ThemeChoice, ThemeChoice> = { auto: 'light', light: 'dark', dark: 'auto' };

/** ページの配色の切り替え(自動 → ライト → ダーク)。盤の見た目は変わらない。 */
export function ThemeToggle() {
  const [theme, setTheme] = useState<ThemeChoice>(loadTheme);
  useEffect(() => {
    if (theme === 'auto') delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = theme;
    saveTheme(theme);
  }, [theme]);
  return (
    <button
      type="button"
      className="theme-toggle"
      onClick={() => setTheme(NEXT_THEME[theme])}
      title="配色を切り替える(自動 → ライト → ダーク)"
    >
      <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
        {theme === 'dark' ? (
          <path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5z" fill="currentColor" />
        ) : theme === 'light' ? (
          <g stroke="currentColor" strokeWidth="2" strokeLinecap="round">
            <circle cx="12" cy="12" r="4" fill="currentColor" />
            <path d="M12 2v2.5M12 19.5V22M2 12h2.5M19.5 12H22M4.9 4.9l1.8 1.8M17.3 17.3l1.8 1.8M4.9 19.1l1.8-1.8M17.3 6.7l1.8-1.8" />
          </g>
        ) : (
          <g>
            <circle cx="12" cy="12" r="8.5" fill="none" stroke="currentColor" strokeWidth="2" />
            <path d="M12 3.5a8.5 8.5 0 0 1 0 17z" fill="currentColor" />
          </g>
        )}
      </svg>
      <span>配色:{THEME_LABEL[theme]}</span>
    </button>
  );
}
