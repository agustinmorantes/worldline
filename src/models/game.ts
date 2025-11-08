import { TrackedDir, TrackedDirSchema } from "@/models/tracked_dir.ts";
import { deleteGame, saveGame } from "@/api/games.ts";
import { z } from 'zod';

export const GameSchema = z.object({
  id: z.number().optional(),
  title: z.string(),
  tracked_dirs: z.array(TrackedDirSchema).optional(),
});

export class Game {
  id?: number
  tracked_dirs?: TrackedDir[]
  
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
    game.tracked_dirs = parsedData.tracked_dirs;
    return game;
  }
  
  save(): Promise<Game> {
    return saveGame(this);
  }
  
  delete(): Promise<void> {
    return deleteGame(this.id);
  }
}
