use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::{domain::CoreError, infrastructure::sha256_file};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseManifest {
    pub schema_version: u32,
    pub product: String,
    pub version: String,
    pub channel: String,
    pub artifact_filename: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub authenticode_required: bool,
    pub automatic_updates: bool,
}

impl ReleaseManifest {
    pub fn validate(&self) -> Result<(), CoreError> {
        let extension = Path::new(&self.artifact_filename)
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase);
        if self.schema_version != 1
            || self.product != "VietDub Studio"
            || self.version.is_empty()
            || self.version.len() > 64
            || !matches!(self.channel.as_str(), "stable" | "beta")
            || self.artifact_filename.is_empty()
            || self.artifact_filename.len() > 128
            || Path::new(&self.artifact_filename).file_name()
                != Some(self.artifact_filename.as_ref())
            || !matches!(extension.as_deref(), Some("exe" | "msi"))
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || self.size_bytes == 0
            || !self.authenticode_required
            || self.automatic_updates
        {
            return Err(CoreError::InvalidInput("release manifest"));
        }
        Ok(())
    }
}

pub fn verify_release_artifact(
    manifest: &ReleaseManifest,
    artifact: &Path,
) -> Result<(), CoreError> {
    manifest.validate()?;
    if !artifact.is_absolute() || artifact.file_name() != Some(manifest.artifact_filename.as_ref())
    {
        return Err(CoreError::UnsafePath);
    }
    let metadata = fs::symlink_metadata(artifact)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() != manifest.size_bytes
    {
        return Err(CoreError::ArtifactIntegrity);
    }
    let (sha256, size) = sha256_file(artifact)?;
    if sha256 != manifest.sha256 || size != manifest.size_bytes {
        return Err(CoreError::ArtifactIntegrity);
    }
    verify_authenticode(artifact)
}

#[cfg(windows)]
fn verify_authenticode(artifact: &Path) -> Result<(), CoreError> {
    use std::os::windows::ffi::OsStrExt;
    use std::{mem::size_of, ptr::null_mut};
    use windows_sys::Win32::Security::WinTrust::{
        WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
        WINTRUST_FILE_INFO, WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_WHOLECHAIN,
        WTD_STATEACTION_IGNORE, WTD_UI_NONE,
    };

    let path = artifact
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: path.as_ptr(),
        hFile: null_mut(),
        pgKnownSubject: null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &mut file_info,
        },
        dwStateAction: WTD_STATEACTION_IGNORE,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: file_info, data, and action contain valid pointers to live buffers for the
    // duration of this synchronous WinVerifyTrust call. UI and network retrieval are disabled.
    let status = unsafe {
        WinVerifyTrust(
            null_mut(),
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        )
    };
    if status != 0 {
        return Err(CoreError::ArtifactIntegrity);
    }
    Ok(())
}

#[cfg(not(windows))]
fn verify_authenticode(_artifact: &Path) -> Result<(), CoreError> {
    Err(CoreError::ArtifactIntegrity)
}
