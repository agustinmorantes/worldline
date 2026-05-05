import { cn } from "@/lib/utils"
import {ReactElement} from "react";
import * as React from "react";

function Skeleton({ className, ...props }: React.ComponentProps<"div">): ReactElement {
  return (
    <div
      data-slot="skeleton"
      className={cn("bg-accent animate-pulse rounded-md", className)}
      {...props}
    />
  )
}

export { Skeleton }
