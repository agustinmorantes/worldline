// What history is kept: the retention tiers on a ruler, a live estimate of what a cleanup
// would free, and the cleanup itself.
import { type ReactElement, useState } from 'react';
import type { RetentionSettings } from '@/ipc/bindings/RetentionSettings';
import { runPruning, updatePruningSettings } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import { reportError } from '@/shared/errors/report';
import { type Messages, useLocale, useT } from '@/shared/i18n';
import { btnQuiet } from '@/shared/ui/classes';
import { Card, Stepper, Toggle } from './controls';
import { saveNow, usePruningPreview, useRetention } from './queries';
import { FORGET_DAYS, formatBytes, freedBytes, ruler, step, stepForget, stepTier, TIER_STEPS, type TierKey, type Tiers } from './settings';

const TIERS: { key: TierKey; label: keyof Messages['settings']['tiers'] }[] = [
  { key: 'keep_all_hours', label: 'all' },
  { key: 'hourly_until_hours', label: 'hourly' },
  { key: 'daily_until_days', label: 'daily' },
  { key: 'weekly_until_days', label: 'weekly' },
];

/** A tier's value in the unit that reads best: hours, days, months or years. */
function tierValue(t: Messages, key: TierKey, value: number): string {
  switch (key) {
    case 'keep_all_hours': return t.settings.hours(value);
    case 'hourly_until_hours': return value % 24 === 0 ? t.settings.days(value / 24) : t.settings.hours(value);
    case 'daily_until_days': return t.settings.days(value);
    case 'weekly_until_days':
      if (value % 365 === 0) return t.settings.years(value / 365);
      return value % 30 === 0 ? t.settings.months(value / 30) : t.settings.days(value);
  }
}

export function KeepSection(): ReactElement {
  const t = useT();
  const locale = useLocale();
  const { data: settings } = useRetention();
  const preview = usePruningPreview(settings);
  /** What the last "Clean up now" freed, until the settings change. */
  const [freed, setFreed] = useState<number | null>(null);
  const [cleaning, setCleaning] = useState(false);

  const change = (next: RetentionSettings): void => {
    setFreed(null);
    saveNow(['pruning-settings'], next, updatePruningSettings, t.settings.failed.keep);
  };

  async function cleanUp(): Promise<void> {
    setCleaning(true);
    try {
      const report = await runPruning();
      setFreed(report.local_gc.bytes_deleted);
      void queryClient.invalidateQueries({ queryKey: ['pruning-preview'] });
      void queryClient.invalidateQueries({ queryKey: ['retired-versions'] });
      void queryClient.invalidateQueries({ queryKey: ['games'] });
      void queryClient.invalidateQueries({ queryKey: ['game'] });
    } catch (err) {
      reportError(t.settings.failed.cleanUp, err);
    } finally {
      setCleaning(false);
    }
  }

  if (settings === undefined) return <Card title={t.settings.keep}>{null}</Card>;

  const forget = settings.branch_expiry_days;
  const result = freed !== null
    ? t.settings.freed(formatBytes(freed, locale))
    : preview.data !== undefined ? t.settings.wouldFree(formatBytes(freedBytes(preview.data), locale)) : '';

  return (
    <Card title={t.settings.keep}>
      <div className="mt-4">
        <Toggle
          label={t.settings.automatic}
          hint={t.settings.automaticHint}
          on={settings.automatic}
          onChange={(automatic) => { change({ ...settings, automatic }); }}
        />
      </div>
      <Ruler tiers={settings} />
      <div className="mt-4 grid grid-cols-[repeat(auto-fit,minmax(250px,1fr))] gap-x-6 gap-y-3">
        {TIERS.map(({ key, label }) => (
          <Stepper
            key={key}
            label={t.settings.tiers[label]}
            value={tierValue(t, key, settings[key])}
            canLess={step(TIER_STEPS[key], settings[key], -1) !== settings[key]}
            canMore={step(TIER_STEPS[key], settings[key], 1) !== settings[key]}
            onStep={(dir) => { change({ ...settings, ...stepTier(settings, key, dir) }); }}
          />
        ))}
      </div>
      <div className="mt-5">
        <Stepper
          label={t.settings.forget}
          hint={t.settings.forgetHint}
          value={forget === null ? t.settings.off : t.settings.days(forget)}
          canLess={forget === null || forget > FORGET_DAYS[0]}
          canMore={forget !== null}
          onStep={(dir) => { change({ ...settings, branch_expiry_days: stepForget(forget, dir) }); }}
        />
      </div>
      <div className="mt-5 flex items-center gap-3.5">
        <button type="button" className={btnQuiet} disabled={cleaning} onClick={() => { void cleanUp(); }}>
          {t.settings.cleanUpNow}
        </button>
        <span aria-live="polite" className="text-[13px] text-tx2">{result}</span>
      </div>
    </Card>
  );
}

/** The tiers on a log-scaled line, now at the right: dots thin out as checkpoints age. */
function Ruler({ tiers }: { tiers: Tiers }): ReactElement {
  const t = useT();
  const { segments, dots } = ruler(tiers);
  const at = (fraction: number): string => `${(fraction * 100).toFixed(2)}%`;
  return (
    <div aria-hidden="true" className="relative mt-5 h-[58px]">
      <span className="absolute inset-x-0 top-[13px] h-0.5 bg-ln" />
      {dots.map((dot) => (
        <span
          key={`${dot.tier}-${dot.n.toString()}`}
          className="absolute top-[11px] -ml-[3px] size-1.5 rounded-full bg-acc transition-[left,opacity] duration-[450ms] ease-ez"
          style={{ left: at(dot.left), opacity: dot.tier === 'monthly' ? 0.45 : 1 }}
        />
      ))}
      {segments.map((segment) => (
        <span
          key={segment.tier}
          className="absolute top-[30px] overflow-hidden whitespace-nowrap border-l-[1.5px] border-ln pl-1.5 text-[11.5px] font-semibold text-tx3 transition-[left,width] duration-[450ms] ease-ez"
          style={{ left: at(segment.left), width: at(segment.width) }}
        >
          {t.settings.ruler[segment.tier]}
        </span>
      ))}
      <span className="absolute right-0 top-[44px] text-[11.5px] text-tx3">{t.settings.ruler.now}</span>
    </div>
  );
}
