import {createFileRoute, Link} from "@tanstack/react-router";
import {ReactElement, useEffect, useState} from "react";
import {Game} from "@/models/game.ts";
import {getGames} from "@/api/games.ts";
import {Button} from "@/components/ui/button.tsx";
import {PlusIcon} from "@heroicons/react/24/solid";
import {TrashIcon} from "@heroicons/react/24/outline";

export const Route = createFileRoute("/")({
  component: GameDashboard
})

function GameDashboard(): ReactElement {
  const [games, setGames] = useState<Game[]|undefined>(undefined);
  
  useEffect(() =>  {
    getGames()
      .then(setGames)
      .catch(console.error);
  }, [setGames])
  
  const handleDelete = async (game: Game) => {
    try {
      await game.delete();
      // Update UI by removing the deleted game from the state
      setGames(prevGames => prevGames?.filter(g => g !== game));
    } catch (error) {
      console.error("Failed to delete game:", error);
    }
  };
  
  return (
    <main className="flex flex-col p-8 gap-4">
      <div className="flex flex-col border rounded overflow-clip">
        <div className="flex flex-row justify-between items-center p-4">
          <h1 className="text-lg">Games</h1>
          <Link to="/game/new">
            <Button size="icon">
              <PlusIcon />
            </Button>
          </Link>
        </div>
        { games?.map(game => game.id === undefined ? undefined : (
          <Link to="/game/$gameId" params={{gameId: game.id.toString()}} key={game.id}>
            <div className="flex justify-between items-center py-2 px-4 border-y">
              <span>{game.title}</span>
              <Button variant="outline" size="icon" className="text-red-500" 
                      onClick={e => {
                        e.preventDefault()
                        void handleDelete(game)
                      }}
              >
                <TrashIcon />
              </Button>
            </div>
          </Link>
        ))}
      </div>
    </main>
  )
}
