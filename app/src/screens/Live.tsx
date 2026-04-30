import { api } from "../api";
import { Card, Eyebrow, Heading, PrimaryButton, StatusDot, Subhead } from "../components/ui";
import { useSessionLog } from "../store";
import type { StateKind } from "../types";

const STAGES: { key: StateKind; label: string }[] = [
  { key: "armed", label: "Armed" },
  { key: "serving", label: "Manifest served" },
  { key: "flashing", label: "Streaming firmware" },
  { key: "verifying", label: "Device flashing" },
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

export function Verify() {
  return (
    <div className="space-y-6">
      <div>
        <Eyebrow>Step 10 · Verify</Eyebrow>
        <Heading>Is CrossPoint running?</Heading>
        <Subhead>
          Your Xteink should have rebooted into CrossPoint. The home screen
          looks recognisably different from stock — the Lyra theme, a
          CrossPoint version string in Settings → System.
        </Subhead>
      </div>
      <Card>
        <ul className="space-y-2 text-sm text-stone-600">
          <li>– Open a book to confirm the reader works.</li>
          <li>– Try changing a font size.</li>
          <li>– Check Settings → System for the CrossPoint version.</li>
        </ul>
      </Card>
      <div className="flex justify-end gap-2">
        <PrimaryButton onClick={() => api.confirmRunning()}>
          Yes, CrossPoint is running
        </PrimaryButton>
      </div>
    </div>
  );
}

export function Done() {
  return (
    <div className="space-y-6">
      <div>
        <Eyebrow>All set</Eyebrow>
        <Heading>CrossPoint is installed. Welcome.</Heading>
        <Subhead>
          You can close Unlocker. Your Wi-Fi has been restored and any
          temporary network changes have been undone.
        </Subhead>
      </div>
      <Card>
        <h2 className="font-serif text-lg font-medium text-stone-900">
          What's next
        </h2>
        <ul className="mt-3 space-y-2 text-sm text-stone-600">
          <li>
            –{" "}
            <a
              href="https://crosspointreader.com"
              target="_blank"
              rel="noreferrer"
              className="text-brand-500 hover:text-brand-600"
            >
              CrossPoint docs
            </a>
            : sync, fonts, plugins.
          </li>
          <li>
            – Calibre plugin for wireless transfers.
          </li>
          <li>
            – Font Builder for custom typefaces.
          </li>
        </ul>
      </Card>
    </div>
  );
}

export function Failed({ error }: { error: string | null }) {
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
      <div className="flex justify-end">
        <PrimaryButton onClick={() => api.cancel()}>Start over</PrimaryButton>
      </div>
    </div>
  );
}
