import { cn } from "@/lib/utils";
import {
  Button,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  Icons,
} from "@wealthfolio/ui";
import { ProfileAvatar } from "./profile-avatar";
import { useProfile } from "./profile-context";

export function ProfileMenu({ collapsed = false }: { collapsed?: boolean }) {
  const context = useProfile();
  if (!context?.profile) return null;
  const { profile, manageProfile, lockProfile, switchProfile } = context;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          className={cn(
            "h-12 min-w-0 gap-3 py-0",
            collapsed
              ? "w-12 shrink-0 rounded-full p-0 has-[>svg]:px-0"
              : "w-full justify-start rounded-lg px-2 has-[>svg]:px-2",
          )}
          aria-label={`Profile menu for ${profile.name}`}
          title={profile.name}
        >
          <ProfileAvatar
            id={profile.avatarId}
            className="mx-0 size-8 shrink-0 rounded-full object-contain [&_.profile-abstract-sculpture]:scale-90"
          />
          {!collapsed && (
            <>
              <span className="min-w-0 flex-1 truncate text-left">{profile.name}</span>
              <Icons.ChevronsUpDown className="text-muted-foreground size-4 shrink-0" />
            </>
          )}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="start" sideOffset={8} className="w-64 rounded-xl p-1">
        <DropdownMenuLabel className="flex items-center gap-3 px-3 py-3">
          <ProfileAvatar id={profile.avatarId} className="mx-0 size-10 shrink-0 rounded-full" />
          <span className="min-w-0 truncate">{profile.name}</span>
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem className="h-11 gap-3 rounded-lg px-3" onSelect={manageProfile}>
          <Icons.User className="size-4" />
          Profile settings
        </DropdownMenuItem>
        <DropdownMenuItem className="h-11 gap-3 rounded-lg px-3" onSelect={switchProfile}>
          <Icons.Users className="size-4" />
          Switch profile
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem className="h-11 gap-3 rounded-lg px-3" onSelect={lockProfile}>
          <Icons.Lock className="size-4" />
          Lock Wealthfolio
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
