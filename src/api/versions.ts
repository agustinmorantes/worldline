import { Version } from "@/models/version";
import {invoke} from "@tauri-apps/api/core";

export async function restoreVersion(id: number): Promise<void> {
  await invoke("restore_version", {id});
}

export async function tagVersion(id: number, tag?: string): Promise<Version|undefined> {
  const response = await invoke("tag_version", { id, tag });
  return Version.fromData(response)
}
