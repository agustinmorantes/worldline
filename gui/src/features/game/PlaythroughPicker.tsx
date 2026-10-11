// The playthrough chip: pick a slot, or start a new one.
import type { ReactElement } from 'react';
import * as Menu from '@radix-ui/react-dropdown-menu';
import { Check, ChevronDown, Layers, Plus } from 'lucide-react';
import type { FullGame } from '@/ipc/bindings/FullGame';
import { useSyncReport } from '@/features/library/queries';
import { useDevice } from '@/features/sessions/queries';
import { formatRelative, useLocale, useT } from '@/shared/i18n';
import { utcDate } from '@/ipc/time';
import { chip, popover, row } from '@/shared/ui/classes';
import { slotDevice, slotName } from './game-view';

export function PlaythroughPicker({ game, disabled, onSwitch, onCreate }: {
  game: FullGame;
  disabled: boolean;
  onSwitch: (slotId: number) => void;
  onCreate: () => void;
}): ReactElement {
  const t = useT();
  const locale = useLocale();
  const { data: here } = useDevice();
  const report = useSyncReport();
  const summaries = new Map(game.slot_summaries.map((s) => [s.slot_id, s]));
  const first = t.game.playthroughName(1);

  return (
    <Menu.Root>
      <Menu.Trigger className={chip} aria-label={t.game.playthrough} disabled={disabled}>
        <Layers size={14} strokeWidth={2.2} aria-hidden="true" />
        <span className="text-tx">{game.active_slot === null ? t.game.playthrough : slotName(game.active_slot.name, first)}</span>
        <ChevronDown size={14} strokeWidth={2.4} aria-hidden="true" />
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Content align="start" sideOffset={6} className={`${popover} w-[300px]`}>
          <Menu.Label className="px-2.5 pb-1.5 pt-2 text-[11px] font-bold uppercase tracking-[0.12em] text-tx3">{t.game.playthroughs}</Menu.Label>
          <Menu.RadioGroup value={game.active_slot?.id.toString()} onValueChange={(value) => { onSwitch(Number(value)); }}>
            {game.slots.map((slot) => {
              const summary = summaries.get(slot.id);
              const on = slot.id === game.active_slot?.id;
              const device = slotDevice(summary, here, report.data);
              const meta = summary === undefined || summary.version_count === 0 || summary.last_created_at === null
                ? t.game.emptySlot
                : t.game.slotMeta(summary.version_count, formatRelative(utcDate(summary.last_created_at), locale))
                  + (device === null ? '' : ` · ${t.game.slotDevice(device)}`);
              return (
                <Menu.RadioItem key={slot.id} value={slot.id.toString()} className={row}>
                  <span aria-hidden="true" className="size-2 shrink-0 rounded-full" style={{ background: on ? 'var(--acc)' : 'var(--tx3)' }} />
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate">{slotName(slot.name, first)}</span>
                    <span className="text-xs font-medium text-tx3">{meta}</span>
                  </span>
                  <Menu.ItemIndicator><Check size={16} strokeWidth={2.6} className="text-acc" aria-hidden="true" /></Menu.ItemIndicator>
                </Menu.RadioItem>
              );
            })}
          </Menu.RadioGroup>
          <Menu.Separator className="mx-1 my-1.5 h-px bg-[oklch(100%_0_0/0.07)]" />
          <Menu.Item className={`${row} text-tx2`} onSelect={onCreate}>
            <Plus size={16} strokeWidth={2.4} aria-hidden="true" />
            {t.game.newPlaythrough}
          </Menu.Item>
        </Menu.Content>
      </Menu.Portal>
    </Menu.Root>
  );
}
