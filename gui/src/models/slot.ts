import { z } from 'zod';

export const SlotSchema = z.object({
  id: z.number(),
  name: z.string(),
  active_version_id: z.number().nullable().optional(),
});

export class Slot {
  id!: number;
  active_version_id?: number;

  constructor(public name: string) {}

  static fromData(data: unknown): Slot | undefined {
    const result = SlotSchema.safeParse(data);
    if (!result.success) {
      console.error('Failed to parse Slot data:', result.error);
      return undefined;
    }

    const slot = new Slot(result.data.name);
    slot.id = result.data.id;
    slot.active_version_id = result.data.active_version_id ?? undefined;
    return slot;
  }
}
