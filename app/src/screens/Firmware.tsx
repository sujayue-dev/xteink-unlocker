import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import {
  Callout,
  Eyebrow,
  Heading,
  Subhead,
} from "../components/ui";
import type { Catalog, Channel, CrossPointRelease, Locale, Model } from "../types";

export function Firmware({ model, locale }: { model: Model; locale: Locale }) {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [betaOpen, setBetaOpen] = useState(false);

  useEffect(() => {
    api.fetchCatalog().then(setCatalog).catch((e) => setError(String(e)));
  }, []);

  const grouped = useMemo(() => {
    if (!catalog) return null;
    const by = (c: Channel) =>
      catalog.releases
        .filter((r) => r.channel === c)
        .sort((a, b) => b.released_at.localeCompare(a.released_at));
    return {
      stable: by("stable"),
      beta: by("beta"),
      insider: by("insider"),
    };
  }, [catalog]);

  if (error) {
    return (
      <Callout variant="error" title="Couldn't reach crosspointreader.com">
        Check your internet connection and try again. {error}
      </Callout>
    );
  }
  if (!catalog || !grouped) {
    return <p className="text-sm text-stone-500">Loading firmware catalog…</p>;
  }

  async function install(release: CrossPointRelease) {
    setPendingId(release.id);
    try {
      await api.selectFirmware(model, locale, release.id);
    } catch (e) {
      setPendingId(null);
      setError(String(e));
    }
  }

  const betaTapBehavior =
    grouped.beta.length === 0
      ? "none"
      : grouped.beta.length === 1
        ? "install"
        : "expand";

  return (
    <div className="space-y-6">
      <div>
        <Eyebrow>Step 3 · Firmware channel</Eyebrow>
        <Heading>Pick a release</Heading>
        <Subhead>
          Stable is the right pick unless you want to test new features. Beta
          and Insider are opt-in.
        </Subhead>
      </div>

      <div className="grid gap-3">
        <ChannelCard
          title="Stable"
          description="The recommended build. Released after beta testing."
          subtitle={
            grouped.stable[0]
              ? `latest: ${grouped.stable[0].name}`
              : "no stable release available"
          }
          loading={!!pendingId && pendingId === grouped.stable[0]?.id}
          disabled={!grouped.stable[0] || (!!pendingId && pendingId !== grouped.stable[0]?.id)}
          onClick={() => grouped.stable[0] && install(grouped.stable[0])}
        />

        <BetaCard
          betas={grouped.beta}
          open={betaOpen}
          onToggle={() => setBetaOpen((v) => !v)}
          onPick={install}
          tapBehavior={betaTapBehavior}
          pendingId={pendingId}
        />

        <ChannelCard
          title="Insider (nightly)"
          description="Auto-built from master. May be unstable. For testing, not daily reading."
          subtitle={
            grouped.insider[0]
              ? `latest: ${grouped.insider[0].name}`
              : "no nightly build available"
          }
          loading={!!pendingId && pendingId === grouped.insider[0]?.id}
          disabled={!grouped.insider[0] || (!!pendingId && pendingId !== grouped.insider[0]?.id)}
          onClick={() => grouped.insider[0] && install(grouped.insider[0])}
        />
      </div>
    </div>
  );
}

function ChannelCard({
  title,
  description,
  subtitle,
  loading,
  disabled,
  onClick,
}: {
  title: string;
  description: string;
  subtitle: string;
  loading: boolean;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`flex items-center justify-between rounded-xl border p-5 text-left transition disabled:cursor-not-allowed disabled:opacity-50 ${
        loading
          ? "border-brand-500 bg-brand-50 ring-1 ring-brand-500"
          : "border-stone-200 bg-white hover:border-stone-300"
      }`}
    >
      <div>
        <div className="font-serif text-base font-medium text-stone-900">
          {title}
        </div>
        <div className="mt-1 text-sm text-stone-500">{description}</div>
        <div className="mt-2 font-mono text-xs text-stone-400">{subtitle}</div>
      </div>
      <span
        className={`shrink-0 rounded-full px-3 py-1 text-xs font-semibold ${
          loading ? "bg-brand-500 text-white" : "bg-stone-100 text-stone-600"
        }`}
      >
        {loading ? "Installing…" : "Install"}
      </span>
    </button>
  );
}

function BetaCard({
  betas,
  open,
  onToggle,
  onPick,
  tapBehavior,
  pendingId,
}: {
  betas: CrossPointRelease[];
  open: boolean;
  onToggle: () => void;
  onPick: (r: CrossPointRelease) => void;
  tapBehavior: "none" | "install" | "expand";
  pendingId: string | null;
}) {
  const handleClick = () => {
    if (tapBehavior === "install") onPick(betas[0]!);
    else if (tapBehavior === "expand") onToggle();
  };

  const installingThisCard =
    !!pendingId && betas.some((b) => b.id === pendingId);

  return (
    <div
      className={`rounded-xl border transition ${
        installingThisCard
          ? "border-brand-500 bg-brand-50 ring-1 ring-brand-500"
          : "border-stone-200 bg-white"
      }`}
    >
      <button
        type="button"
        onClick={handleClick}
        disabled={tapBehavior === "none" || (!!pendingId && !installingThisCard)}
        className="flex w-full items-center justify-between p-5 text-left disabled:cursor-not-allowed disabled:opacity-50"
      >
        <div>
          <div className="font-serif text-base font-medium text-stone-900">
            Beta
          </div>
          <div className="mt-1 text-sm text-stone-500">
            Pre-release builds. Most features work; some rough edges expected.
          </div>
          <div className="mt-2 font-mono text-xs text-stone-400">
            {tapBehavior === "none"
              ? "no betas right now"
              : tapBehavior === "install"
                ? `1 active: ${betas[0]!.name}`
                : `${betas.length} active builds`}
          </div>
        </div>
        <span
          className={`shrink-0 rounded-full px-3 py-1 text-xs font-semibold ${
            installingThisCard
              ? "bg-brand-500 text-white"
              : "bg-stone-100 text-stone-600"
          }`}
        >
          {installingThisCard
            ? "Installing…"
            : tapBehavior === "expand"
              ? open
                ? "Hide"
                : "Choose"
              : "Install"}
        </span>
      </button>

      {tapBehavior === "expand" && open && (
        <ul className="border-t border-stone-200">
          {betas.map((b) => {
            const isPending = pendingId === b.id;
            const otherPending = !!pendingId && !isPending;
            return (
              <li key={b.id}>
                <button
                  type="button"
                  onClick={() => onPick(b)}
                  disabled={otherPending}
                  className="flex w-full items-start justify-between gap-4 px-5 py-4 text-left transition hover:bg-stone-50 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <div>
                    <div className="text-sm font-medium text-stone-900">
                      {b.name}
                    </div>
                    <div className="mt-1 line-clamp-2 whitespace-pre-line text-xs text-stone-500">
                      {b.notes}
                    </div>
                    <div className="mt-1 font-mono text-[11px] text-stone-400">
                      {b.version} ·{" "}
                      {new Date(b.released_at).toLocaleDateString()}
                    </div>
                  </div>
                  <span
                    className={`shrink-0 rounded-full px-3 py-1 text-xs font-semibold ${
                      isPending
                        ? "bg-brand-500 text-white"
                        : "bg-stone-100 text-stone-600"
                    }`}
                  >
                    {isPending ? "Installing…" : "Install"}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
