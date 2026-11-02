import iconUrl from "../../assets/talkngo-icon.png";

/** The TalknGo app icon as an image. Ignores the stroke/fill props icons usually get. */
const TalknGoIcon = ({
  width,
  height,
  className,
}: {
  width?: number | string;
  height?: number | string;
  className?: string;
}) => (
  <img
    src={iconUrl}
    alt=""
    width={width ?? 24}
    height={height ?? width ?? 24}
    draggable={false}
    className={className}
  />
);

export default TalknGoIcon;
