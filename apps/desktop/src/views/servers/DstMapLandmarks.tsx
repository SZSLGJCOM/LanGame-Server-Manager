import { useMemo } from "react";
import { dstLandmarkIconSize, indexDstLandmarks, visibleDstLandmarks, type DstLandmarkViewport } from "../../dst-map-landmarks";
import type { DstWorldMap } from "../../dst-world-map";

interface Props {
  snapshot: DstWorldMap; view: DstLandmarkViewport; unavailableIcons: ReadonlySet<string>; onArtworkFailure: (icon: string) => void;
}

/** Render-only layer: pointer input passes through to the map for dragging and zooming. */
export function DstMapLandmarks({ snapshot, view, unavailableIcons, onArtworkFailure }: Props) {
  const index = useMemo(() => indexDstLandmarks(snapshot.landmarks ?? []), [snapshot.landmarks]);
  const items = useMemo(() => visibleDstLandmarks(index, view), [index, view.centerX, view.centerZ, view.width, view.height, view.scale]);
  const size = dstLandmarkIconSize(view.scale) / Math.max(.01, view.scale);
  return <svg className="dst-map-landmarks" viewBox={`0 0 ${snapshot.width} ${snapshot.height}`} aria-hidden="true">
    {items.map(item => {
      const image = !unavailableIcons.has(item.icon) && snapshot.art && Object.prototype.hasOwnProperty.call(snapshot.art.icons, item.icon)
        ? snapshot.art.icons[item.icon] : undefined;
      return <g key={item.id} data-landmark-id={item.id} transform={`translate(${item.mapX} ${snapshot.height - item.mapZ})`}>
        {image ? <image href={image} x={-size / 2} y={-size / 2} width={size} height={size} onError={() => onArtworkFailure(item.icon)} />
          : <circle className="dst-map-landmark-fallback" r={size / 7} />}
      </g>;
    })}
  </svg>;
}
