import { z } from 'zod';

export const SlotSchema = z.object({
  id: z.number(),
  uuid: z.string(),
  game_id: z.number(),
  name: z.string(),
  name_normalized: z.string(),
  folder_slug: z.string(),
  active_version_id: z.number().nullable().optional(),
});

export class Slot {
  id!: number;
  uuid!: string;
  game_id!: number;
  name_normalized!: string;
  folder_slug!: string;
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
    slot.uuid = result.data.uuid;
    slot.game_id = result.data.game_id;
    slot.name_normalized = result.data.name_normalized;
    slot.folder_slug = result.data.folder_slug;
    slot.active_version_id = result.data.active_version_id ?? undefined;
    return slot;
  }
}
