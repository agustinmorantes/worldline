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
  removeSyncDevice,
  updateDeviceName,
  updateSyncSettings,
} from '@/api/sync';
import { Badge } from '@/components/ui/badge';
import { Switch } from '@/components/ui/switch';
import { formatDistanceToNow } from 'date-fns';
import {
  getPruningSettings,
  presetOf,
  previewPruning,
  RETENTION_PRESETS,
  runPruning,
  updatePruningSettings,
  type PruningPreview,
  type RetentionPreset,
  type RetentionSettings,
} from '@/api/pruning';
import { queryClient } from '@/lib/query-client';
import { useSyncReport } from '@/api/queries';

import { ImportHistorySection } from '@/components/ImportHistorySection';

const MIB = 1024 * 1024;

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * MIB) return `${(bytes / (1024 * MIB)).toFixed(1)} GiB`;
  if (bytes >= MIB) return `${(bytes / MIB).toFixed(1)} MiB`;
  return `${Math.round(bytes / 1024).toString()} KiB`;
}

/** Devices unseen for longer than this are suggested for removal. */
const STALE_DEVICE_MS = 90 * 24 * 60 * 60 * 1000;

export const Route = createFileRoute('/settings')({
  component: SettingsView,
});

function SettingsView(): ReactElement {
  const [debounce, setDebounce] = useState('5');
  const [cooldown, setCooldown] = useState('15');
  const [remotePath, setRemotePath] = useState('');
  const [deviceName, setDeviceName] = useState('');
  const [deviceUuid, setDeviceUuid] = useState('');
  const { data: syncReport, refetch: refetchSyncReport } = useSyncReport();
  const [syncing, setSyncing] = useState(false);
  const [pruning, setPruning] = useState(false);
  const [pruningPreview, setPruningPreview] = useState<PruningPreview>();
  const [retention, setRetention] = useState<RetentionSettings>();
  const [budgetMiB, setBudgetMiB] = useState('');
  const [branchDays, setBranchDays] = useState('');

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
    void Promise.all([getDevice(), getSyncSettings(), getPruningSettings()])
      .then(([device, settings, pruningSettings]) => {
        setDeviceName(device.name);
        setDeviceUuid(device.device_uuid);
        setRemotePath(settings.remote_path ?? '');
        applyRetention(pruningSettings);
      })
      .catch((err: unknown) => {
        console.error(err);
        toast.error('Failed to load sync settings');
      });
  }, []);

  function applyRetention(settings: RetentionSettings): void {
    setRetention(settings);
    setBudgetMiB(settings.max_bytes === null ? '' : Math.round(settings.max_bytes / MIB).toString());
    setBranchDays(settings.branch_expiry_days === null ? '' : settings.branch_expiry_days.toString());
  }

  /** The retention settings as edited, or an error message. */
  function editedRetention(): RetentionSettings | string {
    if (retention === undefined) return 'Retention settings are still loading';
    const budget = budgetMiB.trim() === '' ? null : Number(budgetMiB);
    if (budget !== null && (!Number.isFinite(budget) || budget <= 0)) return 'The disk budget must be a positive number of MiB';
    const days = branchDays.trim() === '' ? null : Number(branchDays);
    if (days !== null && (!Number.isInteger(days) || days < 1)) return 'Branch expiry must be a whole number of days';
    return {
      ...retention,
      max_bytes: budget === null ? null : Math.round(budget * MIB),
      branch_expiry_days: days,
    };
  }

  async function syncNow(): Promise<void> {
    await queryClient.invalidateQueries({ queryKey: ['sync-settings'] });
    await refetchSyncReport();
  }

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

    const retentionSettings = editedRetention();
    if (typeof retentionSettings === 'string') {
      toast.error(retentionSettings);
      return;
    }

    try {
      await Promise.all([
        updateMonitoringSettings({
          snapshot_debounce_seconds: debounceSeconds,
          version_cooldown_minutes: cooldownMinutes,
        }),
        updateDeviceName(deviceName),
      ]);
      const remote = remotePath.trim() === '' ? null : remotePath.trim();
      await updateSyncSettings({ remote_path: remote });
      applyRetention(await updatePruningSettings(retentionSettings));
      if (remote === null) {
        await queryClient.invalidateQueries({ queryKey: ['sync-settings'] });
      } else {
        await syncNow();
      }
    } catch (err) {
      console.error(err);
      toast.error(`Failed to save settings: ${String(err)}`);
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
      await syncNow();
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
      await refetchSyncReport();
      toast.success(`${name} removed`);
    } catch (err) {
      console.error(err);
      toast.error('Failed to remove device');
    }
  };

  const handlePreviewPruning = async (): Promise<void> => {
    const retentionSettings = editedRetention();
    if (typeof retentionSettings === 'string') {
      toast.error(retentionSettings);
      return;
    }
    try {
      setPruning(true);
      // The preview uses the saved rules, so save the edited ones first.
      applyRetention(await updatePruningSettings(retentionSettings));
      setPruningPreview(await previewPruning());
    } catch (err) {
      console.error(err);
      toast.error(`Cleanup preview failed: ${String(err)}`);
    } finally {
      setPruning(false);
    }
  };

  const handleRunPruning = async (): Promise<void> => {
    if (!await confirm('Retire eligible checkpoints and remove unreferenced files locally and from the sync folder? Files other devices still need are kept.', {
      title: 'Run History Cleanup', kind: 'warning',
    })) return;
    try {
      setPruning(true);
      const report = await runPruning();
      setPruningPreview(await previewPruning());
      void queryClient.invalidateQueries({ queryKey: ['retired-versions'] });
      void queryClient.invalidateQueries({ queryKey: ['games'] });
      void queryClient.invalidateQueries({ queryKey: ['game'] });
      const remote = report.remote_gc === null
        ? ''
        : ` and ${formatBytes(report.remote_gc.bytes_deleted)} from the sync folder (${report.remote_gc.chunks_condemned.toString()} more file(s) are removed after 3 days)`;
      toast.success(`Retired ${report.retired_versions.length.toString()} checkpoints; reclaimed ${formatBytes(report.local_gc.bytes_deleted)} locally${remote}`);
      if (report.budget_unreachable) {
        toast.warning('The disk budget is smaller than the checkpoints that must be kept (tags, device positions and slot starts).');
      }
    } catch (err) {
      console.error(err);
      toast.error(`Cleanup failed: ${String(err)}`);
    } finally {
      setPruning(false);
    }
  };

  const handleReset = (): void => {
    setDebounce('5');
    setCooldown('15');
    const defaults: RetentionSettings | undefined = retention === undefined ? undefined : {
      ...retention,
      ...RETENTION_PRESETS.balanced,
      automatic: false,
      branch_expiry_days: 30,
      max_bytes: null,
    };
    void Promise.all([updateMonitoringSettings({
      snapshot_debounce_seconds: 5,
      version_cooldown_minutes: 15,
    }), defaults === undefined ? undefined : updatePruningSettings(defaults).then(applyRetention)])
      .then(() => {
        toast.success('Settings reset to defaults');
      })
      .catch((err: unknown) => {
        console.error(err);
        toast.error('Failed to reset settings');
      });
  };

  return (
    <div className="flex flex-col h-full bg-background overflow-hidden">
      <header className="p-8 pb-4">
        <h1 className="text-3xl font-black tracking-tight text-foreground">Settings</h1>
        <p className="mt-1 text-sm text-muted-foreground">Configure how Worldline watches save folders and creates checkpoints.</p>
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
                    step={0.5}
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

        <ImportHistorySection />

        <section className="flex flex-col gap-4">
          <div className="flex items-center gap-2 px-1">
            <Clock data-icon="inline-start" />
            <h2 className="text-xs font-bold uppercase tracking-wider text-muted-foreground/70">History Cleanup</h2>
          </div>
          <SettingsCard>
            <div className="flex flex-col gap-3">
              <div>
                <h3 className="text-sm font-bold">Smart Retention</h3>
                <p className="mt-0.5 text-xs text-muted-foreground">
                  Older checkpoints are thinned out: all recent ones are kept, then one per hour, day, week and month.
                  Tagged checkpoints, the start of each slot and the checkpoint every device is on are always kept.
                  Retired checkpoints stay in the timeline but can no longer be restored.
                </p>
              </div>
              {retention !== undefined ? (
                <RetentionEditor
                  retention={retention}
                  onChange={setRetention}
                  budgetMiB={budgetMiB}
                  onBudgetChange={setBudgetMiB}
                  branchDays={branchDays}
                  onBranchDaysChange={setBranchDays}
                />
              ) : null}
              {pruningPreview !== undefined ? (
                <p className="text-xs text-muted-foreground">
                  {pruningPreview.retire_versions.length} checkpoint(s) would be retired · stored history {formatBytes(pruningPreview.live_bytes_before)} → {formatBytes(pruningPreview.live_bytes_after)} · {formatBytes(pruningPreview.local_gc.bytes_deleted)} already reclaimable
                  {pruningPreview.budget_unreachable ? ' · the disk budget cannot be met' : ''}
                </p>
              ) : null}
              <div className="flex gap-2">
                <Button variant="outline" disabled={pruning} onClick={() => { void handlePreviewPruning(); }}>Preview Cleanup</Button>
                <Button disabled={pruning} onClick={() => { void handleRunPruning(); }}>{pruning ? 'Working…' : 'Run Cleanup Now'}</Button>
              </div>
            </div>
          </SettingsCard>
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
                          {device.device_uuid !== deviceUuid && Date.now() - new Date(device.last_seen).getTime() > STALE_DEVICE_MS
                            ? ' · not seen for 90+ days: remove it if it is gone, so cleanup can reclaim its old checkpoints'
                            : ''}
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
          Changes apply immediately after saving.
        </p>
      </div>
      </ScrollArea>
    </div>
  );
}

interface RetentionEditorProps {
  retention: RetentionSettings;
  onChange: (settings: RetentionSettings) => void;
  budgetMiB: string;
  onBudgetChange: (value: string) => void;
  branchDays: string;
  onBranchDaysChange: (value: string) => void;
}

type TierKey = 'keep_all_hours' | 'hourly_until_hours' | 'daily_until_days' | 'weekly_until_days';

const PRESET_LABELS: Record<RetentionPreset, string> = { dense: 'Dense', balanced: 'Balanced', sparse: 'Sparse' };
const TIERS: { key: TierKey; label: string }[] = [
  { key: 'keep_all_hours', label: 'Keep all (hours)' },
  { key: 'hourly_until_hours', label: 'Hourly until (hours)' },
  { key: 'daily_until_days', label: 'Daily until (days)' },
  { key: 'weekly_until_days', label: 'Weekly until (days)' },
];

function RetentionEditor({
  retention, onChange, budgetMiB, onBudgetChange, branchDays, onBranchDaysChange,
}: RetentionEditorProps): ReactElement {
  const preset = presetOf(retention);
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-4 rounded-lg bg-el p-3">
        <div>
          <label htmlFor="automatic-pruning" className="text-sm font-semibold">Automatic cleanup</label>
          <p className="text-xs text-muted-foreground">Runs after syncing, at most every four hours, locally and in the sync folder.</p>
        </div>
        <Switch
          id="automatic-pruning"
          checked={retention.automatic}
          onCheckedChange={(automatic) => { onChange({ ...retention, automatic }); }}
        />
      </div>
      <div className="flex flex-wrap items-center gap-2">
        {(Object.keys(RETENTION_PRESETS) as RetentionPreset[]).map(name => (
          <Button
            key={name}
            size="sm"
            variant={preset === name ? 'default' : 'outline'}
            onClick={() => { onChange({ ...retention, ...RETENTION_PRESETS[name] }); }}
          >
            {PRESET_LABELS[name]}
          </Button>
        ))}
        <Badge variant={preset === 'custom' ? 'default' : 'secondary'}>Custom</Badge>
      </div>
      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        {TIERS.map(({ key, label }) => (
          <label key={key} className="flex flex-col gap-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/70">
            {label}
            <Input
              type="number"
              min={0}
              step={1}
              value={retention[key].toString()}
              onChange={(event) => { onChange({ ...retention, [key]: Math.max(0, Math.floor(Number(event.target.value) || 0)) }); }}
              className="bg-el border-border/40"
            />
          </label>
        ))}
      </div>
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
        <label className="flex flex-col gap-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/70">
          Disk budget (MiB, empty for none)
          <Input
            type="number"
            min={1}
            value={budgetMiB}
            onChange={(event) => { onBudgetChange(event.target.value); }}
            className="bg-el border-border/40"
          />
        </label>
        <label className="flex flex-col gap-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/70">
          Retire idle side branches after (days, empty for never)
          <Input
            type="number"
            min={1}
            value={branchDays}
            onChange={(event) => { onBranchDaysChange(event.target.value); }}
            className="bg-el border-border/40"
          />
        </label>
      </div>
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
