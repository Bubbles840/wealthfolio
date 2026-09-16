use super::*;
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};
use fs2::FileExt;
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const MIN_PASSWORD_LENGTH: usize = 8;
const MAX_PASSWORD_LENGTH: usize = 128;

const REGISTRY_FILE: &str = "profiles.json";
const REGISTRY_BACKUP: &str = "profiles.json.bak";
const DELETIONS_DIR: &str = "profile-deletions";
const REGISTRY_VERSION: u32 = 1;
const RECOVERY_DOMAIN: &[u8] = b"wealthfolio/profile-recovery/v1/";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryData {
    version: u32,
    default_profile_id: Option<Uuid>,

    profiles: Vec<Profile>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockRecord {
    verifier: Option<String>,
    recovery_hash: Option<String>,
    failures: u32,
    next_attempt: u64,
}

pub struct ProfileRegistry {
    root: PathBuf,
    data: Mutex<RegistryData>,
    secrets: Arc<dyn SecretStore>,
    secret_gate: Arc<Mutex<()>>,
    pub sessions: ProfileSessions,
    pub auth_flows: ProfileAuthFlows,
    _owner: File,
}

fn storage_error(error: impl std::fmt::Display) -> ProfileError {
    ProfileError::Unavailable(error.to_string())
}

fn credential_error(_: crate::Error) -> ProfileError {
    ProfileError::Unavailable("The profile credential store could not be accessed.".into())
}

fn validate_name(name: &str, avatar: &str) -> ProfileResult<()> {
    if name.trim().is_empty() || name.chars().count() > 60 || name.chars().any(char::is_control) {
        return Err(ProfileError::Invalid(
            "Use a profile name of 1–60 characters.".into(),
        ));
    }
    if !PROFILE_AVATARS.contains(&avatar) {
        return Err(ProfileError::Invalid("Choose a bundled avatar.".into()));
    }
    Ok(())
}

fn validate_data(data: &RegistryData) -> ProfileResult<()> {
    let ids: std::collections::HashSet<_> = data.profiles.iter().map(|p| p.id).collect();
    if data.version != REGISTRY_VERSION
        || ids.len() != data.profiles.len()
        || data.default_profile_id.is_some_and(|id| !ids.contains(&id))
        || (data.default_profile_id.is_none() && !data.profiles.is_empty())
        || data
            .profiles
            .iter()
            .any(|p| p.legacy_database.is_some() && Some(p.id) != data.default_profile_id)
    {
        return Err(ProfileError::Unavailable(
            "The profile registry is invalid.".into(),
        ));
    }
    for profile in &data.profiles {
        validate_name(&profile.name, &profile.avatar_id)?;
    }
    Ok(())
}

// Retired pre-release artwork must not make an otherwise valid registry unreadable.
const RETIRED_AVATARS: &[&str] = &[
    "fox",
    "cat",
    "owl",
    "bear",
    "panda",
    "rabbit",
    "koala",
    "penguin",
    "line-sunny",
    "line-dreamer",
    "line-curious",
    "line-poet",
    "line-orbit",
    "line-sprout",
    "line-muse",
    "line-wink",
    "line-glimmer",
    "line-mellow",
    "line-scout",
    "line-jester",
    "clay-pebble",
    "clay-fluff",
    "clay-bot",
    "clay-sun",
    "sketch-glasses",
    "sketch-curls",
    "sketch-beanie",
    "sketch-scarf",
    "pixel-explorer",
    "pixel-fox",
    "pixel-ghost",
    "pixel-sprite",
    "abstract-arch",
    "abstract-ribbon",
    "abstract-orbit",
    "abstract-pebbles",
];

fn read_registry(path: &Path) -> ProfileResult<RegistryData> {
    let mut data: RegistryData =
        serde_json::from_slice(&fs::read(path).map_err(storage_error)?).map_err(storage_error)?;
    for profile in &mut data.profiles {
        if RETIRED_AVATARS.contains(&profile.avatar_id.as_str()) {
            profile.avatar_id = PROFILE_AVATARS[0].into();
        }
    }
    validate_data(&data)?;
    Ok(data)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> ProfileResult<()> {
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(storage_error)?;
        file.write_all(bytes).map_err(storage_error)?;
        file.sync_all().map_err(storage_error)?;
        fs::rename(&temporary, path).map_err(storage_error)?;
        #[cfg(unix)]
        File::open(path.parent().unwrap())
            .and_then(|f| f.sync_all())
            .map_err(storage_error)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

impl ProfileRegistry {
    /// Holds one installation-level owner even while all native databases are locked.
    pub fn open(
        root: PathBuf,
        legacy_database: PathBuf,
        secrets: Arc<dyn SecretStore>,
    ) -> ProfileResult<Self> {
        fs::create_dir_all(&root).map_err(storage_error)?;
        let owner = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("profiles.lock"))
            .map_err(storage_error)?;
        owner.try_lock_exclusive().map_err(|_| {
            ProfileError::Unavailable("Another Wealthfolio process owns these profiles.".into())
        })?;
        let path = root.join(REGISTRY_FILE);
        let backup = root.join(REGISTRY_BACKUP);
        let data = if path.exists() || backup.exists() {
            match read_registry(&path) {
                Ok(data) => data,
                Err(_) => {
                    let data = read_registry(&backup)?;
                    atomic_write(
                        &path,
                        &serde_json::to_vec_pretty(&data).map_err(storage_error)?,
                    )?;
                    data
                }
            }
        } else {
            if root.join("profiles").exists()
                && fs::read_dir(root.join("profiles"))
                    .map_err(storage_error)?
                    .next()
                    .is_some()
            {
                return Err(ProfileError::Unavailable(
                    "The profile registry is missing. Restore profiles.json from a backup.".into(),
                ));
            }
            if !legacy_database.exists()
                && (PathBuf::from(format!("{}.encrypted", legacy_database.display())).exists()
                    || root
                        .join("backups")
                        .read_dir()
                        .is_ok_and(|mut files| files.next().is_some()))
            {
                return Err(ProfileError::Unavailable("The existing database is missing. Restore its file before migrating profiles; existing keys and backups were preserved.".into()));
            }
            let id = Uuid::new_v4();
            let profile = Profile {
                id,
                name: "Personal".into(),
                avatar_id: PROFILE_AVATARS[0].into(),
                lock_enabled: false,
                legacy_database: legacy_database.exists().then_some(legacy_database),
                legacy_addons_root: None,
                connect: None,
            };
            let data = RegistryData {
                version: REGISTRY_VERSION,
                default_profile_id: Some(id),
                profiles: vec![profile],
            };
            atomic_write(
                &path,
                &serde_json::to_vec_pretty(&data).map_err(storage_error)?,
            )?;
            data
        };
        Ok(Self {
            root,
            data: Mutex::new(data),
            secrets,
            secret_gate: Arc::new(Mutex::new(())),
            sessions: ProfileSessions::default(),
            auth_flows: ProfileAuthFlows::default(),
            _owner: owner,
        })
    }

    pub fn set_legacy_addons_root(&self, root: PathBuf) -> ProfileResult<()> {
        let mut data = self.data()?;
        let mut next = data.clone();
        if let Some(profile) = next
            .profiles
            .iter_mut()
            .find(|p| p.legacy_database.is_some() && !self.is_deleting(p.id))
        {
            if profile.legacy_addons_root.as_ref() == Some(&root) {
                return Ok(());
            }
            profile.legacy_addons_root = Some(root);
            self.save(&mut data, next)?;
        }
        Ok(())
    }

    pub fn is_deleting(&self, id: Uuid) -> bool {
        self.root
            .join(DELETIONS_DIR)
            .join(format!("{id}.json"))
            .exists()
    }

    pub fn deletion_paths(&self, id: Uuid) -> ProfileResult<ProfilePaths> {
        let profile: Profile = serde_json::from_slice(
            &fs::read(self.root.join(DELETIONS_DIR).join(format!("{id}.json")))
                .map_err(storage_error)?,
        )
        .map_err(storage_error)?;
        if profile.id != id {
            return Err(ProfileError::Invalid("Invalid deletion record.".into()));
        }
        Ok(self.paths(&profile))
    }

    pub fn pending_profiles(&self) -> ProfileResult<Vec<ProfileSummary>> {
        Ok(self
            .data()?
            .profiles
            .iter()
            .filter(|p| self.is_deleting(p.id))
            .map(ProfileSummary::from)
            .collect())
    }

    pub fn pending_deletions(&self) -> ProfileResult<Vec<Uuid>> {
        Ok(self
            .data()?
            .profiles
            .iter()
            .filter(|p| self.is_deleting(p.id))
            .map(|p| p.id)
            .collect())
    }

    /// The journal is written before revocation or destructive work. It also
    /// prevents an old registry backup from resurrecting a removed profile.
    pub fn begin_delete(&self, id: Uuid, name: &str, proof: Option<&str>) -> ProfileResult<()> {
        let data = self.data()?;
        let profile = data
            .profiles
            .iter()
            .find(|p| p.id == id && !self.is_deleting(id))
            .ok_or(ProfileError::NotFound)?;
        if profile.name != name {
            return Err(ProfileError::Invalid(
                "Enter the profile name to confirm deletion.".into(),
            ));
        }
        let mut record = self.read_lock(profile)?;
        self.verify_record(profile, &mut record, proof)?;
        let _secret_guard = self
            .secret_gate
            .lock()
            .map_err(|_| storage_error("Credential state is unavailable"))?;
        let directory = self.root.join(DELETIONS_DIR);
        fs::create_dir_all(&directory).map_err(storage_error)?;
        atomic_write(
            &directory.join(format!("{id}.json")),
            &serde_json::to_vec(&profile).map_err(storage_error)?,
        )?;
        self.sessions.revoke_profile(id)?;
        self.auth_flows.cancel_profile(id)?;
        Ok(())
    }

    /// Call only after the runtime has released all database and secret users.
    pub fn finish_delete(&self, id: Uuid) -> ProfileResult<()> {
        let mut data = self.data()?;
        let _secret_guard = self
            .secret_gate
            .lock()
            .map_err(|_| storage_error("Credential state is unavailable"))?;
        if !self.is_deleting(id) {
            return Err(ProfileError::Invalid("Deletion was not confirmed.".into()));
        }
        let profile: Profile = serde_json::from_slice(
            &fs::read(self.root.join(DELETIONS_DIR).join(format!("{id}.json")))
                .map_err(storage_error)?,
        )
        .map_err(storage_error)?;
        if profile.id != id {
            return Err(ProfileError::Invalid("Invalid deletion record.".into()));
        }
        let prefix = if profile.legacy_database.is_some() {
            String::new()
        } else {
            format!("profile:{id}:")
        };
        // Inventory keys before deleting any data; failures remain retryable.
        for key in self.secrets.list_secrets().map_err(credential_error)? {
            let owned = if prefix.is_empty() {
                !key.starts_with("profile:")
            } else {
                key.starts_with(&prefix)
            };
            if owned {
                self.secrets.delete_secret(&key).map_err(credential_error)?;
            }
        }
        let paths = self.paths(&profile);
        if profile.legacy_database.is_some() {
            // The legacy root also contains other profiles and shared vault files.
            for suffix in ["", "-wal", "-shm", "-journal", ".encrypted"] {
                remove_owned_path(&PathBuf::from(format!(
                    "{}{suffix}",
                    paths.database.display()
                )))?;
            }
            for path in [
                paths.backups(),
                paths.scratch(),
                paths.addons(),
                paths.root.join("pending-exports"),
            ] {
                remove_owned_path(&path)?;
            }
            if let Some(root) = &profile.legacy_addons_root {
                remove_owned_path(&root.join("addons"))?;
            }
        } else {
            remove_owned_path(&paths.root)?;
        }
        let mut next = data.clone();
        next.profiles.retain(|p| p.id != id);
        if next.default_profile_id == Some(id) {
            next.default_profile_id = next.profiles.first().map(|p| p.id);
        }
        self.save(&mut data, next)
    }

    fn data(&self) -> ProfileResult<MutexGuard<'_, RegistryData>> {
        self.data
            .lock()
            .map_err(|_| ProfileError::Unavailable("Profile registry state is unavailable.".into()))
    }

    fn save(&self, current: &mut RegistryData, next: RegistryData) -> ProfileResult<()> {
        validate_data(&next)?;
        atomic_write(
            &self.root.join(REGISTRY_BACKUP),
            &serde_json::to_vec_pretty(current).map_err(storage_error)?,
        )?;
        atomic_write(
            &self.root.join(REGISTRY_FILE),
            &serde_json::to_vec_pretty(&next).map_err(storage_error)?,
        )?;
        *current = next;
        Ok(())
    }

    pub fn list(&self) -> ProfileResult<Vec<ProfileSummary>> {
        Ok(self
            .data()?
            .profiles
            .iter()
            .filter(|p| !self.is_deleting(p.id))
            .map(ProfileSummary::from)
            .collect())
    }

    pub fn default_id(&self) -> ProfileResult<Uuid> {
        self.data()?
            .default_profile_id
            .ok_or(ProfileError::NotFound)
    }

    pub fn profile(&self, id: Uuid) -> ProfileResult<Profile> {
        self.data()?
            .profiles
            .iter()
            .find(|p| p.id == id && !self.is_deleting(id))
            .cloned()
            .ok_or(ProfileError::NotFound)
    }

    pub fn paths(&self, profile: &Profile) -> ProfilePaths {
        match &profile.legacy_database {
            Some(database) => ProfilePaths {
                root: self.root.clone(),
                database: database.clone(),
            },
            None => {
                let root = self.root.join("profiles").join(profile.id.to_string());
                ProfilePaths {
                    database: root.join("app.db"),
                    root,
                }
            }
        }
    }

    pub fn validate_import_path(&self, profile_id: Uuid, source: &Path) -> ProfileResult<()> {
        let source = source.canonicalize().map_err(storage_error)?;
        let data = self.data()?;
        for profile in &data.profiles {
            let paths = self.paths(profile);
            if profile.id != profile_id {
                if paths.database.canonicalize().is_ok_and(|p| p == source) {
                    return Err(ProfileError::Locked);
                }
                // The legacy root contains the registry/new profiles, so compare
                // only its financial directories rather than the entire root.
                let roots = if profile.legacy_database.is_some() {
                    vec![
                        paths.backups(),
                        paths.scratch(),
                        paths.addons(),
                        paths.root.join("pending-exports"),
                    ]
                } else {
                    vec![paths.root]
                };
                if roots.iter().any(|root| {
                    root.canonicalize()
                        .is_ok_and(|root| source.starts_with(root))
                }) {
                    return Err(ProfileError::Locked);
                }
            }
        }
        Ok(())
    }

    pub fn secret_store(&self, profile: &Profile) -> Arc<dyn SecretStore> {
        Arc::new(ScopedSecretStore::new(
            self.secrets.clone(),
            profile,
            self.root
                .join(DELETIONS_DIR)
                .join(format!("{}.json", profile.id)),
            self.secret_gate.clone(),
        ))
    }

    pub fn create(&self, name: &str, avatar: &str) -> ProfileResult<ProfileSummary> {
        validate_name(name, avatar)?;
        let mut data = self.data()?;
        let profile = Profile {
            id: Uuid::new_v4(),
            name: name.trim().into(),
            avatar_id: avatar.into(),
            lock_enabled: false,
            legacy_database: None,
            legacy_addons_root: None,
            connect: None,
        };
        let mut next = data.clone();
        next.profiles.push(profile.clone());
        if next.default_profile_id.is_none() {
            next.default_profile_id = Some(profile.id);
        }
        self.save(&mut data, next)?;
        Ok(ProfileSummary::from(&profile))
    }

    pub fn update(&self, id: Uuid, name: &str, avatar: &str) -> ProfileResult<()> {
        validate_name(name, avatar)?;
        let mut data = self.data()?;
        let mut next = data.clone();
        let profile = next
            .profiles
            .iter_mut()
            .find(|p| p.id == id && !self.is_deleting(id))
            .ok_or(ProfileError::NotFound)?;
        profile.name = name.trim().into();
        profile.avatar_id = avatar.into();
        self.save(&mut data, next)
    }

    /// Checked before credentials are committed. Ordinary sign-out retains this reservation.
    pub fn bind_connect(&self, id: Uuid, binding: ConnectBinding) -> ProfileResult<()> {
        let mut data = self.data()?;
        if let Some(existing) = data.profiles.iter().find(|p| {
            p.id != id
                && p.connect
                    .as_ref()
                    .is_some_and(|c| c.issuer == binding.issuer && c.user_id == binding.user_id)
        }) {
            return Err(ProfileError::DuplicateIdentity(existing.id));
        }
        let mut next = data.clone();
        let profile = next
            .profiles
            .iter_mut()
            .find(|p| p.id == id && !self.is_deleting(id))
            .ok_or(ProfileError::NotFound)?;
        if let Some(previous) = &profile.connect {
            if previous.issuer != binding.issuer || previous.user_id != binding.user_id {
                return Err(ProfileError::IdentityMismatch);
            }
            if previous.team_id != binding.team_id {
                return Err(ProfileError::TeamChanged);
            }
            return Ok(());
        }
        profile.connect = Some(binding);
        self.save(&mut data, next)
    }

    fn read_lock(&self, profile: &Profile) -> ProfileResult<LockRecord> {
        match self
            .secret_store(profile)
            .get_secret(PROFILE_LOCK_KEY)
            .map_err(credential_error)?
        {
            Some(value) => {
                let record: LockRecord = serde_json::from_str(&value).map_err(|_| {
                    ProfileError::Unavailable("The profile lock record is invalid.".into())
                })?;
                if record.verifier.is_some() != record.recovery_hash.is_some() {
                    return Err(ProfileError::Unavailable(
                        "The profile lock record is incomplete.".into(),
                    ));
                }
                Ok(record)
            }
            None if profile.lock_enabled => Err(ProfileError::Unavailable(
                "The protected profile's lock record is missing.".into(),
            )),
            None => Ok(LockRecord::default()),
        }
    }

    fn write_lock(&self, profile: &Profile, record: &LockRecord) -> ProfileResult<()> {
        let encoded = Zeroizing::new(serde_json::to_string(record).map_err(storage_error)?);
        let store = self.secret_store(profile);
        store
            .set_secret(PROFILE_LOCK_KEY, &encoded)
            .map_err(credential_error)?;
        if store
            .get_secret(PROFILE_LOCK_KEY)
            .map_err(credential_error)?
            .as_deref()
            != Some(encoded.as_str())
        {
            return Err(ProfileError::Unavailable(
                "The profile lock could not be saved.".into(),
            ));
        }
        Ok(())
    }

    pub fn verify(&self, id: Uuid, proof: Option<&str>) -> ProfileResult<bool> {
        // Serialize attempts across all browser sessions and credential operations.
        let data = self.data()?;
        let profile = data
            .profiles
            .iter()
            .find(|p| p.id == id && !self.is_deleting(id))
            .ok_or(ProfileError::NotFound)?;
        let mut record = self.read_lock(profile)?;
        self.verify_record(profile, &mut record, proof)?;
        Ok(record.verifier.is_some())
    }

    fn verify_record(
        &self,
        profile: &Profile,
        record: &mut LockRecord,
        proof: Option<&str>,
    ) -> ProfileResult<()> {
        let Some(verifier) = record.verifier.as_ref() else {
            return Ok(());
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now < record.next_attempt {
            return Err(ProfileError::Cooldown(record.next_attempt - now));
        }
        let proof = proof
            .filter(|p| !p.is_empty())
            .ok_or(ProfileError::Locked)?;
        let parsed = PasswordHash::new(verifier).map_err(|_| {
            ProfileError::Unavailable("The profile password verifier is invalid.".into())
        })?;
        let password_matches = Argon2::default()
            .verify_password(proof.as_bytes(), &parsed)
            .is_ok();
        let normalized: String = proof
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .flat_map(char::to_uppercase)
            .collect();
        let recovery = recovery_hash(&normalized);
        let recovery_matches = normalized.len() == 32
            && record
                .recovery_hash
                .as_ref()
                .is_some_and(|hash| bool::from(hash.as_bytes().ct_eq(recovery.as_bytes())));
        if !password_matches && !recovery_matches {
            record.failures = record.failures.saturating_add(1);
            if record.failures >= 5 {
                record.next_attempt =
                    now.saturating_add((30_u64 << (record.failures - 5).min(5)).min(900));
            }
            self.write_lock(profile, record)?;
            return if record.next_attempt > now {
                Err(ProfileError::Cooldown(record.next_attempt - now))
            } else {
                Err(ProfileError::IncorrectPassword)
            };
        }
        if record.failures != 0 || record.next_attempt != 0 {
            record.failures = 0;
            record.next_attempt = 0;
            self.write_lock(profile, record)?;
        }
        Ok(())
    }

    /// Setting a password also rotates its one-time recovery code. The host requires an
    /// admitted session for initial setup; an existing password always requires proof.
    pub fn set_password(
        &self,
        id: Uuid,
        proof: Option<&str>,
        password: Option<&str>,
    ) -> ProfileResult<Option<String>> {
        if password.is_some_and(|p| {
            !(MIN_PASSWORD_LENGTH..=MAX_PASSWORD_LENGTH).contains(&p.chars().count())
        }) {
            return Err(ProfileError::Invalid(format!(
                "Use {MIN_PASSWORD_LENGTH}–{MAX_PASSWORD_LENGTH} characters for your password."
            )));
        }
        let mut data = self.data()?;
        let profile = data
            .profiles
            .iter()
            .find(|p| p.id == id && !self.is_deleting(id))
            .ok_or(ProfileError::NotFound)?;
        let mut current = self.read_lock(profile)?;
        self.verify_record(profile, &mut current, proof)?;
        let (record, recovery) = if let Some(password) = password {
            let params = Params::new(19 * 1024, 2, 1, Some(32)).map_err(storage_error)?;
            let salt = SaltString::generate(&mut OsRng);
            let verifier = Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
                .hash_password(password.as_bytes(), &salt)
                .map_err(storage_error)?
                .to_string();
            let mut bytes = [0u8; 16];
            OsRng.fill_bytes(&mut bytes);
            let code = hex::encode_upper(bytes);
            let formatted = code
                .as_bytes()
                .chunks(4)
                .map(|part| std::str::from_utf8(part).unwrap())
                .collect::<Vec<_>>()
                .join("-");
            (
                LockRecord {
                    verifier: Some(verifier),
                    recovery_hash: Some(recovery_hash(&code)),
                    ..Default::default()
                },
                Some(formatted),
            )
        } else {
            (LockRecord::default(), None)
        };
        self.write_lock(profile, &record)?;
        // Invalidate before registry I/O: a saved credential change stays effective
        // even if the display hint cannot be updated.
        self.sessions.revoke_profile(id)?;
        let mut next = data.clone();
        next.profiles
            .iter_mut()
            .find(|p| p.id == id && !self.is_deleting(id))
            .unwrap()
            .lock_enabled = password.is_some();
        self.save(&mut data, next)?;
        Ok(recovery)
    }
}

fn recovery_hash(code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(RECOVERY_DOMAIN);
    hasher.update(code.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Secrets(Mutex<HashMap<String, String>>);
    impl SecretStore for Secrets {
        fn list_secrets(&self) -> crate::Result<Vec<String>> {
            Ok(self.0.lock().unwrap().keys().cloned().collect())
        }
        fn get_secret(&self, key: &str) -> crate::Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set_secret(&self, key: &str, value: &str) -> crate::Result<()> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete_secret(&self, key: &str) -> crate::Result<()> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    #[test]
    fn retired_avatars_do_not_prevent_registry_or_backup_loading() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("app.db");
        let registry =
            ProfileRegistry::open(dir.path().into(), db.clone(), Arc::new(Secrets::default()))
                .unwrap();
        let id = registry.default_id().unwrap();
        drop(registry);
        let path = dir.path().join(REGISTRY_FILE);
        let mut data: RegistryData = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        for avatar in RETIRED_AVATARS {
            data.profiles[0].avatar_id = (*avatar).into();
            fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
            let registry =
                ProfileRegistry::open(dir.path().into(), db.clone(), Arc::new(Secrets::default()))
                    .unwrap();
            let profile = registry.profile(id).unwrap();
            assert_eq!(profile.avatar_id, PROFILE_AVATARS[0]);
            assert_eq!(profile.name, "Personal");
            assert_eq!(profile.legacy_database, data.profiles[0].legacy_database);
        }
        fs::copy(&path, dir.path().join(REGISTRY_BACKUP)).unwrap();
        fs::write(&path, b"broken").unwrap();
        let registry =
            ProfileRegistry::open(dir.path().into(), db, Arc::new(Secrets::default())).unwrap();
        assert_eq!(registry.profile(id).unwrap().avatar_id, PROFILE_AVATARS[0]);
        data.profiles[0].avatar_id = "../../unrecognized".into();
        fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
        assert!(read_registry(&path).is_err());
    }

    #[test]
    fn legacy_deletion_removes_exports_without_touching_other_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("app.db");
        fs::write(&db, b"legacy").unwrap();
        let registry =
            ProfileRegistry::open(dir.path().into(), db, Arc::new(Secrets::default())).unwrap();
        let legacy = registry.default_id().unwrap();
        let other = registry.create("Other", PROFILE_AVATARS[0]).unwrap();
        let legacy_exports = dir.path().join("pending-exports");
        let other_exports = registry
            .paths(&registry.profile(other.id).unwrap())
            .root
            .join("pending-exports");
        for root in [&legacy_exports, &other_exports] {
            fs::create_dir_all(root).unwrap();
            fs::write(root.join("report.csv"), b"financial data").unwrap();
        }
        assert!(registry
            .validate_import_path(other.id, &legacy_exports.join("report.csv"))
            .is_err());
        registry.begin_delete(legacy, "Personal", None).unwrap();
        registry.finish_delete(legacy).unwrap();
        assert!(!legacy_exports.exists());
        assert_eq!(
            fs::read(other_exports.join("report.csv")).unwrap(),
            b"financial data"
        );
    }

    #[test]
    fn delete_requires_confirmation_and_proof_and_revokes_all_profile_grants() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(
            dir.path().into(),
            dir.path().join("app.db"),
            Arc::new(Secrets::default()),
        )
        .unwrap();
        let id = registry.default_id().unwrap();
        registry
            .set_password(id, None, Some("secret passphrase"))
            .unwrap();
        let a = registry.sessions.issue("a", id, true, None).unwrap();
        let b = registry.sessions.issue("b", id, true, None).unwrap();
        assert!(registry
            .begin_delete(id, "Wrong name", Some("secret passphrase"))
            .is_err());
        assert!(registry
            .begin_delete(id, "Personal", Some("wrong password"))
            .is_err());
        assert!(!registry.is_deleting(id));
        let retained_store = registry.secret_store(&registry.profile(id).unwrap());
        registry
            .begin_delete(id, "Personal", Some("secret passphrase"))
            .unwrap();
        assert!(retained_store
            .set_secret("late_credential", "value")
            .is_err());
        assert!(registry
            .set_password(id, Some("secret passphrase"), Some("replacement password"))
            .is_err());
        assert!(registry.verify(id, Some("secret passphrase")).is_err());
        assert!(registry.sessions.admit("a", a.scope_id).is_err());
        assert!(registry.sessions.admit("b", b.scope_id).is_err());
        assert!(registry.profile(id).is_err());
        assert!(registry.list().unwrap().is_empty());
    }

    #[test]
    fn deletion_preserves_other_profiles_and_survives_registry_backup_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("app.db");
        fs::write(&db, "legacy financial data").unwrap();
        let secrets = Arc::new(Secrets::default());
        let registry =
            ProfileRegistry::open(dir.path().into(), db.clone(), secrets.clone()).unwrap();
        let legacy = registry.default_id().unwrap();
        let other = registry.create("Other", PROFILE_AVATARS[0]).unwrap();
        let other_path = registry.paths(&registry.profile(other.id).unwrap()).root;
        fs::create_dir_all(&other_path).unwrap();
        fs::write(other_path.join("app.db"), "other data").unwrap();
        secrets.set_secret("provider", "legacy secret").unwrap();
        let key = format!("profile:{}:provider", other.id);
        secrets.set_secret(&key, "other secret").unwrap();
        registry.begin_delete(legacy, "Personal", None).unwrap();
        registry.finish_delete(legacy).unwrap();
        assert!(!db.exists());
        assert!(other_path.join("app.db").exists());
        assert_eq!(registry.default_id().unwrap(), other.id);
        assert!(secrets.get_secret("provider").unwrap().is_none());
        assert!(secrets.get_secret(&key).unwrap().is_some());
        drop(registry);
        fs::write(dir.path().join(REGISTRY_FILE), "broken registry").unwrap();
        let registry = ProfileRegistry::open(dir.path().into(), db, secrets).unwrap();
        assert!(registry.profile(legacy).is_err());
        for id in registry.pending_deletions().unwrap() {
            registry.finish_delete(id).unwrap();
        }
        assert_eq!(registry.list().unwrap().len(), 1);
        registry.begin_delete(other.id, "Other", None).unwrap();
        registry.finish_delete(other.id).unwrap();
        assert!(registry.list().unwrap().is_empty());
        assert!(registry.default_id().is_err());
        let new = registry.create("New", PROFILE_AVATARS[0]).unwrap();
        assert_eq!(registry.default_id().unwrap(), new.id);
    }

    #[test]
    fn failed_credential_cleanup_remains_pending_without_blocking_other_profiles() {
        struct UnavailableSecrets;
        impl SecretStore for UnavailableSecrets {
            fn get_secret(&self, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn set_secret(&self, _: &str, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn delete_secret(&self, _: &str) -> crate::Result<()> {
                Ok(())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("app.db");
        let registry =
            ProfileRegistry::open(dir.path().into(), db.clone(), Arc::new(UnavailableSecrets))
                .unwrap();
        let id = registry.default_id().unwrap();
        let other = registry.create("Other", PROFILE_AVATARS[0]).unwrap();
        registry.begin_delete(id, "Personal", None).unwrap();
        assert!(registry.finish_delete(id).is_err());
        assert_eq!(registry.pending_profiles().unwrap()[0].id, id);
        assert_eq!(registry.list().unwrap()[0].id, other.id);
        drop(registry);
        let registry =
            ProfileRegistry::open(dir.path().into(), db, Arc::new(Secrets::default())).unwrap();
        assert_eq!(registry.pending_deletions().unwrap(), vec![id]);
        registry.finish_delete(id).unwrap();
        assert!(registry.pending_profiles().unwrap().is_empty());
        assert_eq!(registry.default_id().unwrap(), other.id);
    }

    #[test]
    fn deletion_does_not_follow_profile_directory_symlinks() {
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            fs::write(outside.path().join("keep"), "keep").unwrap();
            let registry = ProfileRegistry::open(
                dir.path().into(),
                dir.path().join("app.db"),
                Arc::new(Secrets::default()),
            )
            .unwrap();
            let id = registry.default_id().unwrap();
            let root = registry.paths(&registry.profile(id).unwrap()).root;
            fs::create_dir_all(root.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(outside.path(), &root).unwrap();
            registry.begin_delete(id, "Personal", None).unwrap();
            registry.finish_delete(id).unwrap();
            assert!(outside.path().join("keep").exists());
            assert!(!root.exists());
        }
    }

    #[test]
    fn legacy_adoption_is_exclusive_and_does_not_copy_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        fs::write(&path, b"existing database").unwrap();
        let secrets = Arc::new(Secrets::default());
        secrets.set_secret("addon_custom_api", "legacy").unwrap();
        let registry =
            ProfileRegistry::open(dir.path().into(), path.clone(), secrets.clone()).unwrap();
        let default = registry.profile(registry.default_id().unwrap()).unwrap();
        assert_eq!(registry.paths(&default).database, path);
        assert_eq!(
            registry
                .secret_store(&default)
                .get_secret("addon_custom_api")
                .unwrap()
                .as_deref(),
            Some("legacy")
        );
        let second = registry.create("Second", "clay-fluff-animated").unwrap();
        let second = registry.profile(second.id).unwrap();
        assert_eq!(
            registry
                .secret_store(&second)
                .get_secret("addon_custom_api")
                .unwrap(),
            None
        );
        registry
            .secret_store(&second)
            .set_secret("addon_custom_api", "second")
            .unwrap();
        assert_eq!(
            secrets.get_secret("addon_custom_api").unwrap().as_deref(),
            Some("legacy")
        );
        assert!(ProfileRegistry::open(dir.path().into(), path.clone(), secrets.clone()).is_err());
        let id = default.id;
        drop(registry);
        assert_eq!(
            ProfileRegistry::open(dir.path().into(), path, secrets)
                .unwrap()
                .default_id()
                .unwrap(),
            id
        );
    }

    #[test]
    fn missing_legacy_database_does_not_adopt_leftover_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Secrets::default());
        store.set_secret("sync_identity", "leftover").unwrap();
        let registry =
            ProfileRegistry::open(dir.path().into(), dir.path().join("missing.db"), store).unwrap();
        let profile = registry.profile(registry.default_id().unwrap()).unwrap();
        assert!(profile.legacy_database.is_none());
        assert!(registry
            .secret_store(&profile)
            .get_secret("sync_identity")
            .unwrap()
            .is_none());
    }

    #[test]
    fn corrupted_registry_recovers_valid_backup_without_new_identity() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Secrets::default());
        let registry =
            ProfileRegistry::open(dir.path().into(), dir.path().join("old.db"), store.clone())
                .unwrap();
        let id = registry.default_id().unwrap();
        registry.update(id, "Renamed", "clay-bot-animated").unwrap();
        drop(registry);
        fs::write(dir.path().join(REGISTRY_FILE), b"broken").unwrap();
        let registry =
            ProfileRegistry::open(dir.path().into(), dir.path().join("old.db"), store).unwrap();
        assert_eq!(registry.default_id().unwrap(), id);
    }

    #[test]
    fn password_recovery_and_cooldown_survive_reopening_without_touching_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Secrets::default());
        let path = dir.path().join("db");
        let registry =
            ProfileRegistry::open(dir.path().into(), path.clone(), store.clone()).unwrap();
        let id = registry.default_id().unwrap();
        let profile = registry.profile(id).unwrap();
        registry
            .secret_store(&profile)
            .set_secret(DATABASE_KEY_SECRET, "unchanged-key")
            .unwrap();
        registry
            .secret_store(&profile)
            .set_secret(crate::secrets::SYNC_IDENTITY_KEY, "unchanged-sync")
            .unwrap();
        let recovery = registry
            .set_password(id, None, Some("old passphrase é🔒"))
            .unwrap()
            .unwrap();
        assert!(registry.verify(id, Some("old passphrase é🔒")).unwrap());
        for _ in 0..4 {
            assert!(matches!(
                registry.verify(id, Some("000000")),
                Err(ProfileError::IncorrectPassword)
            ));
        }
        assert!(matches!(
            registry.verify(id, Some("000000")),
            Err(ProfileError::Cooldown(_))
        ));
        drop(registry);
        let registry = ProfileRegistry::open(dir.path().into(), path, store).unwrap();
        assert!(matches!(
            registry.verify(id, Some("old passphrase é🔒")),
            Err(ProfileError::Cooldown(_))
        ));
        let mut record = registry.read_lock(&profile).unwrap();
        record.next_attempt = 0;
        registry.write_lock(&profile, &record).unwrap();
        let new_recovery = registry
            .set_password(id, Some(&recovery), Some("new passphrase 🔑"))
            .unwrap()
            .unwrap();
        assert_ne!(new_recovery, recovery);
        assert!(registry.verify(id, Some("new passphrase 🔑")).unwrap());
        assert!(matches!(
            registry.verify(id, Some(&recovery)),
            Err(ProfileError::IncorrectPassword)
        ));
        assert_eq!(
            registry
                .secret_store(&profile)
                .get_secret(DATABASE_KEY_SECRET)
                .unwrap()
                .as_deref(),
            Some("unchanged-key")
        );
        assert_eq!(
            registry
                .secret_store(&profile)
                .get_secret(crate::secrets::SYNC_IDENTITY_KEY)
                .unwrap()
                .as_deref(),
            Some("unchanged-sync")
        );
        registry
            .secret_store(&profile)
            .delete_secret(PROFILE_LOCK_KEY)
            .unwrap();
        assert!(registry.verify(id, None).is_err());
    }

    #[test]
    fn password_policy_preserves_exact_text_and_existing_pin_verifiers() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(
            dir.path().into(),
            dir.path().join("db"),
            Arc::new(Secrets::default()),
        )
        .unwrap();
        let id = registry.default_id().unwrap();
        for invalid in ["", "1234567", &"a".repeat(MAX_PASSWORD_LENGTH + 1)] {
            assert!(matches!(
                registry.set_password(id, None, Some(invalid)),
                Err(ProfileError::Invalid(_))
            ));
        }
        // Unicode scalar values determine length; leading/trailing spaces are significant.
        let password = " é🔒hello ";
        registry.set_password(id, None, Some(password)).unwrap();
        assert!(registry.verify(id, Some(password)).unwrap());
        assert!(matches!(
            registry.verify(id, Some(password.trim())),
            Err(ProfileError::IncorrectPassword)
        ));
        let longest = "🔒".repeat(MAX_PASSWORD_LENGTH);
        registry
            .set_password(id, Some(password), Some(&longest))
            .unwrap();
        assert!(registry.verify(id, Some(&longest)).unwrap());
        // Simulate a stored verifier created by the previous six-digit PIN implementation.
        let profile = registry.profile(id).unwrap();
        let mut record = registry.read_lock(&profile).unwrap();
        record.verifier = Some(
            Argon2::default()
                .hash_password(b"123456", &SaltString::generate(&mut OsRng))
                .unwrap()
                .to_string(),
        );
        registry.write_lock(&profile, &record).unwrap();
        assert!(registry.verify(id, Some("123456")).unwrap());
        registry
            .set_password(id, Some("123456"), Some(password))
            .unwrap();
        assert!(registry.verify(id, Some(password)).unwrap());
        registry.set_password(id, Some(password), None).unwrap();
        assert!(!registry.verify(id, None).unwrap());
    }

    #[test]
    fn verified_account_binding_is_unique_and_survives_renaming() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(
            dir.path().into(),
            dir.path().join("db"),
            Arc::new(Secrets::default()),
        )
        .unwrap();
        let a = registry.default_id().unwrap();
        let b = registry.create("B", "clay-sun-animated").unwrap().id;
        let binding = ConnectBinding {
            issuer: "https://auth.example".into(),
            user_id: "user-a".into(),
            team_id: Some("team".into()),
        };
        registry.bind_connect(a, binding.clone()).unwrap();
        registry
            .update(a, "New name", "clay-fluff-animated")
            .unwrap();
        assert!(
            matches!(registry.bind_connect(b, binding), Err(ProfileError::DuplicateIdentity(id)) if id == a)
        );
        registry
            .bind_connect(
                b,
                ConnectBinding {
                    issuer: "https://auth.example".into(),
                    user_id: "user-b".into(),
                    team_id: Some("team".into()),
                },
            )
            .unwrap();
    }
    #[test]
    fn simultaneous_logins_cannot_reserve_the_same_connect_identity() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(
            ProfileRegistry::open(
                dir.path().into(),
                dir.path().join("app.db"),
                Arc::new(Secrets::default()),
            )
            .unwrap(),
        );
        let a = registry.default_id().unwrap();
        let b = registry.create("B", "clay-fluff-animated").unwrap().id;
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let threads = [a, b].map(|id| {
            let registry = registry.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                registry.bind_connect(
                    id,
                    ConnectBinding {
                        issuer: "issuer".into(),
                        user_id: "same-user".into(),
                        team_id: Some("team".into()),
                    },
                )
            })
        });
        let results = threads.map(|t| t.join().unwrap());
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(ProfileError::DuplicateIdentity(_))))
                .count(),
            1
        );
        let winner = if results[0].is_ok() { a } else { b };
        assert!(matches!(
            registry.bind_connect(
                winner,
                ConnectBinding {
                    issuer: "issuer".into(),
                    user_id: "same-user".into(),
                    team_id: Some("other-team".into())
                }
            ),
            Err(ProfileError::TeamChanged)
        ));
    }

    #[test]
    fn missing_legacy_database_with_backups_does_not_create_a_new_registry() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("backups")).unwrap();
        fs::write(dir.path().join("backups/old.db"), b"existing backup").unwrap();
        assert!(ProfileRegistry::open(
            dir.path().into(),
            dir.path().join("app.db"),
            Arc::new(Secrets::default())
        )
        .is_err());
        assert!(!dir.path().join(REGISTRY_FILE).exists());
    }

    #[test]
    fn import_paths_cannot_read_another_profiles_files_or_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("app.db");
        fs::write(&database, b"A").unwrap();
        let registry =
            ProfileRegistry::open(dir.path().into(), database, Arc::new(Secrets::default()))
                .unwrap();
        let a = registry.default_id().unwrap();
        let b = registry.create("B", "clay-fluff-animated").unwrap().id;
        let paths = registry.paths(&registry.profile(b).unwrap());
        fs::create_dir_all(&paths.root).unwrap();
        fs::write(&paths.database, b"B").unwrap();
        assert!(registry.validate_import_path(a, &paths.database).is_err());
        assert!(registry.validate_import_path(b, &paths.database).is_ok());
        #[cfg(unix)]
        {
            let link = dir.path().join("outside.db");
            std::os::unix::fs::symlink(&paths.database, &link).unwrap();
            assert!(registry.validate_import_path(a, &link).is_err());
        }
    }
}

fn remove_owned_path(path: &Path) -> ProfileResult<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {
            fs::remove_dir_all(path).map_err(storage_error)
        }
        Ok(_) => fs::remove_file(path).map_err(storage_error),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_error(error)),
    }
}
