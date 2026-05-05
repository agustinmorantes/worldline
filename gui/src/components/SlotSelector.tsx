import { ReactElement } from 'react';
import { Button } from './ui/button';
import { Plus } from 'lucide-react';

export function SlotSelector(): ReactElement {
  return (
    <div className="flex items-center gap-2">
      <Button
        variant="secondary"
        size="sm"
        className="h-8 rounded-full px-4 text-xs font-bold bg-primary/10 text-primary border border-primary/20"
      >
        Default Slot
      </Button>
      <Button
        variant="ghost"
        size="sm"
        className="h-8 rounded-full px-4 text-xs font-bold text-muted-foreground/40"
        disabled
        title="Coming soon"
      >
        <Plus size={14} className="mr-1.5" /> New Slot
      </Button>
    </div>
  );
}
