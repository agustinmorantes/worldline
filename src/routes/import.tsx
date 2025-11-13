import {createFileRoute, Link, useRouter} from '@tanstack/react-router'
import {Checkbox} from "@/components/ui/checkbox.tsx";
import {CheckedState} from "@radix-ui/react-checkbox";
import {useEffect, useState} from 'react';
import {InstallScannerItem} from "@/models/install_scanner_item.ts";
import {getGames, scanInstalled} from "@/api/games.ts";
import {Button} from "@/components/ui/button.tsx";
import {Game} from "@/models/game.ts";
import {ArrowLeftIcon} from "@heroicons/react/24/solid";

export const Route = createFileRoute('/import')({
  component: ImportGames,
})

function ImportGames() {
  const router = useRouter()
  const [selected, setSelected] = useState<Set<string>>(() => new Set())
  const [scanned, setScanned] = useState<InstallScannerItem[]>([])
  
  useEffect(() => {
    (async function() {
      const games = await getGames()
      
      const installed = await scanInstalled()
      setScanned(
        installed
          .filter(i => !games.find(g => g.title === i.title))
      )
    })()
      .catch(console.error)
    
  }, []);
  
  function checkAllToggle(state: CheckedState) {
    if(state === false) {
      setSelected(new Set())
    } else {
      setSelected(new Set(scanned.map(i => i.title)))
    }
  }
  
  function addSelected(item: InstallScannerItem) {
    selected.add(item.title)
    setSelected(new Set(selected))
  }
  
  function removeSelected(item: InstallScannerItem) {
    selected.delete(item.title)
    setSelected(new Set(selected))
  }

  function setItemSelected(item: InstallScannerItem, state: CheckedState) {
    if(state === false) {
      removeSelected(item)
    } else {
      addSelected(item)
    }
  }
  
  function importSelected() {
    Promise.all(
      scanned
        .filter(i => selected.has(i.title))
        .map(i => new Game(i.title, i.dirs))
        .map(g => g.save())
    ).then(() => { router.history.back() })
      .catch(console.error)
  }

  const allChecked: CheckedState = selected.size === 0 ? false
    : selected.size === scanned.length ? true 
      : "indeterminate";
  
  return (
    <main className="flex flex-col p-8 gap-4">
      <Link to="/" className="flex gap-2 items-center mb-1 -mt-2">
        <ArrowLeftIcon className="size-5" />
        Go back
      </Link>
      <h1 className="text-xl">Import</h1>
      <div className="flex flex-col border rounded overflow-clip">
        <div className="flex flex-row items-center p-4 gap-4">
          <Checkbox onCheckedChange={checkAllToggle} checked={allChecked} />
          <h1 className="text-lg">Installed Games</h1>
        </div>
        { scanned.map(item => (
          <div key={item.title} className="flex items-center py-2 px-4 gap-4 border-y">
            <Checkbox 
              onCheckedChange={(state) => { setItemSelected(item, state); }}
              checked={selected.has(item.title)}
            />
            <img src={`/${item.icon}.ico`} alt="" height="32px" width="32px" className="rounded" />
            <span>{item.title}</span>
          </div>
        ))}
      </div>
      <div className="flex gap-2 self-end">
        <Button onClick={() => { importSelected() }} className="self-end">
          Import
        </Button>
      </div>
    </main>
  )
}
