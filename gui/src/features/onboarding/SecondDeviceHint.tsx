// A one-time hint beside the first game that has progress from another device. Shown once ever
// (`prefs.hints.secondDevice`), never over the tour.
import { type ReactElement, useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { useT } from '@/shared/i18n';
import { getPrefs, setPref } from '@/shared/prefs/prefs';
import { DeviceGlyph } from '@/shared/ui/DeviceGlyph';
import { useTour } from './tour-context';

const SHOW_AFTER_MS = 600;
const SHOW_FOR_MS = 8000;

export function SecondDeviceHint({ anchor, device }: {
  /** `data-nav-anchor` of the game's sidebar icon. */
  anchor: string;
  device: string;
}): ReactElement | null {
  const t = useT();
  const { open: touring } = useTour();
  const [due, setDue] = useState(() => !getPrefs().hints.secondDevice);
  const [at, setAt] = useState<{ left: number; top: number } | null>(null);

  useEffect(() => {
    if (!due || touring) return;
    const show = window.setTimeout(() => {
      const row = document.querySelector(`[data-nav-anchor="${anchor}"]`)?.closest('a');
      if (row === null || row === undefined) return;
      const rect = row.getBoundingClientRect();
      setDue(false);
      setAt({ left: rect.right + 12, top: rect.top + rect.height / 2 });
      setPref('hints', { ...getPrefs().hints, secondDevice: true });
    }, SHOW_AFTER_MS);
    return () => { clearTimeout(show); };
  }, [due, touring, anchor]);

  useEffect(() => {
    if (at === null) return;
    const hide = (): void => { setAt(null); };
    const timer = window.setTimeout(hide, SHOW_FOR_MS);
    window.addEventListener('pointerdown', hide);
    return () => {
      clearTimeout(timer);
      window.removeEventListener('pointerdown', hide);
    };
  }, [at]);

  if (at === null || touring) return null;
  return createPortal(
    <div
      role="status"
      className="lk-tip fixed z-50 flex w-[260px] -translate-y-1/2 items-start gap-2.5 rounded-xl bg-el px-3 py-2.5 text-[13px] font-semibold leading-[1.45] text-tx shadow-[0_16px_40px_oklch(0%_0_0/0.5),inset_0_0_0_1px_oklch(100%_0_0/0.08)]"
      style={{ left: at.left, top: at.top }}
    >
      <span aria-hidden="true" className="absolute -left-1 top-1/2 size-2.5 -translate-y-1/2 rotate-45 bg-el" />
      <span className="mt-0.5 shrink-0 text-deck"><DeviceGlyph kind="handheld" /></span>
      {t.hints.secondDevice(device)}
    </div>,
    document.body,
  );
}
