use std::fmt;

use crate::domain::CoreError;

/// Stable lookup key for a secret held by an operating-system credential store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialReference {
    pub service: String,
    pub account: String,
}

impl CredentialReference {
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Result<Self, CoreError> {
        let value = Self {
            service: service.into(),
            account: account.into(),
        };
        if value.service.trim().is_empty()
            || value.account.trim().is_empty()
            || value.service.len() > 128
            || value.account.len() > 128
            || !value
                .service
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || !value.account.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'@')
            })
        {
            return Err(CoreError::InvalidInput("credential reference"));
        }
        Ok(value)
    }
}

/// Secret wrapper whose Debug representation can never expose its contents.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Result<Self, CoreError> {
        let value = value.into();
        if value.is_empty() || value.len() > 16 * 1024 {
            return Err(CoreError::InvalidInput("credential value"));
        }
        Ok(Self(value))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        // SAFETY: the String remains exclusively borrowed for Drop and the bytes are
        // overwritten without changing its length or UTF-8 layout.
        unsafe { self.0.as_bytes_mut() }.fill(0);
    }
}

/// Boundary implemented by an OS keychain integration. Implementations must not
/// persist returned values in SQLite, project files, logs, or error strings.
pub trait CredentialStore: Send + Sync {
    fn get(&self, reference: &CredentialReference) -> Result<SecretString, CoreError>;

    fn put(
        &self,
        _reference: &CredentialReference,
        _secret: &SecretString,
    ) -> Result<(), CoreError> {
        Err(CoreError::CredentialUnavailable)
    }

    fn delete(&self, _reference: &CredentialReference) -> Result<(), CoreError> {
        Err(CoreError::CredentialUnavailable)
    }
}

/// Secure default until a platform credential-store adapter is configured.
#[derive(Debug, Default)]
pub struct UnavailableCredentialStore;

impl CredentialStore for UnavailableCredentialStore {
    fn get(&self, _reference: &CredentialReference) -> Result<SecretString, CoreError> {
        Err(CoreError::CredentialUnavailable)
    }
}

#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WindowsCredentialStore;

#[cfg(windows)]
impl WindowsCredentialStore {
    fn target(reference: &CredentialReference) -> Vec<u16> {
        format!("VietDub Studio/{}/{}", reference.service, reference.account)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect()
    }
}

#[cfg(windows)]
impl CredentialStore for WindowsCredentialStore {
    fn get(&self, reference: &CredentialReference) -> Result<SecretString, CoreError> {
        use std::{ptr::null_mut, slice};
        use windows_sys::Win32::Security::Credentials::{
            CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
        };

        let target = Self::target(reference);
        let mut raw: *mut CREDENTIALW = null_mut();
        // SAFETY: target is NUL-terminated and raw is a valid out pointer. A successful
        // result is released exactly once with CredFree below.
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) } == 0 {
            return Err(CoreError::CredentialUnavailable);
        }
        if raw.is_null() {
            return Err(CoreError::CredentialUnavailable);
        }
        // SAFETY: CredReadW returned a live CREDENTIALW allocation. Blob size is checked
        // before constructing the slice and the allocation stays live until after copying.
        let credential = unsafe { &*raw };
        let size = credential.CredentialBlobSize as usize;
        let result = if size == 0 || size > 16 * 1024 || credential.CredentialBlob.is_null() {
            Err(CoreError::CredentialUnavailable)
        } else {
            let bytes = unsafe { slice::from_raw_parts(credential.CredentialBlob, size) };
            match String::from_utf8(bytes.to_vec()) {
                Ok(value) => SecretString::new(value),
                Err(_) => Err(CoreError::CredentialUnavailable),
            }
        };
        // SAFETY: raw is the allocation returned by CredReadW and has not been freed.
        unsafe { CredFree(raw.cast()) };
        result
    }

    fn put(&self, reference: &CredentialReference, secret: &SecretString) -> Result<(), CoreError> {
        use std::ptr::null_mut;
        use windows_sys::Win32::Security::Credentials::{
            CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
        };

        let target = Self::target(reference);
        let mut account = reference
            .account
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let blob = secret.expose().as_bytes();
        if blob.len() > 2560 {
            return Err(CoreError::InvalidInput("credential value"));
        }
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_ptr().cast_mut(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: account.as_mut_ptr(),
            Comment: null_mut(),
            TargetAlias: null_mut(),
            Attributes: null_mut(),
            ..Default::default()
        };
        // SAFETY: all pointers reference live buffers for the duration of the call and
        // Credential Manager copies the supplied values before returning.
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err(CoreError::CredentialUnavailable);
        }
        Ok(())
    }

    fn delete(&self, reference: &CredentialReference) -> Result<(), CoreError> {
        use windows_sys::Win32::{
            Foundation::{GetLastError, ERROR_NOT_FOUND},
            Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC},
        };
        let target = Self::target(reference);
        // SAFETY: target is a live NUL-terminated UTF-16 buffer.
        if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
            // SAFETY: GetLastError has no preconditions immediately after the failed call.
            if unsafe { GetLastError() } != ERROR_NOT_FOUND {
                return Err(CoreError::CredentialUnavailable);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_is_redacted() {
        let secret = SecretString::new("super-secret-token").expect("secret");
        let diagnostic = format!("{secret:?}");
        assert_eq!(diagnostic, "SecretString([REDACTED])");
        assert!(!diagnostic.contains("super-secret-token"));
    }
}
