import type { CSSProperties } from "react";
import { cn } from "@/lib/utils";

// Coordinates refer to the original 1536 × 1024 atlas, excluding its gutters.
export const LINE_PORTRAITS = [
  {
    id: "line-curls-animated",
    x: 19,
    y: 51,
    eyes: [
      [158, 268],
      [249, 260],
    ],
  },
  {
    id: "line-wave-animated",
    x: 397,
    y: 51,
    eyes: [
      [535, 253],
      [620, 237],
    ],
  },
  {
    id: "line-bob-animated",
    x: 777,
    y: 51,
    eyes: [
      [909, 276],
      [1007, 259],
    ],
  },
  {
    id: "line-silver-animated",
    x: 1157,
    y: 51,
    eyes: [
      [1294, 258],
      [1385, 265],
    ],
  },
  {
    id: "line-bun-animated",
    x: 19,
    y: 516,
    eyes: [
      [158, 757],
      [250, 739],
    ],
  },
  {
    id: "line-beard-animated",
    x: 397,
    y: 516,
    eyes: [
      [533, 706],
      [622, 693],
    ],
  },
  {
    id: "line-wisps-animated",
    x: 777,
    y: 516,
    eyes: [
      [912, 707],
      [1006, 711],
    ],
  },
  {
    id: "line-freckles-animated",
    x: 1157,
    y: 516,
    eyes: [
      [1297, 737],
      [1385, 715],
    ],
  },
];

export function LinePortraitAvatar({
  portrait,
  className,
}: {
  portrait: (typeof LINE_PORTRAITS)[number];
  className?: string;
}) {
  return (
    <svg
      aria-hidden="true"
      viewBox={`${portrait.x} ${portrait.y} 363 444`}
      preserveAspectRatio="none"
      className={cn(
        "profile-portrait mx-auto block aspect-square w-20 max-w-full overflow-hidden rounded-2xl",
        className,
      )}
      style={{ "--blink-delay": `${LINE_PORTRAITS.indexOf(portrait) * 0.03}s` } as CSSProperties}
      onPointerMove={(event) => {
        if (event.pointerType !== "mouse") return;
        const bounds = event.currentTarget.getBoundingClientRect();
        const x = (event.clientX - bounds.left) / bounds.width;
        const y = (event.clientY - bounds.top) / bounds.height;
        event.currentTarget.style.setProperty("--pointer-x", `${(x - 0.5) * 6}px`);
        event.currentTarget.style.setProperty("--pointer-y", `${(y - 0.5) * 4}px`);
      }}
      onPointerLeave={(event) => {
        event.currentTarget.style.removeProperty("--pointer-x");
        event.currentTarget.style.removeProperty("--pointer-y");
      }}
    >
      <image href="/avatars/line-portraits-animated.png" width="1536" height="1024" />
      {portrait.eyes.map(([x, y], index) => (
        <g key={index} transform={`translate(${x}, ${y})`}>
          <g className="profile-line-eyes">
            <path
              d="M-16 0 Q0-17 16 0"
              fill="none"
              stroke="#242322"
              strokeWidth="4"
              strokeLinecap="round"
            />
            <g className="profile-portrait-gaze">
              <g className="profile-portrait-pointer">
                <ellipse cx="0" cy="3" rx="6.5" ry="10" fill="#242322" />
              </g>
            </g>
          </g>
        </g>
      ))}
    </svg>
  );
}
