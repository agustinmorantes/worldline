// Language, background tone and motion: GUI preferences, applied as soon as they change.
import type { ReactElement } from 'react';
import { useT } from '@/shared/i18n';
import { setPref, usePrefs } from '@/shared/prefs/prefs';
import type { Tone } from '@/shared/prefs/schema';
import { Card, Choice } from './controls';

/** Each tone's window background (`--bg` in `tokens.css`). */
const SWATCHES: Record<Tone, string> = {
  slate: 'oklch(15.5% 0.012 265)',
  black: 'oklch(4.5% 0 265)',
  warm: 'oklch(15.5% 0.0144 55)',
};

export function AppearanceSection(): ReactElement {
  const t = useT();
  const prefs = usePrefs();

  return (
    <Card title={t.settings.appearance}>
      <div className="mt-4 flex flex-col gap-3.5">
        <Choice
          label={t.settings.language}
          value={prefs.language}
          // Language names stay in their own language.
          options={[
            { value: 'system', label: t.settings.system },
            { value: 'en', label: 'English' },
            { value: 'es', label: 'Español' },
          ]}
          onChange={(language) => { setPref('language', language); }}
        />
        <Choice
          label={t.settings.tone}
          value={prefs.tone}
          options={(['slate', 'black', 'warm'] as const).map((tone) => ({
            value: tone,
            label: t.settings.tones[tone],
            icon: <span className="size-3 rounded-full shadow-[inset_0_0_0_1px_oklch(100%_0_0/0.25)]" style={{ background: SWATCHES[tone] }} />,
          }))}
          onChange={(tone) => { setPref('tone', tone); }}
        />
        <Choice
          label={t.settings.motion}
          value={prefs.motion}
          options={(['system', 'full', 'reduced'] as const).map((motion) => ({ value: motion, label: t.settings.motions[motion] }))}
          onChange={(motion) => { setPref('motion', motion); }}
        />
      </div>
    </Card>
  );
}
