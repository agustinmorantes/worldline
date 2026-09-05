import { ReactElement, ReactNode, useEffect, useReducer, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  Download,
  HelpCircle,
  ListTree,
  Play,
  Save,
  Search,
} from "lucide-react";

import { BranchingTimeline } from "@/components/BranchingTimeline";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { Version } from "@/models/version";

export type OnboardingStep =
  | "none"
  | "menu"
  | "slide-1"
  | "slide-2"
  | "slide-3"
  | "slide-4"
  | "skip-confirm"
  | "tour-add-game"
  | "tour-import-games";

type SlideStep = Extract<OnboardingStep, "slide-1" | "slide-2" | "slide-3" | "slide-4">;

interface OnboardingModalProps {
  step: OnboardingStep;
  resumeStep: SlideStep | null;
  portalContainer: HTMLElement | null;
  onStepChange: (step: OnboardingStep) => void;
  onResumeStepChange: (step: SlideStep | null) => void;
  onSkip: () => void;
  onSlidesComplete: () => void;
  onTourComplete: () => void;
}

type TourStep = Extract<OnboardingStep, "tour-add-game" | "tour-import-games">;

const SLIDE_ORDER: SlideStep[] = ["slide-1", "slide-2", "slide-3", "slide-4"];
const TOUR_TARGETS: Record<TourStep, string> = {
  "tour-add-game": "tour-add-game",
  "tour-import-games": "tour-import-games",
};

const slideMotion = {
  initial: { opacity: 0, x: 20 },
  animate: { opacity: 1, x: 0 },
  exit: { opacity: 0, x: -20 },
  transition: { duration: 0.24, ease: [0.2, 0.8, 0.2, 1] as const },
};

function isSlideStep(step: OnboardingStep): step is SlideStep {
  return step === "slide-1" || step === "slide-2" || step === "slide-3" || step === "slide-4";
}

export function OnboardingModal({
  step,
  resumeStep,
  portalContainer,
  onStepChange,
  onResumeStepChange,
  onSkip,
  onSlidesComplete,
  onTourComplete,
}: OnboardingModalProps): ReactElement {
  const currentSlideIndex = SLIDE_ORDER.includes(step as SlideStep) ? SLIDE_ORDER.indexOf(step as SlideStep) : -1;
  const isDialogOpen = step !== "none" && step !== "tour-add-game" && step !== "tour-import-games";

  const openSkipConfirm = (): void => {
    if (isSlideStep(step)) {
      onResumeStepChange(step);
      onStepChange("skip-confirm");
    }
  };

  const goToSlide = (index: number): void => {
    if (index >= 0 && index < SLIDE_ORDER.length) {
      onStepChange(SLIDE_ORDER[index]);
    }
  };

  return (
    <>
      <Dialog
        open={isDialogOpen}
        modal={false}
        onOpenChange={(open) => {
          if (!open && step === "menu") {
            onStepChange("none");
          }
        }}
      >
        <DialogContent
          portalContainer={portalContainer}
          showCloseButton={step === "menu"}
          overlayClassName="absolute inset-0 z-[80] bg-black/60"
          className="z-[90] w-[min(1120px,calc(100vw-1rem))] max-w-[calc(100vw-1rem)] max-h-[calc(100vh-3.75rem)] overflow-hidden border-border/40 bg-sf p-0 sm:max-w-[calc(100vw-1rem)]"
          onEscapeKeyDown={(event) => {
            if (step === "menu") {
              onStepChange("none");
              return;
            }

            event.preventDefault();
            openSkipConfirm();
          }}
          onInteractOutside={(event) => {
            if (step !== "menu") {
              event.preventDefault();
            }
          }}
          onPointerDownOutside={(event) => {
            if (step !== "menu") {
              event.preventDefault();
            }
          }}
        >
          {isSlideStep(step) && (
            <DialogHeader className="sr-only">
              <DialogTitle>Savegame Timeline onboarding</DialogTitle>
              <DialogDescription>
                A short guide to importing games, automatic checkpoints, and branching story paths.
              </DialogDescription>
            </DialogHeader>
          )}
          <AnimatePresence mode="wait" initial={false}>
            {step === "menu" && (
              <motion.div key="menu" {...slideMotion}>
                <ReplayMenuView
                  onClose={() => { onStepChange("none"); }}
                  onRunSlides={() => { onStepChange("slide-1"); }}
                  onRunTour={() => { onStepChange("tour-add-game"); }}
                />
              </motion.div>
            )}

            {step === "skip-confirm" && (
              <motion.div key="skip-confirm" {...slideMotion}>
                <SkipConfirmView
                  onResume={() => {
                    onStepChange(resumeStep ?? "slide-1");
                  }}
                  onSkipEverything={() => {
                    onResumeStepChange(null);
                    onSkip();
                  }}
                />
              </motion.div>
            )}

            {isSlideStep(step) && step !== "slide-4" && (
              <motion.div key={step} {...slideMotion} className="flex h-[min(700px,calc(100vh-3.75rem))] min-h-0 flex-col md:flex-row">
                <div className="flex min-h-0 flex-1 flex-col justify-center border-b border-border/40 p-5 md:border-r md:border-b-0 md:p-8">
                  <div className="min-h-0 flex-1 space-y-6 overflow-y-auto pr-1 md:flex md:flex-col md:justify-center">
                    <div className="flex items-center justify-between">
                      <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-muted-foreground/70">
                        {currentSlideIndex + 1} / {SLIDE_ORDER.length}
                      </div>
                    </div>

                    {step === "slide-1" && <WelcomeCopy />}
                    {step === "slide-2" && <StartTrackingCopy />}
                    {step === "slide-3" && <CheckpointCopy />}
                  </div>

                  <div className="space-y-5 pt-4">
                    <div className="flex items-center gap-2">
                      {SLIDE_ORDER.map((slide, index) => (
                        <div
                          key={slide}
                          className={cn(
                            "h-1.5 rounded-full transition-all",
                            index === currentSlideIndex ? "w-12 bg-primary" : "w-6 bg-primary/20"
                          )}
                        />
                      ))}
                    </div>

                    <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                      <Button variant="ghost" onClick={openSkipConfirm}>
                        Skip
                      </Button>
                      <div className="flex items-center justify-end gap-2">
                        <Button
                          variant="ghost"
                          onClick={() => { goToSlide(currentSlideIndex - 1); }}
                          disabled={currentSlideIndex <= 0}
                        >
                          Back
                        </Button>
                        <Button onClick={() => { goToSlide(currentSlideIndex + 1); }}>
                          Next
                          <ArrowRight size={14} />
                        </Button>
                      </div>
                    </div>
                  </div>
                </div>
                <div className="relative flex h-[220px] shrink-0 flex-col justify-center overflow-hidden bg-[radial-gradient(circle_at_top,rgba(98,143,255,0.18),transparent_42%),linear-gradient(180deg,rgba(24,26,40,0.98),rgba(17,19,32,0.98))] p-5 md:h-auto md:w-[420px] md:p-6 lg:w-[460px]">
                  {step === "slide-1" && <WelcomeVisual />}
                  {step === "slide-2" && <StartTrackingVisual />}
                  {step === "slide-3" && <CheckpointVisual />}
                </div>
              </motion.div>
            )}

            {step === "slide-4" && (
              <motion.div key={step} {...slideMotion} className="flex h-[min(700px,calc(100vh-3.75rem))] min-h-0 flex-col md:flex-row md:items-stretch">
                <div className="flex min-h-0 flex-col justify-center border-b border-border/40 p-5 md:w-[42%] md:border-r md:border-b-0 md:p-8">
                  <div className="min-h-0 flex-1 space-y-6 overflow-y-auto pr-1 md:flex md:flex-col md:justify-center">
                    <div className="flex items-center justify-between">
                      <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-muted-foreground/70">
                        {currentSlideIndex + 1} / {SLIDE_ORDER.length}
                      </div>
                    </div>
                    <BranchingCopy />
                  </div>

                  <div className="shrink-0 space-y-5 pt-4">
                    <div className="flex items-center gap-2">
                      {SLIDE_ORDER.map((slide, index) => (
                        <div
                          key={slide}
                          className={cn(
                            "h-1.5 rounded-full transition-all",
                            index === currentSlideIndex ? "w-12 bg-primary" : "w-6 bg-primary/20"
                          )}
                        />
                      ))}
                    </div>

                    <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                      <Button variant="ghost" onClick={openSkipConfirm}>
                        Skip
                      </Button>
                      <div className="flex items-center justify-end gap-2">
                        <Button
                          variant="ghost"
                          onClick={() => { goToSlide(currentSlideIndex - 1); }}
                          disabled={currentSlideIndex <= 0}
                        >
                          Back
                        </Button>
                        <Button onClick={onSlidesComplete}>
                          Finish
                        </Button>
                      </div>
                    </div>
                  </div>
                </div>

                <div className="flex min-h-0 flex-1 items-center justify-center bg-[radial-gradient(circle_at_top,rgba(98,143,255,0.12),transparent_38%),linear-gradient(180deg,rgba(24,26,40,0.96),rgba(17,19,32,0.98))] p-5 md:w-[58%] md:p-6">
                  <div className="w-full max-w-[34rem]">
                    <BranchingVisual />
                  </div>
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </DialogContent>
      </Dialog>

      {(step === "tour-add-game" || step === "tour-import-games") && (
        <TourTooltip
          step={step}
          onClose={onTourComplete}
          onNext={() => {
            if (step === "tour-add-game") {
              onStepChange("tour-import-games");
              return;
            }

            onTourComplete();
          }}
        />
      )}
    </>
  );
}

function ReplayMenuView({
  onClose,
  onRunSlides,
  onRunTour,
}: {
  onClose: () => void;
  onRunSlides: () => void;
  onRunTour: () => void;
}): ReactElement {
  return (
    <div className="max-h-[calc(100vh-5rem)] overflow-y-auto p-5 md:p-8">
      <DialogHeader className="text-left">
        <DialogTitle className="text-2xl font-black tracking-tight">Help &amp; Tour</DialogTitle>
        <DialogDescription className="max-w-xl text-sm leading-6">
          Replay the product concepts or jump straight to the sidebar tour whenever you want a quick refresher.
        </DialogDescription>
      </DialogHeader>

      <div className="mt-6 space-y-3">
        <MenuActionCard
          icon={<HelpCircle size={18} />}
          title="Replay Concepts Guide"
          description="Walk through automated snapshots, watcher timing, and branch-safe restores."
          onClick={onRunSlides}
        />
        <MenuActionCard
          icon={<Search size={18} />}
          title="Run Interface Tour"
          description="Highlight the two fastest ways to start tracking games from the sidebar."
          onClick={onRunTour}
        />
      </div>

      <div className="mt-6 flex justify-end">
        <Button variant="ghost" onClick={onClose}>Cancel</Button>
      </div>
    </div>
  );
}

function MenuActionCard({
  icon,
  title,
  description,
  onClick,
}: {
  icon: ReactNode;
  title: string;
  description: string;
  onClick: () => void;
}): ReactElement {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex w-full items-start gap-4 rounded-2xl border border-border/40 bg-el/60 p-4 text-left transition-all hover:border-primary/20 hover:bg-el"
    >
      <div className="mt-0.5 flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary">
        {icon}
      </div>
      <div>
        <div className="text-sm font-bold tracking-tight text-foreground">{title}</div>
        <div className="mt-1 text-xs leading-5 text-muted-foreground">{description}</div>
      </div>
    </button>
  );
}

function SkipConfirmView({
  onResume,
  onSkipEverything,
}: {
  onResume: () => void;
  onSkipEverything: () => void;
}): ReactElement {
  return (
    <div className="max-h-[calc(100vh-5rem)] overflow-y-auto p-5 md:p-8">
      <div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-warn/12 text-warn">
        <AlertTriangle size={24} />
      </div>
      <DialogHeader className="mt-5 text-left">
        <DialogTitle className="text-2xl font-black tracking-tight">Skip onboarding?</DialogTitle>
        <DialogDescription className="max-w-xl text-sm leading-6">
          You can always replay this later from the sidebar <span className="font-semibold text-foreground">Help &amp; Tour</span> button.
        </DialogDescription>
      </DialogHeader>

      <div className="mt-8 flex items-center justify-end gap-2">
        <Button variant="ghost" onClick={onResume}>No, resume</Button>
        <Button variant="destructive" onClick={onSkipEverything}>Yes, skip everything</Button>
      </div>
    </div>
  );
}

function WelcomeVisual(): ReactElement {
  return (
    <div className="relative flex h-full min-h-[180px] flex-col items-center justify-center py-4">
      <motion.div
        className="absolute h-44 w-44 rounded-full border border-primary/20"
        animate={{ scale: [1, 1.08, 1], opacity: [0.45, 0.85, 0.45] }}
        transition={{ duration: 3.2, repeat: Infinity, ease: "easeInOut" }}
      />
      <motion.div
        className="absolute h-56 w-56 rounded-full border border-dashed border-primary/15"
        animate={{ rotate: 360 }}
        transition={{ duration: 18, repeat: Infinity, ease: "linear" }}
      />
      <div className="relative flex h-24 w-24 items-center justify-center rounded-[2rem] border border-primary/20 bg-primary/10 text-primary shadow-[0_0_48px_rgba(98,143,255,0.18)]">
        <Save size={42} />
      </div>
      <div className="mt-6 grid gap-3 text-center">
        <VisualChip icon={<CheckCircle2 size={12} />} label="Protected saves" />
        <VisualChip icon={<ListTree size={12} />} label="Try choices freely" />
        <VisualChip icon={<Download size={12} />} label="Import your library" />
      </div>
    </div>
  );
}

function StartTrackingVisual(): ReactElement {
  return (
    <div className="flex h-full min-h-[180px] flex-col justify-center gap-4 py-4">
      <div className="rounded-[1.6rem] border border-border/40 bg-sf/70 p-5 shadow-lg backdrop-blur-sm">
        <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-primary/85">
          Two easy starts
        </div>
        <div className="mt-4 space-y-3">
          <PreviewOption
            icon={<Download size={16} />}
            title="Import your library"
            description="Find games and known save folders automatically."
          />
          <PreviewOption
            icon={<Save size={16} />}
            title="Add one game"
            description="Point Timeline at a single game and its save folder."
          />
        </div>
      </div>
    </div>
  );
}

function CheckpointVisual(): ReactElement {
  const [snapshotCount, setSnapshotCount] = useState(0);
  const versions = createLinearCheckpointVersions(snapshotCount);
  const activeVersionId = versions[versions.length - 1]?.id;

  return (
    <div className="flex h-full min-h-0 flex-col justify-center gap-4">
      <div className="rounded-[1.6rem] border border-border/40 bg-sf/70 p-5 shadow-lg backdrop-blur-sm">
        <div className="flex items-center justify-between">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-primary/85">
            Starfall Gate
          </div>
          <div className="text-[10px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">Act II</div>
        </div>

        <div className="mt-4 rounded-xl border border-border/40 bg-el/80 p-4 text-sm leading-6 text-muted-foreground">
          Your crew reaches a city in orbit. The rebels want your help. The governor offers a cleaner deal.
        </div>

        <Button className="mt-4 w-full font-bold" onClick={() => { setSnapshotCount((count) => Math.min(count + 1, 3)); }}>
          <Play size={14} />
          Play a little
        </Button>
      </div>

      <div className="min-h-0 rounded-2xl border border-border/40 bg-sf/50 p-4">
        <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-muted-foreground/80">
          Timeline preview
        </div>
        <div className="mt-3 overflow-hidden">
          <BranchingTimeline versions={versions} activeVersionId={activeVersionId} showActions={false} compact />
        </div>
      </div>
    </div>
  );
}

function BranchingVisual(): ReactElement {
  const [activeVersionId, setActiveVersionId] = useState(3);
  const isBranched = activeVersionId === 4;
  const versions = createMockBranchVersions();

  return (
    <div className="flex h-auto min-h-0 flex-col justify-start gap-4">
      <div className="flex h-auto min-h-0 flex-col rounded-[1.6rem] border border-border/40 bg-sf/70 p-4 shadow-lg backdrop-blur-sm">
        <div className="flex items-center justify-between">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-primary/85">
            Faction choice
          </div>
          <Button
            size="sm"
            className="font-bold shadow-[0_0_28px_rgba(27,134,255,0.24)]"
            onClick={() => { setActiveVersionId((current) => current === 4 ? 3 : 4); }}
          >
            {isBranched ? "Return to Governor Path" : "Choose Rebel Path"}
          </Button>
        </div>
        <div className="mt-4 overflow-hidden rounded-xl border border-border/40 bg-el/70 px-2 py-3">
          <BranchingTimeline versions={versions} activeVersionId={activeVersionId} showActions={false} compact />
        </div>
      </div>

      <div className={cn(
        "rounded-2xl border px-4 py-3 text-xs leading-5 transition-all",
        isBranched ? "border-primary/20 bg-primary/8 text-foreground" : "border-border/40 bg-sf/50 text-muted-foreground"
      )}>
        {isBranched
          ? "Rebel path started. The governor path is still there if you want it."
          : "Preview the rebel path, then switch back to the governor route whenever you want."}
      </div>
    </div>
  );
}

function WelcomeCopy(): ReactElement {
  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-3xl font-black tracking-tight text-foreground">Your save-game time machine</h2>
        <p className="mt-3 max-w-xl text-sm leading-6 text-muted-foreground">
          Keep checkpoints for big choices, boss fights, and weird experiments. Jump back without losing the path you already played.
        </p>
      </div>

      <div className="grid gap-2">
        <CopyCard
          icon={<Save size={16} />}
          title="Play normally, protect your saves"
          description="Timeline keeps backup checkpoints as you play."
        />
        <CopyCard
          icon={<ListTree size={16} />}
          title="Replay a choice safely"
          description="Go back, try a different outcome, and keep both playthroughs."
        />
        <CopyCard
          icon={<Download size={16} />}
          title="Start from your current library"
          description="Bring in many games at once, or add one specific game."
        />
      </div>
    </div>
  );
}

function StartTrackingCopy(): ReactElement {
  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-3xl font-black tracking-tight text-foreground">Start with the games you already play</h2>
        <p className="mt-3 max-w-xl text-sm leading-6 text-muted-foreground">
          Choose the setup path that fits the game in front of you.
        </p>
      </div>
    </div>
  );
}

function CheckpointCopy(): ReactElement {
  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-3xl font-black tracking-tight text-foreground">Checkpoints while you play</h2>
        <p className="mt-3 max-w-xl text-sm leading-6 text-muted-foreground">
          Timeline keeps moments you can return to. No ritual before every boss fight.
        </p>
      </div>

      <div className="rounded-2xl border border-border/40 bg-el/60 p-4 text-sm leading-6 text-muted-foreground">
        In a sci-fi RPG, that might be docking at a city, finding a secret signal, or standing before a faction choice.
      </div>
    </div>
  );
}

function BranchingCopy(): ReactElement {
  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-3xl font-black tracking-tight text-foreground">Try both story paths</h2>
        <p className="mt-3 max-w-xl text-sm leading-6 text-muted-foreground">
          Side with the governor, then jump back and back the rebels. Timeline starts a new branch while the first story stays intact.
        </p>
      </div>

      <div className="rounded-2xl border border-border/40 bg-el/60 p-4 text-sm leading-6 text-muted-foreground transition-all">
        Try the other path in the preview. The original ending remains available.
      </div>
    </div>
  );
}

function TourTooltip({
  step,
  onClose,
  onNext,
}: {
  step: TourStep;
  onClose: () => void;
  onNext: () => void;
}): ReactElement {
  const isLast = step === "tour-import-games";
  const [layout, setLayout] = useReducer((_state: {
    highlight: {
      height: number;
      left: number;
      top: number;
      width: number;
    } | null;
    tooltip: {
      left: number;
      top: number;
    };
  }, next: {
    highlight: {
      height: number;
      left: number;
      top: number;
      width: number;
    } | null;
    tooltip: {
      left: number;
      top: number;
    };
  }) => next, {
    highlight: null as {
      height: number;
      left: number;
      top: number;
      width: number;
    } | null,
    tooltip: {
      left: 250,
      top: step === "tour-add-game" ? 164 : 224,
    },
  });

  useEffect(() => {
    let frameId = 0;
    let retries = 0;

    const updateLayout = (): void => {
      const targetId = TOUR_TARGETS[step];
      const target = document.getElementById(targetId);

      if (target !== null) {
        const rect = target.getBoundingClientRect();
        setLayout({
          highlight: {
            height: rect.height,
            left: rect.left,
            top: rect.top,
            width: rect.width,
          },
          tooltip: {
            left: rect.right + 16,
            top: rect.top + rect.height / 2,
          },
        });
        return;
      }

      setLayout({
        highlight: null,
        tooltip: {
          left: 250,
          top: step === "tour-add-game" ? 164 : 224,
        },
      });

      if (retries < 12) {
        retries += 1;
        frameId = window.requestAnimationFrame(updateLayout);
      }
    };

    updateLayout();
    window.addEventListener("resize", updateLayout);
    window.addEventListener("scroll", updateLayout, true);

    return () => {
      if (frameId !== 0) {
        window.cancelAnimationFrame(frameId);
      }
      window.removeEventListener("resize", updateLayout);
      window.removeEventListener("scroll", updateLayout, true);
    };
  }, [step]);

  return (
    <div className="pointer-events-none fixed inset-0 z-50">
      {layout.highlight !== null && (
        <div
          className="absolute rounded-md border-2 border-primary shadow-[0_0_0_9999px_rgba(11,12,17,0.24)]"
          style={{
            height: layout.highlight.height,
            left: layout.highlight.left,
            top: layout.highlight.top,
            width: layout.highlight.width,
          }}
        >
          <div className="absolute inset-0 rounded-md border border-primary/60 animate-[pulseRing_1.8s_infinite]" />
        </div>
      )}

      <div
        className="pointer-events-auto absolute w-[320px] max-w-[calc(100vw-2rem)]"
        style={{
          left: layout.tooltip.left,
          top: layout.tooltip.top,
          transform: "translateY(-50%)",
        }}
      >
        <div className="rounded-2xl border border-border/40 bg-sf p-5 shadow-2xl">
          <div className="inline-flex items-center gap-2 rounded-full border border-primary/20 bg-primary/10 px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.18em] text-primary">
            {step === "tour-add-game" ? <Save size={12} /> : <Download size={12} />}
            {step === "tour-add-game" ? "Manual Add" : "Import Library"}
          </div>

          <h3 className="mt-4 text-lg font-black tracking-tight text-foreground">
            {step === "tour-add-game" ? "Add one game directly" : "Scan and import faster"}
          </h3>
          <p className="mt-2 text-sm leading-6 text-muted-foreground">
            {step === "tour-add-game"
              ? "Use this when you want to pick a specific game and point Timeline at the save folders yourself."
              : "Use Import Games to scan known libraries and save locations, then bring detected games into your timeline in bulk."}
          </p>

          <div className="mt-5 flex items-center justify-between">
            <Button variant="ghost" onClick={onClose}>Close</Button>
            <Button onClick={onNext}>
              {isLast ? "Finish" : "Next"}
              {!isLast && <ArrowRight size={14} />}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

function VisualChip({ icon, label }: { icon: ReactNode; label: string }): ReactElement {
  return (
    <div className="inline-flex items-center justify-center gap-2 rounded-full border border-primary/18 bg-primary/10 px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.18em] text-primary/90">
      {icon}
      {label}
    </div>
  );
}

function PreviewOption({
  icon,
  title,
  description,
}: {
  icon: ReactNode;
  title: string;
  description: string;
}): ReactElement {
  return (
    <div className="rounded-xl border border-border/40 bg-el/75 p-4">
      <div className="flex items-center gap-3">
        <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-primary/10 text-primary">
          {icon}
        </div>
        <div className="text-sm font-bold tracking-tight text-foreground">{title}</div>
      </div>
      <p className="mt-2 text-xs leading-5 text-muted-foreground">{description}</p>
    </div>
  );
}

function CopyCard({
  icon,
  title,
  description,
}: {
  icon: ReactNode;
  title: string;
  description: string;
}): ReactElement {
  return (
    <div className="rounded-2xl border border-border/40 bg-el/55 p-4">
      <div className="flex items-center gap-3">
        <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-primary/10 text-primary">
          {icon}
        </div>
        <div className="text-sm font-bold tracking-tight text-foreground">{title}</div>
      </div>
      <p className="mt-3 text-sm leading-6 text-muted-foreground">{description}</p>
    </div>
  );
}

function createMockBranchVersions(): Version[] {
  const now = Date.now();
  const v1 = new Version(1, 1);
  v1.id = 1;
  v1.tag = "Docked at Starfall Gate";
  v1.created_at = new Date(now - 1000 * 60 * 22);

  const v2 = new Version(2, 1);
  v2.id = 2;
  v2.parent_id = 1;
  v2.tag = "Before faction choice";
  v2.created_at = new Date(now - 1000 * 60 * 11);

  const v3 = new Version(3, 1);
  v3.id = 3;
  v3.parent_id = 2;
  v3.tag = "Governor path";
  v3.created_at = new Date(now - 1000 * 60 * 4);

  const v4 = new Version(4, 1);
  v4.id = 4;
  v4.parent_id = 2;
  v4.tag = "Rebel path";
  v4.created_at = new Date(now - 1000 * 60 * 2);

  return [v1, v2, v3, v4];
}

function createLinearCheckpointVersions(extraCount: number): Version[] {
  const now = Date.now();
  const checkpointData = [
    { id: 1, versionNumber: 1, tag: "Docked at Starfall Gate", parentId: undefined, minutesAgo: 26 },
    { id: 2, versionNumber: 2, tag: "Found the rebel signal", parentId: 1, minutesAgo: 14 },
    { id: 3, versionNumber: 3, tag: "Met the governor", parentId: 2, minutesAgo: 7 },
    { id: 4, versionNumber: 4, tag: "Before faction choice", parentId: 3, minutesAgo: 1 },
  ].slice(0, Math.min(extraCount + 1, 4));

  return checkpointData.map((data) => {
    const version = new Version(data.versionNumber, 1);
    version.id = data.id;
    version.parent_id = data.parentId;
    version.tag = data.tag;
    version.created_at = new Date(now - data.minutesAgo * 60 * 1000);
    return version;
  });
}
