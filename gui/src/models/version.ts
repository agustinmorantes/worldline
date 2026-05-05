import {z} from "zod";
import {tagVersion,restoreVersion} from "@/api/versions.ts";

export const VersionSchema = z.object({
  id: z.number().optional().nullable(),
  version_number: z.number(),
  game_id: z.number(),
  tag: z.string().optional().nullable(),
  created_at: z.string().optional()
    .transform(s => s !== undefined ? new Date(s + 'Z') : undefined),
  parent_id: z.number().optional().nullable(),
});

export class Version {
  id?: number;
  tag?: string;
  created_at?: Date;
  parent_id?: number;
  
  constructor(
    public version_number: number,
    public game_id: number,
  ) {}
  
  static fromData(data: unknown): Version|undefined {
    const result = VersionSchema.safeParse(data);
    if (!result.success) {
      console.error('Failed to parse Version data:', result.error);
      return undefined;
    }

    const parsedData = result.data;
    
    const version = new Version(parsedData.version_number, parsedData.game_id);
    version.id = parsedData.id ?? undefined;
    version.created_at = parsedData.created_at;
    version.tag = parsedData.tag ?? undefined;
    version.parent_id = parsedData.parent_id ?? undefined;
    return version;
  }
  
  async restore(): Promise<void> {
    if(this.id === undefined) return;
    await restoreVersion(this.id);
  }
  
  async save(): Promise<void> {
    if(this.id === undefined) return;
    await tagVersion(this.id, this.tag);
  }
}
