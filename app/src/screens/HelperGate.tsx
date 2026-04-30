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

type Phase =
  | "checking"
  | "needs_install"
  | "registering"
  | "needs_approval"
  | "ready"
  | "error";

const checkStatus = () => invoke<HelperStatus>("helper_status");
const installHelper = () => invoke<void>("install_helper");

export function HelperGate({ children }: { children: React.ReactNode }) {
  const [phase, setPhase] = useState<Phase>("checking");
  const [status, setStatus] = useState<HelperStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Poll status. Re-runs whenever phase changes to a polling phase.
  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;

    async function tick() {
      try {
        const s = await checkStatus();
        if (cancelled) return;
        setStatus(s);
        if (s.installed && s.socket_reachable) {
          setPhase("ready");
          return;
        }
        if (s.status_label === "requires_approval") {
          setPhase("needs_approval");
        } else if (
          phase === "checking" ||
          phase === "registering" ||
          phase === "needs_approval"
        ) {
          if (s.status_label === "not_registered" && phase === "checking") {
            setPhase("needs_install");
            return;
          }
          // Keep polling — the helper may still be starting after registration.
          timer = setTimeout(tick, 1500);
        } else {
          setPhase("needs_install");
        }
      } catch (e) {
        if (cancelled) return;
        setError(String(e));
        setPhase("error");
      }
    }

    if (phase === "checking" || phase === "registering" || phase === "needs_approval") {
      tick();
    }

    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [phase]);

  async function onInstall() {
    setError(null);
    setPhase("registering");
    try {
      await installHelper();
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
        <div className="flex size-7 items-center justify-center rounded-md bg-brand-500 font-serif text-sm font-medium text-white">
          X
        </div>
        <span className="text-sm/6 font-medium tracking-tight text-stone-900">
          Xteink Unlocker
        </span>
      </header>

      <main className="mt-10 space-y-6">
        <div>
          <Eyebrow>One-time setup</Eyebrow>
          <Heading>Approve the privileged helper</Heading>
          <Subhead>
            Unlocker needs a small background helper to manage your Mac's
            network during the install. The helper is signed by us and only
            runs while Unlocker is open. You only do this once.
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
              Install the helper
            </h2>
            <p className="mt-2 text-sm text-stone-600">
              When you click Install, macOS will register the helper and may
              ask you to approve it in System Settings → Login Items. After
              you approve, return here and Unlocker will continue
              automatically.
            </p>
            <div className="mt-5 flex justify-end">
              <PrimaryButton onClick={onInstall}>Install helper</PrimaryButton>
            </div>
          </Card>
        )}

        {phase === "registering" && (
          <Card>
            <p className="text-sm text-stone-500">
              Registering helper… If macOS shows a dialog, follow the prompt.
            </p>
          </Card>
        )}

        {phase === "needs_approval" && (
          <Card>
            <h2 className="font-serif text-lg font-medium text-stone-900">
              Approve in System Settings
            </h2>
            <p className="mt-2 text-sm text-stone-600">
              The helper is registered but waiting for your approval. Open{" "}
              <strong>System Settings → General → Login Items &amp; Extensions</strong>
              , find <span className="font-mono">Xteink Unlocker</span> under
              Allow in the Background, and turn it on. Unlocker will continue
              automatically.
            </p>
            <p className="mt-3 text-xs text-stone-400">
              Current status: {status?.status_label ?? "unknown"}
            </p>
            <div className="mt-5 flex justify-end gap-2">
              <SecondaryButton onClick={() => setPhase("checking")}>
                I approved it
              </SecondaryButton>
            </div>
          </Card>
        )}

        {phase === "error" && (
          <Callout variant="error" title="Couldn't set up the helper">
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
