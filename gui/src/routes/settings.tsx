import { createFileRoute } from '@tanstack/react-router';
import { ReactElement, useState, useEffect } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Slider } from '@/components/ui/slider';
import { Switch } from '@/components/ui/switch';
import { Bell, Database, Clock, HardDrive } from 'lucide-react';
import { appLocalDataDir } from '@tauri-apps/api/path';
import { toast } from 'sonner';
import { ScrollArea } from '@/components/ui/scroll-area';

export const Route = createFileRoute('/settings')({
  component: SettingsView,
});

function SettingsView(): ReactElement {
  const [debounce, setDebounce] = useState(() => {
    const saved = localStorage.getItem('setting-debounce');
    return saved !== null ? [parseInt(saved, 10)] : [2];
  });
  const [maxVersions, setMaxVersions] = useState(() => {
    const saved = localStorage.getItem('setting-max-versions');
    return saved !== null ? [parseInt(saved, 10)] : [50];
  });
  const [notifications, setNotifications] = useState(() => {
    return localStorage.getItem('setting-notifications') !== 'false';
  });
  const [storagePath, setStoragePath] = useState('');

  useEffect(() => {
    void appLocalDataDir().then(dir => {
      setStoragePath(dir + 'repository');
    });
  }, []);

  const handleSave = (): void => {
    localStorage.setItem('setting-debounce', (debounce[0] ?? 2).toString());
    localStorage.setItem('setting-max-versions', (maxVersions[0] ?? 50).toString());
    localStorage.setItem('setting-notifications', notifications.toString());
    toast.success('Settings saved');
  };

  const handleReset = (): void => {
    setDebounce([2]);
    setMaxVersions([50]);
    setNotifications(true);
    localStorage.removeItem('setting-debounce');
    localStorage.removeItem('setting-max-versions');
    localStorage.removeItem('setting-notifications');
    toast.success('Settings reset to defaults');
  };

  return (
    <div className="flex flex-col h-full bg-background overflow-hidden">
      <header className="p-8 pb-4">
        <h1 className="text-3xl font-black tracking-tight text-foreground">Settings</h1>
        <p className="text-sm text-muted-foreground mt-1">Configure how Timeline monitors and manages your save files.</p>
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
              <div className="flex items-center justify-between">
                <div>
                  <h3 className="text-sm font-bold">Snapshot Debounce</h3>
                  <p className="text-xs text-muted-foreground mt-0.5">Wait {debounce[0]?.toString() ?? '2'} seconds after a file change before creating a snapshot.</p>
                </div>
                <div className="w-48 px-2">
                  <Slider value={debounce} onValueChange={setDebounce} max={30} min={1} step={1} />
                </div>
              </div>
            </SettingsCard>
          </div>
        </section>

        <section className="space-y-4">
          <div className="flex items-center gap-2 px-1">
            <HardDrive size={16} className="text-primary" />
            <h2 className="text-xs font-bold uppercase tracking-wider text-muted-foreground/70">Storage</h2>
          </div>

          <div className="grid gap-4">
            <SettingsCard>
              <div className="flex items-center justify-between">
                <div>
                  <h3 className="text-sm font-bold">Max Versions</h3>
                  <p className="text-xs text-muted-foreground mt-0.5">Automatically delete oldest snapshots when count exceeds {maxVersions[0]?.toString() ?? '50'}.</p>
                </div>
                <div className="w-48 px-2">
                  <Slider value={maxVersions} onValueChange={setMaxVersions} max={200} min={5} step={5} />
                </div>
              </div>
            </SettingsCard>

            <SettingsCard>
              <div className="flex flex-col gap-3">
                <h3 className="text-sm font-bold">Storage Path</h3>
                <div className="flex gap-2">
                  <div className="relative flex-1">
                    <Database size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground/40" />
                    <Input
                      readOnly
                      value={storagePath}
                      placeholder="Loading..."
                      className="bg-el border-border/40 pl-9 text-xs"
                    />
                  </div>
                </div>
              </div>
            </SettingsCard>
          </div>
        </section>

        <section className="space-y-4">
          <div className="flex items-center gap-2 px-1">
            <Bell size={16} className="text-primary" />
            <h2 className="text-xs font-bold uppercase tracking-wider text-muted-foreground/70">Interface</h2>
          </div>

          <div className="grid gap-4">
            <SettingsCard>
              <div className="flex items-center justify-between">
                <div>
                  <h3 className="text-sm font-bold">Desktop Notifications</h3>
                  <p className="text-xs text-muted-foreground mt-0.5">Show a toast when a new version is automatically created.</p>
                </div>
                <Switch checked={notifications} onCheckedChange={setNotifications} />
              </div>
            </SettingsCard>
          </div>
        </section>

        <div className="pt-8 flex justify-end gap-3">
          <Button variant="ghost" onClick={handleReset}>Reset Defaults</Button>
          <Button className="font-bold px-8" onClick={handleSave}>Save Settings</Button>
        </div>
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
