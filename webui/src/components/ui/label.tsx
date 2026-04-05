import { type ComponentProps, splitProps } from "solid-js";
import { cn } from "@/lib/cn";

type LabelProps = ComponentProps<"label">;

const Label = (props: LabelProps) => {
  const [local, others] = splitProps(props, ["class"]);
  return (
    <label
      class={cn(
        "z-label flex select-none items-center peer-disabled:cursor-not-allowed",
        local.class,
      )}
      data-slot="label"
      {...others}
    />
  );
};

export { Label };
export default Label;
