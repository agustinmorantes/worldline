import { useEffect, useState } from 'react';

/** The current time, refreshed every `intervalMs`, for "5 minutes ago" labels. */
export function useNow(intervalMs: number): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = window.setInterval(() => { setNow(new Date()); }, intervalMs);
    return () => { clearInterval(id); };
  }, [intervalMs]);
  return now;
}
