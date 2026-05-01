import { useState } from "react";
import { api } from "../api";
import {
  Callout,
  Card,
  Eyebrow,
  Heading,
  PrimaryButton,
  SecondaryButton,
  Subhead,
} from "../components/ui";

export function Consent() {
  const [general, setGeneral] = useState(false);
  const [recovery, setRecovery] = useState(false);
  const [repairing, setRepairing] = useState(false);
  const [repairMessage, setRepairMessage] = useState<string | null>(null);
  const [removingHelper, setRemovingHelper] = useState(false);
  const [helperMessage, setHelperMessage] = useState<string | null>(null);

  const ready = general && recovery;

  async function onRepair() {
    setRepairing(true);
    setRepairMessage(null);
    try {
      await api.repairSystem();
      setRepairMessage("Network repair complete. localhost should be restored.");
    } catch (e) {
      setRepairMessage(`Repair failed: ${String(e)}`);
    } finally {
      setRepairing(false);
    }
  }

  async function onRemoveHelper() {
    setRemovingHelper(true);
    setHelperMessage(null);
    try {
      await api.uninstallHelper();
      window.dispatchEvent(new Event("helper-uninstalled"));
    } catch (e) {
      setHelperMessage(`Could not stop helper: ${String(e)}`);
    } finally {
      setRemovingHelper(false);
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <Eyebrow>Step 1 · Welcome</Eyebrow>
        <Heading>Install CrossPoint on your Xteink</Heading>
        <Subhead>
          Unlocker installs CrossPoint by intercepting your device's own update
          mechanism on a local network. It is the install path for devices
          where USB flashing has been disabled.
        </Subhead>
      </div>

      <Card>
        <h2 className="font-serif text-lg font-medium text-stone-900">
          What Unlocker will do
        </h2>
        <ul className="mt-3 space-y-2 text-sm/6 text-stone-600">
          <li>
            – Briefly disconnect your Mac's Wi-Fi to create a private hotspot
            for your device. Wired Ethernet is unaffected.
          </li>
          <li>
            – Replace the firmware on the Xteink with CrossPoint, using the
            device's own native update mechanism.
          </li>
        </ul>

        <div className="mt-6 space-y-3">
          <label className="flex items-start gap-3 text-sm/6 text-stone-700">
            <input
              type="checkbox"
              checked={general}
              onChange={(e) => setGeneral(e.target.checked)}
              className="mt-1 size-4 rounded border-stone-300 text-brand-500 focus:ring-brand-500"
            />
            <span>
              I understand Unlocker will modify network settings on my Mac and
              install firmware on my device.
            </span>
          </label>
          <label className="flex items-start gap-3 text-sm/6 text-stone-700">
            <input
              type="checkbox"
              checked={recovery}
              onChange={(e) => setRecovery(e.target.checked)}
              className="mt-1 size-4 rounded border-stone-300 text-brand-500 focus:ring-brand-500"
            />
            <span>
              I understand that if my device has the USB lockdown,{" "}
              <strong>recovery from a failed install is significantly harder</strong>
              . In the worst case, a failed install can leave the device
              non-functional.
            </span>
          </label>
        </div>
      </Card>

      <Callout variant="warn" title="Already tried the WebSerial flasher?">
        If your Xteink wouldn't enter download mode and the flasher couldn't
        connect, that's the lockdown Unlocker is designed for. You're in the
        right place.{" "}
        <a
          href="https://crosspointreader.com#flash-tools"
          target="_blank"
          rel="noreferrer"
          className="font-medium underline underline-offset-2"
        >
          If USB still works for you
        </a>
        , the WebSerial flasher is a safer first try.
      </Callout>

      <Card>
        <h2 className="font-serif text-lg font-medium text-stone-900">
          Repair this Mac's network settings
        </h2>
        <p className="mt-2 text-sm text-stone-600">
          If Unlocker was closed mid-session and localhost or Wi-Fi routing
          stopped working, run a cleanup pass here to restore loopback and tear
          down Unlocker-managed network changes.
        </p>
        <div className="mt-5 flex items-center justify-between gap-4">
          <p className="text-sm text-stone-500">
            This is safe to run even if no install is active.
          </p>
          <SecondaryButton onClick={onRepair} disabled={repairing}>
            {repairing ? "Repairing…" : "Repair network settings"}
          </SecondaryButton>
        </div>
        {repairMessage && (
          <p className="mt-3 text-sm text-stone-600">{repairMessage}</p>
        )}
      </Card>

      <Card>
        <h2 className="font-serif text-lg font-medium text-stone-900">
          Stop the privileged helper
        </h2>
        <p className="mt-2 text-sm text-stone-600">
          Unlocker leaves a small background helper running with admin rights
          so subsequent runs do not need a password. Stop it if you would
          rather have it gone — you will be prompted again next time you
          launch the app.
        </p>
        <div className="mt-5 flex items-center justify-between gap-4">
          <p className="text-sm text-stone-500">
            Safe to run any time. No effect if the helper is not running.
          </p>
          <SecondaryButton onClick={onRemoveHelper} disabled={removingHelper}>
            {removingHelper ? "Stopping…" : "Stop helper"}
          </SecondaryButton>
        </div>
        {helperMessage && (
          <p className="mt-3 text-sm text-stone-600">{helperMessage}</p>
        )}
      </Card>

      <div className="flex justify-end">
        <PrimaryButton
          disabled={!ready}
          onClick={() => api.acceptConsent(general, recovery)}
        >
          Continue
        </PrimaryButton>
      </div>
    </div>
  );
}
