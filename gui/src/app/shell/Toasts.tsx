import type { CSSProperties, ReactElement } from 'react';
import { Toaster } from 'sonner';

/** sonner, restyled on the design tokens. */
export function Toasts(): ReactElement {
  return (
    <Toaster
      theme="dark"
      position="bottom-right"
      gap={10}
      offset={20}
      style={{
        '--normal-bg': 'var(--el)',
        '--normal-border': 'transparent',
        '--normal-text': 'var(--tx)',
        '--border-radius': '16px',
      } as CSSProperties}
      toastOptions={{
        classNames: {
          toast: 'font-sans !gap-3 !px-4 !py-3.5 shadow-[0_24px_60px_oklch(0%_0_0/0.55),inset_0_0_0_1px_oklch(100%_0_0/0.06)]',
          title: '!text-sm !font-semibold',
          description: '!text-[13px] !text-tx2',
          icon: 'text-tx2',
          error: '[&_[data-icon]]:!text-err',
          warning: '[&_[data-icon]]:!text-warn',
          success: '[&_[data-icon]]:!text-live',
          actionButton: '!h-7 !rounded-lg !bg-ln !px-2.5 !text-xs !font-bold !text-tx transition-transform active:scale-95',
          cancelButton: '!h-7 !rounded-lg !bg-transparent !px-2.5 !text-xs !font-semibold !text-tx2',
          closeButton: '!border-none !bg-ln !text-tx2',
        },
      }}
    />
  );
}
