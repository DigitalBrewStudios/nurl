use eyre::Result;

use crate::{Url, config::FetcherConfig, prefetch::Prefetch, revless::RevlessFetcher};

pub struct Fetchzip;

impl RevlessFetcher for Fetchzip {
    const NAME: &'static str = "fetchzip";

    fn fetch(&self, url: &Url, cfg: &FetcherConfig) -> Result<String> {
        if cfg.has_args() {
            self.fetch_fod(url, cfg)
        } else {
            Prefetch::new().flake_prefetch(format!("tarball+{url}").as_str())
        }
    }
}
