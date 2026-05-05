import { Loader2Icon } from "lucide-react"

import { cn } from "@/lib/utils"
import {ReactElement} from "react";
import * as React from "react";

function Spinner({ className, ...props }: React.ComponentProps<"svg">): ReactElement {
  return (
    <Loader2Icon
      role="status"
      aria-label="Loading"
      className={cn("size-4 animate-spin", className)}
      {...props}
    />
  )
}

export { Spinner }
