import { useState } from "react";
import { api } from "../api";
import {
  Card,
  Eyebrow,
  Heading,
  PrimaryButton,
  Subhead,
} from "../components/ui";
import type { Locale, Model } from "../types";

export function DeviceAndRegion() {
  const [model, setModel] = useState<Model | null>(null);
  const [locale, setLocale] = useState<Locale | null>(null);

  const ready = model && locale;

  return (
    <div className="space-y-6">
      <div>
        <Eyebrow>Step 2 · Your device</Eyebrow>
        <Heading>Which Xteink do you have?</Heading>
        <Subhead>
          Unlocker uses this to spoof the right update server and to label the
          firmware download with the right model code.
        </Subhead>
      </div>

      <Card>
        <h2 className="font-serif text-lg font-medium text-stone-900">Model</h2>
        <p className="mt-1 text-sm text-stone-500">
          Not sure? Look at the back of your device — the model number is
          printed there.
        </p>
        <div className="mt-4 grid grid-cols-2 gap-3">
          <DeviceCard
            title="Xteink X3"
            selected={model === "x3"}
            onClick={() => setModel("x3")}
          />
          <DeviceCard
            title="Xteink X4"
            selected={model === "x4"}
            onClick={() => setModel("x4")}
          />
        </div>
      </Card>

      <Card>
        <h2 className="font-serif text-lg font-medium text-stone-900">
          Region
        </h2>
        <p className="mt-1 text-sm text-stone-500">
          Which language did your device come with from the factory?
        </p>
        <div className="mt-4 grid grid-cols-2 gap-3">
          <DeviceCard
            title="English"
            subtitle="Overseas firmware"
            selected={locale === "english"}
            onClick={() => setLocale("english")}
          />
          <DeviceCard
            title="Chinese"
            subtitle="Domestic firmware"
            selected={locale === "chinese"}
            onClick={() => setLocale("chinese")}
          />
        </div>
      </Card>

      <div className="flex justify-end">
        <PrimaryButton
          disabled={!ready}
          onClick={() => model && locale && api.selectDevice(model, locale)}
        >
          Continue
        </PrimaryButton>
      </div>
    </div>
  );
}

function DeviceCard({
  title,
  subtitle,
  selected,
  onClick,
}: {
  title: string;
  subtitle?: string;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`rounded-xl border p-4 text-left transition ${
        selected
          ? "border-brand-500 bg-brand-50 ring-1 ring-brand-500"
          : "border-stone-200 bg-white hover:border-stone-300"
      }`}
    >
      <div className="font-serif text-base font-medium text-stone-900">
        {title}
      </div>
      {subtitle && (
        <div className="mt-1 text-sm text-stone-500">{subtitle}</div>
      )}
    </button>
  );
}
