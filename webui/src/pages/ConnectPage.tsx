import { type Component, createSignal } from "solid-js";
import { cn } from "@/lib/cn";
import Button from "@/components/ui/button";

const ConnectPage: Component = () => {
  const [apiKey, setApiKey] = createSignal("");
  const [error, setError] = createSignal("");
  const [loading, setLoading] = createSignal(false);

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    const key = apiKey().trim();
    if (!key) return;

    setError("");
    setLoading(true);

    try {
      const res = await fetch("/api/connect", {
        method: "POST",
        headers: { "content-type": "application/json" },
        credentials: "same-origin",
        body: JSON.stringify({ api_key: key }),
      });

      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body.message || `Connection failed (${res.status})`);
      }

      // Redirect to main app
      window.location.hash = "";
    } catch (err) {
      setError(err instanceof Error ? err.message : "Connection failed");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div class="flex items-center justify-center min-h-screen bg-background">
      <div class="w-full max-w-sm px-6">
        <h1 class="font-display text-2xl text-foreground mb-1">Hirsel</h1>
        <p class="text-sm text-muted-foreground mb-6">
          Enter your API key to connect.
        </p>

        <form onSubmit={handleSubmit} class="space-y-4">
          <div>
            <label class="chassis-label mb-1.5 block" for="api-key">
              API Key
            </label>
            <input
              id="api-key"
              type="password"
              class={cn(
                "w-full h-[38px] px-3 text-sm font-mono bg-background text-foreground",
                "border border-input placeholder:text-muted-foreground",
                "focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 focus:ring-offset-background",
              )}
              placeholder="sk-ant-..."
              value={apiKey()}
              onInput={(e) => setApiKey(e.currentTarget.value)}
              autofocus
            />
          </div>

          {error() && (
            <p class="text-xs text-signal-red">{error()}</p>
          )}

          <Button
            variant="primary"
            type="submit"
            class="w-full"
            loading={loading()}
            disabled={!apiKey().trim()}
          >
            Connect
          </Button>
        </form>
      </div>
    </div>
  );
};

export default ConnectPage;
