use std::collections::BTreeSet;
use std::net::IpAddr;
use std::time::{Duration, SystemTime};

use crate::github::Asset;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum State {
    Unassigned,
    Assigned(IpAddr),
    Downloading(IpAddr),
    Booting(IpAddr),
    Reported(IpAddr),
    Finished(IpAddr),
    Failed(IpAddr),
}

impl State {
    pub const fn ip(&self) -> Option<IpAddr> {
        match self {
            Self::Unassigned => None,

            Self::Assigned(ip)
            | Self::Downloading(ip)
            | Self::Booting(ip)
            | Self::Reported(ip)
            | Self::Finished(ip)
            | Self::Failed(ip) => Some(*ip),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Job {
    pub asset: Asset,
    pub state: State,
    pub seen: Option<SystemTime>,
}

impl Job {
    fn elapsed(&self) -> Duration {
        self.seen
            .unwrap_or(SystemTime::UNIX_EPOCH)
            .elapsed()
            .unwrap_or_default()
    }
}

#[derive(Debug)]
pub struct Jobs(Vec<Job>);

impl From<BTreeSet<Asset>> for Jobs {
    fn from(assets: BTreeSet<Asset>) -> Self {
        Self(
            assets
                .into_iter()
                .map(|asset| Job {
                    asset,
                    state: State::Unassigned,
                    seen: None,
                })
                .collect(),
        )
    }
}

impl Jobs {
    const TIMEOUT: Duration = Duration::from_mins(5);

    pub fn iter(&self) -> impl Iterator<Item = &Job> {
        self.0.iter()
    }

    #[tracing::instrument(level = "info", fields(ip = %ip), skip(self))]
    pub fn assign(&mut self, ip: IpAddr) -> Option<Asset> {
        // First, try to find a job that is already assigned to this IP.
        for job in &mut self.0 {
            match job.state {
                State::Assigned(addr) if addr == ip => {
                    tracing::debug!(ip = %ip, asset = %job.asset.name, "re-assigning existing job");
                    job.state = State::Assigned(ip);
                    job.seen = Some(SystemTime::now());
                    return Some(job.asset.clone());
                }

                _ => {}
            }
        }

        // Next, try to find an unassigned or expired job.
        for job in &mut self.0 {
            match job.state {
                State::Unassigned => {
                    tracing::info!(ip = %ip, asset = %job.asset.name, size = job.asset.size, "assigning job");
                    job.state = State::Assigned(ip);
                    job.seen = Some(SystemTime::now());
                    return Some(job.asset.clone());
                }

                State::Assigned(..) | State::Downloading(..) if job.elapsed() > Self::TIMEOUT => {
                    tracing::info!(ip = %ip, asset = %job.asset.name, "re-assigning expired job");
                    job.state = State::Assigned(ip);
                    job.seen = Some(SystemTime::now());
                    return Some(job.asset.clone());
                }

                _ => {}
            }
        }

        tracing::info!(ip = %ip, "no job available for assignment");
        None
    }

    #[tracing::instrument(level = "info", fields(ip = %ip), skip(self))]
    pub fn downloading(&mut self, ip: IpAddr) -> Option<&Asset> {
        for job in &mut self.0 {
            match job.state {
                State::Assigned(addr) if addr == ip => {
                    tracing::info!(ip = %ip, asset = %job.asset.name, size = job.asset.size, "start downloading");
                    job.state = State::Downloading(ip);
                    job.seen = Some(SystemTime::now());
                    return Some(&job.asset);
                }
                _ => {}
            }
        }

        tracing::warn!(ip = %ip, "download attempted by wrong or unassigned IP");
        None
    }

    #[tracing::instrument(level = "info", fields(ip = %ip), skip(self))]
    pub fn booting(&mut self, ip: IpAddr) -> bool {
        for job in &mut self.0 {
            match job.state {
                State::Downloading(addr) if addr == ip => {
                    tracing::info!(ip = %ip, asset = %job.asset.name, "boot beacon accepted");
                    job.state = State::Booting(ip);
                    job.seen = Some(SystemTime::now());
                    return true;
                }
                _ => {}
            }
        }

        tracing::warn!(ip = %ip, "boot beacon from unexpected IP or no downloading job");
        false
    }

    #[tracing::instrument(level = "info", fields(ip = %ip), skip(self))]
    pub fn report(&mut self, ip: IpAddr) -> bool {
        for job in &mut self.0 {
            match job.state {
                State::Booting(addr) if addr == ip => {
                    tracing::info!(ip = %ip, asset = %job.asset.name, "report accepted");
                    job.state = State::Reported(ip);
                    job.seen = Some(SystemTime::now());
                    return true;
                }
                _ => {}
            }
        }

        tracing::warn!(ip = %ip, "report received from wrong IP or not in booting state");
        false
    }

    #[tracing::instrument(level = "info", fields(ip = %ip), skip(self))]
    pub fn finish(&mut self, ip: IpAddr) -> bool {
        for job in &mut self.0 {
            job.state = match job.state {
                State::Assigned(addr) if addr == ip => State::Failed(ip),
                State::Booting(addr) if addr == ip => State::Failed(ip),
                State::Reported(addr) if addr == ip => State::Finished(ip),
                _ => continue,
            };

            tracing::info!(ip = %ip, asset = %job.asset.name, final_state = ?job.state, "job finalised");
            job.seen = Some(SystemTime::now());
            return true;
        }

        tracing::warn!(ip = %ip, "finish called but no matching job for ip");
        false
    }
}
