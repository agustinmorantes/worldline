// Points out the playthrough another device brought in when this game was linked: its progress
// lives in another slot, not in the one this device kept playing. Stays until switched to or dismissed.
import type { ReactElement } from 'react';
import type { DeviceKind } from '@/ipc/bindings/DeviceKind';
import type { Slot } from '@/ipc/bindings/Slot';
import type { SlotSummary } from '@/ipc/bindings/SlotSummary';
import { utcDate } from '@/ipc/time';
import { formatRelative, useLocale, useT } from '@/shared/i18n';
import { DeviceGlyph } from '@/shared/ui/DeviceGlyph';
import { btn, btnQuiet } from '@/shared/ui/classes';
import { slotName } from './game-view';

export function LinkedPlaythroughNotice({ slot, summary, device, deviceKind, busy, onSwitch, onDismiss }: {
  slot: Slot;
  summary: SlotSummary | undefined;
  /** The device that played it, when known. */
  device: string | null;
  deviceKind: DeviceKind;
  busy: boolean;
  onSwitch: () => void;
  onDismiss: () => void;
}): ReactElement {
  const t = useT();
  const locale = useLocale();
  const name = slotName(slot.name, t.game.playthroughName(1));
  const last = summary?.last_created_at ?? null;
  const meta = last === null ? null : t.game.slotMeta(summary?.version_count ?? 0, formatRelative(utcDate(last), locale));

  return (
    <div role="status" className="mx-8 mb-4 flex items-center gap-4 rounded-2xl border-[1.5px] border-dashed border-deck bg-[oklch(80%_0.11_205/0.06)] px-4 py-3">
      <span className="flex min-w-0 flex-1 flex-col gap-1.5">
        <span className="flex items-center gap-2 text-sm font-bold text-deck">
          <DeviceGlyph kind={deviceKind} size={16} />
          <span className="truncate">{t.game.linked.title(device)}</span>
        </span>
        <span className="truncate font-mono text-xs text-tx2">{meta === null ? name : `${name} · ${meta}`}</span>
      </span>
      <button type="button" className={btnQuiet} onClick={onDismiss}>{t.game.linked.dismiss}</button>
      <button type="button" className={btn} disabled={busy} onClick={onSwitch}>{t.game.linked.switch}</button>
    </div>
  );
}
