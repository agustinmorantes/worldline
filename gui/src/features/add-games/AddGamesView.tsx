// One place to add games: from the cloud, from a scan of this PC, or by hand.
import type { ReactElement } from 'react';
import { motion } from 'motion/react';
import { useGames, useSyncReport } from '@/features/library/queries';
import { useT } from '@/shared/i18n';
import { CloudSection } from './CloudSection';
import { ManualSection } from './ManualSection';
import { ScanSection } from './ScanSection';

export function AddGamesView({ scan = false }: { scan?: boolean }): ReactElement {
  const t = useT();
  const { data: games = [] } = useGames();
  const { data: report } = useSyncReport();
  const cloudTitles = report?.remote_games.filter((g) => !g.adopted).map((g) => g.title) ?? [];

  return (
    <motion.div layoutScroll data-add-scroll className="h-full overflow-y-auto px-8 pb-10 pt-5">
      <h1 className="font-display text-[34px] font-extrabold leading-[1.15]">{t.addGames.title}</h1>
      <div className="mt-8 flex flex-col gap-10">
        {report !== undefined && <CloudSection report={report} />}
        <ScanSection library={games} cloudTitles={cloudTitles} autoScan={scan} />
        <ManualSection library={games} />
      </div>
    </motion.div>
  );
}
