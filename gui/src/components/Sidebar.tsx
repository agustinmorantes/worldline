import { ReactElement, useState, useEffect, useMemo } from "react"
import { cn } from "@/lib/utils"
import { useGames } from "@/api/queries"
import { GameIcon } from "./GameIcon"
import { Link } from "@tanstack/react-router"
import { Plus, Download, Settings, Library, HelpCircle } from "lucide-react"
import { Skeleton } from "./ui/skeleton"
import { AddGameModal } from "./AddGameModal"
import { FullGame } from "@/models/game"
import { ScrollArea } from "./ui/scroll-area"

export function Sidebar({ className }: { className?: string }): ReactElement {
  const { data: games, isLoading, error } = useGames()
  const [isAddModalOpen, setIsAddModalOpen] = useState(false)
  const skeletonCount = parseInt(localStorage.getItem('games-count') ?? '3', 10)

  useEffect(() => {
    if (games !== undefined) {
      localStorage.setItem('games-count', games.length.toString());
    }
  }, [games])

  const sortedGames = useMemo(() => {
    if (!games) return games;
    return [...games].sort((a, b) => {
      const aTime = a.versions.length > 0
        ? Math.max(...a.versions.map(v => v.created_at?.getTime() ?? 0))
        : null;
      const bTime = b.versions.length > 0
        ? Math.max(...b.versions.map(v => v.created_at?.getTime() ?? 0))
        : null;
      if (aTime === null && bTime === null) return a.title.localeCompare(b.title);
      if (aTime === null) return 1;
      if (bTime === null) return -1;
      return bTime - aTime;
    });
  }, [games])

  return (
    <aside className={cn("w-[222px] bg-sidebar border-r border-border/40 flex flex-col h-full overflow-x-hidden", className)}>
      <ScrollArea className="flex-1 min-h-0" viewportClassName="px-3 py-2">
      <nav>
        <div className="space-y-4">
          <div>
            <div className="px-3 mb-2 flex items-center justify-between">
              <span className="text-[10px] font-bold uppercase tracking-wider text-muted-foreground/50 flex items-center gap-1.5">
                <Library size={10} /> Library
              </span>
            </div>
            
            <div className="space-y-0.5">
              {isLoading ? (
                Array.from({ length: skeletonCount }).map((_, i) => (
                  <div key={`sidebar-skeleton-${i.toString()}`} className="flex items-center gap-3 px-3 py-2">
                    <Skeleton className="w-7 h-7 rounded-md" />
                    <div className="space-y-1.5">
                      <Skeleton className="h-3 w-24" />
                      <Skeleton className="h-2 w-12" />
                    </div>
                  </div>
                ))
              ) : error ? (
                <div className="px-3 py-2 text-xs text-destructive bg-destructive/10 rounded-md">
                  Error loading games
                </div>
              ) : (
                sortedGames?.map(game => (
                  <SidebarGameItem key={game.id} game={game} />
                ))
              )}
              
              <button 
                id="tour-add-game"
                type="button"
                onClick={() => { setIsAddModalOpen(true); }}
                className="w-full flex items-center gap-3 px-3 py-2 rounded-md border border-dashed border-border/60 hover:border-primary/40 hover:bg-primary/5 text-muted-foreground hover:text-primary transition-all group mt-2"
              >
                <div className="w-7 h-7 rounded-md flex items-center justify-center bg-muted/40 group-hover:bg-primary/10 transition-colors">
                  <Plus size={14} />
                </div>
                <span className="text-xs font-medium tracking-tight">Add Game</span>
              </button>
            </div>
          </div>
        </div>
      </nav>
      </ScrollArea>

      <AddGameModal isOpen={isAddModalOpen} onClose={() => { setIsAddModalOpen(false); }} />

      <div className="p-3 border-t border-border/40 space-y-1">
        <SidebarNavItem id="tour-import-games" to="/import" icon={<Download size={14} />}>
          Import Games
        </SidebarNavItem>
        <SidebarNavItem to="/settings" icon={<Settings size={14} />}>
          Settings
        </SidebarNavItem>
        <button
          type="button"
          onClick={() => { window.dispatchEvent(new CustomEvent("open-onboarding")); }}
          className="w-full flex items-center gap-3 px-3 py-2 rounded-md hover:bg-accent/40 text-muted-foreground hover:text-foreground transition-all text-xs font-medium tracking-tight"
        >
          <div className="text-muted-foreground/60">
            <HelpCircle size={14} />
          </div>
          Help &amp; Tour
        </button>
      </div>
    </aside>
  )
}

function SidebarGameItem({ game }: { game: FullGame }): ReactElement {
  return (
    <Link
      to="/game/$gameId"
      params={{ gameId: game.id.toString() }}
      className="flex items-center gap-3 px-3 py-2 rounded-md hover:bg-accent/40 text-muted-foreground hover:text-foreground transition-all group [&.active]:bg-primary/10 [&.active]:text-foreground [&.active]:border-primary/20 border border-transparent"
    >
      <GameIcon title={game.title} size={28} iconPath={game.icon_path} />
      <div className="flex flex-col min-w-0">
        <span className="text-xs font-semibold truncate tracking-tight">{game.title}</span>
        <span className="text-[10px] text-muted-foreground/70 font-medium">
          {`${game.slots.length.toString()} ${game.slots.length === 1 ? 'slot' : 'slots'}`}
        </span>
      </div>
    </Link>
  )
}

function SidebarNavItem({
  id,
  to,
  icon,
  children,
}: {
  id?: string
  to: string
  icon: React.ReactNode
  children: React.ReactNode
}): ReactElement {
  return (
    <Link
      id={id}
      to={to}
      className="flex items-center gap-3 px-3 py-2 rounded-md hover:bg-accent/40 text-muted-foreground hover:text-foreground transition-all text-xs font-medium tracking-tight [&.active]:bg-accent [&.active]:text-foreground"
    >
      <div className="text-muted-foreground/60 group-hover:text-foreground transition-colors">
        {icon}
      </div>
      {children}
    </Link>
  )
}
