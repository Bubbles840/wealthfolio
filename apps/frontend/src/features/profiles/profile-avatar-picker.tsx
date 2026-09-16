import { useState } from "react";
import { PROFILE_AVATAR_GROUPS, ProfileAvatar } from "./profile-avatar";

export function ProfileAvatarPicker({
  value,
  onChange,
}: {
  value: string;
  onChange: (id: string) => void;
}) {
  const [style, setStyle] = useState("All");
  const groups = [
    { name: "All", avatars: PROFILE_AVATAR_GROUPS.flatMap((group) => group.avatars) },
    ...PROFILE_AVATAR_GROUPS,
  ];
  const group = groups.find((group) => group.name === style)!;
  return (
    <div className="space-y-3">
      <div role="group" aria-label="Avatar style" className="flex flex-wrap gap-1">
        {groups.map((item) => (
          <button
            key={item.name}
            type="button"
            aria-pressed={style === item.name}
            onClick={() => setStyle(item.name)}
            className={`focus-visible:ring-ring rounded-full px-3 py-1.5 text-xs focus-visible:ring-2 ${style === item.name ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground bg-white/20 dark:bg-white/5"}`}
          >
            {item.name}
          </button>
        ))}
      </div>
      <div
        role="group"
        aria-label={`${style} avatars`}
        className="-mx-6 grid max-h-72 grid-cols-4 gap-2 overflow-y-auto px-6 pb-8 pt-4 sm:max-h-[28rem] sm:grid-cols-8"
      >
        {group.avatars.map((id) => (
          <button
            type="button"
            key={id}
            aria-label={id.replaceAll("-", " ")}
            aria-pressed={value === id}
            onClick={() => onChange(id)}
            className={`focus-visible:ring-ring flex justify-start rounded-full p-0 focus-visible:ring-2 sm:p-1 ${value === id ? "bg-primary/10" : "hover:bg-primary/5"}`}
          >
            <span className="profile-lock-avatar [--avatar-rim:3px]">
              <ProfileAvatar
                id={id}
                className="size-13 rounded-full sm:size-14 [&_.profile-abstract-sculpture]:scale-90"
              />
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}
