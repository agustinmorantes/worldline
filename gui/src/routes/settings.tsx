import { createFileRoute } from '@tanstack/react-router';
import { ReactElement, useState, useEffect } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { getMonitoringSettings, updateMonitoringSettings } from '@/api/settings';
import { Clock } from 'lucide-react';
import { toast } from 'sonner';
import { ScrollArea } from '@/components/ui/scroll-area';

export const Route = createFileRoute('/settings')({
  component: SettingsView,
});

function SettingsView(): ReactElement {
  const [debounce, setDebounce] = useState('5');
  const [cooldown, setCooldown] = useState('15');

  useEffect(() => {
    void getMonitoringSettings()
      .then((settings) => {
        setDebounce(settings.snapshot_debounce_seconds.toString());
        setCooldown(settings.version_cooldown_minutes.toString());
      })
      .catch((err: unknown) => {
        console.error(err);
        toast.error('Failed to load monitoring settings');
      });
  }, []);

  const handleSave = async (): Promise<void> => {
    const debounceSeconds = Number(debounce);
    const cooldownMinutes = Number(cooldown);

    if (!Number.isFinite(debounceSeconds) || debounceSeconds < 1 || debounceSeconds > 30) {
      toast.error('Snapshot delay must be between 1 and 30 seconds');
      return;
    }

    if (!Number.isFinite(cooldownMinutes) || cooldownMinutes < 1 || cooldownMinutes > 60) {
      toast.error('Version cooldown must be between 1 and 60 minutes');
      return;
    }

    try {
      await updateMonitoringSettings({
        snapshot_debounce_seconds: debounceSeconds,
        version_cooldown_minutes: cooldownMinutes,
      });
    } catch (err) {
      console.error(err);
      toast.error('Failed to save monitoring settings');
      return;
    }

    toast.success('Settings saved and applied');
  };

  const handleReset = (): void => {
    setDebounce('5');
    setCooldown('15');
    void updateMonitoringSettings({
      snapshot_debounce_seconds: 5,
      version_cooldown_minutes: 15,
    })
      .then(() => {
        toast.success('Settings reset to defaults');
      })
      .catch((err: unknown) => {
        console.error(err);
        toast.error('Failed to reset monitoring settings');
      });
  };

  return (
    <div className="flex flex-col h-full bg-background overflow-hidden">
      <header className="p-8 pb-4">
        <h1 className="text-3xl font-black tracking-tight text-foreground">Settings</h1>
        <p className="mt-1 text-sm text-muted-foreground">Configure how Timeline watches save folders and creates checkpoints.</p>
      </header>

      <ScrollArea className="flex-1 min-h-0" viewportClassName="p-8 pt-4">
      <div className="space-y-6">
        <section className="space-y-4">
          <div className="flex items-center gap-2 px-1">
            <Clock size={16} className="text-primary" />
            <h2 className="text-xs font-bold uppercase tracking-wider text-muted-foreground/70">Monitoring</h2>
          </div>

          <div className="grid gap-4">
            <SettingsCard>
              <div className="flex flex-col gap-4 md:flex-row md:items-start md:justify-between">
                <div className="max-w-xl">
                  <h3 className="text-sm font-bold">Snapshot Debounce</h3>
                  <p className="mt-0.5 text-xs text-muted-foreground">
                    After the cooldown window has passed, wait this many seconds after the latest file change before creating a checkpoint.
                  </p>
                </div>
                <div className="w-full md:w-44">
                  <label className="mb-1 block text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/70">
                    Seconds
                  </label>
                  <Input
                    type="number"
                    min={1}
                    max={30}
                    step={1}
                    value={debounce}
                    onChange={(event) => { setDebounce(event.target.value); }}
                    className="bg-el border-border/40"
                  />
                </div>
              </div>
            </SettingsCard>

            <SettingsCard>
              <div className="flex flex-col gap-4 md:flex-row md:items-start md:justify-between">
                <div className="max-w-xl">
                  <h3 className="text-sm font-bold">Version Cooldown</h3>
                  <p className="mt-0.5 text-xs text-muted-foreground">
                    Do not create another checkpoint for the same game until this many minutes have passed.
                  </p>
                </div>
                <div className="w-full md:w-44">
                  <label className="mb-1 block text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/70">
                    Minutes
                  </label>
                  <Input
                    type="number"
                    min={1}
                    max={60}
                    step={1}
                    value={cooldown}
                    onChange={(event) => { setCooldown(event.target.value); }}
                    className="bg-el border-border/40"
                  />
                </div>
              </div>
            </SettingsCard>
          </div>
        </section>

        <div className="pt-8 flex justify-end gap-3">
          <Button variant="ghost" onClick={handleReset}>Reset Defaults</Button>
          <Button className="font-bold px-8" onClick={() => { void handleSave(); }}>Save Settings</Button>
        </div>
        <p className="text-right text-xs text-muted-foreground">
          Changes apply immediately after saving.
        </p>
      </div>
      </ScrollArea>
    </div>
  );
}

function SettingsCard({ children }: { children: React.ReactNode }): ReactElement {
  return (
    <div className="p-5 rounded-2xl bg-sf border border-border/40 shadow-sm">
      {children}
    </div>
  );
}
