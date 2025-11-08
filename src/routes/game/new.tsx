import {createFileRoute, useRouter} from "@tanstack/react-router";
import {ReactElement, useState} from "react";
import {Input} from "@/components/ui/input.tsx";
import {Label} from "@/components/ui/label.tsx";
import {Button} from "@/components/ui/button.tsx";
import * as React from "react";
import {Game} from "@/models/game.ts";
import {TrashIcon} from "@heroicons/react/24/outline";
import {open} from "@tauri-apps/plugin-dialog";
import {PlusIcon} from "@heroicons/react/24/solid";

export const Route = createFileRoute("/game/new")({
  component: NewGame,
})

function NewGame(): ReactElement {
  const router = useRouter();
  
  const [title, setTitle] = useState("");
  const [loading, setLoading] = useState(false);
  const [dirs, setDirs] = useState<string[]>([]);
  
  function onSubmit(e: React.FormEvent<HTMLFormElement>) {
    e.preventDefault();
    
    if(!title.length) {
      return;
    }
    
    setLoading(true);
    
    const game = new Game(title, dirs);
    game.save()
      .then(() => { router.history.back() })
      .catch(console.error)
      .finally(() => { setLoading(false); });
  }
  
  async function addDir() {
    const newDirs = await open({
      multiple: true,
      directory: true,
      title: "Select a save directory to track",
      canCreateDirectories: false,
      recursive: true,
    })
    
    if(!newDirs) return;
    
    setDirs([...dirs, ...newDirs])
  }
  
  function deleteDir(dir: string) {
    setDirs(dirs.filter(d => d !== dir));
  }
  
  return (
    <main className="flex flex-col gap-4 p-8">
      <h1>New Game</h1>
      <form onSubmit={onSubmit} className="flex flex-col gap-4">
        <div className="flex flex-col gap-3">
          <Label htmlFor="title">Title</Label>
          <Input id="title" name="title" onChange={(e) => { setTitle(e.target.value); }} />
        </div>
        <div className="flex flex-col rounded border overflow-clip">
          <div className="flex justify-between items-center p-4">
            <span className="text-lg">Tracked Directories</span>
            <Button type="button" size="icon" onClick={() => void addDir()}>
              <PlusIcon />
            </Button>
          </div>
          { dirs.map(dir => (
            <div key={dir} className="flex justify-between items-center py-2 px-4 border-y">
              <span>{dir}</span>
              <Button variant="outline" type="button" size="icon" onClick={() => { deleteDir(dir); }}>
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
            Create
          </Button>
        </div>
      </form>
    </main>
  )
}
