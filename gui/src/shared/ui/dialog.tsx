import type { ComponentProps, ReactElement } from 'react';
import { X } from 'lucide-react';
import { Dialog as DialogPrimitive } from 'radix-ui';
import { useT } from '@/shared/i18n';
import { cn } from '@/shared/ui/utils';

export const Dialog = DialogPrimitive.Root;

export function DialogContent({ className, children, ...props }: ComponentProps<typeof DialogPrimitive.Content>): ReactElement {
  const t = useT();
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay className="pointer-events-auto fixed inset-0 z-50 bg-black/50 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:animate-in data-[state=open]:fade-in-0" />
      <DialogPrimitive.Content
        className={cn(
          'pointer-events-auto fixed left-1/2 top-1/2 z-50 grid w-full max-w-[calc(100%-2rem)] -translate-x-1/2 -translate-y-1/2 gap-4 rounded-lg border border-ln/40 bg-sf p-6 shadow-lg outline-none duration-200 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95 data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95',
          className,
        )}
        {...props}
      >
        {children}
        <DialogPrimitive.Close
          aria-label={t.checkpoint.close}
          className="absolute right-4 top-4 rounded-sm text-tx2 transition-colors hover:text-tx focus-visible:outline-2 focus-visible:outline-acc"
        >
          <X size={16} aria-hidden="true" />
        </DialogPrimitive.Close>
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  );
}

export function DialogHeader({ className, ...props }: ComponentProps<'div'>): ReactElement {
  return <div className={cn('flex flex-col gap-2 text-left', className)} {...props} />;
}

export function DialogFooter({ className, ...props }: ComponentProps<'div'>): ReactElement {
  return <div className={cn('flex justify-end gap-2', className)} {...props} />;
}

export function DialogTitle({ className, ...props }: ComponentProps<typeof DialogPrimitive.Title>): ReactElement {
  return <DialogPrimitive.Title className={cn('text-lg font-semibold leading-none', className)} {...props} />;
}

export function DialogDescription({ className, ...props }: ComponentProps<typeof DialogPrimitive.Description>): ReactElement {
  return <DialogPrimitive.Description className={cn('text-sm text-tx2', className)} {...props} />;
}
