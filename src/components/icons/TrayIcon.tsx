/** The tray icon (a waveform, like src-tauri/resources/tray_*.png); uses currentColor like a macOS template image. */
type TrayIconState = "idle" | "recording" | "transcribing";

const TrayIcon = ({
  state,
  size,
}: {
  state: TrayIconState;
  size: number | string;
}) => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 64 64"
    className="fill-current"
    xmlns="http://www.w3.org/2000/svg"
  >
    {state === "transcribing" ? (
      <>
        <circle cx="14" cy="32" r="6" />
        <circle cx="32" cy="32" r="6" />
        <circle cx="50" cy="32" r="6" />
      </>
    ) : (
      <>
        <rect x="4.9" y="24.2" width="5" height="15.7" rx="2.5" />
        <rect x="13.1" y="18.0" width="5" height="28.0" rx="2.5" />
        <rect x="21.3" y="12.4" width="5" height="39.2" rx="2.5" />
        <rect x="29.5" y="5.4" width="5" height="53.2" rx="2.5" />
        <rect x="37.7" y="12.4" width="5" height="39.2" rx="2.5" />
        <rect x="45.9" y="18.0" width="5" height="28.0" rx="2.5" />
        <rect x="54.1" y="24.2" width="5" height="15.7" rx="2.5" />
      </>
    )}
    {state === "recording" && <circle cx="54" cy="10" r="8" />}
  </svg>
);

export default TrayIcon;
