import { useSession } from '../state/SessionProvider';
import type { BannerTone } from '../state/types';

// Solid light pills on the dark chrome — the web app's banner/pill tones
// (amber-100/amber-800 warning, lemon/pine good, white/ink neutral).
const TONE: Record<BannerTone, { bg: string; border: string; fg: string; dot: string }> = {
  warn: {
    bg: '#fef3c7',
    border: 'rgba(180,83,9,0.25)',
    fg: '#92400e',
    dot: '#f59e0b',
  },
  good: {
    bg: '#daf698',
    border: 'rgba(2,69,34,0.25)',
    fg: '#0d2418',
    dot: '#024522',
  },
  neutral: {
    bg: '#ffffff',
    border: 'rgba(36,36,36,0.12)',
    fg: 'rgba(36,36,36,0.75)',
    dot: '#9ca3af',
  },
};

export function StatusBanners() {
  const { state } = useSession();
  if (state.banners.length === 0) return null;

  return (
    <div className="flex flex-col gap-1.5 px-5 pt-2">
      {state.banners.map((b) => {
        const t = TONE[b.tone];
        return (
          <div
            key={b.id}
            role="status"
            className="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm"
            style={{
              background: t.bg,
              border: `1px solid ${t.border}`,
              color: t.fg,
              animation: 'dt-fade-in 0.18s ease',
            }}
          >
            <span
              className="inline-block h-2 w-2 shrink-0 rounded-full"
              style={{ background: t.dot }}
            />
            <span className="tracking-[-0.2px]">{b.text}</span>
          </div>
        );
      })}
    </div>
  );
}
