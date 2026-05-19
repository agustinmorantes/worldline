import {z} from "zod";

export const GameSourceSchema = z.enum(["steam", "epic", "local", "custom"]);

export const InstallScannerItemSchema = z.object({
  title: z.string(),
  dirs: z.array(z.string()),
  install_path: z.string().nullable().optional(),
  icon_path: z.string().nullable().optional(),
  last_modified_at: z.number().nullable().optional(),
  source: GameSourceSchema,
});

export type InstallScannerItem = z.infer<typeof InstallScannerItemSchema>;
export type GameSource = z.infer<typeof GameSourceSchema>;
