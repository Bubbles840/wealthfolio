import type { CSSProperties } from "react";
import "./profile-avatar.css";
import { LINE_PORTRAITS, LinePortraitAvatar } from "./line-portrait-avatars";
import { PERSONA_AVATARS, PersonaAvatar } from "./persona-avatars";
import { cn } from "@/lib/utils";

const ANIMATED_CLAY: Record<
  string,
  { cell: number; tilt: number; eyes: [number, number, number, number][] }
> = {
  "clay-pebble-animated": {
    cell: 0,
    tilt: -18,
    eyes: [
      [44.7, 46.9, 7.6, 10],
      [70.5, 40.5, 7.6, 10],
    ],
  },
  "clay-fluff-animated": {
    cell: 1,
    tilt: 8,
    eyes: [
      [40.5, 42, 12, 16],
      [65, 43, 12, 16],
    ],
  },
  "clay-bot-animated": {
    cell: 2,
    tilt: 8,
    eyes: [
      [36.7, 40.2, 8.5, 10],
      [58.1, 43.7, 8.5, 10],
    ],
  },
  "clay-sun-animated": {
    cell: 3,
    tilt: -18,
    eyes: [
      [43.5, 47.5, 7.2, 10],
      [66.5, 42.4, 7.2, 10],
    ],
  },
};

const ANIMATED_PIXEL: Record<string, { cell: number; color: string; eyes: [number, number][] }> = {
  "pixel-explorer-animated": {
    cell: 0,
    color: "#BBFFF1",
    eyes: [
      [47.8, 44.3],
      [65.7, 44.3],
    ],
  },
  "pixel-fox-animated": {
    cell: 1,
    color: "#352522",
    eyes: [
      [36.5, 48.2],
      [63.3, 47.8],
    ],
  },
  "pixel-ghost-animated": {
    cell: 2,
    color: "#482578",
    eyes: [
      [42.1, 40.2],
      [60, 38.6],
    ],
  },
  "pixel-sprite-animated": {
    cell: 3,
    color: "#382D25",
    eyes: [
      [40, 46.9],
      [58.9, 47.2],
    ],
  },
};

const ANIMATED_SKETCH: Record<
  string,
  { cell: number; tilt: number; eyes: [number, number, number, number][] }
> = {
  "sketch-glasses-animated": {
    cell: 0,
    tilt: -20,
    eyes: [
      [44.7, 42.6, 9.2, 6.8],
      [65.4, 35.2, 7.8, 6.8],
    ],
  },
  "sketch-curls-animated": {
    cell: 1,
    tilt: -22,
    eyes: [
      [41.9, 42.6, 10, 7],
      [61.4, 34, 8.6, 6.8],
    ],
  },
  "sketch-beanie-animated": {
    cell: 2,
    tilt: -18,
    eyes: [
      [44.3, 35.6, 9, 5.8],
      [63.8, 30.5, 8.2, 5.6],
    ],
  },
  "sketch-scarf-animated": {
    cell: 3,
    tilt: -20,
    eyes: [
      [47.7, 42.6, 10, 7],
      [65.2, 35.9, 7.8, 6.8],
    ],
  },
};

export const PROFILE_AVATAR_GROUPS = [
  {
    name: "Sketch",
    avatars: [
      ...PERSONA_AVATARS.filter((persona) => persona.row === 1).map((persona) => persona.id),
      ...Object.keys(ANIMATED_SKETCH),
    ],
  },
  {
    name: "Line",
    avatars: LINE_PORTRAITS.map((portrait) => portrait.id),
  },
  {
    name: "Pixel art",
    avatars: [
      ...PERSONA_AVATARS.filter((persona) => persona.row === 2).map((persona) => persona.id),
      ...Object.keys(ANIMATED_PIXEL),
    ],
  },
  {
    name: "3D",
    avatars: [
      ...PERSONA_AVATARS.filter((persona) => persona.row === 0).map((persona) => persona.id),
      ...Object.keys(ANIMATED_CLAY),
    ],
  },
  {
    name: "Abstract",
    avatars: [
      ...PERSONA_AVATARS.filter((persona) => persona.row === 3).map((persona) => persona.id),
      "abstract-lantern",
      "abstract-comet",
      "abstract-duet",
      "abstract-mobile",
    ],
  },
];
export const PROFILE_AVATARS = PROFILE_AVATAR_GROUPS.flatMap((group) => group.avatars);
export const DEFAULT_PROFILE_AVATAR = LINE_PORTRAITS[0].id;

interface Sculpture {
  cell: number;
  color: string;
  patch: string;
  offset: [number, number];
  eyes: [number, number, number, number][];
}
const ABSTRACT_SCULPTURES: Record<string, Sculpture> = {
  "abstract-lantern": {
    cell: 0,
    offset: [-6, -2.5],
    color: "#F0B5A5",
    patch: "ellipse(13.5% 10% at 63% 39%)",
    eyes: [
      [57.3, 40.8, 8.3, 11.8],
      [68.6, 36.4, 8, 11.8],
    ],
  },
  "abstract-duet": {
    cell: 1,
    offset: [3.3, -2.8],
    color: "#CBB7E4",
    patch: "ellipse(12.7% 8% at 54.5% 51.8%)",
    eyes: [
      [48.5, 53.5, 7.6, 9.8],
      [61.5, 50.3, 7.6, 9.8],
    ],
  },
  "abstract-comet": {
    cell: 2,
    offset: [-6.7, 4.5],
    color: "#AFCBE4",
    patch: "ellipse(10.5% 8.5% at 66.2% 35.6%)",
    eyes: [
      [62.5, 36.8, 6.5, 9],
      [70, 33.7, 6.5, 9],
    ],
  },
  "abstract-mobile": {
    cell: 3,
    offset: [-0.9, 5.1],
    color: "#C3D1A4",
    patch: "ellipse(12.8% 9.5% at 50.7% 27.6%)",
    eyes: [
      [45.5, 29.4, 8.3, 11.2],
      [56.6, 25.8, 8.3, 11.2],
    ],
  },
};

export function ProfileAvatar({ id, className }: { id: string; className?: string }) {
  const portrait = LINE_PORTRAITS.find((candidate) => candidate.id === id);
  if (portrait) return <LinePortraitAvatar portrait={portrait} className={className} />;
  const persona = PERSONA_AVATARS.find((candidate) => candidate.id === id);
  if (persona) return <PersonaAvatar persona={persona} className={className} />;
  const sketch = ANIMATED_SKETCH[id];
  if (sketch) {
    return (
      <span
        aria-hidden="true"
        className={cn(
          "profile-sketch-avatar relative mx-auto block aspect-square w-20 max-w-full overflow-hidden rounded-2xl",
          className,
        )}
        style={
          {
            backgroundImage: "url('/avatars/sketch-animated-atlas.png')",
            backgroundSize: "400% 400%",
            backgroundPosition: `${(sketch.cell * 100) / 3}% ${100 / 3}%`,
            "--sculpture-eye-delay": `${sketch.cell * 0.03}s`,
          } as CSSProperties
        }
      >
        {sketch.eyes.map(([x, y, width, height], index) => (
          <span
            key={index}
            className="profile-sculpture-eye-position"
            style={{
              left: `${x}%`,
              top: `${y}%`,
              width: `${width}%`,
              height: `${height}%`,
              transform: `translate(-50%, -50%) rotate(${sketch.tilt}deg)`,
            }}
          >
            <span className="profile-sculpture-eye">
              <span className="profile-sculpture-pupil" />
            </span>
          </span>
        ))}
      </span>
    );
  }
  const pixel = ANIMATED_PIXEL[id];
  if (pixel) {
    return (
      <span
        aria-hidden="true"
        className={cn(
          "profile-pixel-avatar relative mx-auto block aspect-square w-20 max-w-full overflow-hidden rounded-2xl",
          className,
        )}
        style={
          {
            backgroundImage: "url('/avatars/pixel-animated-atlas.png')",
            backgroundSize: "400% 400%",
            backgroundPosition: `${(pixel.cell * 100) / 3}% ${200 / 3}%`,
            "--pixel-eye-color": pixel.color,
            "--pixel-eye-delay": `${pixel.cell * 0.03}s`,
          } as CSSProperties
        }
      >
        {pixel.eyes.map(([x, y], index) => (
          <span
            key={index}
            className="profile-pixel-eye-position"
            style={{ left: `${x}%`, top: `${y}%` }}
          >
            <span className="profile-pixel-gaze">
              <span className="profile-pixel-blink">
                <span className="profile-pixel-eye" />
              </span>
            </span>
          </span>
        ))}
      </span>
    );
  }
  const clay = ANIMATED_CLAY[id];
  if (clay) {
    return (
      <span
        aria-hidden="true"
        className={cn(
          `profile-clay-${clay.cell} relative mx-auto block aspect-square w-20 max-w-full overflow-hidden rounded-2xl`,
          className,
        )}
        style={
          {
            backgroundImage: "url('/avatars/clay-animated-atlas.png')",
            backgroundSize: "400% 400%",
            backgroundPosition: `${(clay.cell * 100) / 3}% 0%`,
            "--sculpture-eye-delay": `${clay.cell * 0.03}s`,
          } as CSSProperties
        }
      >
        {clay.eyes.map(([x, y, width, height], index) => (
          <span
            key={index}
            className="profile-sculpture-eye-position"
            style={{
              left: `${x}%`,
              top: `${y}%`,
              width: `${width}%`,
              height: `${height}%`,
              transform: `translate(-50%, -50%) rotate(${clay.tilt}deg)`,
            }}
          >
            <span className="profile-sculpture-eye">
              <span className="profile-sculpture-pupil" />
            </span>
          </span>
        ))}
      </span>
    );
  }
  const sculpture = ABSTRACT_SCULPTURES[id];
  if (sculpture) {
    return (
      <span
        aria-hidden="true"
        className={cn(
          "mx-auto block aspect-square w-20 max-w-full overflow-hidden rounded-2xl",
          className,
        )}
        style={{ backgroundColor: sculpture.color }}
      >
        <span
          className="profile-abstract-sculpture relative block size-full"
          style={
            {
              backgroundImage: "url('/avatars/abstract-sculptures.png')",
              backgroundSize: "200% 200%",
              backgroundPosition: `${(sculpture.cell % 2) * 100}% ${Math.floor(sculpture.cell / 2) * 100}%`,
              animationDelay: `${sculpture.cell * -1.2}s`,
              clipPath: sculpture.cell < 2 ? "inset(0 0 2% 0)" : undefined,
              "--sculpture-eye-delay": `${sculpture.cell * 0.03}s`,
              "--sculpture-x": `${sculpture.offset[0]}%`,
              "--sculpture-y": `${sculpture.offset[1]}%`,
            } as CSSProperties
          }
        >
          {/* Only the small face region is used; the original transparent silhouette stays intact. */}
          <span
            className="absolute inset-0"
            style={{
              backgroundImage: "url('/avatars/abstract-sculptures-face-patches.png')",
              backgroundSize: "200% 200%",
              backgroundPosition: `${(sculpture.cell % 2) * 100}% ${Math.floor(sculpture.cell / 2) * 100}%`,
              clipPath: sculpture.patch,
            }}
          />
          {sculpture.eyes.map(([x, y, width, height], index) => (
            <span
              key={index}
              className="profile-sculpture-eye-position"
              style={{ left: `${x}%`, top: `${y}%`, width: `${width}%`, height: `${height}%` }}
            >
              <span className="profile-sculpture-eye">
                <span className="profile-sculpture-pupil" />
              </span>
            </span>
          ))}
        </span>
      </span>
    );
  }
  return <LinePortraitAvatar portrait={LINE_PORTRAITS[0]} className={className} />;
}
