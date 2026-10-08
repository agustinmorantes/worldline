// Class strings for the small controls every screen shares (`.chip`, `.btn`, … in the mocks).

/** A pill button: header chips, menu triggers. */
export const chip =
  'inline-flex h-[30px] items-center gap-[7px] whitespace-nowrap rounded-full bg-el px-3 text-[13px] font-semibold text-tx2 transition-[background-color,color,transform] duration-200 ease-spring hover:bg-ln active:scale-[0.94] disabled:opacity-60';
/** The main action: accent fill. */
export const btn =
  'inline-flex h-[38px] shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-xl bg-acc px-[18px] text-sm font-bold text-acc-ink transition-[transform,filter,opacity] duration-[180ms] ease-spring hover:-translate-y-px hover:brightness-110 active:scale-95 disabled:pointer-events-none disabled:opacity-40';
/** A secondary action next to `btn`. */
export const btnQuiet =
  'inline-flex h-[38px] shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-xl bg-el px-4 text-sm font-semibold text-tx2 transition-[background-color,color,transform] duration-200 ease-spring hover:bg-ln hover:text-tx active:scale-95 disabled:pointer-events-none disabled:opacity-40';
/** Small uppercase section label. */
export const eyebrow = 'text-[11px] font-bold uppercase tracking-[0.12em] text-tx3';
export const popover =
  'z-50 rounded-2xl bg-el p-1.5 shadow-[0_24px_60px_oklch(0%_0_0/0.55),inset_0_0_0_1px_oklch(100%_0_0/0.06)] data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95';
/** A menu item inside `popover`. */
export const row =
  'flex min-h-10 w-full cursor-default items-center gap-2.5 rounded-[10px] px-2.5 py-2 text-left text-sm font-semibold outline-none transition-colors duration-150 data-[highlighted]:bg-[oklch(100%_0_0/0.06)] data-[disabled]:opacity-50';
/** A text field. */
export const field =
  'h-10 w-full min-w-0 rounded-xl bg-el px-3 text-sm font-semibold text-tx outline-none transition-shadow duration-200 placeholder:font-normal placeholder:text-tx3 focus:shadow-[inset_0_0_0_1.5px_var(--acc-line)]';
