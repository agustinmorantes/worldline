// The GUI shows its own localized message; the raw error goes to the log file
// and behind the toast's "Copy details".
import { toast } from 'sonner';
import { error as logError } from '@tauri-apps/plugin-log';
import { getT } from '@/shared/i18n';

/** The raw error as text: Tauri command errors are strings already. */
export function errorDetails(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return [`${error.name}: ${error.message}`, error.stack].filter(Boolean).join('\n');
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

/** `hint`, when given, says what to do about it under the message. */
export function reportError(message: string, error: unknown, hint?: string): void {
  const details = errorDetails(error);
  void logError(`${message}: ${details}`).catch(() => undefined);
  toast.error(message, {
    description: hint,
    action: {
      label: getT().errors.copyDetails,
      onClick: () => { void navigator.clipboard.writeText(details); },
    },
  });
}
