// What the main pane shows with no game open: a way in for an empty library, a nudge otherwise.
import type { ReactElement } from 'react';
import { useRouter } from '@tanstack/react-router';
import { Search } from 'lucide-react';
import { useTour } from '@/features/onboarding/tour-context';
import { useT } from '@/shared/i18n';
import { btn, btnQuiet } from '@/shared/ui/classes';
import { WorldlineGlyph } from '@/shared/ui/WorldlineGlyph';
import { useGames } from './queries';

export function Home(): ReactElement {
  const t = useT();
  const router = useRouter();
  const tour = useTour();
  const { data: games } = useGames();

  return (
    <div className="flex h-full flex-col items-center justify-center p-8 text-center">
      <WorldlineGlyph size={56} />
      {games?.length === 0 ? (
        <>
          <h1 className="mt-6 font-display text-[28px] font-extrabold leading-[1.15]">{t.home.noGames}</h1>
          <p className="mt-2 max-w-[400px] text-[15px] leading-[1.55] text-tx2">{t.home.findHint}</p>
          <div className="mt-6 flex gap-2.5">
            <button type="button" className={btn} onClick={() => { void router.navigate({ to: '/add', search: { scan: true } }); }}>
              <Search size={15} strokeWidth={2.6} aria-hidden="true" />
              {t.home.findGames}
            </button>
            <button type="button" className={btnQuiet} onClick={() => { tour.openAt(1); }}>{t.home.takeTour}</button>
          </div>
        </>
      ) : (
        games !== undefined && <p className="mt-6 text-[15px] text-tx3">{t.home.pickGame}</p>
      )}
    </div>
  );
}
