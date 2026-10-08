// How saves become checkpoints: the wait after a save (debounce) and the cooldown.
import type { ReactElement } from 'react';
import type { MonitoringSettings } from '@/ipc/bindings/MonitoringSettings';
import { updateMonitoringSettings } from '@/ipc/commands';
import { useT } from '@/shared/i18n';
import { Card, Stepper } from './controls';
import { saveNow, useMonitoringSettings } from './queries';
import { COOLDOWN_MINUTES, step, WAIT_SECONDS } from './settings';

export function CaptureSection(): ReactElement {
  const t = useT();
  const { data: settings } = useMonitoringSettings();

  const change = (next: MonitoringSettings): void => {
    saveNow(['monitoring-settings'], next, updateMonitoringSettings, t.settings.failed.capture);
  };

  return (
    <Card title={t.settings.capture}>
      {settings !== undefined && (
        <div className="mt-4 flex flex-col gap-3.5">
          <Stepper
            label={t.settings.wait}
            hint={t.settings.waitHint}
            value={t.settings.seconds(settings.snapshot_debounce_seconds)}
            canLess={settings.snapshot_debounce_seconds > WAIT_SECONDS[0]}
            canMore={settings.snapshot_debounce_seconds < WAIT_SECONDS[WAIT_SECONDS.length - 1]}
            onStep={(dir) => {
              change({ ...settings, snapshot_debounce_seconds: step(WAIT_SECONDS, settings.snapshot_debounce_seconds, dir) });
            }}
          />
          <Stepper
            label={t.settings.cooldown}
            hint={t.settings.cooldownHint}
            value={t.settings.minutes(settings.version_cooldown_minutes)}
            canLess={settings.version_cooldown_minutes > COOLDOWN_MINUTES[0]}
            canMore={settings.version_cooldown_minutes < COOLDOWN_MINUTES[COOLDOWN_MINUTES.length - 1]}
            onStep={(dir) => {
              change({ ...settings, version_cooldown_minutes: step(COOLDOWN_MINUTES, settings.version_cooldown_minutes, dir) });
            }}
          />
        </div>
      )}
    </Card>
  );
}
