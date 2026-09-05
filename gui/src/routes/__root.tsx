import { createRootRoute, Outlet, useLocation } from '@tanstack/react-router'
import { ReactElement, useEffect, useState } from "react"
import { ThemeProvider } from "@/components/theme-provider"
import { Toaster } from "sonner"
import { Sidebar } from "@/components/Sidebar"
import { TitleBar } from "@/components/TitleBar"
import { motion, useAnimationControls } from "framer-motion"
import { OnboardingModal, OnboardingStep } from "@/components/OnboardingModal"

const ONBOARDING_STORAGE_KEY = "timeline-onboarding-completed";
type OnboardingSlideStep = Extract<OnboardingStep, "slide-1" | "slide-2" | "slide-3" | "slide-4">;

export const RootLayout = (): ReactElement => {
  const location = useLocation()
  const panelControls = useAnimationControls()
  const hasSeenOnboarding = typeof window !== "undefined"
    && window.localStorage.getItem(ONBOARDING_STORAGE_KEY) === "true"
  const [onboardingStep, setOnboardingStep] = useState<OnboardingStep>(() => {
    if (typeof window === "undefined") {
      return "none"
    }

    return hasSeenOnboarding ? "none" : "slide-1"
  })
  const [resumeStep, setResumeStep] = useState<OnboardingSlideStep | null>(null)
  const [appDialogLayer, setAppDialogLayer] = useState<HTMLDivElement | null>(null)
  const isOnboardingDialogOpen = onboardingStep !== "none"
    && onboardingStep !== "tour-add-game"
    && onboardingStep !== "tour-import-games"

  useEffect(() => {
    panelControls.set({ y: 5 })
    void panelControls.start({
      y: 0,
      transition: {
        duration: 0.36,
        ease: [0.2, 0.8, 0.2, 1],
      },
    })
  }, [location.pathname])

  useEffect(() => {
    if (typeof window === "undefined") {
      return
    }

    if (!hasSeenOnboarding && onboardingStep === "slide-1") {
      window.localStorage.setItem(ONBOARDING_STORAGE_KEY, "true")
    }
  }, [hasSeenOnboarding, onboardingStep])

  useEffect(() => {
    const handleOpenOnboarding = (): void => {
      setResumeStep(null)
      setOnboardingStep("slide-1")
    }

    window.addEventListener("open-onboarding", handleOpenOnboarding)
    return () => {
      window.removeEventListener("open-onboarding", handleOpenOnboarding)
    }
  }, [])

  const handleSkipOnboarding = (): void => {
    window.localStorage.setItem(ONBOARDING_STORAGE_KEY, "true")
    setResumeStep(null)
    setOnboardingStep("none")
  }

  const handleFinishSlides = (): void => {
    window.localStorage.setItem(ONBOARDING_STORAGE_KEY, "true")
    setResumeStep(null)
    setOnboardingStep("tour-add-game")
  }

  const handleFinishTour = (): void => {
    window.localStorage.setItem(ONBOARDING_STORAGE_KEY, "true")
    setResumeStep(null)
    setOnboardingStep("none")
  }

  return (
    <ThemeProvider defaultTheme="dark" storageKey="ui-theme">
      <div className="flex flex-col h-screen w-screen overflow-hidden bg-background text-foreground">
        <TitleBar />
        <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <main className="flex-1 flex flex-col relative overflow-hidden isolate z-10">
          <motion.div
            animate={panelControls}
            initial={false}
            className="flex-1 min-h-0 overflow-hidden"
          >
            <Outlet />
          </motion.div>
          <Toaster theme="dark" position="top-right" expand={false} richColors />
          <OnboardingModal
            step={onboardingStep}
            resumeStep={resumeStep}
            portalContainer={appDialogLayer}
            onStepChange={setOnboardingStep}
            onResumeStepChange={setResumeStep}
            onSkip={handleSkipOnboarding}
            onSlidesComplete={handleFinishSlides}
            onTourComplete={handleFinishTour}
          />
        </main>
        </div>
        <div
          id="app-dialog-layer"
          ref={setAppDialogLayer}
          className={`fixed inset-x-0 top-12 bottom-0 z-[80] ${isOnboardingDialogOpen ? "pointer-events-auto bg-black/60" : "pointer-events-none"}`}
        />
      </div>
    </ThemeProvider>
  )
}

export const Route = createRootRoute({ component: RootLayout })
