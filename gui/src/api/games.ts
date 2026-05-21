import {Game, FullGame} from "@/models/game.ts";
import {invoke} from "@tauri-apps/api/core";
import {InstallScannerItem, InstallScannerItemSchema} from "@/models/install_scanner_item.ts";
import {z} from "zod";

export async function getGames(): Promise<FullGame[]> {
  const response = await invoke("get_games");
  if(!Array.isArray(response)) throw new Error("Invalid response")

  return response
    .map(e => Game.fromData(e))
    .filter((g): g is FullGame => g?.id !== undefined)
}

export async function getGame(id: number): Promise<FullGame|undefined> {
  const response = await invoke("get_game", { id });
  const game = Game.fromData(response);
  if (game?.id === undefined) return undefined;
  return game as FullGame;
}

export async function createSlot(gameId: number, name: string): Promise<FullGame> {
  const response = await invoke("create_slot", { gameId, name });
  const game = Game.fromData(response);
  if (game?.id === undefined) throw new Error('Invalid game returned from create_slot');
  return game as FullGame;
}

export async function switchSlot(gameId: number, slotId: number): Promise<FullGame> {
  const response = await invoke("switch_slot", { gameId, slotId });
  const game = Game.fromData(response);
  if (game?.id === undefined) throw new Error('Invalid game returned from switch_slot');
  return game as FullGame;
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

export async function setGameIcon(gameId: number, sourcePath: string): Promise<string> {
  return await invoke('set_game_icon', { gameId, sourcePath });
}

export async function scanInstalled(): Promise<InstallScannerItem[]> {
  const res = await invoke("scan_installed");
  if(!Array.isArray(res)) throw new Error("Invalid response")
  return res.map(e => InstallScannerItemSchema.parse(e))
}

export async function getScanRoots(): Promise<string[]> {
  const res = await invoke("get_scan_roots");
  return z.array(z.string()).parse(res);
}

export async function addScanRoot(path: string): Promise<void> {
  await invoke("add_scan_root", { path });
}

export async function removeScanRoot(path: string): Promise<void> {
  await invoke("remove_scan_root", { path });
}
