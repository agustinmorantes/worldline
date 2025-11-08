import {listen} from "@tauri-apps/api/event";
import {toast} from "sonner";
import {getGame} from "@/api/games.ts";

export function listenNotfications(): void {
  void listen<number>("change-detected", event => {
    const gameId = event.payload;
    void getGame(gameId)
      .then(game => {
        toast("Change detected", {
          description: game?.title
        })
      });
  })

  void listen<number[]>("version-created", event => {
    const gameId = event.payload[0];
    // const version_id = event.payload[1];
    
    void getGame(gameId)
      .then(game => {
        toast("Version created", {
          description: game?.title
        })
      });
  })

  void listen<string>("detection-error", event => {
    toast("Detection error", {
      description: event.payload
    });
  })
}
