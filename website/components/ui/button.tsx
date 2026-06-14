import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { Slot } from "@radix-ui/react-slot"

import { cn } from "@/lib/utils"

const buttonVariants = cva("btn", {
  variants: {
    variant: {
      primary: "btn-primary",
      outline: "btn-outline",
      ghost: "btn-ghost",
    },
    size: {
      default: "",
      sm: "btn-sm",
      icon: "btn-icon",
    },
  },
  defaultVariants: {
    variant: "primary",
    size: "default",
  },
})

function Button({
  className,
  variant,
  size,
  asChild = false,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean
  }) {
  const Comp = asChild ? Slot : "button"

  return (
    <Comp
      data-slot="button"
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
    />
  )
}

export { Button, buttonVariants }
