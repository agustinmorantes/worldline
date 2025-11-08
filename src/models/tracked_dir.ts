import { z } from 'zod';

export const TrackedDirSchema = z.object({
  id: z.number().optional(),
  game_id: z.number().optional(),
  path: z.string(),
});

export type TrackedDir = z.infer<typeof TrackedDirSchema>;
