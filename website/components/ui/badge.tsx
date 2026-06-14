import * as React from "react"
import { Slot } from "@radix-ui/react-slot"

import { cn } from "@/lib/utils"

function Badge({
  className,
  asChild = false,
  ...props
}: React.ComponentProps<"span"> & { asChild?: boolean }) {
  const Comp = asChild ? Slot : "span"

  return (
    <Comp
      data-slot="badge"
      className={cn(className)}
      {...props}
    />
  )
}

export { Badge }
