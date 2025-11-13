import {Game} from "@/models/game.ts";
import {invoke} from "@tauri-apps/api/core";
import {InstallScannerItem, InstallScannerItemSchema} from "@/models/install_scanner_item.ts";

export async function getGames(): Promise<Game[]> {
  const response = await invoke("get_games");
  if(!Array.isArray(response)) throw new Error("Invalid response")
  
  return response
    .map(e => Game.fromData(e))
    .filter(g => g !== undefined)
}

export async function getGame(id: number): Promise<Game|undefined> {
  const response = await invoke("get_game", { id });
  return Game.fromData(response);
}

export async function saveGame(game: Game): Promise<Game> {
  const tracked_dirs = game.tracked_dirs?.map(t => t.path);
  const gameCopy = Object.assign(new Game(""), game);
  gameCopy.tracked_dirs = undefined;
    
  return await invoke("save_game", {game: gameCopy, dirs: tracked_dirs});
}

export async function deleteGame(id?: number): Promise<void> {
  if(id === undefined) return;
  await invoke("delete_game", {id});
}

export async function scanInstalled(): Promise<InstallScannerItem[]> {
  const res = await invoke("scan_installed");
  if(!Array.isArray(res)) throw new Error("Invalid response")
  return res.map(e => InstallScannerItemSchema.parse(e))
}
