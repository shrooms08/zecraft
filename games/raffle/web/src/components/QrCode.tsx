import { useEffect, useState } from "react";
import QRCode from "qrcode";

/** A QR code drawn at 240 px and shown at `size` px. */
export function QrCode({ value, label, size = 240 }: { value: string; label: string; size?: number }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    QRCode.toDataURL(value, { margin: 1, width: 240, errorCorrectionLevel: "M" })
      .then((url) => live && setSrc(url))
      .catch(() => live && setSrc(null));
    return () => {
      live = false;
    };
  }, [value]);
  return src ? <img className="qr" src={src} width={size} height={size} alt={label} /> : <span className="qr qr-loading" style={{ width: size, height: size }} />;
}
