import {createFileRoute, Link, useRouter} from '@tanstack/react-router'
import * as React from "react";
import {useEffect, useState} from "react";
import {Game} from "@/models/game.ts";
import {getGame} from "@/api/games.ts";
import {open} from "@tauri-apps/plugin-dialog";
import {Input} from "@/components/ui/input.tsx";
import {Button} from "@/components/ui/button.tsx";
import {ArrowLeftIcon, PlusIcon} from "@heroicons/react/24/solid";
import {ClockIcon, TrashIcon} from "@heroicons/react/24/outline";
import {Version} from "@/models/version.ts";
import {listen} from "@tauri-apps/api/event";
import {mockIconPath, timeAgo} from "@/lib/utils.ts";

export const Route = createFileRoute('/game/$gameId')({
  component: GameDetail,
})

function GameDetail() {
  const router = useRouter();
  const [loading, setLoading] = useState(false);
  
  const { gameId } = Route.useParams()
  const id = parseInt(gameId)
  
  const [game, setGame] = useState<Game|undefined>()
  
  useEffect(() => {
    if(isNaN(id)) return;
    
    getGame(id)
      .then(game => { setGame(game); })
      .catch(console.error)
    
    // Update state after version creation
    const unlisten = listen<number[]>("version-created", event => {
      const gameId = event.payload[0];
      // const versionId = event.payload[1];
      
      if(gameId !== id) return;

      getGame(id)
        .then(game => { setGame(game); })
        .catch(console.error)
    })
    
    return () => {
      unlisten
        .then(unlistenFn => { unlistenFn() })
        .catch(console.error)
    }
  }, [gameId])

  function onSubmit(e: React.FormEvent<HTMLFormElement>) {
    e.preventDefault();
    
    if(game === undefined) return;
    if(game.title === "") return

    setLoading(true);

    game.save()
      .then(() => { router.history.back() })
      .catch(console.error)
      .finally(() => { setLoading(false); });
  }

  async function addDir() {
    if(game === undefined) return;
    
    const newDirs = await open({
      multiple: true,
      directory: true,
      title: "Select a save directory to track",
      canCreateDirectories: false,
      recursive: true,
    })

    if(!newDirs) return;
    
    const updatedGame = Object.assign(new Game(game.title), game);
    updatedGame.tracked_dirs = [...(game.tracked_dirs ?? []), ...newDirs.map(dir => ({path: dir}))];
    setGame(updatedGame);
  }

  function deleteDir(dir: string) {
    if(game === undefined) return;
    
    const updatedGame = Object.assign(new Game(game.title), game);
    updatedGame.tracked_dirs = game.tracked_dirs?.filter(d => d.path !== dir);
    setGame(updatedGame); 
  }
  
  function handleRestore(v: Version) {
    v.restore()
      .then(() => getGame(id))
      .then(game => { setGame(game); })
      .catch(console.error)
  }

  async function handleTagBlur(v: Version, value: string) {
    if(game === undefined) return;

    v.tag = value.trim();

    try {
      await v.save();
      setGame(game);
    } catch (err) {
      console.error(err);
    } finally {
      setLoading(false);
    }
  }

  return !game ? null : (
    <main className="flex flex-col gap-4 p-8">
      <Link to="/" className="flex gap-2 items-center mb-1 -mt-2">
        <ArrowLeftIcon className="size-5" />
        Go back
      </Link>
      <form onSubmit={onSubmit} className="flex flex-col gap-4">
        <div className="flex gap-4 items-center p-2">
          <img
            src={`/${mockIconPath(game.title)}`}
            onError={e => { e.currentTarget.classList.add("invisible"); }}
            className="rounded-lg"
            alt=""
            width="64px"
            height="64px"
            />
          <Input id="title" name="title" value={game.title}
                 className="!text-lg"
                 onChange={(e) => { 
                    const updatedGame = Object.assign(new Game(e.target.value), game);
                    updatedGame.title = e.target.value;
                    setGame(updatedGame);
                 }} 
          />
        </div>
        <div className="flex flex-col rounded border overflow-clip">
          <div className="flex justify-between items-center p-4">
            <span className="text-lg">Tracked Directories</span>
            <Button type="button" size="icon" onClick={() => void addDir()}>
              <PlusIcon />
            </Button>
          </div>
          { game.tracked_dirs?.map(dir => (
            <div key={dir.path} className="flex justify-between items-center py-2 px-4 border-y">
              <span>{dir.path}</span>
              <Button variant="outline" type="button" size="icon" onClick={() => { deleteDir(dir.path); }}>
                <TrashIcon className="size-4 text-red-500" />
              </Button>
            </div>
          )) }
        </div>
        <div className="flex flex-row gap-2 self-end">
          <Button type="button" variant="outline" onClick={() => { router.history.back(); }}>
            Back
          </Button>
          <Button disabled={loading}>
            Save
          </Button>
        </div>
      </form>

      <div className="flex flex-col border rounded overflow-clip">
        <div className="flex flex-row justify-between items-center p-4">
          <h1 className="text-lg">Versions</h1>
        </div>
        { game.versions?.map(v => v.id === undefined ? undefined : (
          <div key={v.id} className="flex justify-between items-center py-2 px-4 gap-4 border-y">
            <div className="flex gap-1 items-center">
              <ClockIcon className="size-5"/>
              
              <span className="text-nowrap text-sm min-w-25">{v.created_at ? timeAgo(v.created_at) : ""}</span>
            </div>
            <Input autoComplete="off"
                   placeholder={`Version ${v.version_number.toString()}`}
                   defaultValue={v.tag ?? ""} 
                   onBlur={(e) => void handleTagBlur(v, e.target.value)} />
            <Button variant="outline"
                    onClick={e => {
                      e.preventDefault()
                      handleRestore(v)
                    }}
            >
              Restore
            </Button>
          </div>
        ))}
      </div>
    </main>
  )
}
