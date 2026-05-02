import { ReactNode } from "react";
import { api } from "../api";
import { SecondaryButton } from "./ui";
import { CheckForUpdatesLink } from "./CheckForUpdatesLink";
import type { StateKind } from "../types";

const STEPS: { id: string; label: string; states: StateKind[] }[] = [
  { id: "consent", label: "Consent", states: ["consenting"] },
  {
    id: "device",
    label: "Device",
    states: ["selecting_device_and_region", "selecting_firmware"],
  },
  {
    id: "connect",
    label: "Connect",
    states: [
      "downloading_firmware",
      "setting_up_hotspot",
      "waiting_for_internet_sharing",
      "awaiting_client",
      "awaiting_device_request",
    ],
  },
  {
    id: "install",
    label: "Install",
    states: ["armed", "serving", "flashing"],
  },
  { id: "verify", label: "Verify", states: ["verifying", "done"] },
];

function activeIndex(state: StateKind): number {
  const idx = STEPS.findIndex((s) => s.states.includes(state));
  return idx === -1 ? 0 : idx;
}

export function Shell({
  state,
  children,
}: {
  state: StateKind;
  children: ReactNode;
}) {
  const idx = activeIndex(state);
  const isTerminal = state === "done" || state === "failed" || state === "idle";
  return (
    <div className="mx-auto flex min-h-full max-w-3xl flex-col px-6 py-8">
      <header className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <img src="/logo.png" alt="" className="size-7 rounded-md" />
          <span className="text-sm/6 font-medium tracking-tight text-stone-900">
            Xteink Unlocker
          </span>
          <span className="ml-1 inline-flex items-center rounded-full bg-amber-100 px-2 py-0.5 text-xs font-semibold text-amber-800">
            Beta
          </span>
        </div>
        {!isTerminal && (
          <SecondaryButton onClick={() => api.cancel()}>
            Cancel and clean up
          </SecondaryButton>
        )}
      </header>

      <nav className="mt-6">
        <ol className="flex items-center gap-3 text-xs">
          {STEPS.map((step, i) => {
            const isActive = i === idx;
            const isDone = i < idx;
            return (
              <li key={step.id} className="flex items-center gap-3">
                <span
                  className={`flex size-6 items-center justify-center rounded-full font-mono text-[11px] font-semibold ${
                    isActive
                      ? "bg-brand-500 text-white"
                      : isDone
                        ? "bg-brand-100 text-brand-700"
                        : "bg-stone-200 text-stone-500"
                  }`}
                >
                  {isDone ? "✓" : i + 1}
                </span>
                <span
                  className={
                    isActive
                      ? "font-medium text-stone-900"
                      : isDone
                        ? "text-stone-500"
                        : "text-stone-400"
                  }
                >
                  {step.label}
                </span>
                {i < STEPS.length - 1 && (
                  <span className="h-px w-6 bg-stone-200" />
                )}
              </li>
            );
          })}
        </ol>
      </nav>

      <main className="mt-8 flex-1">{children}</main>

      <footer className="mt-10 flex items-center justify-between text-xs text-stone-400">
        <span>CrossPoint Reader · MIT licensed</span>
        <div className="flex items-center gap-4">
          <CheckForUpdatesLink />
          <a
            href="https://crosspointreader.com"
            className="hover:text-stone-600"
            target="_blank"
            rel="noreferrer"
          >
            crosspointreader.com
          </a>
        </div>
      </footer>
    </div>
  );
}
