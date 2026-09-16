import { useState } from "react";
import { backupDatabase, isWeb } from "@/adapters";
import { Button, Input, Label } from "@wealthfolio/ui";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@wealthfolio/ui/components/ui/dialog";
import { PasswordInput } from "@wealthfolio/ui/components/ui/password-input";
import { BackupExportDialog } from "@/pages/settings/exports/backup-export-dialog";
import { ProfileAvatar } from "./profile-avatar";
import type { ProfileSummary } from "./api";

interface DeleteProfileDialogProps {
  profile: ProfileSummary;
  error?: string;
  onClose: () => void;
  onDelete: (confirmation: string, proof: string) => void;
}

export function DeleteProfileDialog({
  profile,
  onClose,
  onDelete,
  error: deletionError,
}: DeleteProfileDialogProps) {
  const [confirmation, setConfirmation] = useState("");
  const [proof, setProof] = useState("");
  const [exporting, setExporting] = useState(false);
  const [backup, setBackup] = useState<string>();
  const [error, setError] = useState("");
  if (backup) return <BackupExportDialog filename={backup} onClose={() => setBackup(undefined)} />;
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !exporting) onClose();
      }}
    >
      <DialogContent>
        <DialogHeader>
          <ProfileAvatar id={profile.avatarId} className="size-12" />
          <DialogTitle>
            Delete “{profile.name}” from this {isWeb ? "server" : "device"}?
          </DialogTitle>
          <DialogDescription>
            This permanently removes this profile’s financial data, settings, saved credentials, and
            local backups. Exported files, other devices, and your Wealthfolio Connect account are
            unaffected. {isWeb && "Everyone using this profile on this server will lose access."}
          </DialogDescription>
        </DialogHeader>
        <Button
          type="button"
          variant="outline"
          disabled={exporting}
          onClick={async () => {
            setExporting(true);
            setError("");
            try {
              setBackup((await backupDatabase()).filename);
            } catch (cause) {
              setError(String(cause));
            } finally {
              setExporting(false);
            }
          }}
        >
          {exporting ? "Preparing backup…" : "Export backup first"}
        </Button>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            if (confirmation === profile.name && (!profile.lockEnabled || proof))
              onDelete(confirmation, proof);
          }}
          className="space-y-4"
        >
          <div className="space-y-2">
            <Label htmlFor="delete-profile-name">Type {profile.name} to confirm</Label>
            <Input
              id="delete-profile-name"
              autoComplete="off"
              value={confirmation}
              onChange={(event) => setConfirmation(event.target.value)}
              disabled={exporting}
            />
          </div>
          {profile.lockEnabled && (
            <div className="space-y-2">
              <Label htmlFor="delete-profile-proof">Current password or recovery code</Label>
              <PasswordInput
                id="delete-profile-proof"
                showLabel="Show password or recovery code"
                hideLabel="Hide password or recovery code"
                autoComplete="off"
                value={proof}
                onChange={(event) => setProof(event.target.value)}
                disabled={exporting}
              />
            </div>
          )}
          {(error || deletionError) && (
            <p role="alert" className="text-destructive text-sm">
              {error || deletionError}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose} disabled={exporting}>
              Cancel
            </Button>
            <Button
              type="submit"
              variant="destructive"
              disabled={
                exporting || confirmation !== profile.name || (profile.lockEnabled && !proof)
              }
            >
              Delete profile
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
