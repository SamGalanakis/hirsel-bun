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
    <div class="flex min-h-screen items-center justify-center bg-background px-6">
      <Card class="w-full max-w-md">
        <CardHeader>
          <CardTitle class="font-display text-3xl tracking-tight">Sign in</CardTitle>
          <p class="text-sm text-muted-foreground">
            Enter your API key to connect.
          </p>
        </CardHeader>

        <CardContent>
          <form onSubmit={handleSubmit} class="space-y-4">
            <div class="space-y-1.5">
              <Label for="api-key">API Key</Label>
              <Input
                id="api-key"
                type="password"
                placeholder="sk-ant-..."
                value={apiKey()}
                onInput={(e) => setApiKey(e.currentTarget.value)}
                autofocus
              />
            </div>

            {error() && <p class="text-xs text-signal-red">{error()}</p>}

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
  );
};

export default ConnectPage;
