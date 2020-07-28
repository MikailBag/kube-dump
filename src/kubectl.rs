//! Utilities for running kubectl
use anyhow::Context as _;
use std::sync::Arc;
use tokio::{process::Command, sync::Semaphore};
const MAX_CONCURRENCY: usize = 3;

/// Allows invoking kubectl
pub struct Kubectl {
    enabled: bool,
    sem: Arc<Semaphore>,
}

impl Kubectl {
    pub async fn try_new() -> Kubectl {
        match Kubectl::new().await {
            Ok(k) => k,
            Err(err) => {
                eprintln!("Kubectl integration will be disabled: {:#}", err);
                Kubectl::disabled()
            }
        }
    }

    pub async fn new() -> anyhow::Result<Kubectl> {
        let mut cmd = Command::new("kubectl");
        cmd.arg("version");
        let out = cmd.output().await?;
        if !out.status.success() {
            // either kubectl not available, or it is unable to connect to cluster
            anyhow::bail!(
                "kubectl does not work: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(Kubectl {
            enabled: true,
            sem: Arc::new(Semaphore::new(MAX_CONCURRENCY)),
        })
    }

    pub fn disabled() -> Kubectl {
        Kubectl {
            enabled: false,
            sem: Arc::new(Semaphore::new(MAX_CONCURRENCY)),
        }
    }

    pub async fn exec<S: AsRef<std::ffi::OsStr>>(&self, args: &[S]) -> anyhow::Result<Option<String>> {
        if !self.enabled {
            return Ok(None);
        }
        let _permit = self.sem.clone().acquire_owned().await;
        let mut cmd = Command::new("kubectl");
        // disable colors
        cmd.env("TERM", "dumb");
        cmd.args(args);
        let out = cmd.output().await?;
        if !out.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(Some(String::from_utf8(out.stdout).context("kubectl output was not utf-8")?))
    }
}
