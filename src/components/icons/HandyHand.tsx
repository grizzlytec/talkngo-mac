import TalknGoIcon from "./TalknGoIcon";

/** Kept under its old name so callers stay unchanged: now the TalknGo icon. */
const HandyHand = ({
  width,
  height,
  className,
}: {
  width?: number | string;
  height?: number | string;
  className?: string;
}) => <TalknGoIcon width={width} height={height} className={className} />;

export default HandyHand;
