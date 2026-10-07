import { ReactElement, useState } from 'react';
import { FolderArchive, FolderOpen, History } from 'lucide-react';
import { toast } from 'sonner';
import { useQueryClient } from '@tanstack/react-query';
import { confirm, importGameHistory, inspectHistoryBundle, pickFolder } from '@/ipc/commands';
import type { HistoryBundleSummary } from '@/ipc/bindings/HistoryBundleSummary';
import { reportError } from '@/shared/errors/report';
import { Button } from '@/shared/ui/button';
import { Input } from '@/shared/ui/input';

export function ImportHistorySection(): ReactElement {
  const queryClient = useQueryClient();
  const [bundlePath, setBundlePath] = useState<string>();
  const [summary, setSummary] = useState<HistoryBundleSummary>();
  const [mappings, setMappings] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);

  async function chooseBundle(): Promise<void> {
    try {
      const selected = await pickFolder('Select History Bundle');
      if (selected === null) return;
      setBundlePath(undefined);
      setSummary(undefined);
      setMappings({});
      setBusy(true);
      const inspected = await inspectHistoryBundle(selected);
      setBundlePath(selected);
      setSummary(inspected);
      setMappings(Object.fromEntries(inspected.active_tracked_dirs.map(({ uuid }) => [uuid, ''])));
    } catch (error) {
      reportError('Could not inspect history bundle', error);
    } finally {
      setBusy(false);
    }
  }

  async function chooseDestination(uuid: string): Promise<void> {
    try {
      const selected = await pickFolder('Select Destination Save Directory');
      if (selected !== null) setMappings((current) => ({ ...current, [uuid]: selected }));
    } catch (error) {
      reportError('Could not select destination', error);
    }
  }

  async function importHistory(): Promise<void> {
    if (bundlePath === undefined || summary === undefined) return;
    try {
      if (!await confirm(
        'Import History',
        `Import the complete history for ${summary.title}? This imports history only; live save files and active slot/version pointers are left untouched.`,
      )) return;
      setBusy(true);
      const suppliedMappings = Object.fromEntries(
        Object.entries(mappings).filter(([, path]) => path.trim() !== ''),
      );
      const report = await importGameHistory(bundlePath, suppliedMappings);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['games'] }),
        queryClient.invalidateQueries({ queryKey: ['game'] }),
        queryClient.invalidateQueries({ queryKey: ['retired-versions'] }),
      ]);
      toast.success(`Imported ${report.version_count.toString()} version(s) for ${report.title}`);
      setBundlePath(undefined);
      setSummary(undefined);
      setMappings({});
    } catch (error) {
      reportError('History import failed', error);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="flex flex-col gap-4">
      <div className="flex items-center gap-2 px-1">
        <History size={16} className="text-primary" />
        <h2 className="text-xs font-bold uppercase tracking-wider text-muted-foreground/70">Import History</h2>
      </div>
      <div className="p-5 rounded-2xl bg-sf border border-border/40 shadow-sm space-y-4">
        <div>
          <h3 className="text-sm font-bold">Import History</h3>
          <p className="mt-0.5 text-xs text-muted-foreground">
            Inspect a standalone game-history bundle. Existing save paths are reused; map any new save directories for this device.
          </p>
        </div>
        <Button variant="outline" disabled={busy} onClick={() => { void chooseBundle(); }}>
          <FolderArchive data-icon="inline-start" /> Choose and Inspect Bundle
        </Button>
        {summary !== undefined ? (
          <div className="space-y-3 rounded-xl border border-border/40 bg-el p-4">
            <div>
              <p className="text-sm font-semibold">{summary.title}</p>
              <p className="text-xs text-muted-foreground">
                {summary.slot_count} slot(s) · {summary.version_count} version(s) · {summary.blob_count} blob(s) · {summary.retired_version_count} retired
              </p>
            </div>
            {summary.active_tracked_dirs.map((tracked) => (
              <div key={tracked.uuid} className="space-y-1">
                <label htmlFor={`history-map-${tracked.uuid}`} className="block text-xs font-semibold">
                  {tracked.label ?? 'Save directory'} · {tracked.uuid}
                </label>
                <p className="truncate text-[11px] text-muted-foreground">Source path hint: {tracked.source_path_hint}</p>
                <div className="flex gap-2">
                  <Input
                    id={`history-map-${tracked.uuid}`}
                    aria-label={`Destination for ${tracked.label ?? tracked.uuid}`}
                    value={mappings[tracked.uuid] ?? ''}
                    onChange={(event) => { setMappings((current) => ({ ...current, [tracked.uuid]: event.target.value })); }}
                    placeholder="Choose a destination directory"
                    className="bg-background border-border/40"
                  />
                  <Button
                    variant="outline"
                    aria-label={`Browse destination for ${tracked.label ?? tracked.uuid}`}
                    disabled={busy}
                    onClick={() => { void chooseDestination(tracked.uuid); }}
                  >
                    <FolderOpen data-icon="inline-start" /> Browse
                  </Button>
                </div>
              </div>
            ))}
            <p className="text-xs text-muted-foreground">
              Import does not restore or write live saves. Choose a version later if you want to restore it.
            </p>
            <Button disabled={busy} onClick={() => { void importHistory(); }}>
              {busy ? 'Working…' : 'Import History'}
            </Button>
          </div>
        ) : null}
      </div>
    </section>
  );
}
