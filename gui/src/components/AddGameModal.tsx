import { ReactElement, useState } from "react";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from "./ui/dialog";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { X, Plus, MapPin } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { Game } from "@/models/game";
import { saveGame } from "@/api/games";
import { queryClient } from "@/lib/query-client";
import { toast } from "sonner";
import { useRouter } from "@tanstack/react-router";
import { ScrollArea } from "./ui/scroll-area";

interface AddGameModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export function AddGameModal({ isOpen, onClose }: AddGameModalProps): ReactElement {
  const router = useRouter();
  const [title, setTitle] = useState("");
  const [paths, setPaths] = useState<string[]>([""]);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const addPath = async (): Promise<void> => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Select Save Directory"
    });
    
    if (selected !== null) {
      const newPaths = [...paths];
      if (newPaths[newPaths.length - 1] === "") {
        newPaths[newPaths.length - 1] = selected;
      } else {
        newPaths.push(selected);
      }
      setPaths(newPaths);
    }
  };

  const removePath = (index: number): void => {
    if (paths.length > 1) {
      setPaths(paths.filter((_, i) => i !== index));
    } else {
      setPaths([""]);
    }
  };

  const handleSubmit = async (): Promise<void> => {
    if (title.trim() === "") {
      toast.error("Please enter a game title");
      return;
    }

    const validPaths = paths.filter(p => p.trim() !== "");
    if (validPaths.length === 0) {
      toast.error("Please add at least one save directory");
      return;
    }

    setIsSubmitting(true);
    try {
      const game = new Game(title);
      game.tracked_dirs = validPaths.map(p => ({ path: p }));
      const saved = await saveGame(game);
      void queryClient.invalidateQueries({ queryKey: ['games'] });
      toast.success("Game added successfully");
      onClose();
      if (saved.id !== undefined) {
        void router.navigate({ to: '/game/$gameId', params: { gameId: saved.id.toString() } });
      }
    } catch (err) {
      console.error(err);
      toast.error("Failed to add game");
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="sm:max-w-[500px] bg-sf border-border/40">
        <DialogHeader>
          <DialogTitle className="text-xl font-bold tracking-tight">Add New Game</DialogTitle>
        </DialogHeader>

        <div className="grid gap-6 py-4">
          <div className="grid gap-2">
            <Label htmlFor="title" className="text-xs font-bold uppercase tracking-wider text-muted-foreground/70">
              Game Title
            </Label>
            <Input
              id="title"
              placeholder="e.g. Elden Ring"
              value={title}
              onChange={(e) => { setTitle(e.target.value); }}
              className="bg-el border-border/40"
            />
          </div>

          <div className="grid gap-3">
            <Label className="text-xs font-bold uppercase tracking-wider text-muted-foreground/70 flex items-center justify-between">
              Save Directories
              <button type="button" onClick={() => { void addPath(); }} className="text-primary hover:underline lowercase font-medium flex items-center gap-1">
                <Plus size={12} /> Add
              </button>
            </Label>
            
            <ScrollArea className="max-h-[200px]" viewportClassName="pr-3">
            <div className="space-y-2">
              {paths.map((path, index) => (
                <div key={`${path}-${index.toString()}`} className="flex gap-2">
                  <div className="relative flex-1">
                    <MapPin size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground/40" />
                    <Input
                      value={path}
                      placeholder="C:\Users\...\Saves"
                      onChange={(e) => {
                        const newPaths = [...paths];
                        newPaths[index] = e.target.value;
                        setPaths(newPaths);
                      }}
                      className="bg-el border-border/40 pl-9"
                    />
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => { removePath(index); }}
                    className="shrink-0 text-muted-foreground/40 hover:text-destructive hover:bg-destructive/10"
                  >
                    <X size={16} />
                  </Button>
                </div>
              ))}
            </div>
            </ScrollArea>
          </div>
        </div>

        <DialogFooter className="gap-2 sm:gap-0">
          <Button type="button" variant="ghost" onClick={onClose} disabled={isSubmitting}>
            Cancel
          </Button>
          <Button type="button" onClick={() => { void handleSubmit(); }} disabled={isSubmitting} className="font-bold">
            {isSubmitting ? "Adding..." : "Add Game"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
