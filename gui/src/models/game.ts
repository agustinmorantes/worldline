import { TrackedDir, TrackedDirSchema } from "@/models/tracked_dir.ts";
import { deleteGame, saveGame } from "@/api/games.ts";
import { z } from 'zod';
import {Version} from "@/models/version.ts";
import { Slot, SlotSchema } from '@/models/slot.ts';

export const GameSchema = z.object({
  id: z.number().optional(),
  title: z.string(),
  active_version_id: z.number().nullable().optional(),
  active_slot_id: z.number().nullable().optional(),
  icon_path: z.string().nullable().optional(),
  tracked_dirs: z.array(TrackedDirSchema).optional(),
  slots: z.array(SlotSchema).optional(),
  active_slot: SlotSchema.nullable().optional(),
  versions: z.array(z.unknown()).optional(),
});

export class Game {
  id?: number
  active_version_id?: number
  active_slot_id?: number
  icon_path?: string
  tracked_dirs?: TrackedDir[]
  slots?: Slot[];
  active_slot?: Slot;
  versions?: Version[];

  constructor(
    public title: string,
    tracked_dirs?: string[],
  ) {
    this.title = title;
    this.tracked_dirs = tracked_dirs?.map(d => ({ path: d }))
  }
  
  static fromData(data: unknown): Game | undefined {
    const result = GameSchema.safeParse(data);
    if (!result.success) {
      console.error('Failed to parse Game data:', result.error);
      return undefined;
    }
    
    const parsedData = result.data;
    const game = new Game(parsedData.title);
    game.id = parsedData.id;
    game.active_version_id = parsedData.active_version_id ?? undefined;
    game.active_slot_id = parsedData.active_slot_id ?? undefined;
    game.icon_path = parsedData.icon_path ?? undefined;
    game.tracked_dirs = parsedData.tracked_dirs;
    game.slots = parsedData.slots
      ?.map(slot => Slot.fromData(slot))
      .filter((slot): slot is Slot => slot !== undefined);
    game.active_slot = parsedData.active_slot ? Slot.fromData(parsedData.active_slot) : undefined;
    game.versions = parsedData.versions
      ?.map(v => Version.fromData(v))
      .filter(v => v !== undefined);
    return game;
  }
  
  save(): Promise<Game> {
    return saveGame(this);
  }

  delete(): Promise<void> {
    return deleteGame(this.id);
  }
}

export type FullGame = Game & {
  id: number;
  tracked_dirs: TrackedDir[];
  slots: Slot[];
  versions: Version[];
};
