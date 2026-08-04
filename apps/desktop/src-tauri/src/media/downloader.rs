use std::{future::Future, path::PathBuf, pin::Pin, time::Duration};

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::domain::MediaSite;

use super::{NetworkPolicy, ResolvedEndpoint, ValidatedRemoteUrl};

#[derive(Debug, Clone)]
pub struct DownloadAdapterContract {
    pub site: MediaSite,
    pub max_bytes: u64,
    pub timeout: Duration,
    pub network_policy: NetworkPolicy,
}

impl DownloadAdapterContract {
    pub fn for_site(
        site: MediaSite,
        max_bytes: u64,
        timeout: Duration,
    ) -> Result<Self, DownloaderError> {
        if max_bytes == 0
            || max_bytes > 1024 * 1024 * 1024 * 1024
            || timeout.is_zero()
            || timeout > Duration::from_secs(6 * 60 * 60)
        {
            return Err(DownloaderError::InvalidContract);
        }
        Ok(Self {
            site,
            max_bytes,
            timeout,
            network_policy: NetworkPolicy::new(5).map_err(|_| DownloaderError::InvalidContract)?,
        })
    }

    pub fn validate_url(&self, raw: &str) -> Result<ValidatedRemoteUrl, DownloaderError> {
        let url = self
            .network_policy
            .validate_url(raw)
            .map_err(|_| DownloaderError::UrlRejected)?;
        if url.site() != self.site {
            return Err(DownloaderError::WrongSite);
        }
        Ok(url)
    }

    pub async fn resolve_initial(&self, raw: &str) -> Result<ResolvedEndpoint, DownloaderError> {
        let url = self.validate_url(raw)?;
        self.network_policy
            .resolve_and_validate(url)
            .await
            .map_err(|_| DownloaderError::EndpointRejected)
    }
}

#[derive(Debug, Error)]
pub enum DownloaderError {
    #[error("download adapter contract is invalid")]
    InvalidContract,
    #[error("remote URL was rejected")]
    UrlRejected,
    #[error("remote URL belongs to another site adapter")]
    WrongSite,
    #[error("remote endpoint was rejected")]
    EndpointRejected,
    #[error("remote download exceeded its size limit")]
    SizeLimit,
    #[error("remote download timed out")]
    Timeout,
    #[error("remote download was cancelled")]
    Cancelled,
    #[error("remote downloader failed")]
    Failed,
}

pub trait RemoteDownloader: Send + Sync {
    fn contract(&self) -> &DownloadAdapterContract;

    /// Implementations must connect to an address from `initial.addresses` and use
    /// `network_policy` to resolve and validate every redirect before connecting.
    fn download<'a>(
        &'a self,
        initial: &'a ResolvedEndpoint,
        network_policy: &'a NetworkPolicy,
        project_temp: &'a std::path::Path,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<PathBuf, DownloaderError>> + Send + 'a>>;
}
