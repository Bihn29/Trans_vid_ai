mod credentials;

#[cfg(windows)]
pub use credentials::WindowsCredentialStore;
pub use credentials::{
    CredentialReference, CredentialStore, SecretString, UnavailableCredentialStore,
};
