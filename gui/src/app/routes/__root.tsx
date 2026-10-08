import { createRootRoute, Outlet } from '@tanstack/react-router'
import { ReactElement, useEffect, useMemo, useState } from "react"
import { Sidebar } from "@/features/library/Sidebar"
import { TitleBar } from "@/app/shell/TitleBar"
import { Shell } from "@/app/shell/Shell"
import { Toasts } from "@/app/shell/Toasts"
import { OnboardingModal, OnboardingStep } from "@/features/onboarding/OnboardingModal"
import { getPrefs, setPref } from "@/shared/prefs/prefs"

type OnboardingSlideStep = Extract<OnboardingStep, "slide-1" | "slide-2" | "slide-3" | "slide-4">;

export const RootLayout = (): ReactElement => {
  // Read once: marking onboarding seen must not close it mid-way.
  const hasSeenOnboarding = useMemo(() => getPrefs().onboardingSeen, [])
  const [onboardingStep, setOnboardingStep] = useState<OnboardingStep>(hasSeenOnboarding ? "none" : "slide-1")
  const [resumeStep, setResumeStep] = useState<OnboardingSlideStep | null>(null)
  const [appDialogLayer, setAppDialogLayer] = useState<HTMLDivElement | null>(null)
  const isOnboardingDialogOpen = onboardingStep !== "none"
    && onboardingStep !== "tour-add-game"
    && onboardingStep !== "tour-import-games"

  useEffect(() => {
    if (!hasSeenOnboarding && onboardingStep === "slide-1") {
      setPref("onboardingSeen", true)
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
    setPref("onboardingSeen", true)
    setResumeStep(null)
    setOnboardingStep("none")
  }

  const handleFinishSlides = (): void => {
    setPref("onboardingSeen", true)
    setResumeStep(null)
    setOnboardingStep("tour-add-game")
  }

  const handleFinishTour = (): void => {
    setPref("onboardingSeen", true)
    setResumeStep(null)
    setOnboardingStep("none")
  }

  return (
    <>
      <Shell titleBar={<TitleBar />} sidebar={<Sidebar />}>
        <Outlet />
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
      </Shell>
      <Toasts />
      <div
        id="app-dialog-layer"
        ref={setAppDialogLayer}
        className={`fixed inset-x-0 top-9 bottom-0 z-[80] ${isOnboardingDialogOpen ? "pointer-events-auto bg-black/60" : "pointer-events-none"}`}
      />
    </>
  )
}

export const Route = createRootRoute({ component: RootLayout })
