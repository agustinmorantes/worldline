// The cloud: with sync off, a setup (choose the folder, then name this device); with sync on,
// where it is stored and the devices using it.
import { type ReactElement, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { FolderClosed, Pencil, Trash2 } from 'lucide-react';
import type { DeviceKind } from '@/ipc/bindings/DeviceKind';
import type { DeviceMetadata } from '@/ipc/bindings/DeviceMetadata';
import { confirm, pickFolder, removeSyncDevice, updateDevice, updateSyncSettings } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import { useSyncReport, useSyncSettings } from '@/features/library/queries';
import { useSyncRunning } from '@/features/library/sync-activity';
import { useNow } from '@/features/library/useNow';
import { reportError } from '@/shared/errors/report';
import { formatRelative, useLocale, useT } from '@/shared/i18n';
import { ease, useMotionReduced } from '@/shared/motion/motion';
import { btn, btnQuiet, chip, eyebrow, field } from '@/shared/ui/classes';
import { DeviceGlyph } from '@/shared/ui/DeviceGlyph';
import { cn } from '@/shared/ui/utils';
import { Card } from './controls';
import { useDevice } from './queries';
import { osName } from './settings';

const KINDS: DeviceKind[] = ['desktop', 'laptop', 'handheld'];

export function CloudSection(): ReactElement {
  const t = useT();
  const reduced = useMotionReduced();
  const { data: sync } = useSyncSettings();
  const { data: device } = useDevice();
  const remote = sync?.remote_path ?? null;
  const on = remote !== null && remote !== '';

  return (
    <Card title={t.settings.cloud}>
      {sync !== undefined && device !== undefined && (
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={on ? 'on' : 'setup'}
            initial={reduced ? false : { opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={reduced ? undefined : { opacity: 0, y: -6, transition: { duration: 0.18 } }}
            transition={{ duration: 0.36, ease }}
          >
            {on ? <CloudOn remote={remote} device={device} /> : <CloudSetup device={device} />}
          </motion.div>
        </AnimatePresence>
      )}
    </Card>
  );
}

function StoredIn({ path, action }: { path: string; action?: ReactElement }): ReactElement {
  const t = useT();
  return (
    <div className="mt-4 flex items-center gap-3 rounded-[14px] bg-el px-3.5 py-3">
      <FolderClosed size={20} strokeWidth={1.8} aria-hidden="true" className="shrink-0" />
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="text-xs text-tx3">{t.settings.storedIn}</span>
        <span className="truncate font-mono text-[12.5px]" title={path}>{path}</span>
      </span>
      {action}
    </div>
  );
}

function CloudSetup({ device }: { device: DeviceMetadata }): ReactElement {
  const t = useT();
  const [folder, setFolder] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function choose(): Promise<void> {
    const path = await pickFolder(t.settings.pickCloudFolder);
    if (path !== null) setFolder(path);
  }

  async function turnOn(name: string, kind: DeviceKind): Promise<void> {
    if (folder === null) return;
    setBusy(true);
    try {
      queryClient.setQueryData(['device'], await updateDevice(name, kind));
      await updateSyncSettings({ remote_path: folder });
      // Sync settings now name a folder, so the sync report query starts the first sync.
      await queryClient.invalidateQueries({ queryKey: ['sync-settings'] });
    } catch (err) {
      reportError(t.settings.failed.cloud, err);
      setBusy(false);
    }
  }

  if (folder === null) {
    return (
      <div className="mt-4 flex flex-col items-start gap-1.5 rounded-[14px] bg-el p-4">
        <FolderClosed size={22} strokeWidth={1.8} aria-hidden="true" className="mb-1 text-tx2" />
        <span className="text-[15px] font-bold">{t.settings.cloudEmpty}</span>
        <span className="text-[12.5px] text-tx3">{t.settings.cloudEmptyHint}</span>
        <button type="button" className={cn(btn, 'mt-3')} onClick={() => { void choose(); }}>{t.settings.chooseFolder}</button>
      </div>
    );
  }

  return (
    <>
      <StoredIn path={folder} />
      <h3 className={cn(eyebrow, 'mt-[22px]')}>{t.settings.thisDevice}</h3>
      <p className="mt-1 text-[12.5px] text-tx3">{t.settings.thisDeviceHint}</p>
      <DeviceForm
        device={device}
        confirm={t.settings.turnOn}
        cancel={t.settings.back}
        busy={busy}
        onCancel={() => { setFolder(null); }}
        onSave={(name, kind) => { void turnOn(name, kind); }}
      />
    </>
  );
}

/** This device's name and kind. */
function DeviceForm({ device, confirm: confirmLabel, cancel, busy, onCancel, onSave }: {
  device: DeviceMetadata;
  confirm: string;
  cancel: string;
  busy: boolean;
  onCancel: () => void;
  onSave: (name: string, kind: DeviceKind) => void;
}): ReactElement {
  const t = useT();
  const [name, setName] = useState(device.name);
  const [kind, setKind] = useState<DeviceKind>(device.kind ?? 'desktop');

  return (
    <form className="mt-3 flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); onSave(name.trim(), kind); }}>
      <label className="flex flex-col gap-1.5">
        <span className="text-[13px] font-semibold text-tx2">{t.settings.deviceName}</span>
        <input className={field} value={name} autoFocus onChange={(e) => { setName(e.target.value); }} />
      </label>
      <div role="radiogroup" aria-label={t.settings.deviceKind} className="grid grid-cols-3 gap-2">
        {KINDS.map((k) => (
          <button
            key={k}
            type="button"
            role="radio"
            aria-checked={kind === k}
            className={cn(
              'flex flex-col items-center gap-1.5 rounded-[14px] bg-el px-2 py-3 text-xs font-semibold text-tx2 transition-[box-shadow,color,transform] duration-200 ease-spring hover:text-tx active:scale-95',
              kind === k && 'text-tx shadow-[inset_0_0_0_1.5px_var(--acc-line)]',
            )}
            onClick={() => { setKind(k); }}
          >
            <span className={cn('flex', kind === k && 'text-acc')}><DeviceGlyph kind={k} size={22} /></span>
            {t.settings.kinds[k]}
          </button>
        ))}
      </div>
      <div className="flex justify-end gap-2">
        <button type="button" className={btnQuiet} onClick={onCancel}>{cancel}</button>
        <button type="submit" className={btn} disabled={busy || name.trim() === ''}>{confirmLabel}</button>
      </div>
    </form>
  );
}

function CloudOn({ remote, device }: { remote: string; device: DeviceMetadata }): ReactElement {
  const t = useT();
  const report = useSyncReport();
  const running = useSyncRunning();
  const syncing = running || report.isFetching;
  const others = report.data?.devices.filter((d) => d.device_uuid !== device.device_uuid) ?? [];

  const resync = async (failed: string): Promise<void> => {
    const result = await report.refetch();
    if (result.isError) reportError(failed, result.error);
  };

  async function change(): Promise<void> {
    const path = await pickFolder(t.settings.pickCloudFolder);
    if (path === null) return;
    try {
      await updateSyncSettings({ remote_path: path });
      await queryClient.invalidateQueries({ queryKey: ['sync-settings'] });
    } catch (err) {
      reportError(t.settings.failed.cloud, err);
      return;
    }
    await resync(t.sync.failed);
  }

  async function remove(other: DeviceMetadata): Promise<void> {
    if (!await confirm(t.settings.removeTitle, t.settings.removeBody(other.name))) return;
    try {
      await removeSyncDevice(other.device_uuid);
    } catch (err) {
      reportError(t.settings.failed.remove, err);
      return;
    }
    await resync(t.sync.failed);
  }

  return (
    <>
      <StoredIn
        path={remote}
        action={<button type="button" className={cn(chip, 'bg-ln')} onClick={() => { void change(); }}>{t.settings.change}</button>}
      />
      <h3 className={cn(eyebrow, 'mt-[22px]')}>{t.settings.devices}</h3>
      <ul className="mt-2.5 flex flex-col gap-2">
        <li><ThisDevice device={device} onSaved={() => { void resync(t.sync.failed); }} /></li>
        {others.map((other) => (
          <li key={other.device_uuid}><OtherDevice device={other} onRemove={() => { void remove(other); }} /></li>
        ))}
      </ul>
      <button
        type="button"
        className={cn(btn, 'mt-5')}
        disabled={syncing}
        onClick={() => { void resync(t.sync.failed); }}
      >
        {syncing ? t.sync.syncing : t.sync.syncNow}
      </button>
    </>
  );
}

function ThisDevice({ device, onSaved }: { device: DeviceMetadata; onSaved: () => void }): ReactElement {
  const t = useT();
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);

  async function save(name: string, kind: DeviceKind): Promise<void> {
    setBusy(true);
    try {
      queryClient.setQueryData(['device'], await updateDevice(name, kind));
      setEditing(false);
      onSaved();
    } catch (err) {
      reportError(t.settings.failed.device, err);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="rounded-[14px] px-3.5 py-3 shadow-[inset_0_0_0_1.5px_var(--acc-line)]">
      {editing ? (
        <DeviceForm
          device={device}
          confirm={t.settings.save}
          cancel={t.settings.cancel}
          busy={busy}
          onCancel={() => { setEditing(false); }}
          onSave={(name, kind) => { void save(name, kind); }}
        />
      ) : (
        <div className="flex items-center gap-3">
          <DeviceGlyph kind={device.kind ?? 'desktop'} size={20} />
          <span className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="truncate text-sm font-semibold">{device.name}</span>
            <span className="text-xs text-tx3">{t.settings.thisDeviceMeta(osName(device.platform))}</span>
          </span>
          <button type="button" className={cn(chip, 'w-[30px] justify-center px-0')} aria-label={t.settings.editDevice} title={t.settings.editDevice} onClick={() => { setEditing(true); }}>
            <Pencil size={14} aria-hidden="true" />
          </button>
        </div>
      )}
    </div>
  );
}

function OtherDevice({ device, onRemove }: { device: DeviceMetadata; onRemove: () => void }): ReactElement {
  const t = useT();
  const locale = useLocale();
  const now = useNow(60_000);
  return (
    <div className="flex items-center gap-3 rounded-[14px] bg-el px-3.5 py-3">
      <span className="flex text-deck"><DeviceGlyph kind={device.kind ?? 'desktop'} size={20} /></span>
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="truncate text-sm font-semibold">{device.name}</span>
        <span className="text-xs text-tx3">
          {t.settings.seen(osName(device.platform), formatRelative(new Date(device.last_seen), locale, now))}
        </span>
      </span>
      <button
        type="button"
        className={cn(chip, 'w-[30px] justify-center bg-transparent px-0 hover:text-err')}
        aria-label={t.settings.removeDevice(device.name)}
        title={t.settings.removeDevice(device.name)}
        onClick={onRemove}
      >
        <Trash2 size={14} aria-hidden="true" />
      </button>
    </div>
  );
}
