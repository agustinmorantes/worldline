import { createFileRoute } from "@tanstack/react-router";
import { ReactElement } from "react";
import { Save, ListTree, History } from "lucide-react";

export const Route = createFileRoute("/")({
  component: WelcomeView
})

function WelcomeView(): ReactElement {
  return (
    <div className="flex flex-col items-center justify-center h-full text-center p-8">
      <div className="w-20 h-20 bg-primary/10 rounded-3xl flex items-center justify-center text-primary mb-8 shadow-inner border border-primary/20">
        <Save size={40} />
      </div>
      
      <h1 className="text-4xl font-black tracking-tight text-foreground mb-3">
        Welcome to <span className="text-primary">Worldline</span>
      </h1>
      
      <p className="text-muted-foreground max-w-md text-sm leading-relaxed mb-12">
        Your automated savegame companion. Select a game from the sidebar or import your library to start tracking your progress.
      </p>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-6 max-w-3xl w-full">
        <FeatureCard 
          icon={<History size={20} />} 
          title="Automated Snapshots" 
          description="Worldline monitors your save folders and creates versions automatically while you play."
        />
        <FeatureCard 
          icon={<ListTree size={20} />} 
          title="Branching History" 
          description="Rewind to any point in time. Restoration creates branches so you never lose a timeline."
        />
        <FeatureCard 
          icon={<Save size={20} />} 
          title="Local Persistence" 
          description="Your data stays local and private. Built with Rust and React for performance and safety."
        />
      </div>

      <div className="mt-16 text-[10px] font-bold uppercase tracking-[0.3em] text-muted-foreground/30">
        v0.1.1 — Beta
      </div>
    </div>
  )
}

function FeatureCard({ icon, title, description }: { icon: React.ReactNode, title: string, description: string }): ReactElement {
  return (
    <div className="p-6 rounded-2xl bg-sf border border-border/40 text-left flex flex-col gap-3 group hover:border-primary/20 transition-all">
      <div className="w-10 h-10 rounded-lg bg-el flex items-center justify-center text-primary group-hover:scale-110 transition-transform">
        {icon}
      </div>
      <h3 className="text-sm font-bold tracking-tight text-foreground">{title}</h3>
      <p className="text-xs text-muted-foreground leading-normal">{description}</p>
    </div>
  )
}
