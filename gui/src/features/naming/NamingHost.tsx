// The naming card in the app: where the Name it hotkey asks for a name when the card can't show
// over the game (the popup is off, the game is in exclusive fullscreen, or the focus was refused).
import { useEffect, useState, type ReactElement } from 'react';
import { useRouter } from '@tanstack/react-router';
import { nameItClosed } from '@/ipc/commands';
import { on, type NamingCard as Card, type NamingClose } from '@/ipc/events';
import { reportError } from '@/shared/errors/report';
import { en } from '@/shared/i18n/en';
import { es } from '@/shared/i18n/es';
import { NamingCard, type Flight } from './NamingCard';

const MESSAGES = { en, es };

/** The spring the row's tag pops with when the name lands (`--spring`). */
const SPRING = 'cubic-bezier(.34, 1.5, .5, 1)';

/** The tag chip of the session row that holds `versionId` (`SessionList`), if it's on screen. */
function tagOf(versionId: number): Element | null {
  return document.querySelector(`[data-versions~="${versionId.toString()}"]`);
}

/** From the card's tile (80px above the window's centre) to the middle of `el`. */
function flightTo(el: Element): Flight {
  const rect = el.getBoundingClientRect();
  return {
    x: rect.left + rect.width / 2 - window.innerWidth / 2,
    y: rect.top + rect.height / 2 - (window.innerHeight / 2 - 80),
  };
}

/** Listens for `name-it-open`: shows the game's view with the card over it, after the window
 * restores. On close Rust renames the checkpoint and gives the focus back to the game when it
 * should; the card then just folds, otherwise its tile flies onto the row. The row's tag pops. A
 * name that couldn't be saved is said, in the card's language. */
export function NamingHost(): ReactElement | null {
  const router = useRouter();
  const [card, setCard] = useState<Card | null>(null);

  useEffect(() => {
    let live = true;
    let unlisten = (): void => undefined;
    void on('name-it-open', (next) => {
      void router.navigate({ to: '/game/$gameId', params: { gameId: next.gameId.toString() }, search: {} });
      if (next.motion === 'full') {
        document.querySelector('[data-app-frame]')?.animate(
          [{ opacity: 0, transform: 'scale(.96)' }, { opacity: 1, transform: 'none' }],
          { duration: 420, easing: 'cubic-bezier(.2, .9, .25, 1)' },
        );
      }
      setCard(next);
    }).then((fn) => {
      if (live) unlisten = fn;
      else fn();
    });
    return () => {
      live = false;
      unlisten();
    };
  }, [router]);

  if (card === null) return null;

  const closed = async (how: NamingClose, name: string): Promise<Flight | null> => {
    const { handed, error } = await nameItClosed(card.session, how, name).catch((err: unknown) => {
      console.warn('Naming card: closing failed', err);
      return { handed: false, error: null };
    });
    if (error !== null) {
      const t = MESSAGES[card.language];
      reportError(t.game.failed.name, error, t.naming.keptAs(card.auto));
    }
    const tag = tagOf(card.versionId);
    return handed || tag === null ? null : flightTo(tag);
  };
  const done = (): void => {
    setCard(null);
    if (card.motion === 'full') {
      tagOf(card.versionId)?.animate(
        [{ transform: 'scale(1)', easing: SPRING }, { transform: 'scale(1.12)', offset: 0.4, easing: SPRING }, { transform: 'scale(1)' }],
        { duration: 520 },
      );
    }
  };

  return <NamingCard key={card.session} card={card} place="app" onClose={closed} onDone={done} />;
}
