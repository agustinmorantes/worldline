// Settings: Capture, In-game popup and Recently deleted on the left; Cloud, Appearance and Keep
// on the right. Every change applies at once; there is no Save button.
import { type ReactElement, useRef } from 'react';
import { useT } from '@/shared/i18n';
import { AppearanceSection } from './AppearanceSection';
import { CaptureSection } from './CaptureSection';
import { CloudSection } from './CloudSection';
import { KeepSection } from './KeepSection';
import { OverlaySection } from './OverlaySection';
import { RecentlyDeletedSection } from './RecentlyDeletedSection';

export function SettingsView(): ReactElement {
  const t = useT();
  // Where the focus goes when the last Recently deleted row leaves.
  const headingRef = useRef<HTMLHeadingElement>(null);
  return (
    <div className="h-full overflow-y-auto px-8 pb-10 pt-5">
      <h1 ref={headingRef} tabIndex={-1} className="font-display text-[34px] font-extrabold leading-[1.15] outline-none">{t.settings.title}</h1>
      <div className="mt-8 flex flex-wrap items-start gap-4">
        <div className="flex min-w-[420px] max-w-[640px] flex-[3_1_420px] flex-col gap-4">
          <CaptureSection />
          <OverlaySection />
          <RecentlyDeletedSection fallbackFocus={headingRef} />
        </div>
        <div className="flex min-w-[340px] max-w-[460px] flex-[2_1_340px] flex-col gap-4">
          <CloudSection />
          <AppearanceSection />
          <KeepSection />
        </div>
      </div>
    </div>
  );
}
