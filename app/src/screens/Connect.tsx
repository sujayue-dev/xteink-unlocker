import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { Card, Eyebrow, Heading, StatusDot, Subhead } from "../components/ui";
import { useSessionLog } from "../store";
import type { StateKind } from "../types";

type Phase =
  | "preparing"
  | "hotspot_starting"
  | "waiting_for_sharing"
  | "waiting_for_device"
  | "waiting_for_check";

export function Connect({ state }: { state: StateKind }) {
  const phase: Phase =
    state === "downloading_firmware"
      ? "preparing"
      : state === "setting_up_hotspot"
        ? "hotspot_starting"
        : state === "waiting_for_internet_sharing"
          ? "waiting_for_sharing"
          : state === "awaiting_client"
            ? "waiting_for_device"
            : "waiting_for_check";

  const logs = useSessionLog();

  const [info, setInfo] = useState<{
    ssid: string | null;
    psk: string | null;
    bridge_ip: string | null;
    device_ip: string | null;
  }>({ ssid: null, psk: null, bridge_ip: null, device_ip: null });

  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      const s = await api.getSession();
      if (!cancelled) {
        setInfo({
          ssid: s.ssid,
          psk: s.psk,
          bridge_ip: s.bridge_ip,
          device_ip: s.device_ip,
        });
      }
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  if (phase === "preparing" || phase === "hotspot_starting") {
    return (
      <div className="space-y-6">
        <div>
          <Eyebrow>Step 4 · Preparing</Eyebrow>
          <Heading>
            {phase === "preparing"
              ? "Downloading firmware…"
              : "Setting up the local network…"}
          </Heading>
          <Subhead>
            {phase === "preparing"
              ? "Verifying SHA-256 as it streams. After this Unlocker is fully offline — your Mac can lose internet without affecting the install."
              : "Preparing the virtual network interface…"}
          </Subhead>
        </div>
        <Card>
          <ProgressBar />
        </Card>
        <LogPanel entries={logs} />
      </div>
    );
  }

  if (phase === "waiting_for_sharing") {
    return (
      <div className="space-y-6">
        <div>
          <Eyebrow>Step 4 · Enable Internet Sharing</Eyebrow>
          <Heading>Turn on Internet Sharing</Heading>
          <Subhead>
            Unlocker needs your Mac to act as a Wi-Fi hotspot for your device.
            Follow the steps below — Unlocker will detect it automatically and
            continue.
          </Subhead>
        </div>

        <ol className="space-y-3">
          <Step n={1} title="Open Internet Sharing" done={false} active={true}>
            <strong>System Settings → General → Sharing → Internet Sharing</strong>.
            If Internet Sharing is already on, turn it off first — you
            can't change settings while it's active.
          </Step>

          <Step n={2} title="Configure sharing" done={false} active={true}>
            Set <strong>Share your connection from</strong> to{" "}
            <span className="rounded bg-stone-100 px-1.5 py-0.5 font-mono text-xs text-stone-700">
              UnlockerUpstream
            </span>{" "}
            and check{" "}
            <strong>Wi-Fi</strong> in the "To devices using" list. Click{" "}
            <strong>Wi-Fi Options</strong> and set a simple password like{" "}
            <span className="font-mono text-stone-700">11111111</span>{" "}
            — you'll need to type this on your Xteink.
          </Step>

          <Step n={3} title="Turn it on" done={false} active={true}>
            Toggle Internet Sharing on and click Start when macOS asks to
            confirm. Your Mac's Wi-Fi will disconnect — this is expected.
          </Step>
        </ol>

        <Card>
          <div className="flex items-center gap-3">
            <StatusDot variant="active" />
            <p className="text-sm text-stone-500">
              Waiting for Internet Sharing to start…
            </p>
          </div>
        </Card>
        <LogPanel entries={logs} />
      </div>
    );
  }

  const deviceConnected = phase === "waiting_for_check";

  return (
    <div className="space-y-6">
      <div>
        <Eyebrow>Step 4 · Connect your Xteink</Eyebrow>
        <Heading>Two quick steps on your device</Heading>
        <Subhead>
          Unlocker is now serving a local network for your Xteink. Follow the
          steps below — the install will start as soon as your device asks for
          an update.
        </Subhead>
      </div>

      <Card>
        <div className="grid grid-cols-2 gap-3">
          <InfoBox label="Network name" value={info.ssid ?? "…"} />
          <InfoBox label="Password" value={info.psk ?? "…"} />
        </div>
      </Card>

      <ol className="space-y-3">
        <Step
          n={1}
          title="Join the network on your Xteink"
          done={deviceConnected}
          active={!deviceConnected}
        >
          Settings → Wi-Fi → tap{" "}
          <span className="font-mono text-stone-700">{info.ssid ?? "the network"}</span>
          , enter the password.
          {deviceConnected && info.device_ip && (
            <span className="ml-2 text-xs text-brand-500">
              connected ({info.device_ip})
            </span>
          )}
        </Step>

        <Step
          n={2}
          title="Tap Check for Updates"
          done={false}
          active={deviceConnected}
        >
          Settings → System → Check for Updates. Unlocker will detect the
          request and continue automatically.
        </Step>
      </ol>

      <LogPanel entries={logs} />
    </div>
  );
}

function LogPanel({ entries }: { entries: { ts: string; level: string; message: string }[] }) {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [entries.length]);

  if (entries.length === 0) return null;

  return (
    <div className="rounded-xl border border-stone-200 bg-stone-950 p-4">
      <div className="max-h-48 overflow-y-auto font-mono text-xs leading-5">
        {entries.map((e, i) => (
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
        <div ref={bottomRef} />
      </div>
    </div>
  );
}

function Step({
  n,
  title,
  done,
  active,
  children,
}: {
  n: number;
  title: string;
  done: boolean;
  active: boolean;
  children: React.ReactNode;
}) {
  return (
    <li className="flex gap-4 rounded-xl border border-stone-200 bg-white p-4">
      <span
        className={`flex size-7 shrink-0 items-center justify-center rounded-full font-mono text-xs font-semibold ${
          done
            ? "bg-brand-100 text-brand-700"
            : active
              ? "bg-brand-500 text-white"
              : "bg-stone-200 text-stone-500"
        }`}
      >
        {done ? "✓" : n}
      </span>
      <div className="flex-1">
        <div className="flex items-center gap-2 text-sm font-medium text-stone-900">
          {title}
          {active && !done && <StatusDot variant="active" />}
        </div>
        <div className="mt-1 text-sm text-stone-600">{children}</div>
      </div>
    </li>
  );
}

function InfoBox({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg bg-stone-50 px-4 py-3">
      <div className="text-xs font-medium uppercase tracking-wide text-stone-400">
        {label}
      </div>
      <div className="mt-1 break-all font-mono text-sm font-semibold text-stone-900">
        {value}
      </div>
    </div>
  );
}

function ProgressBar() {
  return (
    <div className="space-y-3">
      <div className="h-2 overflow-hidden rounded-full bg-stone-100">
        <div className="h-full w-2/3 animate-pulse rounded-full bg-brand-500" />
      </div>
      <p className="text-sm text-stone-500">Working…</p>
    </div>
  );
}
