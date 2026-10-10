// The naming card the Name it hotkey opens (O4): over the game in the overlay window, or in the
// app when it can't show there. The checkpoint is kept before it opens; the card only renames it.
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactElement, type ReactNode } from 'react';
import { Dialog } from 'radix-ui';
import type { NamingCard as Card, NamingClose } from '@/ipc/events';
import { en } from '@/shared/i18n/en';
import { es } from '@/shared/i18n/es';
import { accentVars } from '@/shared/tokens/accent';
import './naming.css';

const MESSAGES = { en, es };
/** The longest name the field takes. */
export const MAX_NAME = 60;
/** The body's fade and the card's fold into the tile. */
const FOLD_MS = 420;
/** The tile's flight: into the popup's corner, or onto its row in the app. */
const FLY_MS = { overlay: 440, app: 460 };
/** Reduced motion: the card fades out instead. */
const FADE_MS = 200;
/** After a fold without a flight, the folded tile's fade. */
const FOLDED_MS = 120;

/** Where the tile's centre flies to, from where it sits. */
export interface Flight { x: number; y: number }

type Phase = 'open' | 'closing' | 'flying' | 'folded';

export interface NamingCardProps {
  card: Card;
  /** Over the game, or in the app (it starts later, after the window restores). */
  place: 'overlay' | 'app';
  /** The focus went elsewhere and only the host knows: close as a focus loss. */
  left?: boolean;
  /** Closed: `name` is what the field holds. Says where the tile flies, or `null` to only fold. */
  onClose: (how: NamingClose, name: string) => Flight | null | Promise<Flight | null>;
  /** The fold, and the flight if any, ended. */
  onDone: () => void;
}

/** The card itself: one name field, saved with Enter or ✓, kept automatic with Esc. It covers the
 * window, and a click in it never moves the focus off the field, so the focus leaving the card
 * means the window lost it. In the app it's a modal over any other, so it holds the focus. */
export function NamingCard({ card, place, left = false, onClose, onDone }: NamingCardProps): ReactElement {
  const t = MESSAGES[card.language].naming;
  const reduced = card.motion === 'reduced';
  const [draft, setDraft] = useState(card.auto);
  const [phase, setPhase] = useState<Phase>('open');
  const [flight, setFlight] = useState<Flight | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const closedRef = useRef(false);
  const timersRef = useRef<number[]>([]);
  const later = (ms: number): Promise<void> => new Promise((resolve) => {
    timersRef.current.push(window.setTimeout(resolve, ms));
  });

  useEffect(() => () => { timersRef.current.forEach((id) => { clearTimeout(id); }); }, []);

  // Focused the instant it opens, so typing works during the animation, with the automatic name
  // selected so the first key replaces it. Again once the window has the focus to give, unless
  // the card has it already: what was typed meanwhile stays.
  useLayoutEffect(() => {
    const select = (): void => {
      if (closedRef.current) return;
      if (document.hasFocus() && rootRef.current?.contains(document.activeElement) === true) return;
      inputRef.current?.focus();
      inputRef.current?.select();
    };
    inputRef.current?.focus();
    inputRef.current?.select();
    const id = window.setTimeout(select, 30);
    return () => { clearTimeout(id); };
  }, []);

  const close = (how: NamingClose): void => {
    if (closedRef.current) return;
    closedRef.current = true;
    setPhase('closing');
    const target = Promise.resolve(onClose(how, how === 'escape' ? '' : draft)).catch(() => null);
    void (async () => {
      const [to] = await Promise.all([target, later(reduced ? FADE_MS : FOLD_MS)]);
      if (reduced) {
        onDone();
      } else if (to === null) {
        setPhase('folded');
        await later(FOLDED_MS);
        onDone();
      } else {
        setFlight(to);
        setPhase('flying');
        await later(FLY_MS[place]);
        onDone();
      }
    })();
  };

  const closeRef = useRef(close);
  closeRef.current = close;
  useEffect(() => {
    if (left) closeRef.current('blur');
  }, [left]);
  // The window lost the focus, wherever it was in the card.
  useEffect(() => {
    const blurred = (): void => { closeRef.current('blur'); };
    window.addEventListener('blur', blurred);
    return () => { window.removeEventListener('blur', blurred); };
  }, []);

  const style = {
    ...accentVars(card.accentHue),
    ...(flight && { '--fx': `${flight.x.toString()}px`, '--fy': `${flight.y.toString()}px` }),
  } as CSSProperties;

  const keep = (e: Event): void => { e.preventDefault(); };
  const cardOf = (title: ReactNode): ReactElement => (
    <div className="card" {...(place === 'overlay' && { role: 'dialog', 'aria-modal': true, 'aria-label': t.eyebrow })}>
      {title}
      <div className="ctile">
        <svg className="mk" width="48" height="48" viewBox="0 0 64 64" aria-hidden="true">
          <path className="trunk" pathLength={1} d="M10 32 L26 32" />
          <path className="up" pathLength={1} d="M26 32 C 36 32 40 18 52 18" />
          <path className="dn" pathLength={1} d="M26 32 C 36 32 40 46 52 46" />
          <circle className="end" cx="52" cy="18" r="3.4" />
          <circle className="end" cx="52" cy="46" r="3.4" />
          <path className="dia" d="M26 23 L35 32 L26 41 L17 32 Z" />
        </svg>
      </div>
      <div className="cbody">
        <div className="eyebrow">{t.eyebrow}</div>
        <div className="cmeta">
          <MetaLine text={t.meta(card.version, card.title)} number={`#${card.version.toString()}`} />
        </div>
        <div className="crow">
          <label className="cfield">
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M6 1l5 5-5 5-5-5z" fill="currentColor" /></svg>
            <input
              ref={inputRef}
              className="cin"
              value={draft}
              maxLength={MAX_NAME}
              aria-label={t.field}
              spellCheck={false}
              onChange={(e) => { setDraft(e.target.value); }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  close('save');
                }
              }}
            />
          </label>
          <button type="button" className="ok" aria-label={t.save} onClick={() => { close('save'); }}>
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M5 12.5l4.5 4.5L19 7.5" />
            </svg>
          </button>
        </div>
        <div className="chint">
          <span className="kc">Enter</span> {t.saves} <span style={{ opacity: 0.5 }}>·</span> <span className="kc">Esc</span> {t.keeps(card.auto)}
        </div>
      </div>
    </div>
  );
  const root = (inner: ReactElement): ReactElement => (
    <div
      ref={rootRef}
      className={['nb', place, phase, reduced && 'rm'].filter(Boolean).join(' ')}
      style={style}
      // A click in the card never takes the focus off the field.
      onMouseDown={(e) => { if (e.target !== inputRef.current) e.preventDefault(); }}
      // The focus leaving the card, not just moving between the field and ✓, is a focus loss.
      onBlur={(e) => {
        if (e.relatedTarget instanceof Node && e.currentTarget.contains(e.relatedTarget)) return;
        close('blur');
      }}
      onKeyDown={(e) => {
        if (e.key === 'Escape') {
          e.preventDefault();
          close('escape');
        }
      }}
    >
      <div className="scrim" />
      {inner}
    </div>
  );

  if (place === 'overlay') return root(cardOf(null));
  // In the app, a modal above any already open (the tour, a dialog): the focus is the card's, and
  // so are Esc and clicks around it.
  return (
    <Dialog.Root open>
      <Dialog.Portal>
        {root(
          <Dialog.Content
            asChild
            aria-describedby={undefined}
            onOpenAutoFocus={keep}
            onCloseAutoFocus={keep}
            onEscapeKeyDown={keep}
            onInteractOutside={keep}
          >
            {cardOf(<Dialog.Title className="sr-only">{t.eyebrow}</Dialog.Title>)}
          </Dialog.Content>,
        )}
      </Dialog.Portal>
    </Dialog.Root>
  );
}

/** The meta line with its checkpoint number in the accent's mono. */
function MetaLine({ text, number }: { text: string; number: string }): ReactElement {
  const at = text.indexOf(number);
  if (at < 0) return <>{text}</>;
  return (
    <>
      {text.slice(0, at)}
      <span className="mono cnum">{number}</span>
      {text.slice(at + number.length)}
    </>
  );
}
