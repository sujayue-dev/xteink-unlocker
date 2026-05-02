import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Card,
  Eyebrow,
  Heading,
  PrimaryButton,
  SecondaryButton,
  Subhead,
  Callout,
} from "../components/ui";

interface HelperStatus {
  installed: boolean;
  status_label: string;
  socket_reachable: boolean;
}

type Phase = "checking" | "needs_install" | "registering" | "ready" | "error";

const checkStatus = () => invoke<HelperStatus>("helper_status");
const installHelper = () => invoke<void>("install_helper");

// Dev escape hatch: set VITE_SKIP_HELPER=1 (or pass it inline to npm run tauri dev)
// to walk through the UI without installing the privileged helper. Only honored
// when Vite is running in dev mode, so it can never leak into a release build.
const SKIP_HELPER =
  import.meta.env.DEV && import.meta.env.VITE_SKIP_HELPER === "1";

export function HelperGate({ children }: { children: React.ReactNode }) {
  const [phase, setPhase] = useState<Phase>(SKIP_HELPER ? "ready" : "checking");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;

    async function tick() {
      try {
        const s = await checkStatus();
        if (cancelled) return;
        if (s.socket_reachable) {
          setPhase("ready");
          return;
        }
        if (phase === "checking") {
          setPhase("needs_install");
          return;
        }
        // Still waiting for the helper to come up after install.
        timer = setTimeout(tick, 1500);
      } catch (e) {
        if (cancelled) return;
        setError(String(e));
        setPhase("error");
      }
    }

    if (phase === "checking" || phase === "registering") {
      tick();
    }

    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [phase]);

  useEffect(() => {
    const onUninstalled = () => {
      setError(null);
      setPhase("checking");
    };
    window.addEventListener("helper-uninstalled", onUninstalled);
    return () => window.removeEventListener("helper-uninstalled", onUninstalled);
  }, []);

  async function onInstall() {
    setError(null);
    setPhase("registering");
    try {
      await installHelper();
      // install_helper waits 500ms for the socket; re-check now.
      setPhase("checking");
    } catch (e) {
      setError(String(e));
      setPhase("error");
    }
  }

  if (phase === "ready") {
    return <>{children}</>;
  }

  return (
    <div className="mx-auto flex min-h-full max-w-2xl flex-col px-6 py-12">
      <header className="flex items-center gap-2">
        <img src="/logo.png" alt="" className="size-7 rounded-md" />
        <span className="text-sm/6 font-medium tracking-tight text-stone-900">
          Xteink Unlocker
        </span>
      </header>

      <main className="mt-10 space-y-6">
        <div>
          <Eyebrow>One-time setup</Eyebrow>
          <Heading>Start the privileged helper</Heading>
          <Subhead>
            Unlocker needs a small background helper to manage your Mac's
            network during the install. macOS will ask for your password to
            authorize it.
          </Subhead>
        </div>

        {phase === "checking" && (
          <Card>
            <p className="text-sm text-stone-500">Checking helper status…</p>
          </Card>
        )}

        {phase === "needs_install" && (
          <Card>
            <h2 className="font-serif text-lg font-medium text-stone-900">
              Start the helper
            </h2>
            <p className="mt-2 text-sm text-stone-600">
              Click Start and enter your Mac password when prompted. The helper
              runs only while Unlocker is open.
            </p>
            <div className="mt-5 flex justify-end">
              <PrimaryButton onClick={onInstall}>Start helper</PrimaryButton>
            </div>
          </Card>
        )}

        {phase === "registering" && (
          <Card>
            <p className="text-sm text-stone-500">
              Starting helper… Enter your password if macOS prompts you.
            </p>
          </Card>
        )}

        {phase === "error" && (
          <Callout variant="error" title="Couldn't start the helper">
            {error ?? "Unknown error."}
            <div className="mt-3 flex gap-2">
              <SecondaryButton onClick={() => setPhase("checking")}>
                Retry
              </SecondaryButton>
            </div>
          </Callout>
        )}
      </main>
    </div>
  );
}
