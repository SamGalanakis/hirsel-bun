import { cva, type VariantProps } from "class-variance-authority";
import { type ComponentProps, splitProps } from "solid-js";
import { cn } from "@/lib/cn";

const badgeVariants = cva(
  "group/badge z-badge inline-flex w-fit shrink-0 items-center justify-center overflow-hidden whitespace-nowrap transition-colors",
  {
    variants: {
      variant: {
        default: "z-badge-variant-default",
        secondary: "z-badge-variant-secondary",
        destructive: "z-badge-variant-destructive",
        outline: "z-badge-variant-outline",
        ghost: "z-badge-variant-ghost",
        success: "z-badge-variant-success",
        warning: "z-badge-variant-warning",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  },
);

type BadgeProps = ComponentProps<"span"> & VariantProps<typeof badgeVariants>;

const Badge = (props: BadgeProps) => {
  const [local, others] = splitProps(props, ["class", "variant"]);
  return (
    <span
      class={cn(badgeVariants({ variant: local.variant }), local.class)}
      data-slot="badge"
      data-variant={local.variant}
      {...others}
    />
  );
};

export { Badge, badgeVariants };
export default Badge;
