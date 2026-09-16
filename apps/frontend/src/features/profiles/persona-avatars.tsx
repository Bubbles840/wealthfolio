import type { CSSProperties } from "react";
import { cn } from "@/lib/utils";

interface Persona {
  id: string;
  row: number;
  column: number;
  tilt?: number;
  eyes: [number, number, number, number][];
}

export const PERSONA_AVATARS: Persona[] = [
  {
    id: "clay-artist-animated",
    row: 0,
    column: 0,
    tilt: 0,
    eyes: [
      [42.8, 47.3, 11, 14],
      [63.1, 47.3, 11, 14],
    ],
  },
  {
    id: "clay-traveler-animated",
    row: 0,
    column: 1,
    tilt: 0,
    eyes: [
      [40.5, 41, 10, 13],
      [60.1, 41, 10, 13],
    ],
  },
  {
    id: "clay-maker-animated",
    row: 0,
    column: 2,
    tilt: 0,
    eyes: [
      [41.9, 45.7, 10, 13],
      [60, 45.7, 10, 13],
    ],
  },
  {
    id: "clay-sailor-animated",
    row: 0,
    column: 3,
    tilt: -10,
    eyes: [
      [37.2, 46.8, 9, 12],
      [54, 44.5, 9, 12],
    ],
  },
  {
    id: "sketch-storyteller-animated",
    row: 1,
    column: 0,
    tilt: -22,
    eyes: [
      [43.5, 40.9, 10, 8.5],
      [61.6, 32, 9.5, 8.5],
    ],
  },
  {
    id: "sketch-thinker-animated",
    row: 1,
    column: 1,
    tilt: -22,
    eyes: [
      [50.6, 40, 10, 8.5],
      [70, 32.8, 9.5, 8.5],
    ],
  },
  {
    id: "sketch-wanderer-animated",
    row: 1,
    column: 2,
    tilt: -22,
    eyes: [
      [46.8, 40, 10, 8.5],
      [67.9, 32.4, 9.5, 8.5],
    ],
  },
  {
    id: "sketch-producer-animated",
    row: 1,
    column: 3,
    tilt: -22,
    eyes: [
      [48.8, 40.7, 10, 8.5],
      [66.9, 34.1, 9.5, 8.5],
    ],
  },
  {
    id: "pixel-wizard-animated",
    row: 2,
    column: 0,
    eyes: [
      [38, 45.6, 6, 9],
      [60.6, 45.6, 6, 9],
    ],
  },
  {
    id: "pixel-inventor-animated",
    row: 2,
    column: 1,
    eyes: [
      [39.7, 44.3, 6, 9],
      [62.7, 44.3, 6, 9],
    ],
  },
  {
    id: "pixel-courier-animated",
    row: 2,
    column: 2,
    eyes: [
      [38.3, 43.7, 6, 9],
      [61.6, 43.7, 6, 9],
    ],
  },
  {
    id: "pixel-musician-animated",
    row: 2,
    column: 3,
    eyes: [
      [41.9, 42.4, 6, 9],
      [60.8, 41.8, 6, 9],
    ],
  },
  {
    id: "abstract-architect-animated",
    row: 3,
    column: 0,
    eyes: [
      [39.9, 42.3, 10, 13],
      [61.6, 42.3, 10, 13],
    ],
  },
  {
    id: "abstract-dancer-animated",
    row: 3,
    column: 1,
    eyes: [
      [38.8, 39.7, 10, 12],
      [61.1, 39.7, 10, 12],
    ],
  },
  {
    id: "abstract-explorer-animated",
    row: 3,
    column: 2,
    eyes: [
      [38.3, 39.4, 10, 12],
      [60.9, 39.4, 10, 12],
    ],
  },
  {
    id: "abstract-dreamer-animated",
    row: 3,
    column: 3,
    eyes: [
      [47.7, 42.3, 9, 11],
      [65.6, 42.3, 9, 11],
    ],
  },
];

export function PersonaAvatar({ persona, className }: { persona: Persona; className?: string }) {
  const pixel = persona.row === 2;
  return (
    <span
      aria-hidden="true"
      className={cn(
        "relative mx-auto block aspect-square w-20 max-w-full overflow-hidden rounded-2xl",

        persona.row === 1 && "profile-sketch-avatar",
        pixel && "profile-pixel-avatar",
        className,
      )}
      style={
        {
          backgroundImage:
            persona.row < 2
              ? "url('/avatars/creatures-vintage-atlas.png')"
              : "url('/avatars/personas-animated-atlas.png')",
          // The generated two-row atlas has a slightly taller sketch row.
          backgroundSize:
            persona.row === 0 ? "400% 207.73%" : persona.row === 1 ? "400% 192.83%" : "400% 400%",
          backgroundPosition: `${(persona.column * 100) / 3}% ${persona.row < 2 ? persona.row * 100 : (persona.row * 100) / 3}%`,
          "--sculpture-eye-delay": `${persona.column * 0.03 + persona.row * 0.03}s`,
          "--pixel-eye-delay": `${persona.column * 0.03}s`,
          "--pixel-eye-color": "#352c2c",
        } as CSSProperties
      }
    >
      {persona.eyes.map(([x, y, width, height], index) => (
        <span
          key={index}
          className={pixel ? "profile-pixel-eye-position" : "profile-sculpture-eye-position"}
          style={{
            left: `${x}%`,
            top: `${y}%`,
            width: `${width}%`,
            height: `${height}%`,
            transform: `translate(-50%, -50%) rotate(${persona.tilt ?? 0}deg)`,
          }}
        >
          {pixel ? (
            <span className="profile-pixel-gaze">
              <span className="profile-pixel-blink">
                <span className="profile-pixel-eye" />
              </span>
            </span>
          ) : (
            <span className="profile-sculpture-eye">
              <span className="profile-sculpture-pupil" />
            </span>
          )}
        </span>
      ))}
    </span>
  );
}
