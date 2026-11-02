import TalknGoIcon from "./TalknGoIcon";

/** The product name is a brand, not translated text. */
const BRAND_NAME = "TalknGo";

/** Icon plus wordmark, for the sidebar and the welcome screen. `width` is the whole logo's width. */
const TalknGoLogo = ({
  width = 120,
  className = "",
}: {
  width?: number;
  className?: string;
}) => {
  const icon = Math.round(width * 0.27);
  return (
    <span
      className={`inline-flex items-center gap-2 select-none ${className}`}
      style={{ width }}
    >
      <TalknGoIcon width={icon} height={icon} className="shrink-0" />
      <span
        className="font-semibold tracking-tight text-text leading-none"
        style={{ fontSize: Math.round(width * 0.17) }}
      >
        {BRAND_NAME}
      </span>
    </span>
  );
};

export default TalknGoLogo;
