import { type Component, createSignal } from "solid-js";
import Button from "@/components/ui/button";
import Card, { CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import Input from "@/components/ui/input";
import Label from "@/components/ui/label";

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

      window.location.assign("/");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Connection failed");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div class="workspace-shell relative flex min-h-screen items-center justify-center bg-background px-6">
      <div class="w-full max-w-sm">
        <div class="mb-8 text-center">
          <h1 class="font-display text-2xl font-semibold tracking-tight text-foreground">HIRSEL</h1>
          <p class="mt-2 text-sm text-muted-foreground/70">
            Connect your API key to get started.
          </p>
        </div>

        <Card class="shadow-md">
          <CardContent class="p-6">
            <form onSubmit={handleSubmit} class="space-y-5">
              <div class="space-y-2">
                <Label for="api-key">API Key</Label>
                <Input
                  id="api-key"
                  type="password"
                  placeholder="sk-ant-..."
                  value={apiKey()}
                  onInput={(e) => setApiKey(e.currentTarget.value)}
                  autofocus
                />
                <p class="text-[11px] text-muted-foreground/50">
                  Your key is stored locally and never shared.
                </p>
              </div>

              {error() && (
                <div class="border border-signal-red/20 bg-signal-red/[0.05] px-3 py-2 text-xs text-signal-red">
                  {error()}
                </div>
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
          </CardContent>
        </Card>
      </div>
    </div>
  );
};

export default ConnectPage;
