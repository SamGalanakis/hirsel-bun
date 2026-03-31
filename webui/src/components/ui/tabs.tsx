import { Tabs as KTabs } from "@kobalte/core/tabs";
import type { ComponentProps } from "solid-js";
import { splitProps } from "solid-js";
import { cn } from "@/lib/cn";

/* ── Root ── */
const Tabs = (props: ComponentProps<typeof KTabs>) => {
  const [local, others] = splitProps(props, ["class"]);
  return <KTabs class={cn("flex flex-col", local.class)} {...others} />;
};

/* ── List ── */
const TabsList = (props: ComponentProps<typeof KTabs.List>) => {
  const [local, others] = splitProps(props, ["class"]);
  return (
    <KTabs.List
      class={cn("flex border-b border-border", local.class)}
      {...others}
    />
  );
};

/* ── Trigger ── */
const TabsTrigger = (props: ComponentProps<typeof KTabs.Trigger>) => {
  const [local, others] = splitProps(props, ["class"]);
  return (
    <KTabs.Trigger
      class={cn(
        "font-mono text-[11px] uppercase tracking-[0.1em] px-4 py-2.5",
        "text-muted-foreground transition-colors",
        "border-b-[2px] border-transparent -mb-[1.5px]",
        "hover:text-foreground",
        "data-[selected]:text-foreground data-[selected]:border-foreground",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
        "disabled:pointer-events-none disabled:opacity-50",
        local.class,
      )}
      {...others}
    />
  );
};

/* ── Content ── */
const TabsContent = (props: ComponentProps<typeof KTabs.Content>) => {
  const [local, others] = splitProps(props, ["class"]);
  return (
    <KTabs.Content
      class={cn(
        "pt-6 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2",
        local.class,
      )}
      {...others}
    />
  );
};

export default Tabs;
export { TabsList, TabsTrigger, TabsContent };
