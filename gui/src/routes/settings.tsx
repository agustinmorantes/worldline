import { createFileRoute } from '@tanstack/react-router';
import { ReactElement, useState, useEffect } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { getMonitoringSettings, updateMonitoringSettings } from '@/api/settings';
import { Clock } from 'lucide-react';
import { toast } from 'sonner';
import { ScrollArea } from '@/components/ui/scroll-area';
import { FolderSync, Trash2 } from 'lucide-react';
import { confirm, open } from '@tauri-apps/plugin-dialog';
import {
  getDevice,
  getSyncSettings,
  refreshSync,
  removeSyncDevice,
  updateDeviceName,
  updateSyncSettings,
} from '@/api/sync';
import { SyncReport } from '@/models/sync';
import { Badge } from '@/components/ui/badge';
import { formatDistanceToNow } from 'date-fns';

export const Route = createFileRoute('/settings')({
  component: SettingsView,
});

function SettingsView(): ReactElement {
  const [debounce, setDebounce] = useState('0.2');
  const [cooldown, setCooldown] = useState('0');
  const [remotePath, setRemotePath] = useState('');
  const [deviceName, setDeviceName] = useState('');
  const [deviceUuid, setDeviceUuid] = useState('');
  const [syncReport, setSyncReport] = useState<SyncReport>();
  const [syncing, setSyncing] = useState(false);

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

  useEffect(() => {
    void Promise.all([getDevice(), getSyncSettings()])
      .then(([device, settings]) => {
        setDeviceName(device.name);
        setDeviceUuid(device.device_uuid);
        setRemotePath(settings.remote_path ?? '');
        if (settings.remote_path !== null && settings.remote_path !== undefined) {
          setSyncing(true);
          void refreshSync()
            .then(setSyncReport)
            .catch((err: unknown) => { console.error(err); })
            .finally(() => { setSyncing(false); });
        }
      })
      .catch((err: unknown) => {
        console.error(err);
        toast.error('Failed to load sync settings');
      });
  }, []);

  const handleSave = async (): Promise<void> => {
    const debounceSeconds = Number(debounce);
    const cooldownMinutes = Number(cooldown);

    if (!Number.isFinite(debounceSeconds) || debounceSeconds < 0.1 || debounceSeconds > 30) {
      toast.error('Snapshot delay must be between 0.1 and 30 seconds');
      return;
    }

    if (!Number.isFinite(cooldownMinutes) || cooldownMinutes < 0 || cooldownMinutes > 60) {
      toast.error('Version cooldown must be between 0 and 60 minutes');
      return;
    }

    try {
      await Promise.all([
        updateMonitoringSettings({
          snapshot_debounce_seconds: debounceSeconds,
          version_cooldown_minutes: cooldownMinutes,
        }),
        updateSyncSettings({ remote_path: remotePath.trim() === '' ? null : remotePath.trim() }),
        updateDeviceName(deviceName),
      ]);
      if (remotePath.trim() !== '') {
        setSyncReport(await refreshSync());
      }
    } catch (err) {
      console.error(err);
      toast.error('Failed to save monitoring settings');
      return;
    }

    toast.success('Settings saved and applied');
  };

  const handleChooseRemote = async (): Promise<void> => {
    const selected = await open({ directory: true, multiple: false, title: 'Select Sync Folder' });
    if (selected !== null) setRemotePath(selected);
  };

  const handleRefreshSync = async (): Promise<void> => {
    try {
      setSyncing(true);
      await updateSyncSettings({ remote_path: remotePath.trim() === '' ? null : remotePath.trim() });
      setSyncReport(await refreshSync());
      toast.success('Sync complete');
    } catch (err) {
      console.error(err);
      toast.error('Sync failed');
    } finally {
      setSyncing(false);
    }
  };

  const handleRemoveDevice = async (uuid: string, name: string): Promise<void> => {
    if (!await confirm(`Remove ${name} from this sync folder?`, { title: 'Remove Device', kind: 'warning' })) return;
    try {
      await removeSyncDevice(uuid);
      setSyncReport(await refreshSync());
      toast.success(`${name} removed`);
    } catch (err) {
      console.error(err);
      toast.error('Failed to remove device');
    }
  };

  const handleReset = (): void => {
    setDebounce('0.2');
    setCooldown('0');
    void updateMonitoringSettings({
      snapshot_debounce_seconds: 0.2,
      version_cooldown_minutes: 0,
    })
      .then(() => {
        toast.success('Settings reset to demo defaults');
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
                    min={0.1}
                    max={30}
                    step={0.1}
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
                    min={0}
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

        <section className="flex flex-col gap-4">
          <div className="flex items-center gap-2 px-1">
            <FolderSync data-icon="inline-start" />
            <h2 className="text-xs font-bold uppercase tracking-wider text-muted-foreground/70">Device Sync</h2>
          </div>

          <div className="grid gap-4">
            <SettingsCard>
              <div className="flex flex-col gap-4">
                <div>
                  <h3 className="text-sm font-bold">Shared Folder</h3>
                  <p className="mt-0.5 text-xs text-muted-foreground">Choose a folder managed by your cloud-drive client.</p>
                </div>
                <div className="flex gap-2">
                  <Input
                    aria-label="Sync folder"
                    value={remotePath}
                    onChange={(event) => { setRemotePath(event.target.value); }}
                    placeholder="Select a shared folder"
                    className="bg-el border-border/40"
                  />
                  <Button variant="outline" onClick={() => { void handleChooseRemote(); }}>Browse</Button>
                  <Button disabled={syncing || remotePath.trim() === ''} onClick={() => { void handleRefreshSync(); }}>
                    {syncing ? 'Syncing…' : 'Refresh'}
                  </Button>
                </div>
              </div>
            </SettingsCard>

            <SettingsCard>
              <div className="flex flex-col gap-4">
                <div>
                  <h3 className="text-sm font-bold">This Device</h3>
                  <p className="mt-0.5 text-xs text-muted-foreground">This name appears beside progress created here.</p>
                </div>
                <Input
                  aria-label="Device name"
                  value={deviceName}
                  onChange={(event) => { setDeviceName(event.target.value); }}
                  className="bg-el border-border/40"
                />
              </div>
            </SettingsCard>

            {syncReport !== undefined ? (
              <SettingsCard>
                <div className="flex flex-col gap-3">
                  <div>
                    <h3 className="text-sm font-bold">Devices</h3>
                    <p className="mt-0.5 text-xs text-muted-foreground">Removing a retired device lets future cleanup ignore it.</p>
                  </div>
                  {syncReport.devices.map(device => (
                    <div key={device.device_uuid} className="flex items-center justify-between gap-3 rounded-lg bg-el p-3">
                      <div className="min-w-0">
                        <p className="truncate text-sm font-semibold">{device.name}</p>
                        <p className="text-xs text-muted-foreground">
                          {device.platform} · seen {formatDistanceToNow(new Date(device.last_seen), { addSuffix: true })}
                        </p>
                      </div>
                      {device.device_uuid === deviceUuid ? (
                        <Badge variant="secondary">This device</Badge>
                      ) : (
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          aria-label={`Remove ${device.name}`}
                          onClick={() => { void handleRemoveDevice(device.device_uuid, device.name); }}
                        >
                          <Trash2 data-icon="inline-start" />
                        </Button>
                      )}
                    </div>
                  ))}
                </div>
              </SettingsCard>
            ) : null}
          </div>
        </section>

        <div className="pt-8 flex justify-end gap-3">
          <Button variant="ghost" onClick={handleReset}>Reset Defaults</Button>
          <Button className="font-bold px-8" onClick={() => { void handleSave(); }}>Save Settings</Button>
        </div>
        <p className="text-right text-xs text-muted-foreground">
          Changes apply immediately after saving. Reset uses demo-friendly defaults.
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
