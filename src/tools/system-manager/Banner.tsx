export type BannerVariant = "info" | "warning" | "error";

interface BannerProps {
  variant: BannerVariant;
  message: string;
  onDismiss: () => void;
}

/** Dismissible banner shown above a section's content. Variant decides the
 * color: info (neutral), warning (yellow), error (red). */
export default function Banner({ variant, message, onDismiss }: BannerProps) {
  return (
    <div className={`sm-banner sm-banner-${variant}`} role="status">
      <span className="sm-banner-message">{message}</span>
      <button
        type="button"
        className="sm-banner-dismiss"
        onClick={onDismiss}
        aria-label="Dismiss message"
      >
        Dismiss
      </button>
    </div>
  );
}
