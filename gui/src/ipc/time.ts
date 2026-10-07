/** Index timestamps (`created_at`) are naive UTC without an offset. */
export function utcDate(naive: string): Date {
  return new Date(`${naive}Z`);
}
