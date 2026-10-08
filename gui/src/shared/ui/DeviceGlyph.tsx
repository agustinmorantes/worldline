import type { ReactElement } from 'react';
import type { DeviceKind } from '@/ipc/bindings/DeviceKind';

/** Desktop, laptop or handheld, as the user picked for the device. */
export function DeviceGlyph({ kind, size = 14 }: { kind: DeviceKind; size?: number }): ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="shrink-0">
      {kind === 'handheld' ? (
        <>
          <rect x="2" y="6" width="20" height="12" rx="4" />
          <path d="M6 12h4M8 10v4" />
          <circle cx="16" cy="11" r="1" />
          <circle cx="18" cy="13" r="1" />
        </>
      ) : kind === 'laptop' ? (
        <>
          <rect x="4" y="4" width="16" height="11" rx="2" />
          <path d="M2 19h20" />
        </>
      ) : (
        <>
          <rect x="2" y="3" width="20" height="14" rx="2" />
          <path d="M8 21h8M12 17v4" />
        </>
      )}
    </svg>
  );
}
