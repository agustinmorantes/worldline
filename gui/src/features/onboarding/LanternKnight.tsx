// Lantern Knight, the tour's 30-second demo game, and the Worldline strip under it where its
// saves land as checkpoints. Drawn on the tour card's fixed 960×620 canvas.
import type { CSSProperties, ReactElement } from 'react';
import { useT } from '@/shared/i18n';
import { cn } from '@/shared/ui/utils';
import { type Demo, gold, type StripCheckpoint, stripOf } from './tour';

/** The scene and the strip on the card. */
export const SCENE = { left: 404, top: 80, width: 524, height: 300 } as const;
export const STRIP = { left: 404, top: 396, width: 524, height: 176 } as const;
/** Where a save leaves the scene: the "Saving…" badge. */
const SAVE_FROM = { x: 888, y: 106 };

/** A save on its way from the game down to the strip. */
export interface Flying { x: number; bad: boolean }

const ACC = 'var(--acc)';
const BAD = 'var(--err)';

export function Scene({ demo }: { demo: Demo }): ReactElement {
  const t = useT();
  const hurt = demo.mimic && !demo.mimicSaved;
  const coins = gold(demo);

  return (
    <div
      className={cn(
        'absolute overflow-hidden rounded-[18px] shadow-[inset_0_0_0_1px_oklch(100%_0_0/0.06)]',
        demo.rewinding ? 'lk-rewinding' : hurt && 'lk-shake',
      )}
      style={SCENE}
    >
      <svg viewBox="0 0 524 300" width="524" height="300" role="img" aria-label={t.tour.game} className="block">
        <defs>
          <linearGradient id="lk-sky" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="oklch(32% .09 290)" />
            <stop offset="1" stopColor="oklch(20% .06 275)" />
          </linearGradient>
        </defs>
        <rect width="524" height="300" fill="url(#lk-sky)" />
        <g fill="oklch(95% .02 290)">
          {[[40, 40, 1.4, 0], [96, 74, 1, 0.6], [170, 30, 1.6, 1.2], [252, 62, 1.1, 0.3], [304, 26, 1.4, 1.8], [362, 88, 1, 0.9], [486, 44, 1.5, 1.5], [212, 108, 1, 2.1], [60, 126, 1.2, 0.4]].map(([cx, cy, r, delay]) => (
            <circle key={`${String(cx)}-${String(cy)}`} className="lk-star" cx={cx} cy={cy} r={r} style={{ animationDelay: `${String(delay)}s` }} />
          ))}
        </g>
        <circle cx="420" cy="62" r="20" fill="oklch(92% .04 95)" />
        <circle cx="413" cy="56" r="4" fill="oklch(84% .04 95)" />
        <circle cx="426" cy="70" r="3" fill="oklch(84% .04 95)" />
        <path d="M0 236 V172 L60 124 L112 170 L172 112 L240 178 L300 132 L372 182 L432 144 L524 192 V236Z" fill="oklch(26% .07 288 / .75)" />
        <path d="M0 236 V198 Q 70 166 150 192 T 300 186 T 420 180 T 524 190 V236Z" fill="oklch(22% .06 284)" />
        <rect y="236" width="524" height="64" fill="oklch(15.5% .03 280)" />
        <path d="M0 236.5 H524" stroke="oklch(48% .1 290 / .5)" />
        <g fill="oklch(24% .05 165)">
          <polygon points="170,236 186,188 202,236" />
          <polygon points="246,236 266,176 286,236" />
          <polygon points="272,236 284,204 296,236" />
        </g>
        <rect x="232" y="214" width="3" height="22" fill="oklch(40% .06 60)" />
        <path d="M224 210 h18 l5 5 l-5 5 h-18z" fill="oklch(50% .09 60)" />
        <Campfire />
        <Castle />
        {demo.mimic ? <Mimic /> : <Chest />}
        <g style={{ transform: `translateX(${String(demo.heroX)}px)`, transition: 'transform 700ms linear' }}>
          <g className={demo.walking ? 'lk-walk' : hurt ? 'lk-hurt' : undefined}>
            <Knight />
          </g>
        </g>
      </svg>

      <div
        className="absolute left-3.5 top-3 flex h-7 items-center gap-[7px] rounded-full bg-black/45 pl-2 pr-[11px] font-mono text-[13px] font-bold transition-colors duration-300"
        style={{ color: demo.mimic ? BAD : 'var(--tx)' }}
      >
        <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
          <circle cx="7" cy="7" r="6" fill="oklch(84% .14 85)" />
          <circle cx="7" cy="7" r="3.4" fill="none" stroke="oklch(64% .12 70)" strokeWidth="1.4" />
        </svg>
        {coins}
      </div>
      {demo.mimic && <span className="lk-gold-fx absolute left-24 top-4 font-mono text-[13px] font-bold text-err">−120</span>}
      {demo.rewound && demo.step === 2 && <span className="lk-gold-fx absolute left-24 top-4 font-mono text-[13px] font-bold text-[oklch(84%_0.14_85)]">+120</span>}
      <div
        aria-hidden="true"
        className={cn(
          'absolute right-3.5 top-3 flex h-7 items-center gap-[7px] rounded-full bg-black/45 pl-[9px] pr-[11px] text-xs font-bold transition-[opacity,transform] duration-[200ms,320ms] ease-spring',
          demo.saving ? 'opacity-100' : '-translate-y-2 opacity-0',
        )}
      >
        <svg className="lk-spin" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round">
          <path d="M12 3a9 9 0 1 1-9 9" />
        </svg>
        {t.tour.saving}
      </div>
      {hurt && (
        <span
          aria-hidden="true"
          className="lk-pop absolute top-40 -translate-x-1/2 rounded-[10px] bg-tx px-[9px] py-1 font-display text-[13px] font-extrabold text-bg"
          style={{ left: demo.heroX + 11 }}
        >
          !!
        </span>
      )}
      <span aria-hidden="true" className="absolute bottom-2.5 left-3.5 font-mono text-[10px] font-bold tracking-[0.2em] text-white/40">LANTERN KNIGHT</span>
      {demo.mimic && <div aria-hidden="true" className="lk-flash pointer-events-none absolute inset-0 bg-[oklch(62%_0.22_25/0.45)]" />}
      {demo.rewinding && (
        <div aria-hidden="true" className="pointer-events-none absolute inset-0 bg-[repeating-linear-gradient(0deg,oklch(100%_0_0/0.07)_0_2px,transparent_2px_4px)]">
          <div className="lk-band absolute inset-x-0 h-[46px] bg-[linear-gradient(transparent,oklch(100%_0_0/0.14),transparent)]" />
          <span className="lk-rewind-text absolute left-[18px] top-[52px] font-mono text-lg font-bold tracking-[0.08em] text-white/90 [text-shadow:2px_0_oklch(70%_0.2_25/0.7),-2px_0_oklch(75%_0.14_220/0.7)]">
            {t.tour.rewindFx}
          </span>
        </div>
      )}
    </div>
  );
}

function Campfire(): ReactElement {
  return (
    <>
      <circle className="lk-fire-glow" cx="135" cy="226" r="34" fill="oklch(75% .17 55 / .16)" />
      <rect x="122" y="229" width="26" height="5" rx="2.5" fill="oklch(38% .06 50)" transform="rotate(-12 135 231)" />
      <rect x="122" y="229" width="26" height="5" rx="2.5" fill="oklch(33% .06 50)" transform="rotate(12 135 231)" />
      <path className="lk-flame" d="M135 204 C 144 214 146 222 141 229 C 138 232 132 232 129 229 C 124 222 128 214 135 204Z" fill="oklch(76% .17 55)" />
      <path className="lk-flame lk-flame-inner" d="M135 214 C 140 220 140 225 138 228 C 136 230 134 230 132 228 C 130 225 131 220 135 214Z" fill="oklch(92% .12 90)" />
    </>
  );
}

function Castle(): ReactElement {
  return (
    <g>
      <rect x="436" y="150" width="22" height="86" fill="oklch(30% .04 280)" />
      <rect x="490" y="150" width="22" height="86" fill="oklch(30% .04 280)" />
      <rect x="458" y="172" width="32" height="64" fill="oklch(25% .04 280)" />
      <g fill="oklch(30% .04 280)">
        {[436, 444, 452, 490, 498, 506].map((x) => <rect key={x} x={x} y="142" width="6" height="8" />)}
      </g>
      <rect x="444" y="168" width="6" height="9" rx="1" fill="oklch(82% .13 80)" />
      <rect x="498" y="168" width="6" height="9" rx="1" fill="oklch(82% .13 80)" />
      <path d="M464 236 v-24 a10 10 0 0 1 20 0 v24z" fill="oklch(13% .03 280)" />
      <path d="M447 142 v-24" stroke="oklch(62% .02 265)" strokeWidth="2" />
      <path className="lk-flag" d="M448 118 l16 5 l-16 5z" fill={ACC} />
    </g>
  );
}

function Chest(): ReactElement {
  return (
    <g>
      <rect x="322" y="216" width="40" height="20" rx="3" fill="oklch(46% .1 55)" />
      <path d="M320 216 v-6 a10 10 0 0 1 10 -10 h24 a10 10 0 0 1 10 10 v6z" fill="oklch(55% .11 55)" />
      <rect x="320" y="214" width="44" height="4" fill="oklch(34% .06 55)" />
      <rect x="338" y="211" width="8" height="10" rx="1.5" fill="oklch(85% .14 90)" />
    </g>
  );
}

function Mimic(): ReactElement {
  return (
    <g className="lk-mimic">
      <path d="M322 216 L364 216 L329 189Z" fill="oklch(14% .04 20)" />
      <path d="M326 216 Q 343 205 360 216Z" fill="oklch(60% .19 15)" />
      <rect x="322" y="216" width="40" height="20" rx="3" fill="oklch(46% .1 55)" />
      <path d="M323 216 l3 -6 l3 6 l3 -6 l3 6 l3 -6 l3 6 l3 -6 l3 6 l3 -6 l3 6 l3 -6 l3 6 l3 -6 l2 6z" fill="oklch(96% .01 90)" />
      <g transform="rotate(30 364 216)">
        <path d="M322 216 l3.5 6 l3.5 -6 l3.5 6 l3.5 -6 l3.5 6 l3.5 -6 l3.5 6 l3.5 -6 l3.5 6 l3.5 -6 l3.5 6 l3.5 -6z" fill="oklch(96% .01 90)" />
        <path d="M320 216 v-6 a10 10 0 0 1 10 -10 h24 a10 10 0 0 1 10 10 v6z" fill="oklch(55% .11 55)" />
        <circle className="lk-eye" cx="333" cy="207" r="3" fill="oklch(72% .22 25)" />
        <circle className="lk-eye" cx="347" cy="207" r="3" fill="oklch(72% .22 25)" />
      </g>
    </g>
  );
}

function Knight(): ReactElement {
  return (
    <>
      <ellipse cx="11" cy="237" rx="12" ry="3" fill="oklch(0% 0 0 / .35)" />
      <rect x="4" y="224" width="5" height="12" rx="2" fill="oklch(30% .03 265)" />
      <rect x="13" y="224" width="5" height="12" rx="2" fill="oklch(30% .03 265)" />
      <path d="M2 210 h18 v14 a3 3 0 0 1 -3 3 h-12 a3 3 0 0 1 -3 -3z" fill={ACC} />
      <rect x="2" y="216" width="18" height="3" fill="oklch(36% .06 60)" />
      <circle cx="11" cy="201" r="8" fill="oklch(84% .05 60)" />
      <path d="M3 200 a8 8 0 0 1 16 0 h-4 v-2 h-8 v2z" fill="oklch(72% .02 265)" />
      <circle cx="15" cy="202" r="1.4" fill="oklch(20% .02 265)" />
      <path d="M21 216 L31 198" stroke="oklch(90% .01 265)" strokeWidth="2.5" strokeLinecap="round" />
      <path d="M19 213 L24 218" stroke="oklch(60% .1 60)" strokeWidth="2.5" strokeLinecap="round" />
    </>
  );
}

const LOOKS: Record<StripCheckpoint['look'], { fill: string; ring: string; fg: string; bg: string }> = {
  main: { fill: ACC, ring: 'transparent', fg: ACC, bg: 'var(--acc-soft)' },
  bad: { fill: BAD, ring: BAD, fg: BAD, bg: 'oklch(68% 0.19 25 / 0.14)' },
  kept: { fill: 'transparent', ring: BAD, fg: BAD, bg: 'oklch(68% 0.19 25 / 0.14)' },
  lane: { fill: 'var(--acc-dim)', ring: 'transparent', fg: 'var(--acc-dim)', bg: 'color-mix(in oklch, var(--acc-dim) 14%, transparent)' },
};

export function Strip({ demo }: { demo: Demo }): ReactElement {
  const t = useT();
  const strip = stripOf(demo);

  return (
    <div className="absolute rounded-[18px] bg-black/20 shadow-[inset_0_0_0_1px_oklch(100%_0_0/0.05)]" style={STRIP}>
      <div className="absolute left-[18px] top-3.5 flex items-center gap-2 text-[11px] font-bold uppercase tracking-[0.12em] text-tx3">
        <svg width="14" height="14" viewBox="0 0 64 64" fill="none" strokeLinecap="round" aria-hidden="true">
          <path d="M10 32 L26 32 M26 32 C 36 32 40 18 52 18 M26 32 C 36 32 40 46 52 46" stroke={ACC} strokeWidth="7" />
        </svg>
        {t.tour.strip}
      </div>
      {demo.saved === 0 && <div className="absolute inset-x-0 top-24 text-center text-[13px] text-tx3">{t.tour.stripEmpty}</div>}
      <div
        className="absolute left-5 top-[73px] h-1.5 rounded-full bg-acc transition-[width] duration-600 ease-ez"
        style={{ width: strip.lineTo - 20, opacity: demo.saved > 0 ? 1 : 0 }}
      />
      {strip.ahead && <div className="lk-fade-in absolute left-[244px] top-[75px] w-[116px] border-t-2 border-dashed border-err opacity-60" />}
      {strip.branch && (
        <svg width="524" height="176" aria-hidden="true" className="absolute inset-0 overflow-visible">
          <path className="lk-draw-branch" pathLength={1} d="M244 76 C 296 76 308 124 360 124" fill="none" stroke="var(--acc-dim)" strokeWidth="3" strokeLinecap="round" />
        </svg>
      )}
      {strip.checkpoints.map((c) => {
        const look = LOOKS[c.look];
        return (
          <div key={c.name === 'mimicRun' ? 'mimic' : c.name} className="contents">
            <div
              className="absolute -ml-[11px] -mt-[11px] flex size-[22px] items-center justify-center transition-[left,top] duration-[560ms] ease-ez"
              style={{ left: c.x, top: c.y }}
            >
              <span
                className="lk-pop size-3 rounded-full transition-[background-color,box-shadow] duration-400"
                style={{ background: look.fill, boxShadow: `inset 0 0 0 2px ${look.ring}` }}
              />
            </div>
            <div
              className="lk-label-in absolute flex h-[22px] -translate-x-1/2 items-center gap-1.5 whitespace-nowrap rounded-md px-2 text-xs font-bold transition-[top,color,background-color] duration-[560ms,400ms,400ms] ease-ez"
              style={{ left: c.x, top: c.look === 'lane' ? 142 : 42, color: look.fg, background: look.bg }}
            >
              <span aria-hidden="true" className="size-1.5 shrink-0 rotate-45 bg-current" />
              {t.tour.checkpoints[c.name]}
            </div>
          </div>
        );
      })}
      {strip.token !== null && (
        <div
          aria-hidden="true"
          className="pointer-events-none absolute -ml-[15px] -mt-[15px] size-[30px] transition-[left] duration-[640ms] ease-ez"
          style={{ left: strip.token.x, top: 76, '--tc': strip.token.bad ? BAD : ACC } as CSSProperties}
        >
          <i className="absolute inset-0 rounded-full border-2 border-[var(--tc)] transition-colors duration-300" />
          <i className="absolute inset-0 rounded-full border-2 border-[var(--tc)] [animation:halo_2.2s_ease-out_infinite]" />
        </div>
      )}
    </div>
  );
}

/** The dot that flies from the game's "Saving…" down onto the strip. */
export function FlyingSave({ save }: { save: Flying }): ReactElement {
  const color = save.bad ? BAD : ACC;
  return (
    <div
      aria-hidden="true"
      className="lk-fly pointer-events-none absolute left-0 top-0 z-[5] -ml-1.5 -mt-1.5 size-3 rounded-full"
      style={{
        '--x0': `${String(SAVE_FROM.x)}px`,
        '--y0': `${String(SAVE_FROM.y)}px`,
        '--x1': `${String(STRIP.left + save.x)}px`,
        '--y1': `${String(STRIP.top + 76)}px`,
        background: color,
        boxShadow: `0 0 14px ${color}`,
      } as CSSProperties}
    />
  );
}
