import { PixelIcon } from "./Pixel";

export function PrivacyNote() {
  return (
    <section className="wrap privacy-wrap" aria-label="Privacy">
      <div className="panel privacy">
        <span className="accent">
          <PixelIcon name="lock" size={36} />
        </span>
        <div>
          <div className="label label-accent">privacy</div>
          <p>
            <strong>Private:</strong> who entered, which entries belong to the same person, and who won. <strong>Public by design:</strong> each entry's amount and ticket
            count, and every payout amount, via the published testnet viewing key, so anyone can verify the draw.
          </p>
        </div>
      </div>
    </section>
  );
}
