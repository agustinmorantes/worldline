// Settings: Capture and Keep on the left, Cloud and Appearance on the right. Every change
// applies at once; there is no Save button.
import type { ReactElement } from 'react';
import { useT } from '@/shared/i18n';
import { AppearanceSection } from './AppearanceSection';
import { CaptureSection } from './CaptureSection';
import { CloudSection } from './CloudSection';
import { KeepSection } from './KeepSection';

export function SettingsView(): ReactElement {
  const t = useT();
  return (
    <div className="h-full overflow-y-auto px-8 pb-10 pt-5">
      <h1 className="font-display text-[34px] font-extrabold leading-[1.15]">{t.settings.title}</h1>
      <div className="mt-8 flex flex-wrap items-start gap-4">
        <div className="flex min-w-[420px] max-w-[640px] flex-[3_1_420px] flex-col gap-4">
          <CaptureSection />
          <KeepSection />
        </div>
        <div className="flex min-w-[340px] max-w-[460px] flex-[2_1_340px] flex-col gap-4">
          <CloudSection />
          <AppearanceSection />
        </div>
      </div>
    </div>
  );
}
