import { api } from "../api";
import {
  Card,
  Eyebrow,
  Heading,
  PrimaryButton,
  StatusDot,
  Subhead,
} from "../components/ui";
import { useSessionLog } from "../store";
import { useState } from "react";
import type { StateKind } from "../types";

const STAGES: { key: StateKind; label: string }[] = [
  { key: "armed", label: "Armed" },
  { key: "serving", label: "Manifest served" },
];

export function Live({ state }: { state: StateKind }) {
  const log = useSessionLog();
  const activeIdx = STAGES.findIndex((s) => s.key === state);

  return (
    <div className="space-y-6">
      <div>
        <Eyebrow>Step 9 · Installing</Eyebrow>
        <Heading>CrossPoint is installing on your device</Heading>
        <Subhead>
          Don't disconnect your Xteink. It will reboot automatically when the
          install is complete.
        </Subhead>
      </div>
      <Card>
        <ul className="space-y-3">
          {STAGES.map((s, i) => {
            const isDone = i < activeIdx;
            const isActive = i === activeIdx;
            return (
              <li key={s.key} className="flex items-center gap-3 text-sm">
                <StatusDot
                  variant={isDone ? "ok" : isActive ? "active" : "idle"}
                />
                <span
                  className={
                    isActive
                      ? "font-medium text-stone-900"
                      : isDone
                        ? "text-stone-500"
                        : "text-stone-400"
                  }
                >
                  {s.label}
                </span>
              </li>
            );
          })}
        </ul>
      </Card>

      <Card className="!p-5">
        <div className="text-xs font-medium uppercase tracking-wide text-stone-400">
          Live log
        </div>
        <div className="mt-2 max-h-48 overflow-auto rounded-md bg-stone-50 p-3 font-mono text-xs text-stone-700">
          {log.length === 0 ? (
            <span className="text-stone-400">no events yet…</span>
          ) : (
            log.map((e, i) => (
              <div key={i} className="whitespace-pre-wrap">
                <span className="text-stone-400">
                  {new Date(e.ts).toLocaleTimeString()}
                </span>{" "}
                {e.message}
              </div>
            ))
          )}
        </div>
      </Card>
    </div>
  );
}

export function Done() {
  const [cleaning, setCleaning] = useState(false);
  const [cleanupMessage, setCleanupMessage] = useState<string | null>(null);

  async function onCleanup() {
    setCleaning(true);
    setCleanupMessage(null);
    try {
      await api.cleanupAfterInstall();
      setCleanupMessage("Cleanup complete. You can close Unlocker.");
    } catch (e) {
      setCleanupMessage(`Cleanup failed: ${String(e)}`);
    } finally {
      setCleaning(false);
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <Eyebrow>Final step</Eyebrow>
        <Heading>Check your Xteink, then clean up this Mac</Heading>
        <Subhead>
          Unlocker can only confirm that the device started its own updater.
          The final result is visible on the Xteink itself, not from the Mac.
        </Subhead>
      </div>
      <Card>
        <h2 className="font-serif text-lg font-medium text-stone-900">
          On your device
        </h2>
        <ul className="mt-3 space-y-2 text-sm text-stone-600">
          <li>– Wait for the update to finish or for the device to reboot.</li>
          <li>– Check whether the CrossPoint home screen appears.</li>
          <li>– If it booted, open Settings → System and confirm the version.</li>
          <li>– If it did not boot, leave this screen open and follow your recovery path.</li>
        </ul>
      </Card>
      <Card>
        <h2 className="font-serif text-lg font-medium text-stone-900">
          On this Mac
        </h2>
        <ul className="mt-3 space-y-2 text-sm text-stone-600">
          <li>– Turn Internet Sharing off in System Settings if it is still on.</li>
          <li>– Click the button below to tear down Unlocker’s local network changes.</li>
        </ul>
        <div className="mt-5 flex justify-end">
          <PrimaryButton onClick={onCleanup} disabled={cleaning}>
            {cleaning
              ? "Cleaning up…"
              : "Turn off Internet Sharing and clean up"}
          </PrimaryButton>
        </div>
        {cleanupMessage && (
          <p className="mt-3 text-sm text-stone-600">{cleanupMessage}</p>
        )}
      </Card>
    </div>
  );
}

export function Failed({ error }: { error: string | null }) {
  const log = useSessionLog();

  return (
    <div className="space-y-6">
      <div>
        <Eyebrow>Something went wrong</Eyebrow>
        <Heading>Install failed</Heading>
        <Subhead>
          Unlocker has rolled back any network changes on your Mac. Your
          device may have its own rollback path; see below.
        </Subhead>
      </div>
      <Card>
        <p className="text-sm text-stone-700">
          <strong>What happened:</strong>{" "}
          <span className="text-stone-600">{error ?? "Unknown error"}</span>
        </p>
        <p className="mt-3 text-sm text-stone-600">
          If the install was interrupted partway through, your device may
          auto-rollback to stock on next boot. If your device has working USB,
          you can also recover via the WebSerial flasher's full-flash restore.
        </p>
      </Card>
      {log.length > 0 && (
        <div className="rounded-xl border border-stone-200 bg-stone-950 p-4">
          <div className="max-h-48 overflow-y-auto font-mono text-xs leading-5">
            {log.map((e, i) => (
              <div key={i} className="flex gap-2">
                <span className="shrink-0 text-stone-500">
                  {new Date(e.ts).toLocaleTimeString()}
                </span>
                <span
                  className={
                    e.level === "warn"
                      ? "text-amber-400"
                      : e.level === "error"
                        ? "text-red-400"
                        : "text-stone-300"
                  }
                >
                  {e.message}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
      <div className="flex justify-end">
        <PrimaryButton onClick={() => api.cancel()}>Start over</PrimaryButton>
      </div>
    </div>
  );
}
