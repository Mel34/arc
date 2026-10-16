use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

const USER_AGENT: &str = "arc-tui/0.1.0 (aur-commander)";
const AUR_RPC_URL: &str = "https://aur.archlinux.org/rpc/v5";
const AUR_CGIT_URL: &str = "https://aur.archlinux.org/cgit/aur.git/plain/PKGBUILD?h=";

#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct AurRpcResponse {
    pub version: u32,
    #[serde(rename = "type")]
    pub response_type: String,
    pub resultcount: usize,
    pub results: Vec<AurPackage>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct AurPackage {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "Description")]
    pub description: Option<String>,
    #[serde(rename = "NumVotes")]
    pub votes: i32,
    #[serde(rename = "Popularity")]
    pub popularity: f64,
    #[serde(rename = "Maintainer")]
    pub maintainer: Option<String>,
    #[serde(rename = "URL")]
    pub url: Option<String>,
    #[serde(rename = "OutOfDate")]
    pub out_of_date: Option<i64>,
}

pub struct AurClient {
    agent: ureq::Agent,
}

impl AurClient {
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(10))
            .user_agent(USER_AGENT)
            .build();
        Self { agent }
    }

    pub fn search(&self, query: &str) -> Result<Vec<AurPackage>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let url = format!("{}/search/{}", AUR_RPC_URL, query.trim());
        let response: AurRpcResponse = self
            .agent
            .get(&url)
            .call()
            .context("Failed to communicate with AUR RPC")?
            .into_json()
            .context("Failed to parse AUR RPC response JSON")?;

        let mut results = response.results;
        // Sort A-Z by package name
        results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(results)
    }

    pub fn info(&self, pkg_names: &[&str]) -> Result<Vec<AurPackage>> {
        if pkg_names.is_empty() {
            return Ok(Vec::new());
        }

        let mut request = self.agent.get(&format!("{}/info", AUR_RPC_URL));
        for name in pkg_names {
            request = request.query("arg[]", name);
        }

        let response: AurRpcResponse = request
            .call()
            .context("AUR info request failed")?
            .into_json()
            .context("Failed to parse AUR info JSON")?;

        Ok(response.results)
    }

    pub fn fetch_pkgbuild(&self, pkg_name: &str) -> Result<String> {
        let url = format!("{}{}", AUR_CGIT_URL, pkg_name);
        let content = self
            .agent
            .get(&url)
            .call()
            .context("Failed to fetch PKGBUILD")?
            .into_string()
            .context("Failed to read PKGBUILD text")?;

        Ok(content)
    }

    pub fn check_updates(&self, local_pkgs: &[(&str, &str)]) -> Result<Vec<(String, String)>> {
        if local_pkgs.is_empty() {
            return Ok(Vec::new());
        }

        let names: Vec<&str> = local_pkgs.iter().map(|(n, _)| *n).collect();
        let aur_info = self.info(&names)?;

        let mut updates = Vec::new();
        for (local_name, local_ver) in local_pkgs {
            if let Some(aur_pkg) = aur_info.iter().find(|p| p.name == *local_name) {
                if alpm::vercmp(aur_pkg.version.as_str(), *local_ver).is_gt() {
                    updates.push((local_name.to_string(), aur_pkg.version.clone()));
                }
            }
        }
        Ok(updates)
    }
}
