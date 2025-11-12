import {invoke} from "@tauri-apps/api/core";

export async function restoreVersion(id?: number): Promise<void> {
  if(id === undefined) return;
  await invoke("restore_version", {id});
}
