// How saves become checkpoints: the wait after a save (debounce), the cooldown, and the
// opt-in screenshots where the OS can take them.
import type { ReactElement } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import type { MonitoringSettings } from '@/ipc/bindings/MonitoringSettings';
import { updateMonitoringSettings } from '@/ipc/commands';
import { useSyncSettings } from '@/features/library/queries';
import { useT } from '@/shared/i18n';
import { duration, ease, useMotionReduced } from '@/shared/motion/motion';
import { Card, Stepper, Toggle } from './controls';
import { saveNow, useMonitoringSettings, useScreenshotsSupported } from './queries';
import { COOLDOWN_MINUTES, step, WAIT_SECONDS } from './settings';

export function CaptureSection(): ReactElement {
  const t = useT();
  const { data: settings } = useMonitoringSettings();
  const { data: supported } = useScreenshotsSupported();
  const { data: sync } = useSyncSettings();
  const syncOn = typeof sync?.remote_path === 'string' && sync.remote_path !== '';
  const reduced = useMotionReduced();

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
          {supported === true && (
            <Toggle
              label={t.settings.screenshots}
              hint={t.settings.screenshotsHint}
              on={settings.screenshots_enabled}
              onChange={(screenshots_enabled) => { change({ ...settings, screenshots_enabled }); }}
            />
          )}
          {/* Only the cloud carries screenshots between devices, so the switch unfolds with it. */}
          <AnimatePresence initial={false}>
            {supported === true && syncOn && (
              <motion.div
                key="sync-screenshots"
                className="-mt-3.5 overflow-hidden"
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: 'auto', opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                transition={reduced ? { duration: 0 } : { duration: duration.base, ease }}
              >
                <div className="pt-3.5">
                  <Toggle
                    label={t.settings.syncScreenshots}
                    hint={t.settings.syncScreenshotsHint}
                    on={settings.sync_screenshots}
                    onChange={(sync_screenshots) => { change({ ...settings, sync_screenshots }); }}
                  />
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      )}
    </Card>
  );
}
